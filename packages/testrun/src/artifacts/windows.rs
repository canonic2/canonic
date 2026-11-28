//! Windows artifact publication and owned-tree cleanup.
use std::{io, os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

/// Remove an owned tree after process teardown, allowing Windows to release file locks.
pub(crate) async fn remove_tree(path: &Path, deadline: tokio::time::Instant) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{
        ERROR_DIR_NOT_EMPTY, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION,
    };
    loop {
        match tokio::fs::remove_dir_all(path).await {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                let code = error
                    .raw_os_error()
                    .and_then(|code| u32::try_from(code).ok());
                if !matches!(
                    code,
                    Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION | ERROR_DIR_NOT_EMPTY)
                ) {
                    return Err(error);
                }
            }
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        tokio::time::sleep(remaining.min(std::time::Duration::from_millis(25))).await;
    }
}

/// Move a flushed staging file or directory within its caller-owned cache.
pub(crate) fn publish(source: &Path, destination: &Path, replace: bool) -> io::Result<()> {
    let wide = |path: &Path| -> io::Result<Vec<u16>> {
        let parent = path
            .parent()
            .ok_or(io::ErrorKind::InvalidInput)?
            .canonicalize()?;
        let leaf = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
        Ok(parent
            .join(leaf)
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect())
    };
    let source = wide(source)?;
    let destination = wide(destination)?;
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    // SAFETY: the two NUL-terminated native paths remain live across the call.
    let result = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
