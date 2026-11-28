//! Selected credential state is copied from an owned private regular file.
use super::MAX_BYTES;
use crate::browser::session::Error;
use std::{io::Read, path::Path};

pub(crate) fn read_private(path: &Path) -> Result<Vec<u8>, Error> {
    if !path.is_absolute() {
        return Err("PRIVATE_STATE_REQUIRED".into());
    }
    crate::artifacts::check_private_directory(path.parent().ok_or("PRIVATE_STATE_REQUIRED")?)
        .map_err(|_| Error::from("PRIVATE_STATE_REQUIRED"))?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let file = options
        .open(path)
        .map_err(|_| Error::from("PRIVATE_STATE_REQUIRED"))?;
    let metadata = file
        .metadata()
        .map_err(|_| Error::from("PRIVATE_STATE_REQUIRED"))?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES as u64 {
        return Err("STORAGE_BOUNDS".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0
            || metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err("PRIVATE_STATE_REQUIRED".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::from("STORAGE_IO"))?;
    if bytes.len() > MAX_BYTES {
        return Err("STORAGE_BOUNDS".into());
    }
    Ok(bytes)
}
