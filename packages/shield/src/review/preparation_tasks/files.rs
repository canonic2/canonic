//! Checked private files and atomic publication, bounded before allocation.
use super::{CAPTURE_BYTES, Error, META_BYTES, Path, RESULT_BYTES, Value, storage};
use std::fs;
pub(super) use storage::private_files::{directory, read, write};

/// Only actual task writers call this for their known namespace/record folders.
/// Unix changes the checked directory inode, never a swapped pathname target.
pub(super) fn writer_directory(path: &Path, create: bool) -> Result<(), Error> {
    let parent = path
        .parent()
        .ok_or(Error::Invalid("task directory parent"))?;
    directory(parent, false)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
            return directory(path, true);
        }
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Invalid("task directory cannot be a link or file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        if metadata.uid() != fs::symlink_metadata(parent)?.uid() {
            return Err(Error::Invalid("task directory owner differs"));
        }
        let held = fs::File::open(path)?;
        let actual = held.metadata()?;
        if !actual.is_dir() || (actual.dev(), actual.ino()) != (metadata.dev(), metadata.ino()) {
            return Err(Error::Invalid("task directory identity changed"));
        }
        if actual.mode() & 0o077 != 0 {
            held.set_permissions(fs::Permissions::from_mode(0o700))?;
        }
        let named = fs::symlink_metadata(path)?;
        if named.file_type().is_symlink()
            || (named.dev(), named.ino()) != (actual.dev(), actual.ino())
        {
            return Err(Error::Invalid("task directory identity changed"));
        }
    }
    #[cfg(windows)]
    crate::windows_security::make_private(path)?;
    directory(path, false)
}

pub(super) fn digest(value: &Value) -> Result<String, Error> {
    crate::json::fingerprint(value).map_err(|_| Error::Invalid("preparation digest encoding"))
}

pub(super) fn inventory(directory: &Path) -> Result<u64, Error> {
    let mut bytes = 0;
    for (index, entry) in fs::read_dir(directory)?.take(74).enumerate() {
        if index >= 73 {
            return Err(Error::Invalid("preparation directory exceeds scan bound"));
        }
        let entry = entry?;
        let name = entry.file_name();
        let limit = match name.to_str() {
            Some(".objects.lock") => 0,
            Some("record.json" | "record.pending") => META_BYTES,
            Some("input.json" | "input.pending") => 524_288,
            Some("capture.json" | "capture.pending") => CAPTURE_BYTES,
            Some("result.json" | "result.pending") => RESULT_BYTES,
            _ => {
                bytes += fs::symlink_metadata(entry.path())?.len();
                continue;
            }
        };
        let file = match storage::objects::read_file(&entry.path()) {
            Ok(file) => file,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let size = file.metadata()?.len();
        if size > limit as u64 {
            return Err(Error::Invalid("preparation file exceeds bound"));
        }
        bytes += size;
    }
    Ok(bytes)
}

pub(super) fn remove(directory: &Path) -> Result<(), Error> {
    let entries = fs::read_dir(directory)?
        .take(10)
        .collect::<Result<Vec<_>, _>>()?;
    if entries.len() > 9 {
        return Err(Error::Invalid("preparation directory exceeds its bound"));
    }
    for entry in &entries {
        let name = entry.file_name();
        if ![
            ".objects.lock",
            "record.json",
            "input.json",
            "capture.json",
            "result.json",
            "record.pending",
            "input.pending",
            "capture.pending",
            "result.pending",
        ]
        .iter()
        .any(|known| name == *known)
        {
            return Err(Error::Invalid("unknown preparation file"));
        }
        storage::objects::read_file(&entry.path())?;
    }
    for entry in entries {
        storage::objects::read_file(&entry.path())?;
        fs::remove_file(entry.path())?;
    }
    fs::remove_dir(directory)?;
    Ok(())
}
