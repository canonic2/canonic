//! Private atomic evidence files, registration of finalized native artifacts
//! and the flush every durable Testrun write uses.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use uuid::Uuid;

#[cfg(windows)]
pub(crate) mod windows;

pub(crate) fn check_private_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| Error::new("HOST_IO", "Private runtime state is unavailable"))?;
    if !metadata.is_dir() {
        return Err(Error::new(
            "UNAUTHORIZED",
            "Runtime state must be a private directory",
        ));
    }
    #[cfg(windows)]
    if is_reparse_point(&metadata) {
        return Err(Error::new(
            "UNAUTHORIZED",
            "Runtime state must be a private directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // SAFETY: read-only identity query.
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(Error::new(
                "UNAUTHORIZED",
                "Runtime state must be private and owned by this user",
            ));
        }
    }
    Ok(())
}

/// A private output file can also be published in an owner-controlled or
/// sticky shared directory. Other writable parents could replace the staging file.
pub(crate) fn check_output_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let error = || {
            Error::new(
                "UNAUTHORIZED",
                "Evidence needs an owner-controlled output directory",
            )
        };
        let metadata = fs::symlink_metadata(path).map_err(|_| error())?;
        // SAFETY: read-only process identity query.
        let user = unsafe { libc::geteuid() };
        let mode = metadata.mode();
        if metadata.is_dir()
            && (metadata.uid() == user && mode & 0o022 == 0
                || (metadata.uid() == user || metadata.uid() == 0) && mode & 0o1000 != 0)
        {
            return Ok(());
        }
        Err(error())
    }
    #[cfg(not(unix))]
    check_private_directory(path)
}

/// Read an explicitly selected private file without following a final symlink.
pub(crate) fn read_private_file(path: &Path, max: usize) -> Result<Vec<u8>> {
    read_private_file_with(path, max, |_| Ok(()))
}

/// Verify any additional ownership requirements on the handle used for the read.
pub(crate) fn read_private_file_with(
    path: &Path,
    max: usize,
    verify: impl FnOnce(&fs::File) -> Result<()>,
) -> Result<Vec<u8>> {
    if !path.is_absolute() || !fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) {
        return Err(Error::invalid(
            "Select an absolute private regular file; symlinks/special files are refused",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    no_follow(&mut options);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|_| {
        Error::new(
            "PRIVATE_FILE_UNAVAILABLE",
            "Cannot read selected private file",
        )
    })?;
    let metadata = file
        .metadata()
        .map_err(|_| Error::invalid("Cannot inspect selected private file"))?;
    if !metadata.is_file() || metadata.len() > max as u64 {
        return Err(Error::invalid(
            "Selected private file exceeds its byte limit",
        ));
    }
    #[cfg(windows)]
    if is_reparse_point(&metadata) {
        return Err(Error::invalid(
            "Selected private file must be a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(Error::invalid(
                "Selected private file must be owned by this user without group/other access",
            ));
        }
    }
    verify(&file)?;
    let mut bytes = vec![];
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            Error::new(
                "PRIVATE_FILE_UNAVAILABLE",
                "Cannot read selected private file",
            )
        })?;
    if bytes.len() > max {
        return Err(Error::invalid(
            "Selected private file exceeds its byte limit",
        ));
    }
    Ok(bytes)
}

#[cfg(windows)]
pub(crate) fn no_follow(options: &mut OpenOptions) {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
}

#[cfg(windows)]
pub(crate) fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Flushes a durable file or directory to the device with `fsync`.
///
/// On macOS this is plain `fsync`, not the `F_FULLFSYNC` that `File::sync_all`
/// issues: that also empties the drive's write cache and waits behind every
/// other process's flush, so frequent journal and evidence writes, and
/// parallel tests, serialize on it. Elsewhere it is `File::sync_all`.
pub(crate) fn sync(file: &fs::File) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: the descriptor stays open for the borrowed file's lifetime.
        if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        file.sync_all()
    }
}

/// Flushes a directory's entries on Unix. Windows publishes through
/// write-through moves and refuses to open a directory as a file, so there
/// this does nothing.
pub(crate) fn sync_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        sync(&fs::File::open(path)?)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

pub fn directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .recursive(false)
        .create(path)
        .map_err(|_| Error::new("EVIDENCE_IO", "Could not create private evidence directory"))
}
pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    write_checked(path, bytes, |_| Ok(()))
}

#[cfg(windows)]
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    write_checked(path, bytes, |file| {
        canonic_process_kit::endpoint::check_private_child_file(file).map_err(|_| {
            Error::new(
                "EVIDENCE_IO",
                "Evidence file must be private and owned by this user",
            )
        })
    })
}

fn write_checked(
    path: &Path,
    bytes: &[u8],
    verify: impl FnOnce(&fs::File) -> Result<()>,
) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Artifact has no directory"))?;
    let temporary = parent.join(format!(".{}.partial", Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| Error::new("EVIDENCE_IO", "Could not create evidence artifact"))?;
        verify(&file)?;
        file.write_all(bytes)
            .and_then(|()| sync(&file))
            .map_err(|_| Error::new("EVIDENCE_IO", "Could not complete evidence artifact"))?;
        // Publish exclusively even if two requests choose the same artifact name.
        #[cfg(unix)]
        fs::hard_link(&temporary, path)
            .map_err(|_| Error::new("EVIDENCE_IO", "Could not publish evidence artifact"))?;
        #[cfg(windows)]
        windows::publish(&temporary, path, false)
            .map_err(|_| Error::new("EVIDENCE_IO", "Could not publish evidence artifact"))?;
        Ok(())
    })();
    #[cfg(windows)]
    if result.is_ok() {
        return result; // The move consumed the staged name.
    }
    let cleanup = fs::remove_file(&temporary);
    match (result, cleanup) {
        (Ok(()), Err(_)) => Err(Error::new(
            "CLEANUP_FAILED",
            "Could not remove temporary evidence artifact",
        )),
        (result, _) => result,
    }
}
pub fn register(temporary: &Path, final_path: &Path, mime: &str) -> Result<Value> {
    let metadata = fs::symlink_metadata(temporary)
        .map_err(|_| Error::new("EVIDENCE_IO", "Native artifact is unavailable"))?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > 512 * 1024 * 1024
    {
        return Err(Error::new(
            "INVALID_ARTIFACT",
            "Native artifact is empty, oversized or not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(temporary, fs::Permissions::from_mode(0o600)).map_err(|_| {
            Error::new("EVIDENCE_IO", "Cannot restrict native artifact permissions")
        })?;
    }
    let mut file = std::fs::File::open(temporary)
        .map_err(|_| Error::new("EVIDENCE_IO", "Cannot read native artifact"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| Error::new("EVIDENCE_IO", "Cannot hash native artifact"))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    fs::hard_link(temporary, final_path)
        .map_err(|_| Error::new("EVIDENCE_IO", "Cannot publish native artifact"))?;
    fs::remove_file(temporary)
        .map_err(|_| Error::new("CLEANUP_FAILED", "Cannot remove temporary native artifact"))?;
    Ok(
        json!({"path":final_path,"mimeType":mime,"bytes":metadata.len(),"sha256":format!("{:x}",hash.finalize())}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_never_overwrites_existing_evidence() {
        let parent = tempfile::tempdir().unwrap();
        let file = parent.path().join("artifact.json");
        write(&file, b"first").unwrap();
        assert!(write(&file, b"second").is_err());
        assert_eq!(fs::read(&file).unwrap(), b"first");
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
