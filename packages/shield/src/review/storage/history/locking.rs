//! Same-inode history locking; confirmed controls may restore owned permissions.
use super::{Error, File, OpenOptions, Path, Store, fs, private};
use std::{thread, time::Duration};

pub(in crate::review) fn lock(store: &Store) -> Result<File, Error> {
    acquire(store, false, 500, true)
}

#[cfg(test)]
pub(in crate::review) fn lock_diagnostics(store: &Store) -> Result<File, Error> {
    acquire(store, false, 500, false)
}

/// Optional callbacks never wait behind a reader/control. Losing observation
/// evidence cannot occupy an operation's permission deadline.
pub(in crate::review) fn lock_optional(store: &Store) -> Result<File, Error> {
    acquire(store, false, 1, true)
}

/// Called only while publishing a human-confirmed history control. Never
/// replace/unlink a lock file: existing holders and contenders keep one inode.
pub(in crate::review) fn lock_control(store: &Store) -> Result<File, Error> {
    acquire(store, true, 500, true)
}

fn open(path: &Path, control: bool, create: bool) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(create)
        .truncate(false);
    private(&mut options);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            nix::fcntl::OFlag::O_NOFOLLOW.bits() | nix::fcntl::OFlag::O_NONBLOCK.bits(),
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(3); // pin against rename/delete; do not follow reparse points
        if control {
            options.access_mode(0xc006_0080);
        } // GENERIC_READ/WRITE, attributes, READ_CONTROL and WRITE_DAC
    }
    match options.open(path) {
        Ok(file) => Ok(file),
        #[cfg(windows)]
        Err(error) if control && error.kind() == std::io::ErrorKind::PermissionDenied => {
            // LockFileEx still needs an available data-access direction. Do
            // not normalize before acquiring the lease when both are denied.
            options
                .create(false)
                .read(true)
                .write(false)
                .access_mode(0x8006_0080);
            match options.open(path) {
                Ok(file) => Ok(file),
                Err(_) => Ok(options
                    .read(false)
                    .write(true)
                    .access_mode(0x4006_0080)
                    .open(path)?),
            }
        }
        #[cfg(unix)]
        Err(error) if control && error.kind() == std::io::ErrorKind::PermissionDenied => {
            use std::os::unix::fs::OpenOptionsExt;
            // flock works with either access direction. A held readable or
            // writable descriptor can restore missing owner permission bits.
            let mut options = OpenOptions::new();
            options.custom_flags(
                nix::fcntl::OFlag::O_NOFOLLOW.bits() | nix::fcntl::OFlag::O_NONBLOCK.bits(),
            );
            options.read(true);
            match options.open(path) {
                Ok(file) => Ok(file),
                Err(_) => Ok(options.read(false).write(true).open(path)?),
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn acquire(store: &Store, control: bool, attempts: usize, create: bool) -> Result<File, Error> {
    crate::diagnostics::trace::count("history.lock_attempts", 1);
    let path = store.directory.join("history.lock");
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err(Error::Invalid("history lock must be a regular file"));
    }
    let mut file = open(&path, control, create)?;
    checked(&path, &file, !control)?;
    for _ in 0..attempts {
        match file.try_lock() {
            Ok(()) => {
                checked(&path, &file, !control)?;
                if control {
                    normalize(&mut file)?;
                    checked(&path, &file, true)?;
                }
                return Ok(file);
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                crate::diagnostics::trace::count("history.lock_waits", 1);
                if attempts > 1 {
                    thread::sleep(Duration::from_millis(1));
                }
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
    Err(Error::Locked)
}

fn checked(path: &Path, file: &File, private: bool) -> Result<(), Error> {
    let current = fs::symlink_metadata(path)?;
    let held = file.metadata()?;
    if !current.is_file() || current.file_type().is_symlink() || !held.is_file() {
        return Err(Error::Invalid("history lock must be a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let parent = fs::metadata(path.parent().ok_or(Error::Invalid("history lock parent"))?)?;
        if (current.dev(), current.ino()) != (held.dev(), held.ino())
            || held.nlink() != 1
            || held.uid() != parent.uid()
            || (private && held.mode() & 0o077 != 0)
        {
            return Err(Error::Invalid(
                "history lock ownership, permissions, links or identity changed",
            ));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let a = winapi_util::file::information(file)?;
        let other = crate::windows_security::inspect_regular(path)?;
        let b = winapi_util::file::information(&other)?;
        if held.file_attributes() & 0x0400 != 0
            || a.number_of_links() != 1
            || (a.volume_serial_number(), a.file_index())
                != (b.volume_serial_number(), b.file_index())
        {
            return Err(Error::Invalid(
                "history lock ownership, permissions, links or identity changed",
            ));
        }
        crate::windows_security::check_owned_file(file)?;
        if private {
            crate::windows_security::check_private_file(file)?;
        }
    }
    Ok(())
}

fn normalize(file: &mut File) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if file.metadata()?.permissions().mode() & 0o7777 != 0o600 {
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
    }
    #[cfg(windows)]
    if crate::windows_security::check_private_file(file).is_err() {
        crate::windows_security::make_private_file(file)?;
    }
    Ok(())
}
