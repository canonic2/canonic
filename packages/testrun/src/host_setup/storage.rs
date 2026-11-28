//! Private, locked settings transactions in the native configuration directory.
mod upgrades;
use super::{Selection, Teams, choose, validate_team};
use crate::{
    artifacts,
    error::{Error, Result},
};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

pub(super) struct Store {
    pub path: PathBuf,
}

fn invalid(message: &str) -> Error {
    Error::new("SIGNING_PREFERENCE_INVALID", message)
        .details(json!({"nextAction":"Repair the private Testrun settings file, then run testrun signing setup"}))
}

fn check_settings_directory(path: &Path) -> Result<()> {
    artifacts::check_private_directory(path).map_err(|cause| {
        let metadata = fs::symlink_metadata(path).ok();
        let reason = if metadata.as_ref().is_some_and(|m| !m.is_dir()) {
            "not_directory"
        } else {
            "unavailable"
        };
        let mut details = json!({
            "settingsDirectory":path,
            "nextAction":"testrun signing setup",
            "manualAction":"Restore the named settings directory as a real directory owned by this OS user with private access, then run testrun signing setup. A symlink or file must be replaced with a directory rather than changing its target's permissions.",
            "cause":{"code":cause.code,"message":cause.message}
        });
        #[cfg(unix)]
        let reason = if let Some(metadata) = metadata.filter(|m| m.is_dir()) {
            use std::os::unix::fs::MetadataExt;
            // SAFETY: read-only identity query for advisory repair guidance.
            if metadata.uid() != unsafe { libc::geteuid() } {
                details["manualAction"] = json!("Restore ownership of the named Testrun settings directory to this OS user, restrict its permissions to 0700, then run testrun signing setup.");
                "ownership"
            } else if metadata.mode() & 0o077 != 0 {
                details["requiredMode"] = json!("0700");
                details["manualAction"] = json!("Restrict the named Testrun settings directory to mode 0700 with chmod, then run testrun signing setup.");
                details["recoveryCommand"] = json!({"program":"chmod","args":["700",path]});
                "permissions"
            } else {
                reason
            }
        } else {
            reason
        };
        details["reason"] = json!(reason);
        Error::new("SIGNING_PREFERENCE_INVALID", format!("Testrun settings directory must be private and accessible: {}", path.display()))
            .details(details)
    })
}

impl Store {
    pub fn host() -> Result<Self> {
        let base = dirs::config_local_dir()
            .filter(|p| p.is_absolute())
            .ok_or_else(|| {
                Error::new(
                    "HOST_IO",
                    "Cannot locate the user's local configuration directory",
                )
            })?;
        let application = if cfg!(any(target_os = "macos", target_os = "windows")) {
            "Testrun"
        } else {
            "testrun"
        };
        Ok(Self {
            path: base.join(application).join("settings.json"),
        })
    }

    fn transaction<T>(&self, action: impl FnOnce(&mut Value) -> Result<T>) -> Result<T> {
        let parent = self.path.parent().unwrap();
        fs::create_dir_all(parent.parent().unwrap())
            .map_err(|_| Error::new("HOST_IO", "Cannot create configuration parent directory"))?;
        match artifacts::directory(parent) {
            Ok(()) => (),
            Err(_) if parent.exists() => (),
            Err(error) => return Err(error),
        }
        check_settings_directory(parent)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let lock = options
            .open(parent.join("settings.lock"))
            .map_err(|_| Error::new("HOST_IO", "Cannot open Testrun settings lock"))?;
        private_file(&lock)?;
        lock.lock()
            .map_err(|_| Error::new("HOST_IO", "Cannot lock Testrun settings"))?;
        // The stable lock anchor is never renamed or deleted. Every transaction
        // reloads the file under this lock, including in an already running owner.
        let mut settings = read(&self.path)?.unwrap_or_else(|| json!({}));
        team(&settings)?;
        action(&mut settings)
    }

