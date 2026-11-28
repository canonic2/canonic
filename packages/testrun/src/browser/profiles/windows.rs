//! Windows profile privacy and native lock inspection.
use super::{incompatible, lease_error, native_lock_busy, profile_io};
use crate::{artifacts, browser::contract::Engine, error::Result, lease::ControlLease};
use canonic_process_kit::endpoint;
use std::{
    fs::OpenOptions,
    io::ErrorKind,
    os::windows::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf, Prefix},
};
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_DELETE_PENDING, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION,
};

pub(super) fn native_profile_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    !matches!(
        name.as_bytes(),
        b"CON"
            | b"PRN"
            | b"AUX"
            | b"NUL"
            | [b'C', b'O', b'M', b'1'..=b'9']
            | [b'L', b'P', b'T', b'1'..=b'9']
    )
}

pub(super) fn native_profile_path(path: &Path) -> bool {
    let mut components = path.components();
    matches!(components.next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
        && !components.any(|component| {
            component == Component::ParentDir
                || matches!(component, Component::Normal(name) if name.as_encoded_bytes().contains(&b':'))
        })
}

pub(super) fn lock(path: &Path) -> Result<(PathBuf, ControlLease)> {
    let (path, volume, object) =
        endpoint::private_directory_identity(path).map_err(|_| profile_io())?;
    let lease = ControlLease::acquire_browser(&format!("browser-profile:{volume}:{object}"))
        .map_err(lease_error)?;
    Ok((path, lease))
}

pub(super) fn read_marker(path: &Path) -> Result<Vec<u8>> {
    artifacts::read_private_file_with(path, 1024, |file| {
        endpoint::check_private_child_file(file).map_err(|_| incompatible())
    })
    .map_err(|_| incompatible())
}

pub(super) fn check_closed(path: &Path, engine: Engine) -> Result<()> {
    let name = match engine {
        Engine::Chrome | Engine::Chromium => "lockfile",
        Engine::Firefox => "parent.lock",
        Engine::Webkit => return Ok(()),
    };
    // The browser takes its native lock at startup. Probe without truncating
    // the file or changing Chrome's delete-on-close and Firefox's crash marker.
    let mut options = OpenOptions::new();
    options.read(true).share_mode(0);
    artifacts::no_follow(&mut options);
    match options.open(path.join(name)) {
        Ok(file) => endpoint::check_private_child_file(&file).map_err(|_| profile_io()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => match error
            .raw_os_error()
            .and_then(|code| u32::try_from(code).ok())
        {
            Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION | ERROR_DELETE_PENDING) => {
                Err(native_lock_busy())
            }
            // A delete-on-close Chrome file may already be pending deletion.
            Some(ERROR_ACCESS_DENIED) if name == "lockfile" => Err(native_lock_busy()),
            _ => Err(profile_io()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::profiles::Selection;
    use std::fs;
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_DELETE_ON_CLOSE, FILE_SHARE_READ};

    #[test]
    fn windows_profile_selectors_exclude_device_and_stream_paths() {
        for name in ["con", "AUX", "lpt9", "CoM1"] {
            let selection = Selection::Retained { name: name.into() };
            assert_eq!(selection.validate(), Err("PROFILE_SELECTION_INVALID"));
        }
        Selection::Retained {
            name: "acme-1".into(),
        }
        .validate()
        .unwrap();
        for path in [
            r"\\server\share\profile",
            r"\\?\C:\profile",
            r"C:\profile:stream",
            r"C:\dir\..\profile",
        ] {
            let selection = Selection::Borrowed { path: path.into() };
            assert_eq!(selection.validate(), Err("PROFILE_SELECTION_INVALID"));
        }
        Selection::Borrowed {
            path: r"C:\Acme Profile".into(),
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn windows_profile_probe_respects_chrome_and_firefox_file_sharing() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("profile");
        crate::browser::preparation::private_tree(&root).unwrap();
        let firefox = root.join("parent.lock");
        fs::write(&firefox, b"acme").unwrap();
        let active = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&firefox)
            .unwrap();
        assert_eq!(
            check_closed(&root, Engine::Firefox).unwrap_err().code,
            "PROFILE_BUSY"
        );
        drop(active);
        check_closed(&root, Engine::Firefox).unwrap();
        assert_eq!(fs::read(&firefox).unwrap(), b"acme");

        let chrome = root.join("lockfile");
        let active = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
            .open(&chrome)
            .unwrap();
        assert_eq!(
            check_closed(&root, Engine::Chrome).unwrap_err().code,
            "PROFILE_BUSY"
        );
        drop(active);
        check_closed(&root, Engine::Chrome).unwrap();
        assert!(!chrome.exists());

        let linked = root.join("linked.lock");
        fs::hard_link(&firefox, &linked).unwrap();
        assert_eq!(
            check_closed(&root, Engine::Firefox).unwrap_err().code,
            "PROFILE_IO"
        );
        assert_eq!(fs::read(&linked).unwrap(), b"acme");
    }

    #[test]
    fn windows_profile_lock_follows_directory_identity_across_spellings() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("Acme");
        crate::browser::preparation::private_tree(&root).unwrap();
        let alternate = temporary.path().join("ACME");
        let (_, lease) = lock(&root).unwrap();
        assert_eq!(lock(&alternate).err().unwrap().code, "PROFILE_BUSY");
        drop(lease);
        lock(&alternate).unwrap();

        let inherited = root.join("inherited");
        fs::create_dir(&inherited).unwrap();
        assert_eq!(lock(&inherited).err().unwrap().code, "PROFILE_IO");
    }
}
