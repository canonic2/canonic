//! Existing-only admission and writer-owned bounded private indexes.
use super::model::{Contexts, INDEX_BYTES, Profiles};
use crate::review::{
    Error,
    storage::{objects, private_files},
};
use serde::de::DeserializeOwned;
use std::{
    fs::{self, File, TryLockError},
    path::{Path, PathBuf},
};

#[cfg(test)]
thread_local! {
    static TEST_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn with_home<T>(
    home: &Path,
    operation: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_HOME.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(TEST_HOME.with(|slot| slot.replace(Some(home.into()))));
    operation()
}

pub(super) fn directory(name: &str, create: bool) -> Result<PathBuf, Error> {
    if !matches!(name, "sdk-read-profiles" | "sdk-read-contexts") {
        return Err(Error::Invalid("SDK read namespace is invalid"));
    }
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA");
    #[cfg(not(windows))]
    let base = std::env::var_os("HOME");
    let base = base
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or(Error::Invalid("SDK read private storage unavailable"))?;
    #[cfg(test)]
    let base = TEST_HOME.with(|slot| slot.borrow().clone()).unwrap_or(base);
    let parent = base.join(".shield");
    private_files::directory(&parent, create)?;
    let directory = parent.join(name);
    private_files::directory(&directory, create)?;
    Ok(directory)
}

fn read<T: DeserializeOwned>(directory: &Path) -> Result<Option<T>, Error> {
    let value = match private_files::read(&directory.join("state.json"), INDEX_BYTES) {
        Ok(value) => value,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    serde_json::from_value(value)
        .map(Some)
        .map_err(|_| Error::Invalid("SDK read private index is invalid"))
}

pub(super) fn profiles(directory: &Path) -> Result<Profiles, Error> {
    let profiles = read(directory)?.unwrap_or_else(|| Profiles {
        schema_version: 1,
        ..Profiles::default()
    });
    profiles.validate()?;
    Ok(profiles)
}

pub(super) fn contexts(directory: &Path) -> Result<Contexts, Error> {
    let contexts = read(directory)?.unwrap_or_else(|| Contexts {
        schema_version: 1,
        ..Contexts::default()
    });
    contexts.validate()?;
    Ok(contexts)
}

fn owner_path(directory: &Path, id: &str) -> PathBuf {
    directory.join(format!("owner-{id}.lock"))
}

pub(super) fn own_context(directory: &Path, id: &str) -> Result<File, Error> {
    objects::own_file(&owner_path(directory, id))
}

pub(super) fn context_alive(directory: &Path, id: &str) -> Result<bool, Error> {
    match objects::read_file(&owner_path(directory, id)) {
        Ok(file) => match file.try_lock_shared() {
            Ok(()) => Ok(false),
            Err(TryLockError::WouldBlock) => Ok(true),
            Err(TryLockError::Error(error)) => Err(error.into()),
        },
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(super) fn remove_owner(directory: &Path, id: &str) -> Result<(), Error> {
    let path = owner_path(directory, id);
    match objects::read_file(&path) {
        Ok(_) => Ok(fs::remove_file(path)?),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Caller owns the namespace's exclusive kernel lease.
pub(super) fn write(directory: &Path, index: &impl serde::Serialize) -> Result<(), Error> {
    let value = serde_json::to_value(index)
        .map_err(|_| Error::Invalid("SDK read private index encoding"))?;
    private_files::write(&directory.join("state.json"), &value, INDEX_BYTES)
}
