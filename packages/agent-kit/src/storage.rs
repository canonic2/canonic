//! Bounded reads, path validation and recoverable entry ownership publication.
use super::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Largest configuration file setup reads or writes. Claude Code's user file
/// (`~/.claude.json`) also keeps per-project history and grows with use.
pub const LIMIT: u64 = 16 * 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub version: u8,
    pub path: PathBuf,
    pub product: String,
    pub previous: Option<Value>,
    pub desired: Option<Value>,
    pub pending: bool,
}

/// Hash the path's native encoding for an ownership record or lock name.
#[must_use]
pub fn identity(path: &Path) -> String {
    format!("{:x}", Sha256::digest(path.as_os_str().as_encoded_bytes()))
}

/// Validate that existing ancestors are directories and no component is a symlink.
///
/// # Errors
/// Returns an error for linked components, non-directory ancestors or metadata
/// failures other than an absent component.
pub fn safe(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::invalid(format!(
                    "Linked configuration path refused: {}",
                    ancestor.display()
                )));
            }
            Ok(metadata) if ancestor != path && !metadata.is_dir() => {
                return Err(Error::invalid(format!(
                    "Configuration parent is not a directory: {}",
                    ancestor.display()
                )));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Read a bounded UTF-8 configuration file, returning `None` when absent.
///
/// # Errors
/// Returns an error for invalid UTF-8 or any failure described by [`read_file`].
pub fn read(path: &Path) -> Result<Option<String>> {
    read_file(path)?
        .map(|(bytes, _)| String::from_utf8(bytes).map_err(|e| Error::invalid(e.to_string())))
        .transpose()
}

/// Native bundle files share configuration's bounded/no-follow read boundary.
/// Returns bytes and Unix executable bits, or `None` when absent.
///
/// # Errors
/// Returns an error for linked paths, special or oversized files, or filesystem
/// failures. Files must fit within [`LIMIT`].
pub fn read_file(path: &Path) -> Result<Option<(Vec<u8>, u32)>> {
    let refused = || {
        Error::invalid(format!(
            "Configuration is not a bounded regular file: {}",
            path.display()
        ))
    };
    safe(path)?;
    // Opening a FIFO without a writer blocks; refuse special files unopened.
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => return Err(refused()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    // A FIFO swapped in after that check must still not block the open; the
    // descriptor is validated below and regular-file reads ignore the flag.
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .custom_flags((nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_NOFOLLOW).bits());
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(refused());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if metadata.len() > LIMIT || bytes.len() as u64 > LIMIT {
        return Err(Error::invalid(format!(
            "Configuration exceeds 16 MiB: {}",
            path.display()
        )));
    }
    #[cfg(unix)]
    let execute_bits = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111
    };
    #[cfg(not(unix))]
    let execute_bits = 0;
    Ok(Some((bytes, execute_bits)))
}

/// Create missing directories, using owner-only permissions for private Unix roots.
///
/// # Errors
/// Returns an error for unsafe paths or directory creation failures.
pub fn directory(path: &Path, private: bool) -> Result<()> {
    safe(path)?;
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        directory(parent, private)?;
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        if private {
            builder.mode(0o700);
        }
    }
    let _ = private;
    match builder.recursive(false).create(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => safe(path),
        Err(e) => Err(e.into()),
    }
}

/// A setup scope owns the lock independently of duplicated or forked file
/// descriptors, which can survive briefly after a transaction returns.
pub struct Lock(File);

impl Drop for Lock {
    fn drop(&mut self) {
        let _unlocked = self.0.unlock();
    }
}

/// Acquire a nonblocking identity lock under the state root until the guard drops.
///
/// # Errors
/// Returns an error for unsafe paths, filesystem failures or lock contention.
pub fn lock(state: &Path, identity: &str) -> Result<Lock> {
    let parent = state.join("locks");
    directory(&parent, true)?;
    let path = parent.join(format!("{identity}.lock"));
    safe(&path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => {
            Error::Conflict("Another setup owns this configuration; retry after it finishes".into())
        }
        std::fs::TryLockError::Error(error) => Error::Io(error),
    })?;
    Ok(Lock(file))
}

/// Replace `path` only while it still holds `expected` (`None`: absent).
///
/// # Errors
/// Returns an error for unsafe paths, oversized contents, concurrent edits or
/// staging/publication I/O failures. See [`stage`] and [`Staged::replace`].
pub fn write(path: &Path, contents: &str, private: bool, expected: Option<&str>) -> Result<()> {
    stage(path, contents, private)?.replace(expected)
}

/// A durable temporary file beside its destination, removed unless published.
pub struct Staged {
    path: PathBuf,
    temporary: Option<PathBuf>,
}

