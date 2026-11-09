//! Checked private files and atomic publication, bounded before allocation.
use super::{CAPTURE_BYTES, Error, META_BYTES, Path, RESULT_BYTES, Value, storage};
use std::fs;
pub(super) use storage::private_files::{directory, read, write};

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
