//! Android SDK emulator adapter: explicit SDK and AVD locations, file-only AVD
//! inventory, console identity queries and owned read-only launches.
use super::{Android, adb::validate_serial};
use crate::{
    error::{Error, Result},
    process,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

/// Target identity prefix for an AVD definition that has no adb serial yet.
pub(crate) const AVD_PREFIX: &str = "android:avd:";
/// Guest changes are discarded when the instance stops; snapshots are not saved.
pub(crate) const DATA_POLICY: &str = "read-only";

#[derive(Clone, Debug)]
pub(crate) struct Avd {
    pub(crate) name: String,
    pub(crate) display_name: Option<String>,
    pub(crate) api: Option<String>,
    pub(crate) abi: Option<String>,
    pub(crate) tag: Option<String>,
}

/// The emulator executable: `TESTRUN_EMULATOR`, else `emulator/emulator` under
/// `ANDROID_HOME`, else under `ANDROID_SDK_ROOT`. `PATH` is never searched.
pub(crate) fn executable() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("TESTRUN_EMULATOR").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(std::env::var_os)
        .filter(|root| !root.is_empty())
        .map(|root| PathBuf::from(root).join("emulator/emulator"))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            Error::new(
                "PREREQUISITE_MISSING",
                "Android Emulator was not found; set ANDROID_HOME to the SDK or TESTRUN_EMULATOR to the emulator executable",
            )
        })
}

/// AVD definitions: `ANDROID_AVD_HOME`, else `ANDROID_USER_HOME/avd`, else `~/.android/avd`.
pub(crate) fn home() -> Option<PathBuf> {
    let set = |name| std::env::var_os(name).filter(|value| !value.is_empty());
    set("ANDROID_AVD_HOME")
        .map(PathBuf::from)
        .or_else(|| set("ANDROID_USER_HOME").map(|home| PathBuf::from(home).join("avd")))
        .or_else(|| set("HOME").map(|home| PathBuf::from(home).join(".android/avd")))
}

pub(crate) fn valid_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// AVDs defined under `home`, read from files only: no process, lock or console.
/// An AVD is `<name>.ini` whose `path` names a directory holding `config.ini`.
pub(crate) fn list(home: &Path) -> Result<Vec<Avd>> {
    let entries = match std::fs::read_dir(home) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => {
            return Err(Error::new(
                "HOST_IO",
                "Cannot read the Android AVD directory",
            ));
        }
    };
    let mut avds = Vec::new();
    for entry in entries.take(1024).flatten() {
        let file = entry.path();
        let Some(name) = file
            .extension()
            .filter(|extension| *extension == "ini")
            .and(file.file_stem())
            .and_then(|stem| stem.to_str())
            .filter(|stem| valid_name(stem))
        else {
            continue;
        };
        let Some(definition) = read_ini(&file) else {
            continue;
        };
        let Some(directory) = definition
            .get("path")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                let relative = definition.get("path.rel")?;
                Some(home.parent()?.join(relative))
            })
        else {
            continue;
        };
        let Some(config) = read_ini(&directory.join("config.ini")) else {
            continue;
        };
        avds.push(Avd {
            name: name.to_owned(),
            display_name: config.get("avd.ini.displayname").cloned(),
            api: definition.get("target").cloned(),
            abi: config.get("abi.type").cloned(),
            tag: config.get("tag.id").cloned(),
        });
    }
    avds.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(avds)
}

fn read_ini(path: &Path) -> Option<BTreeMap<String, String>> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    Some(
        text.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
            .collect(),
    )
}

impl Android {
    /// The AVD name an emulator's console reports for `serial`.
    pub(crate) async fn avd_name(&self, serial: &str, timeout: Duration) -> Result<String> {
        validate_serial(serial)?;
        let output = process::run(
            &self.adb,
            &["-s", serial, "emu", "avd", "name"],
            timeout,
            4096,
        )
        .await?;
        let text = String::from_utf8_lossy(&output.bytes);
        let lines: Vec<_> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        match lines.as_slice() {
            [name, "OK"] => Ok((*name).to_owned()),
            _ => Err(Error::new(
                "PLATFORM_PROTOCOL",
                "The emulator console did not report an AVD name",
            )),
        }
    }
    /// Ask the emulator behind `serial` to exit through its console.
    pub(crate) async fn emu_kill(&self, serial: &str) -> Result<()> {
        validate_serial(serial)?;
        process::run(
            &self.adb,
            &["-s", serial, "emu", "kill"],
            Duration::from_secs(5),
            4096,
        )
        .await
        .map(|_| ())
    }
}

