//! Kernel publication ownership, linked at the old marker name so older
//! create-new writers cannot race generation writers.
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Read, Seek, Write},
    path::Path,
};
const MARKER: &[u8] = b"registry-kit-kernel-lease-1\n";

fn refused() -> io::Error {
    io::Error::other("registry lease identity or ownership changed")
}

fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .access_mode(0xc006_0080)
            .custom_flags(0x0020_0000)
            .share_mode(3);
    }
    options
}

fn identity(path: &Path, file: &File) -> io::Result<()> {
    let a = fs::symlink_metadata(path)?;
    let b = file.metadata()?;
    if !a.is_file() || a.file_type().is_symlink() || !b.is_file() {
        return Err(refused());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if (a.dev(), a.ino()) != (b.dev(), b.ino())
            || b.uid() != fs::metadata(parent)?.uid()
            || b.nlink() > 2
        {
            return Err(refused());
        }
    }
    #[cfg(windows)]
    if !super::windows::same_file(path, file)? {
        return Err(refused());
    }
    Ok(())
}

pub(crate) fn acquire(path: &Path, legacy: &Path) -> io::Result<File> {
    let mut file = options().open(path)?;
    identity(path, &file)?;
    file.try_lock().map_err(|error| match error {
        TryLockError::WouldBlock => io::Error::from(io::ErrorKind::WouldBlock),
        TryLockError::Error(error) => error,
    })?;
    identity(path, &file)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(windows)]
    super::windows::private(&mut file)?;
    let mut marker = Vec::new();
    (&mut file).take(128).read_to_end(&mut marker)?;
    if marker != MARKER {
        // Never adopt a different old marker whose owner cannot be verified.
        if legacy.exists() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        file.rewind()?;
        file.set_len(0)?;
        file.write_all(MARKER)?;
        super::sync(&file)?;
    }
    match fs::hard_link(path, legacy) {
        Ok(()) => super::sync_directory(legacy)?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    // The legacy name must be a link to this locked file. Windows denies reads
    // of a locked range through any other handle, so identity compares native
    // file identities and the marker was read through the locked handle.
    identity(legacy, &file).map_err(|_| io::Error::from(io::ErrorKind::WouldBlock))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::{TempDir, prelude::*};

    #[test]
    fn leases_publish_initially_are_reused_and_exclude_concurrent_owners() {
        let root = TempDir::new().unwrap();
        let (path, legacy) = (
            root.path().join("acme.lease"),
            root.path().join("acme.lock"),
        );
        let held = acquire(&path, &legacy).unwrap();
        assert_eq!(
            acquire(&path, &legacy).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(held);
        drop(acquire(&path, &legacy).unwrap());
        assert_eq!(fs::read(&legacy).unwrap(), MARKER);
    }

    #[test]
    fn leases_refuse_a_legacy_marker_that_is_a_different_file() {
        let root = TempDir::new().unwrap();
        let (path, legacy) = (
            root.path().join("acme.lease"),
            root.path().join("acme.lock"),
        );
        drop(acquire(&path, &legacy).unwrap());
        fs::remove_file(&legacy).unwrap();
        root.child("acme.lock").write_binary(MARKER).unwrap();
        assert_eq!(
            acquire(&path, &legacy).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(fs::read(&legacy).unwrap(), MARKER);
    }
}
