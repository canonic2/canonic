//! Owned private regular files and directory mutexes for review storage.
use super::{Checkpoint, Error, locking::Backoff, private};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

pub(in crate::review) const FILE_LIMIT: usize = 4096;

pub(in crate::review) struct Directory {
    path: PathBuf,
    guard: File,
}

fn open(path: &Path, write: bool, create: bool) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(write)
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
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    checked(path, &file)?;
    Ok(file)
}
pub(in crate::review) fn read_file(path: &Path) -> Result<File, Error> {
    open(path, false, false)
}

/// Acquire a private owner lease while the caller holds the namespace writer.
pub(in crate::review) fn own_file(path: &Path) -> Result<File, Error> {
    let file = open(path, true, true)?;
    match file.try_lock() {
        Ok(()) => checked(path, &file)?,
        Err(fs::TryLockError::WouldBlock) => return Err(Error::Locked),
        Err(fs::TryLockError::Error(error)) => return Err(error.into()),
    }
    Ok(file)
}

pub(in crate::review) fn initialize_mutex(path: &Path) -> Result<(), Error> {
    open(&path.join(".objects.lock"), true, true)?;
    Ok(())
}

/// Share the directory's mutex. Callers without a checkpoint can choose to try once.
pub(in crate::review) fn read_lease(
    path: &Path,
    checkpoint: Option<Checkpoint<'_>>,
) -> Result<File, Error> {
    lease(path, false, checkpoint)
}

/// Wait for exclusive ownership of an initialized mutex, with cancellation.
pub(in crate::review) fn write_lease(
    path: &Path,
    checkpoint: Checkpoint<'_>,
) -> Result<File, Error> {
    lease(path, true, Some(checkpoint))
}

fn lease(path: &Path, exclusive: bool, checkpoint: Option<Checkpoint<'_>>) -> Result<File, Error> {
    let path = path.join(".objects.lock");
    let file = open(&path, exclusive, false)?;
    let mut backoff = Backoff::new();
    loop {
        if let Some(checkpoint) = checkpoint {
            checkpoint()?;
        }
        let locked = if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        };
        match locked {
            Ok(()) => {
                checked(&path, &file)?;
                return Ok(file);
            }
            Err(fs::TryLockError::WouldBlock) if checkpoint.is_some() => {
                crate::diagnostics::trace::count("state.object_lease_waits", 1);
                backoff.wait();
            }
            Err(fs::TryLockError::WouldBlock) => return Err(Error::Locked),
            Err(fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

/// Bounded, read-only diagnostics; unsafe neighbors are never followed or fixed.
pub(in crate::review) fn anomalies(path: &Path) -> Result<Vec<PathBuf>, Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(vec![path.into()]);
    }
    let mut problems = Vec::new();
    for entry in fs::read_dir(path)?.take(FILE_LIMIT + 1) {
        let path = entry?.path();
        if let Err(error) = read_file(&path)
            && !matches!(&error, Error::Io(error) if error.kind() == std::io::ErrorKind::NotFound)
        {
            problems.push(path);
        }
        if problems.len() == 64 {
            break;
        }
    }
    Ok(problems)
}

fn checked(path: &Path, file: &File) -> Result<(), Error> {
    let current = fs::symlink_metadata(path)?;
    let held = file.metadata()?;
    if !current.is_file() || current.file_type().is_symlink() || !held.is_file() {
        return Err(Error::Invalid(
            "object storage must contain owned regular files",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let parent = fs::metadata(path.parent().ok_or(Error::Invalid("object parent"))?)?;
        if (current.dev(), current.ino()) != (held.dev(), held.ino())
            || held.nlink() != 1
            || held.uid() != parent.uid()
            || held.mode() & 0o077 != 0
        {
            return Err(Error::Invalid(
                "object ownership, privacy or identity differs",
            ));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if held.file_attributes() & 0x0400 != 0 {
            return Err(Error::Invalid("object cannot be a reparse point"));
        }
        crate::windows_security::check_private_file(file)?;
        let other = crate::windows_security::read_regular(path)?;
        let a = winapi_util::file::information(file)?;
        let b = winapi_util::file::information(&other)?;
        if a.number_of_links() != 1
            || (a.volume_serial_number(), a.file_index())
                != (b.volume_serial_number(), b.file_index())
        {
            return Err(Error::Invalid("object identity differs"));
        }
    }
    Ok(())
}

impl Directory {
    pub(in crate::review) fn check_owner(&self) -> Result<(), Error> {
        checked(&self.path.join(".objects.lock"), &self.guard)
    }
    /// Callers verify/create the private directory before entering. The stable
    /// mutex serializes its writers, never ordinary evaluation.
    pub(in crate::review) fn enter(path: &Path) -> Result<Self, Error> {
        Self::enter_attempts(path, 20)
    }

    pub(in crate::review) fn try_enter(path: &Path) -> Result<Self, Error> {
        Self::enter_attempts(path, 1)
    }

    fn enter_attempts(path: &Path, attempts: usize) -> Result<Self, Error> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Invalid("object directory cannot be a link"));
        }
        let guard = open(&path.join(".objects.lock"), true, true)?;
        let mut acquired = false;
        for _ in 0..attempts {
            match guard.try_lock() {
                Ok(()) => {
                    acquired = true;
                    break;
                }
                Err(fs::TryLockError::WouldBlock) => {
                    if attempts > 1 {
                        std::thread::sleep(std::time::Duration::from_millis(25));
                    }
                }
                Err(fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
        if !acquired {
            return Err(Error::Locked);
        }
        checked(&path.join(".objects.lock"), &guard)?;
        Ok(Self {
            path: path.into(),
            guard,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_authority_excludes_exclusive_publication_and_accepts_other_readers() {
        let root = assert_fs::TempDir::new().unwrap();
        super::super::private_directory(root.path()).unwrap();
        initialize_mutex(root.path()).unwrap();
        let reader = read_lease(root.path(), None).unwrap();
        let other = read_lease(root.path(), None).unwrap();
        assert!(matches!(
            Directory::try_enter(root.path()),
            Err(Error::Locked)
        ));
        drop((reader, other));
        Directory::try_enter(root.path()).unwrap();
    }

    #[test]
    fn waiting_leases_check_cancellation_while_publisher_holds_lock() {
        let root = assert_fs::TempDir::new().unwrap();
        super::super::private_directory(root.path()).unwrap();
        let writer = Directory::enter(root.path()).unwrap();
        assert!(matches!(read_lease(root.path(), None), Err(Error::Locked)));
        for exclusive in [false, true] {
            let checks = std::cell::Cell::new(0);
            let checkpoint = || {
                checks.set(checks.get() + 1);
                if checks.get() == 3 {
                    Err(crate::policy::Error::Interrupted)
                } else {
                    Ok(())
                }
            };
            let result = if exclusive {
                write_lease(root.path(), &checkpoint)
            } else {
                read_lease(root.path(), Some(&checkpoint))
            };
            assert!(matches!(
                result,
                Err(Error::Policy(crate::policy::Error::Interrupted))
            ));
        }
        drop(writer);
        read_lease(root.path(), None).unwrap();
        write_lease(root.path(), &|| Ok(())).unwrap();
    }
}
