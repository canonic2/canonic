//! An explicitly retained browser data directory, held until native shutdown.

use super::{contract::Engine, preparation};
use crate::{
    artifacts,
    error::{Error, Result},
    lease::ControlLease,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MARKER: &str = ".testrun-profile-v1.json";

#[cfg(windows)]
mod windows;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Selection {
    Disposable,
    Retained { name: String },
    Borrowed { path: PathBuf },
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity<'a> {
    pub engine: Engine,
    pub channel: Option<preparation::Channel>,
    pub version: &'a str,
}

pub(super) struct Profile {
    path: PathBuf,
    summary: Value,
    retained: bool,
    _lease: ControlLease,
}

impl Selection {
    pub(super) fn validate(&self) -> std::result::Result<(), &'static str> {
        match self {
            Self::Disposable => Ok(()),
            #[cfg(not(any(unix, windows)))]
            Self::Retained { .. } | Self::Borrowed { .. } => Err("PROFILE_HOST_UNSUPPORTED"),
            #[cfg(any(unix, windows))]
            Self::Retained { name }
                if !name.is_empty()
                    && name.len() <= 64
                    && name.as_bytes()[0].is_ascii_alphanumeric()
                    && name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                    })
                    && native_profile_name(name) =>
            {
                Ok(())
            }
            #[cfg(any(unix, windows))]
            Self::Borrowed { path }
                if path.is_absolute()
                    && path
                        .to_str()
                        .is_some_and(|path| path.len() <= 4096 && !path.contains('\0'))
                    && native_profile_path(path) =>
            {
                Ok(())
            }
            #[cfg(any(unix, windows))]
            _ => Err("PROFILE_SELECTION_INVALID"),
        }
    }

    pub(super) fn acquire(&self, identity: &Identity<'_>) -> Result<Profile> {
        self.validate().map_err(|code| {
            Error::new(
                code,
                "Use a valid retained profile name or absolute borrowed path",
            )
        })?;
        let (path, summary, retained) = match self {
            Self::Disposable => {
                return Err(Error::invalid(
                    "Disposable profiles are allocated by the Chrome session",
                ));
            }
            Self::Retained { name } => {
                let root = preparation::root()?.join("profiles");
                let path = root.join(name);
                preparation::private_tree(&path)?;
                (path, json!({"kind":"retained","name":name}), true)
            }
            Self::Borrowed { path } => (path.clone(), json!({"kind":"borrowed"}), false),
        };
        let (path, lease) = lock(&path)?;
        check_closed(&path, identity.engine)?;
        let marker = path.join(MARKER);
        if retained
            && matches!(fs::symlink_metadata(&marker), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        {
            if fs::read_dir(&path)
                .map_err(|_| profile_io())?
                .next()
                .is_some()
            {
                return Err(incompatible());
            }
            artifacts::write(
                &marker,
                &serde_json::to_vec(identity).map_err(|_| profile_io())?,
            )?;
        } else {
            let bytes = read_marker(&marker)?;
            let actual: Identity<'_> =
                serde_json::from_slice(&bytes).map_err(|_| incompatible())?;
            if &actual != identity {
                return Err(incompatible());
            }
        }
        Ok(Profile {
            path,
            summary,
            retained,
            _lease: lease,
        })
    }
}

