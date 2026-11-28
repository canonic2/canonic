//! Kernel-owned controller exclusion across MCP processes and direct CLI invocations.
use crate::error::{Error, Result};
#[cfg(unix)]
use sha2::{Digest, Sha256};
use std::fs::File;
#[cfg(unix)]
use std::path::Path;

pub struct ControlLease {
    _file: File,
}
#[cfg(unix)]
impl Drop for ControlLease {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: this live descriptor owns the acquired lock. Explicit unlock
        // releases this controller even while a forked child still holds an
        // inherited descriptor before close-on-exec takes effect.
        unsafe {
            libc::flock(self._file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
impl ControlLease {
    /// Browser ownership spans independent local manager roots.
    pub(crate) fn acquire_browser(target: &str) -> Result<Self> {
        #[cfg(unix)]
        return Self::acquire(target);
        #[cfg(windows)]
        {
            use canonic_process_kit::endpoint;
            use sha2::{Digest, Sha256};
            let unavailable = || Error::new("LEASE_IO", "Cannot open private browser control lock");
            let path = dirs::data_local_dir()
                .ok_or_else(unavailable)?
                .join("Testrun/browser-control")
                .join(format!("{:x}.lock", Sha256::digest(target.as_bytes())));
            let file = endpoint::lock_private_file(&path).map_err(|error| {
                if matches!(error, canonic_process_kit::Error::AlreadyRunning) {
                    let mut error = Error::new(
                        "TARGET_BUSY",
                        "Another Testrun connection holds browser control",
                    ).details(serde_json::json!({
                        "blockingState":"local_process_lease","lockPath":path,
                        "recoveryAction":"Close the owning session or wait for its cleanup. Keep the lock file in place."
                    }));
                    error.retryable = true;
                    error
                } else {
                    unavailable()
                }
            })?;
            Ok(Self { _file: file })
        }
        #[cfg(not(any(unix, windows)))]
        Self::acquire(target)
    }

    #[cfg(not(unix))]
    pub(crate) fn inspect(_target: &str) -> Result<Option<(bool, serde_json::Value)>> {
        Err(Error::new(
            "UNSUPPORTED_HOST",
            "Managed iOS control requires a Unix host",
        ))
    }
    pub(crate) fn publish(&mut self, mut value: serde_json::Value) -> Result<()> {
        use std::io::{Seek, Write};
        value["pid"] = serde_json::json!(std::process::id());
        self._file
            .rewind()
            .and_then(|()| self._file.set_len(0))
            .and_then(|()| self._file.write_all(value.to_string().as_bytes()))
            .and_then(|()| crate::artifacts::sync(&self._file))
            .map_err(|_| Error::new("LEASE_IO", "Cannot publish controller ownership"))
    }
    #[cfg(unix)]
    pub(crate) fn inspect(target: &str) -> Result<Option<(bool, serde_json::Value)>> {
        use std::{
            io::Read,
            os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
        };
        // SAFETY: read-only identity query.
        let root = std::path::PathBuf::from("/tmp")
            .join(format!("testrun-control-{}", unsafe { libc::geteuid() }));
        if !root.exists() {
            return Ok(None);
        }
        crate::artifacts::check_private_directory(&root)?;
        let path = root.join(format!("{:x}.lock", Sha256::digest(target.as_bytes())));
        let file = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(Error::new(
                    "LEASE_IO",
                    "Cannot inspect controller ownership",
                ));
            }
        };
        // SAFETY: probe only this opened lock; never delete or replace the anchor.
        let acquired = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        if acquired {
            // SAFETY: release only the probe's lock.
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        } else if std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock {
            return Err(Error::new("LEASE_IO", "Cannot probe controller ownership"));
        }
        let mut bytes = Vec::new();
        file.take(16384)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::new("LEASE_IO", "Cannot read controller ownership"))?;
        Ok(Some((
            !acquired,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )))
    }
    #[cfg(unix)]
    pub fn acquire(target: &str) -> Result<Self> {
        // SAFETY: this read-only syscall reports our own effective user identity.
        let uid = unsafe { libc::geteuid() };
        Self::acquire_in(
            target,
            &std::path::PathBuf::from("/tmp").join(format!("testrun-control-{uid}")),
        )
    }
    #[cfg(unix)]
    fn acquire_in(target: &str, root: &Path) -> Result<Self> {
        use std::os::{
            fd::AsRawFd,
            unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        };
        match std::fs::DirBuilder::new().mode(0o700).create(root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                return Err(Error::new(
                    "LEASE_IO",
                    "Cannot create private controller lock directory",
                ));
            }
        }
        let metadata = std::fs::symlink_metadata(root)
            .map_err(|_| Error::new("LEASE_IO", "Cannot inspect controller lock directory"))?;
        // SAFETY: this read-only syscall reports our own effective user identity.
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(Error::new(
                "LEASE_IO",
                "Controller lock directory must be private and owned by this user",
            ));
        }
        let path = root.join(format!("{:x}.lock", Sha256::digest(target.as_bytes())));
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|_| Error::new("LEASE_IO", "Cannot open controller lock"))?;
        // SAFETY: the live file descriptor is owned here; a nonblocking exclusive
        // flock changes only this lock file. Closing it releases kernel authority.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = std::io::Error::last_os_error();
            let busy = error.kind() == std::io::ErrorKind::WouldBlock;
            let mut result = Error::new(
                if busy { "TARGET_BUSY" } else { "LEASE_IO" },
                "Target control is held by another Testrun process or could not be acquired",
            );
            if busy {
                use std::io::Read;
                let mut metadata = String::new();
                let _ = (&file).take(1024).read_to_string(&mut metadata);
                let owner = serde_json::from_str::<serde_json::Value>(&metadata).ok();
                result.retryable = true;
                result.details = Some(serde_json::json!({
                    "blockingState":"local_process_lease","lockPath":path,
                    "reportedOwnerPid":owner.and_then(|v|v["pid"].as_u64()),
                    "recoveryAction":"Close the owning session or wait for its cleanup; use lsof on lockPath to confirm the holder. Persistent lock files are not authority and must not be deleted."
                }));
            }
            return Err(result);
        }
        use std::io::Write;
        let owner = serde_json::json!({"pid":std::process::id()}).to_string();
        file.set_len(0)
            .and_then(|()| file.write_all(owner.as_bytes()))
            .map_err(|_| Error::new("LEASE_IO", "Cannot record controller lease diagnostics"))?;
        Ok(Self { _file: file })
    }
    #[cfg(not(unix))]
    pub fn acquire(_target: &str) -> Result<Self> {
        Err(Error::new(
            "UNSUPPORTED_HOST",
            "Cross-process controller leases require a Unix host",
        ))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn controllers_exclude_other_process_handles_and_release_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("private-locks");
        let first = ControlLease::acquire_in("simulator:Acme", &root).unwrap();
        assert_eq!(
            ControlLease::acquire_in("simulator:Acme", &root)
                .err()
                .unwrap()
                .code,
            "TARGET_BUSY"
        );
        let other = ControlLease::acquire_in("simulator:Other", &root).unwrap();
        drop(first);
        ControlLease::acquire_in("simulator:Acme", &root).unwrap();
        drop(other);
    }
    #[test]
    fn busy_diagnostics_are_advisory_and_stale_files_allow_reacquisition() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("locks");
        let owner = ControlLease::acquire_in("device:Acme", &root).unwrap();
        let busy = ControlLease::acquire_in("device:Acme", &root)
            .err()
            .unwrap();
        assert!(busy.retryable);
        let details = busy.details.unwrap();
        assert_eq!(details["reportedOwnerPid"], std::process::id());
        assert_eq!(details["blockingState"], "local_process_lease");
        drop(owner);
        assert!(Path::new(details["lockPath"].as_str().unwrap()).exists());
        ControlLease::acquire_in("device:Acme", &root).unwrap();
    }
    #[test]
    fn process_lease_fixture() {
        let Some(root) = std::env::var_os("TESTRUN_LEASE_TEST_ROOT") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let _control = ControlLease::acquire_in("device:Acme", &root).unwrap();
        let _media = ControlLease::acquire_in("device-media:Acme", &root).unwrap();
        let _endpoint = std::os::unix::net::UnixListener::bind(root.join("control.sock")).unwrap();
        std::fs::write(root.join("ready"), b"ready").unwrap();
        use std::io::Read;
        let _ = std::io::stdin().read_exact(&mut [0]);
    }
    #[test]
    fn abrupt_owner_termination_releases_both_leases_with_a_stale_endpoint() {
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("locks");
        let mut child = Child(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "lease::tests::process_lease_fixture",
                    "--nocapture",
                ])
                .env("TESTRUN_LEASE_TEST_ROOT", &root)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !root.join("ready").exists() {
            assert!(child.0.try_wait().unwrap().is_none());
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        for target in ["device:Acme", "device-media:Acme"] {
            assert_eq!(
                ControlLease::acquire_in(target, &root).err().unwrap().code,
                "TARGET_BUSY"
            );
        }
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(root.join("control.sock").exists());
        assert!(std::os::unix::net::UnixStream::connect(root.join("control.sock")).is_err());
        for target in ["device:Acme", "device-media:Acme"] {
            ControlLease::acquire_in(target, &root).unwrap();
        }
    }
    #[test]
    fn releasing_owner_does_not_wait_for_an_inherited_descriptor() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("private-locks");
        let owner = ControlLease::acquire_in("simulator:Acme", &root).unwrap();
        let inherited = owner._file.try_clone().unwrap();
        drop(owner);
        let replacement = ControlLease::acquire_in("simulator:Acme", &root).unwrap();
        drop(inherited);
        assert_eq!(
            ControlLease::acquire_in("simulator:Acme", &root)
                .err()
                .unwrap()
                .code,
            "TARGET_BUSY"
        );
        drop(replacement);
        ControlLease::acquire_in("simulator:Acme", &root).unwrap();
    }
}
