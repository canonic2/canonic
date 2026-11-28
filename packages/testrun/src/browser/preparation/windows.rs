//! Windows system-directory discovery and browser preparation locks.
use crate::error::{Error, Result};
use std::{
    ffi::OsString,
    fs::{File, OpenOptions, TryLockError},
    os::windows::{ffi::OsStringExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE},
    System::SystemInformation::GetSystemDirectoryW,
};

pub(in crate::browser) fn system_directory() -> Result<PathBuf> {
    let mut buffer = [0u16; 32768];
    // SAFETY: the buffer is writable for exactly the supplied number of units.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(Error::new(
            "HOST_IO",
            "Cannot locate the Windows system directory",
        ));
    }
    Ok(OsString::from_wide(&buffer[..length]).into())
}

pub(super) fn lock(root: &Path, id: &str) -> Result<File> {
    let unavailable = || Error::new("BUNDLE_IO", "Cannot lock the private browser cache");
    let mut options = OpenOptions::new();
    crate::artifacts::no_follow(&mut options);
    let file = options
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(root.join(format!(".{id}.lock")))
        .map_err(|_| unavailable())?;
    let metadata = file.metadata().map_err(|_| unavailable())?;
    if !metadata.is_file() || crate::artifacts::is_reparse_point(&metadata) {
        return Err(unavailable());
    }
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => {
            let mut error = Error::new("TARGET_BUSY", "Browser preparation is already running");
            error.retryable = true;
            Err(error)
        }
        Err(TryLockError::Error(_)) => Err(unavailable()),
    }
}
