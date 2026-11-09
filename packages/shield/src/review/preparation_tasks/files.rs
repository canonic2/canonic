//! Checked private files and atomic publication, bounded before allocation.
use super::{CAPTURE_BYTES, Error, META_BYTES, Path, RESULT_BYTES, Value, storage};
use std::{
    fs::{self, OpenOptions},
    io::{Read as _, Write as _},
};

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

pub(super) fn directory(path: &Path, create: bool) -> Result<(), Error> {
    if create && !path.exists() {
        // Publish Unix privacy with the directory itself, so a concurrent
        // creator never observes the default mode before a later chmod.
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            builder.mode(0o700);
        }
        // Windows children inherit the checked parent's private DACL. Verify
        // both sides rather than normalizing an already published directory.
        #[cfg(windows)]
        crate::windows_security::check_private(
            path.parent().ok_or(Error::Invalid("task parent"))?,
        )?;
        match builder.create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Invalid(
            "task directory must be private and contained",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let parent = fs::metadata(
            path.parent()
                .ok_or(Error::Invalid("task directory parent"))?,
        )?;
        if metadata.uid() != parent.uid() || metadata.mode() & 0o077 != 0 {
            return Err(Error::Invalid("task directory owner or privacy differs"));
        }
    }
    #[cfg(windows)]
    crate::windows_security::check_private(path)?;
    Ok(())
}

pub(super) fn read(path: &Path, limit: usize) -> Result<Value, Error> {
    let file = storage::objects::read_file(path)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(crate::policy::Error::Limit.into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(crate::policy::Error::Limit.into());
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("preparation file invalid"))
}

pub(super) fn write(path: &Path, value: &Value, limit: usize) -> Result<(), Error> {
    if !crate::json::fits(value, limit) {
        return Err(crate::policy::Error::Limit.into());
    }
    match fs::symlink_metadata(path) {
        Ok(_) => {
            storage::objects::read_file(path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Invalid("preparation encoding"))?;
    let temporary = path.with_extension("pending");
    // The namespace or worker lease grants exclusive ownership of this temporary.
    match fs::symlink_metadata(&temporary) {
        Ok(_) => {
            storage::objects::read_file(&temporary)?;
            fs::remove_file(&temporary)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    storage::private(&mut options);
    let mut file = options.open(&temporary)?;
    #[cfg(windows)]
    crate::windows_security::make_private(&temporary)?;
    let result = (|| -> Result<(), Error> {
        file.write_all(&bytes)?;
        storage::sync(&file)?;
        drop(file);
        storage::replace(&temporary, path)?;
        #[cfg(unix)]
        storage::sync(&fs::File::open(
            path.parent().ok_or(Error::Invalid("task parent"))?,
        )?)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
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