/// Start `name` read-only, without saving a snapshot, on an explicit console
/// port, in its own process group. Output goes to `log`. The caller owns the
/// child and its group; nothing kills it on drop.
pub(crate) fn launch(
    emulator: &Path,
    name: &str,
    port: u16,
    window: bool,
    log: &Path,
) -> Result<tokio::process::Child> {
    if !valid_name(name) {
        return Err(Error::invalid("Invalid AVD name"));
    }
    let output = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(log)
        .map_err(|_| Error::new("HOST_IO", "Cannot create the emulator log"))?;
    let errors = output
        .try_clone()
        .map_err(|_| Error::new("HOST_IO", "Cannot share the emulator log"))?;
    let port = port.to_string();
    let mut args = vec![
        "-avd",
        name,
        "-port",
        &port,
        "-read-only",
        "-no-snapshot-save",
        "-no-boot-anim",
    ];
    if !window {
        args.push("-no-window");
    }
    let mut command = tokio::process::Command::new(emulator);
    command
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(output)
        .stderr(errors)
        .kill_on_drop(false);
    #[cfg(unix)]
    command.process_group(0);
    command.spawn().map_err(|error| {
        Error::new("PREREQUISITE_MISSING", "Could not start Android Emulator")
            .details(serde_json::json!({"osCode":error.raw_os_error()}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn define(home: &Path, name: &str, definition: &str, config: Option<&str>) -> PathBuf {
        let directory = home.join(format!("{name}.avd"));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            home.join(format!("{name}.ini")),
            definition.replace("DIR", directory.to_str().unwrap()),
        )
        .unwrap();
        if let Some(config) = config {
            std::fs::write(directory.join("config.ini"), config).unwrap();
        }
        directory
    }

    #[test]
    fn inventory_reads_only_complete_definitions_with_valid_names() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("avd");
        std::fs::create_dir(&home).unwrap();
        define(
            &home,
            "Acme-QA_1.0",
            "path=DIR\ntarget=android-36\n",
            Some("avd.ini.displayname = Acme QA\nabi.type = x86_64\ntag.id = google_apis\n"),
        );
        // A relative definition resolves against the Android user home.
        define(
            &home,
            "Acme-relative",
            "path.rel=avd/Acme-relative.avd\n",
            Some(""),
        );
        define(&home, "Acme-no-config", "path=DIR\n", None);
        define(&home, "Acme-no-path", "target=android-36\n", Some(""));
        define(&home, "Acme bad", "path=DIR\n", Some(""));
        std::fs::write(home.join("notes.txt"), "path=/tmp\n").unwrap();
        let avds = list(&home).unwrap();
        let names: Vec<_> = avds.iter().map(|avd| avd.name.as_str()).collect();
        assert_eq!(names, ["Acme-QA_1.0", "Acme-relative"]);
        assert_eq!(avds[0].display_name.as_deref(), Some("Acme QA"));
        assert_eq!(avds[0].api.as_deref(), Some("android-36"));
        assert_eq!(avds[0].abi.as_deref(), Some("x86_64"));
        assert_eq!(avds[0].tag.as_deref(), Some("google_apis"));
        assert!(list(&root.path().join("absent")).unwrap().is_empty());
    }

    #[test]
    fn names_that_could_become_options_or_paths_are_refused() {
        for name in ["Acme", "Acme-QA_01.test", "A"] {
            assert!(valid_name(name), "{name}");
        }
        for name in [
            "",
            "-wipe-data",
            ".hidden",
            "Acme/../x",
            "Acme QA",
            "Acme;x",
            &"a".repeat(129),
        ] {
            assert!(!valid_name(name), "{name}");
        }
        let log = tempfile::tempdir().unwrap();
        assert_eq!(
            launch(
                Path::new("/bin/true"),
                "-wipe-data",
                5554,
                false,
                &log.path().join("l")
            )
            .unwrap_err()
            .code,
            "INVALID_ARGUMENT"
        );
    }
}