impl Profile {
    pub(super) fn is_retained(&self) -> bool {
        self.retained
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn summary(&self) -> &Value {
        &self.summary
    }
}

#[cfg(unix)]
fn native_profile_name(_: &str) -> bool {
    true
}

#[cfg(unix)]
fn native_profile_path(_: &Path) -> bool {
    true
}

#[cfg(unix)]
fn lock(path: &Path) -> Result<(PathBuf, ControlLease)> {
    use std::os::unix::fs::MetadataExt;
    artifacts::check_private_directory(path)?;
    let path = fs::canonicalize(path).map_err(|_| profile_io())?;
    let metadata = fs::metadata(&path).map_err(|_| profile_io())?;
    let (volume, object) = (metadata.dev(), metadata.ino());
    let lease = ControlLease::acquire(&format!("browser-profile:{volume}:{object}"))
        .map_err(lease_error)?;
    Ok((path, lease))
}

#[cfg(windows)]
use windows::{check_closed, lock, native_profile_name, native_profile_path, read_marker};

fn lease_error(error: Error) -> Error {
    if error.code == "TARGET_BUSY" {
        Error::new(
            "PROFILE_BUSY",
            "Another Testrun connection holds this profile",
        )
    } else {
        profile_io()
    }
}

#[cfg(unix)]
fn read_marker(path: &Path) -> Result<Vec<u8>> {
    artifacts::read_private_file(path, 1024).map_err(|_| incompatible())
}

#[cfg(unix)]
fn check_closed(path: &Path, engine: Engine) -> Result<()> {
    let markers: &[&str] = match engine {
        Engine::Chrome | Engine::Chromium => &["SingletonLock", "SingletonSocket"],
        Engine::Firefox if cfg!(target_os = "linux") => &["parent.lock"],
        Engine::Firefox => &["parent.lock", "lock"],
        Engine::Webkit => &[],
    };
    for marker in markers {
        match fs::symlink_metadata(path.join(marker)) {
            Ok(_) => {
                return Err(native_lock_busy());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(profile_io()),
        }
    }
    if engine == Engine::Firefox {
        let advisory_unlocked = check_firefox_lock(&path.join(".parentlock"))?;
        if cfg!(target_os = "linux") {
            check_firefox_compatibility_link(&path.join("lock"), advisory_unlocked)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn check_firefox_compatibility_link(path: &Path, advisory_unlocked: bool) -> Result<()> {
    let target = match fs::read_link(path) {
        Ok(target) => target,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            return Err(native_lock_busy());
        }
        Err(_) => return Err(profile_io()),
    };
    // Firefox's IPv4:+pid link is a compatibility marker for an advisory
    // .parentlock. A plain pid denotes a standalone lock. Leave either link
    // untouched so Firefox can acquire the real lock itself at startup.
    let compatibility_link = target
        .to_str()
        .and_then(|target| target.split_once(":+"))
        .is_some_and(|(address, pid)| {
            address
                .parse::<std::net::Ipv4Addr>()
                .is_ok_and(|address| address != std::net::Ipv4Addr::BROADCAST)
                && pid.bytes().all(|digit| digit.is_ascii_digit())
                && pid.parse::<i32>().is_ok_and(|pid| pid > 0)
        });
    if !advisory_unlocked || !compatibility_link {
        return Err(native_lock_busy());
    }
    Ok(())
}

#[cfg(unix)]
fn check_firefox_lock(path: &Path) -> Result<bool> {
    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(native_lock_busy()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(profile_io()),
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| profile_io())?;
    // SAFETY: zero initializes the native flock fields (which vary by Unix);
    // the live fd and writable lock pointer are used only for this query.
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = libc::F_WRLCK as _;
    lock.l_whence = libc::SEEK_SET as _;
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &mut lock) } < 0 {
        return Err(profile_io());
    }
    if i64::from(lock.l_type) != i64::from(libc::F_UNLCK) {
        return Err(native_lock_busy());
    }
    Ok(true)
}

fn native_lock_busy() -> Error {
    Error::new(
        "PROFILE_BUSY",
        "A browser lock is present in the selected profile",
    )
}

fn incompatible() -> Error {
    Error::new(
        "PROFILE_INCOMPATIBLE",
        "The private profile record must match the selected engine, channel and build",
    )
}

fn profile_io() -> Error {
    Error::new("PROFILE_IO", "Cannot acquire the selected browser profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_profile_lock_and_build_checks_preserve_data() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("profile");
        preparation::private_tree(&root).unwrap();
        let borrowed = Selection::Borrowed { path: root.clone() };
        let identity = Identity {
            engine: Engine::Chrome,
            channel: None,
            version: "156.0.8078.47",
        };
        let refused = |selection: &Selection, identity: &Identity<'_>| {
            selection
                .acquire(identity)
                .err()
                .expect("Profile refused")
                .code
        };
        assert_eq!(refused(&borrowed, &identity), "PROFILE_INCOMPATIBLE");
        artifacts::write(&root.join(MARKER), &serde_json::to_vec(&identity).unwrap()).unwrap();
        fs::write(root.join("fixture"), b"retained").unwrap();
        let first = borrowed.acquire(&identity).unwrap();
        assert_eq!(first.summary(), &json!({"kind":"borrowed"}));
        assert_eq!(refused(&borrowed, &identity), "PROFILE_BUSY");
        drop(first);
        assert_eq!(
            refused(
                &borrowed,
                &Identity {
                    version: "157.0.0.1",
                    ..identity
                }
            ),
            "PROFILE_INCOMPATIBLE"
        );
        assert_eq!(
            refused(
                &borrowed,
                &Identity {
                    channel: Some(preparation::Channel::Chrome),
                    ..identity
                }
            ),
            "PROFILE_INCOMPATIBLE"
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("absent", root.join("SingletonLock")).unwrap();
            assert_eq!(refused(&borrowed, &identity), "PROFILE_BUSY");
            fs::remove_file(root.join("SingletonLock")).unwrap();
        }
        let second = borrowed.acquire(&identity).unwrap();
        assert_eq!(
            fs::read(second.path().join("fixture")).unwrap(),
            b"retained"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_regular_unlocked_firefox_marker_is_preserved_and_reusable() {
        let temporary = tempfile::tempdir().unwrap();
        let marker = temporary.path().join(".parentlock");
        fs::write(&marker, b"acme").unwrap();
        check_closed(temporary.path(), Engine::Firefox).unwrap();
        assert_eq!(fs::read(marker).unwrap(), b"acme");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn firefox_compatibility_link_requires_an_unlocked_advisory_marker() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join(".parentlock");
        let link = directory.path().join("lock");
        let check = || check_closed(directory.path(), Engine::Firefox);
        std::os::unix::fs::symlink("127.0.1.1:+123", &link).unwrap();
        assert_eq!(check().unwrap_err().code, "PROFILE_BUSY");
        fs::write(marker, []).unwrap();
        check().unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), Path::new("127.0.1.1:+123"));
        for target in ["127.0.1.1:123", "127.0.1.1:++123", "foreign:+123"] {
            fs::remove_file(&link).unwrap();
            std::os::unix::fs::symlink(target, &link).unwrap();
            assert_eq!(check().unwrap_err().code, "PROFILE_BUSY");
        }
    }
}
