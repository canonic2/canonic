//! The user's global registrations file: validated reads and locked atomic
//! replacement.

use super::{RegistrationError, Registrations, Scope};
use crate::files::{self, Bounded, Commit};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Maximum size of the global registrations file.
pub const MAX_FILE_BYTES: u64 = 1_048_576;

/// One product's global registrations, stored as JSON at a path it chooses.
///
/// The lock and temporary files sit beside it with `.lock` and `.new`
/// extensions. On Unix the file is mode 0600, and a parent directory created
/// here is mode 0700.
#[derive(Clone, Debug)]
pub struct GlobalFile {
    path: PathBuf,
    builtin: String,
}

impl GlobalFile {
    /// A registrations file at `path` for a product whose built-in alias is `builtin`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, builtin: &str) -> Self {
        Self {
            path: path.into(),
            builtin: builtin.to_owned(),
        }
    }

    /// The file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads and validates the file; a missing file is empty.
    ///
    /// # Errors
    /// Returns [`RegistrationError`] for links, oversized or invalid content and I/O failure.
    pub fn read(&self) -> Result<Registrations, RegistrationError> {
        match files::kind(&self.path)? {
            None => return Ok(Registrations::default()),
            Some(kind) if kind.is_symlink() => return Err(RegistrationError::Link),
            Some(_) => {}
        }
        let Bounded::Bytes(bytes) = files::read_bounded(&self.path, MAX_FILE_BYTES)? else {
            return Err(RegistrationError::Malformed);
        };
        let registrations: Registrations =
            serde_json::from_slice(&bytes).map_err(|_| RegistrationError::Malformed)?;
        registrations.validate(&self.builtin, Scope::Global)?;
        Ok(registrations)
    }

    /// Replaces `before` with `after`, refusing if the file changed meanwhile.
    ///
    /// The parent directory is created when missing. The write holds an
    /// exclusive lock file, writes a temporary file, syncs it and renames it
    /// over the original, then syncs the directory and removes the lock.
    ///
    /// # Errors
    /// Every error except two means the file is unchanged:
    /// [`RegistrationError::Changed`] when the file no longer equals `before`,
    /// [`RegistrationError::Locked`] while another writer holds the lock, and
    /// other [`RegistrationError`] values for invalid content or I/O.
    ///
    /// [`RegistrationError::NotDurable`] and [`RegistrationError::LockRemains`]
    /// mean `after` was written: respectively, the directory sync failed, or
    /// the lock could not be removed and must be removed before the next write.
    /// [`RegistrationError::RefusedLockRemains`] means nothing was written and
    /// the lock could not be removed; it carries the original refusal.
    pub fn replace(
        &self,
        before: &Registrations,
        after: &Registrations,
    ) -> Result<(), RegistrationError> {
        after.validate(&self.builtin, Scope::Global)?;
        // The lock and temporary files replace the extension; a file already
        // named like one of them would collide with its own lock or temporary.
        // Compare case-insensitively (with Unicode folding, as case-insensitive
        // filesystems do): on such a filesystem `registries.NEW` *is* its own
        // temporary file, which the write would otherwise delete.
        if self
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_lowercase)
            .is_some_and(|extension| extension == "lock" || extension == "new")
        {
            return Err(RegistrationError::Path);
        }
        let parent = self.path.parent().ok_or(RegistrationError::Path)?;
        // A bare file name lives in the current directory.
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        match files::kind(parent)? {
            None => files::create_private_dir(parent)?,
            Some(kind) if kind.is_symlink() => return Err(RegistrationError::Link),
            Some(_) => {}
        }
        let lock = self.path.with_extension("lock");
        let guard = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(guard) => guard,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(RegistrationError::Locked(lock));
            }
            Err(error) => return Err(error.into()),
        };
        let result = self.write_locked(before, after);
        drop(guard);
        let released = fs::remove_file(&lock);
        conclude(result, released, lock)
    }

    fn write_locked(
        &self,
        before: &Registrations,
        after: &Registrations,
    ) -> Result<Commit, RegistrationError> {
        if &self.read()? != before {
            return Err(RegistrationError::Changed);
        }
        let mut bytes =
            serde_json::to_vec_pretty(after).map_err(|_| RegistrationError::Malformed)?;
        bytes.push(b'\n');
        // Escaping can make valid registrations larger than the reader accepts;
        // refuse before anything on disk changes, so the file stays readable.
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(RegistrationError::TooLarge);
        }
        let temporary = self.path.with_extension("new");
        // A previous writer crashed after creating it; the lock is ours now.
        if let Err(error) = fs::remove_file(&temporary)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error.into());
        }
        Ok(files::replace_private(&temporary, &self.path, &bytes)?)
    }
}

/// Combines the write's outcome with the lock's release, so neither hides
/// the other and every error says whether the new value was written.
fn conclude(
    result: Result<Commit, RegistrationError>,
    released: io::Result<()>,
    lock: PathBuf,
) -> Result<(), RegistrationError> {
    match (result, released) {
        // Nothing was written, and the lock could not be removed either:
        // report both, so the stuck lock is not hidden behind the refusal.
        (Err(refusal), Err(error)) => Err(RegistrationError::RefusedLockRemains {
            refusal: Box::new(refusal),
            lock,
            source: error,
        }),
        (Err(refusal), Ok(())) => Err(refusal),
        // From here the new registrations are in place: report what remains
        // undone rather than a plain failure, and never roll back.
        (Ok(_), Err(error)) => Err(RegistrationError::LockRemains(lock, error)),
        (Ok(Commit::Durable), Ok(())) => Ok(()),
        (Ok(Commit::NotDurable(error)), Ok(())) => Err(RegistrationError::NotDurable(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure() -> io::Error {
        io::Error::other("fixture failure")
    }

    #[test]
    fn every_outcome_reports_whether_the_value_was_written_and_any_stuck_lock() {
        let lock = || PathBuf::from("/srv/acme/registries.lock");
        assert!(conclude(Ok(Commit::Durable), Ok(()), lock()).is_ok());
        assert!(matches!(
            conclude(Ok(Commit::NotDurable(failure())), Ok(()), lock()),
            Err(RegistrationError::NotDurable(_))
        ));
        // Committed, with a stuck lock: the lock wins over durability.
        assert!(matches!(
            conclude(Ok(Commit::NotDurable(failure())), Err(failure()), lock()),
            Err(RegistrationError::LockRemains(path, _)) if path == lock()
        ));
        assert!(matches!(
            conclude(Err(RegistrationError::Changed), Ok(()), lock()),
            Err(RegistrationError::Changed)
        ));
        // Refused, with a stuck lock: both are reported.
        match conclude(Err(RegistrationError::TooLarge), Err(failure()), lock()) {
            Err(RegistrationError::RefusedLockRemains {
                refusal,
                lock: path,
                ..
            }) => {
                assert!(matches!(*refusal, RegistrationError::TooLarge));
                assert_eq!(path, lock());
            }
            other => panic!("expected a refusal with a stuck lock, got {other:?}"),
        }
    }
}
