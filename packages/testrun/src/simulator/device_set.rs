//! One explicit CoreSimulator device set shared by discovery, commands and native workers.
use crate::error::{Error, Result};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
    time::Duration,
};

pub(crate) struct DeviceSet {
    pub(crate) path: Option<PathBuf>,
    pub(crate) identity: String,
}

thread_local! {
    /// The device set of the session a device actor thread serves. One owner
    /// process serves sessions with different sets, so the set travels with
    /// the session rather than the process environment.
    static BOUND: std::cell::RefCell<Option<Option<PathBuf>>> = const { std::cell::RefCell::new(None) };
}

/// Binds this thread to a session's device set until the guard drops.
pub(crate) struct Binding(Option<Option<PathBuf>>);
impl Drop for Binding {
    fn drop(&mut self) {
        let previous = self.0.take();
        BOUND.with(|bound| *bound.borrow_mut() = previous);
    }
}

impl DeviceSet {
    /// Native workers must receive the actor's bound set, independently of the
    /// shared owner's process environment or another session's binding.
    pub(crate) fn apply_command(&self, command: &mut tokio::process::Command) {
        if let Some(path) = &self.path {
            command.env("TESTRUN_SIMULATOR_DEVICE_SET", path);
        } else {
            command.env_remove("TESTRUN_SIMULATOR_DEVICE_SET");
        }
    }
    /// The same binding for workers started with an explicit environment.
    pub(crate) fn apply_environment(
        &self,
        environment: &mut std::collections::BTreeMap<OsString, OsString>,
    ) {
        environment.remove(OsStr::new("TESTRUN_SIMULATOR_DEVICE_SET"));
        if let Some(path) = &self.path {
            environment.insert("TESTRUN_SIMULATOR_DEVICE_SET".into(), path.into());
        }
    }
    /// Make `path` (or the default set) this thread's device set. A device
    /// actor's current-thread runtime runs all of its async work here.
    pub(crate) fn bind(path: Option<PathBuf>) -> Binding {
        Binding(BOUND.with(|bound| bound.borrow_mut().replace(path)))
    }
    /// The process's own setting, ignoring any thread binding: what a client
    /// sends as its session's device set.
    pub(crate) fn configured() -> Option<PathBuf> {
        std::env::var_os("TESTRUN_SIMULATOR_DEVICE_SET").map(PathBuf::from)
    }
    pub(crate) fn current() -> Result<Self> {
        if let Some(bound) = BOUND.with(|bound| bound.borrow().clone()) {
            return bound.map_or_else(
                || {
                    Ok(Self {
                        path: None,
                        identity: "default".into(),
                    })
                },
                Self::at,
            );
        }
        let Some(path) = std::env::var_os("TESTRUN_SIMULATOR_DEVICE_SET") else {
            return Ok(Self {
                path: None,
                identity: "default".into(),
            });
        };
        Self::at(PathBuf::from(path))
    }
    pub(crate) fn at(path: PathBuf) -> Result<Self> {
        if !path.is_absolute() {
            return Err(Error::invalid(
                "TESTRUN_SIMULATOR_DEVICE_SET must be an absolute existing directory",
            ));
        }
        let path = path.canonicalize().map_err(|_| {
            Error::new(
                "DEVICE_SET_UNAVAILABLE",
                "Selected simulator device set is unavailable",
            )
        })?;
        if !path.is_dir()
            || path
                .to_str()
                .is_none_or(|value| value.chars().any(char::is_control))
        {
            return Err(Error::invalid(
                "Simulator device set must be a UTF-8 directory",
            ));
        }
        let default = std::env::var_os("HOME")
            .map(PathBuf::from)
            .and_then(|home| {
                home.join("Library/Developer/CoreSimulator/Devices")
                    .canonicalize()
                    .ok()
            });
        if default.as_ref() == Some(&path) {
            return Ok(Self {
                path: None,
                identity: "default".into(),
            });
        }
        #[cfg(unix)]
        let anchor = {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(&path).map_err(|_| {
                Error::new(
                    "DEVICE_SET_UNAVAILABLE",
                    "Cannot inspect device set identity",
                )
            })?;
            format!("{}:{}:{}", path.display(), metadata.dev(), metadata.ino())
        };
        #[cfg(not(unix))]
        let anchor = path.display().to_string();
        Ok(Self {
            identity: format!("{:x}", Sha256::digest(anchor.as_bytes())),
            path: Some(path),
        })
    }
    pub(crate) fn target_id(&self, uuid: &str) -> String {
        match self.path {
            Some(_) => format!("simulator:{}:{uuid}", self.identity),
            None => format!("simulator:{uuid}"),
        }
    }
    pub(crate) fn uuid<'a>(&self, target: &'a str) -> Result<&'a str> {
        let id = target
            .strip_prefix("simulator:")
            .ok_or_else(|| Error::invalid("Choose an explicit simulator target"))?;
        let uuid = if self.path.is_some() {
            id.strip_prefix(&format!("{}:", self.identity))
                .ok_or_else(|| {
                    Error::new(
                        "TARGET_KIND_MISMATCH",
                        "Target belongs to a different simulator device set",
                    )
                })?
        } else {
            id
        };
        if uuid.is_empty()
            || uuid.contains(':')
            || uuid.starts_with('-')
            || uuid.chars().any(char::is_control)
        {
            return Err(Error::invalid("Invalid selected simulator identity"));
        }
        Ok(uuid)
    }
    pub(crate) fn arguments(&self, args: &[&str]) -> Vec<String> {
        let mut result = vec!["simctl".into()];
        if let Some(path) = &self.path {
            result.extend(["--set".into(), path.to_str().unwrap().into()]);
        }
        result.extend(args.iter().map(|value| (*value).to_owned()));
        result
    }
    pub(crate) async fn run(
        &self,
        args: &[&str],
        timeout: Duration,
        limit: usize,
    ) -> Result<crate::process::Output> {
        let args = self.arguments(args);
        crate::process::run(
            executable(),
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            timeout,
            limit,
        )
        .await
    }
}
pub(crate) fn executable() -> PathBuf {
    std::env::var_os("TESTRUN_XCRUN").map_or_else(|| "/usr/bin/xcrun".into(), PathBuf::from)
}
pub(crate) fn validate_target(target: &crate::targets::Target) -> Result<()> {
    let set = DeviceSet::current()?;
    if set.uuid(&target.identity.target_id)? != target.platform_id {
        return Err(Error::new(
            "STALE_TARGET",
            "Simulator device-set binding changed",
        ));
    }
    Ok(())
}
pub(crate) async fn run(
    args: &[&str],
    timeout: Duration,
    limit: usize,
) -> Result<crate::process::Output> {
    DeviceSet::current()?.run(args, timeout, limit).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn set_identity_is_canonical_and_cannot_route_another_sets_target() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("Acme first");
        let second = root.path().join("Acme second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        {
            // A thread binding overrides the process setting until it drops.
            let _bound = DeviceSet::bind(Some(first.clone()));
            assert_eq!(
                DeviceSet::current().unwrap().path,
                Some(first.canonicalize().unwrap())
            );
            let _default = DeviceSet::bind(None);
            assert!(DeviceSet::current().unwrap().path.is_none());
        }
        let one = DeviceSet::at(first.clone()).unwrap();
        let two = DeviceSet::at(second).unwrap();
        let target = one.target_id("Acme-UUID");
        assert_eq!(one.uuid(&target).unwrap(), "Acme-UUID");
        assert!(two.uuid(&target).is_err());
        assert!(one.uuid("simulator:Acme-UUID").is_err());
        #[cfg(unix)]
        {
            let alias = root.path().join("alias");
            std::os::unix::fs::symlink(first, &alias).unwrap();
            assert_eq!(DeviceSet::at(alias).unwrap().target_id("Acme-UUID"), target);
        }
    }
    #[test]
    fn worker_environment_carries_only_the_bound_set() {
        let root = tempfile::tempdir().unwrap();
        let key = OsString::from("TESTRUN_SIMULATOR_DEVICE_SET");
        let mut stale = std::collections::BTreeMap::from([(
            key.clone(),
            OsString::from("/acme/other-session"),
        )]);
        // Unrelated variables pass through even when they are not UTF-8.
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            stale.insert("ACME_RAW".into(), OsString::from_vec(vec![b'a', 0xff]));
        }
        let mut environment = stale.clone();
        DeviceSet::at(root.path().to_owned())
            .unwrap()
            .apply_environment(&mut environment);
        assert_eq!(
            environment[&key],
            root.path().canonicalize().unwrap().into_os_string()
        );
        let mut environment = stale.clone();
        let _default = DeviceSet::bind(None);
        DeviceSet::current()
            .unwrap()
            .apply_environment(&mut environment);
        assert!(!environment.contains_key(&key));
        stale.remove(&key);
        assert_eq!(environment, stale);
    }
}