    pub fn saved(&self) -> Result<Option<String>> {
        self.transaction(|settings| team(settings))
    }

    pub fn connection(&self, teams: &Teams, requested: Option<&str>) -> Result<Selection> {
        if requested.is_some() {
            return Ok(Selection {
                runner_team_id: choose(requested, teams)?,
                source: "override",
            });
        }
        self.resolve(teams, None)
    }

    pub fn resolve(&self, teams: &Teams, requested: Option<&str>) -> Result<Selection> {
        self.transaction(|settings| {
            let saved = team(settings)?;
            let selected = choose(requested.or(saved.as_deref()), teams)?;
            let unchanged = saved.as_deref() == Some(selected.as_str());
            let source = if unchanged {
                "saved"
            } else if requested.is_some() {
                "setup"
            } else {
                "automatic"
            };
            if !unchanged {
                set_team(settings, &selected)?;
                self.write(settings)?;
            }
            Ok(Selection {
                runner_team_id: selected,
                source,
            })
        })
    }

    fn write(&self, settings: &Value) -> Result<()> {
        let parent = self.path.parent().unwrap();
        let bytes = settings.to_string();
        if bytes.len() > 64 * 1024 {
            return Err(invalid(
                "Updated Testrun settings exceed their size limit; existing settings were preserved",
            ));
        }
        let temporary = parent.join(format!("settings-{}.partial", uuid::Uuid::new_v4()));
        artifacts::write(&temporary, bytes.as_bytes()).map_err(|error| {
            Error::new(
                "HOST_IO",
                "Cannot write Testrun settings; check the configuration directory permissions",
            )
            .details(json!({"cause":error.code}))
        })?;
        let result = fs::rename(&temporary, &self.path).and_then(|()| {
            #[cfg(unix)]
            artifacts::sync(&File::open(parent)?)?;
            Ok(())
        });
        if let Err(error) = result {
            let cleanup = fs::remove_file(&temporary);
            return Err(Error::new("HOST_IO", "Cannot publish Testrun settings")
                .details(json!({"cause":error.to_string(),"temporaryCleanupFailed":cleanup.is_err() && temporary.exists()})));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

fn private_file(file: &File) -> Result<()> {
    let metadata = file
        .metadata()
        .map_err(|_| invalid("Cannot inspect signing settings"))?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Err(invalid("Signing settings must be a bounded regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: read-only identity query.
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(invalid(
                "Signing settings must be private and owned by this user",
            ));
        }
    }
    Ok(())
}

fn read(path: &Path) -> Result<Option<Value>> {
    // Missing settings mean no saved preference, even before the first setup.
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(invalid("Cannot inspect signing settings")),
        Ok(metadata) if !metadata.is_file() => {
            return Err(invalid("Signing settings must be a regular file"));
        }
        Ok(_) => (),
    }
    check_settings_directory(path.parent().unwrap())?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| invalid("Cannot read signing settings"))?;
    private_file(&file)?;
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("Cannot read signing settings"))?;
    if bytes.len() > 64 * 1024 {
        return Err(invalid("Signing settings exceed their size limit"));
    }
    upgrades::settings(&bytes).map(Some)
}

fn team(settings: &Value) -> Result<Option<String>> {
    let Some(ios) = settings.get("ios") else {
        return Ok(None);
    };
    if !ios.is_object() {
        return Err(invalid("The ios settings must be an object"));
    }
    let Some(value) = ios.get("runnerTeamId") else {
        return Ok(None);
    };
    let team = value
        .as_str()
        .ok_or_else(|| invalid("The saved signing team must be a string"))?;
    validate_team(team).map_err(|_| invalid("The saved signing team is invalid"))?;
    Ok(Some(team.into()))
}

fn set_team(settings: &mut Value, selected: &str) -> Result<()> {
    team(settings)?;
    if settings.get("ios").is_none() {
        settings["ios"] = json!({});
    }
    settings["ios"]["runnerTeamId"] = json!(selected);
    Ok(())
}