impl Staged {
    /// Set executable bits on the unpublished file, never on the destination.
    ///
    /// # Errors
    /// Returns an error if opening, inspecting, changing or flushing the staged
    /// Unix file fails. This is a no-op on other platforms.
    ///
    /// # Panics
    /// Panics on Unix if the private unpublished-file invariant is violated.
    /// Publication consumes the staging value, so callers cannot reach that state.
    pub fn execute_bits(&self, execute_bits: u32) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file = File::open(self.temporary.as_ref().expect("unpublished staging file"))?;
            let mut mode = file.metadata()?.permissions().mode() & 0o777;
            mode = (mode & !0o111) | (execute_bits & 0o111);
            file.set_permissions(fs::Permissions::from_mode(mode))?;
            sync(&file)?;
        }
        let _ = execute_bits;
        Ok(())
    }

    /// Publish a bundle file after comparing both its bytes and executable state.
    ///
    /// # Errors
    /// Returns an error for unsafe paths, changed destination contents or modes,
    /// or rename/directory-flush failures.
    ///
    /// # Panics
    /// Panics if the private unpublished-file invariant is violated. Publication
    /// consumes this value, so callers cannot reach that state.
    pub fn replace_file(mut self, expected: Option<&(Vec<u8>, u32)>) -> Result<()> {
        safe(&self.path)?;
        if read_file(&self.path)?.as_ref() != expected {
            return Err(Error::Conflict(
                "Native file changed during publication".into(),
            ));
        }
        let temporary = self.temporary.as_ref().expect("unpublished staging file");
        fs::rename(temporary, &self.path)?;
        self.temporary.take();
        sync_parent(&self.path)?;
        Ok(())
    }
    /// Revalidate the destination after the slow write and flush, immediately
    /// before the rename. Editors do not take setup locks, so this narrows the
    /// window for a concurrent save to the final compare and rename.
    ///
    /// # Errors
    /// Returns an error for unsafe paths, changed destination contents, invalid
    /// UTF-8 or rename/directory-flush failures.
    ///
    /// # Panics
    /// Panics if the private unpublished-file invariant is violated. Publication
    /// consumes this value, so callers cannot reach that state.
    pub fn replace(mut self, expected: Option<&str>) -> Result<()> {
        safe(&self.path)?;
        if read(&self.path)?.as_deref() != expected {
            return Err(Error::Conflict(format!(
                "Configuration changed during setup: {}; run setup again",
                self.path.display()
            )));
        }
        let temporary = self
            .temporary
            .take()
            .expect("staged file is published once");
        if let Err(error) = fs::rename(&temporary, &self.path) {
            let _ = fs::remove_file(temporary);
            return Err(error.into());
        }
        sync_parent(&self.path)?;
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if let Some(temporary) = self.temporary.take() {
            let _ = fs::remove_file(temporary);
        }
    }
}

/// Stage UTF-8 contents durably beside the destination, without publishing them.
///
/// # Errors
/// Returns an error for any failure described by [`stage_bytes`].
pub fn stage(path: &Path, contents: &str, private: bool) -> Result<Staged> {
    stage_bytes(path, contents.as_bytes(), private)
}

/// Stage bounded bytes durably, preserving existing permissions unless private.
/// Private Unix files use owner-only permissions; abandoned staging files are removed.
///
/// # Errors
/// Returns an error for contents exceeding [`LIMIT`], unsafe paths, a missing
/// parent, or directory creation, staging, permission, write or flush failures.
pub fn stage_bytes(path: &Path, contents: &[u8], private: bool) -> Result<Staged> {
    if contents.len() as u64 > LIMIT {
        return Err(Error::invalid("Generated configuration exceeds 16 MiB"));
    }
    safe(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Configuration has no parent"))?;
    directory(parent, private)?;
    let permissions = fs::metadata(path).ok().map(|m| m.permissions());
    let temporary = parent.join(format!(
        ".canonic-setup-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o666 });
    }
    let mut file = options.open(&temporary)?;
    let staged = Staged {
        path: path.to_path_buf(),
        temporary: Some(temporary),
    };
    // Private staged contents must stay owner-only even if another writer
    // broadens the destination after the publisher's permission check.
    if let Some(permissions) = permissions.filter(|_| !private) {
        file.set_permissions(permissions)?;
    }
    file.write_all(contents)?;
    sync(&file)?;
    Ok(staged)
}

/// Remove an already reviewed owned file and persist the Unix directory change.
pub(crate) fn remove(path: &Path) -> Result<()> {
    safe(path)?;
    fs::remove_file(path)?;
    sync_parent(path)
}

#[cfg_attr(
    not(unix),
    expect(
        clippy::unnecessary_wraps,
        reason = "Unix directory flushing is fallible"
    )
)]
fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        sync(&File::open(parent)?)?;
    }
    // Ordinary File::open cannot open a Windows directory. The staged file
    // itself is flushed before rename; no directory-durability claim is made.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Durable flush without macOS's drive-wide `F_FULLFSYNC` barrier.
fn sync(file: &File) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        nix::unistd::fsync(file).map_err(std::io::Error::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        file.sync_all()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn text_and_binary_replacement_and_removal_finish_without_partial_failure() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let path = root.join("owned");
        super::write(&path, "before", false, None).unwrap();
        super::write(&path, "after", false, Some("before")).unwrap();
        assert_eq!(super::read(&path).unwrap().as_deref(), Some("after"));
        let previous = super::read_file(&path).unwrap();
        super::stage_bytes(&path, &[0, 255], false)
            .unwrap()
            .replace_file(previous.as_ref())
            .unwrap();
        assert_eq!(super::read_file(&path).unwrap().unwrap().0, [0, 255]);
        super::remove(&path).unwrap();
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }

    #[test]
    fn ending_a_lock_scope_releases_it_even_while_a_duplicate_descriptor_survives() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let first = super::lock(&root, "shared").unwrap();
        assert!(super::lock(&root, "shared").is_err());
        let inherited = first.0.try_clone().unwrap();
        drop(first);
        let second = super::lock(&root, "shared").unwrap();
        drop(second);
        drop(inherited);
    }
}
