//! Digest-addressed retained artifacts shared by every project of one user.
//!
//! Each artifact is stored once as `<root>/<sha256>`. Writes are atomic and
//! files are never modified in place; reads verify the digest, so a corrupted
//! or substituted file is reported rather than used.

use crate::{
    digest,
    files::{self, Bounded},
};
use std::{
    io,
    path::{Path, PathBuf},
};

/// A cache directory owned by the caller's product.
#[derive(Clone, Debug)]
pub struct ArtifactCache {
    root: PathBuf,
}

/// Cache failures.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// The requested digest is not a lowercase SHA-256.
    #[error("invalid artifact digest")]
    Digest,
    /// The cache directory or an artifact is a symbolic link or not a regular file.
    #[error("artifact cache entry is not a regular file: {0}")]
    NotRegular(PathBuf),
    /// Retained bytes no longer match their name.
    #[error("cached artifact is corrupt: {0}")]
    Corrupt(String),
    /// Supplied bytes do not match the expected digest.
    #[error("artifact bytes do not match the expected digest")]
    Mismatch,
    /// The artifact exceeds the caller's size limit.
    #[error("cached artifact exceeds the size limit")]
    TooLarge,
    /// Filesystem failure.
    #[error("artifact cache I/O failed")]
    Io(#[from] io::Error),
}

impl ArtifactCache {
    /// A cache rooted at `root`, created on first write.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The cache directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads the artifact named `digest`, verifying its bytes.
    ///
    /// Returns `None` when it is not cached.
    ///
    /// # Errors
    /// Returns [`CacheError`] for invalid digests, links, oversized or corrupt
    /// files, and I/O failure.
    pub fn get(&self, digest: &str, max_bytes: u64) -> Result<Option<Vec<u8>>, CacheError> {
        let path = self.path(digest)?;
        if !self.root_exists()? {
            return Ok(None);
        }
        match files::kind(&path)? {
            None => return Ok(None),
            Some(kind) if !kind.is_file() => return Err(CacheError::NotRegular(path)),
            Some(_) => {}
        }
        let bytes = match files::read_bounded(&path, max_bytes)? {
            Bounded::Bytes(bytes) => bytes,
            Bounded::TooLarge => return Err(CacheError::TooLarge),
            Bounded::NotFile => return Err(CacheError::NotRegular(path)),
        };
        if digest::sha256_hex(&bytes) != digest {
            return Err(CacheError::Corrupt(digest.to_owned()));
        }
        Ok(Some(bytes))
    }

    /// Whether a regular file is cached under `digest`, without verifying it.
    ///
    /// # Errors
    /// Returns [`CacheError::Digest`] for an invalid digest,
    /// [`CacheError::NotRegular`] for a linked cache directory or an entry
    /// that is a link or not a regular file, and [`CacheError::Io`] when the
    /// entry cannot be inspected.
    pub fn contains(&self, digest: &str) -> Result<bool, CacheError> {
        let path = self.path(digest)?;
        if !self.root_exists()? {
            return Ok(false);
        }
        match files::kind(&path)? {
            None => Ok(false),
            Some(kind) if kind.is_file() => Ok(true),
            Some(_) => Err(CacheError::NotRegular(path)),
        }
    }

    /// Whether the cache directory exists; a root that is a link or not a
    /// directory is refused.
    fn root_exists(&self) -> Result<bool, CacheError> {
        match files::kind(&self.root)? {
            None => Ok(false),
            Some(kind) if kind.is_dir() => Ok(true),
            Some(_) => Err(CacheError::NotRegular(self.root.clone())),
        }
    }

    /// Stores `bytes` and returns their digest. An existing valid copy is
    /// kept; a corrupt or wrongly sized regular file is replaced.
    ///
    /// # Errors
    /// Returns [`CacheError::NotRegular`] for a linked cache directory, or an
    /// existing entry that is a link or not a regular file, which is never
    /// replaced; and [`CacheError::Io`] for I/O failure.
    pub fn put(&self, bytes: &[u8]) -> Result<String, CacheError> {
        let digest = digest::sha256_hex(bytes);
        self.store(&digest, bytes)?;
        Ok(digest)
    }

    /// Stores `bytes` only if they match `expected`.
    ///
    /// # Errors
    /// Returns [`CacheError::Mismatch`] for other bytes, and other
    /// [`CacheError`] values as [`ArtifactCache::put`].
    pub fn put_expected(&self, expected: &str, bytes: &[u8]) -> Result<(), CacheError> {
        self.path(expected)?;
        if digest::sha256_hex(bytes) != expected {
            return Err(CacheError::Mismatch);
        }
        self.store(expected, bytes)
    }

    fn store(&self, digest: &str, bytes: &[u8]) -> Result<(), CacheError> {
        let path = self.path(digest)?;
        self.prepare_root()?;
        match self.get(digest, bytes.len() as u64) {
            Ok(Some(_)) => return Ok(()),
            // Missing, or a regular file with other bytes: write the true bytes.
            Ok(None) | Err(CacheError::Corrupt(_) | CacheError::TooLarge) => {}
            // A link or other non-file entry is refused, never replaced.
            Err(error) => return Err(error),
        }
        let temporary = files::temporary_path(&self.root, digest);
        // A committed entry is usable even if its directory sync failed: reads
        // verify it, and losing it in a crash only means fetching it again.
        files::replace_private(&temporary, &path, bytes)?;
        Ok(())
    }

    fn prepare_root(&self) -> Result<(), CacheError> {
        if !self.root_exists()? {
            files::create_private_dir(&self.root)?;
        }
        Ok(())
    }

    fn path(&self, digest: &str) -> Result<PathBuf, CacheError> {
        if digest::is_sha256_hex(digest) {
            Ok(self.root.join(digest))
        } else {
            Err(CacheError::Digest)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::{TempDir, prelude::*};
    use std::fs;

    #[test]
    fn artifacts_round_trip_by_digest_and_deduplicate() {
        let home = TempDir::new().unwrap();
        let cache = ArtifactCache::new(home.path().join("cache"));
        let digest = cache.put(b"acme pack").unwrap();
        assert_eq!(digest, digest::sha256_hex(b"acme pack"));
        assert!(cache.contains(&digest).unwrap());
        assert_eq!(cache.get(&digest, 1024).unwrap().unwrap(), b"acme pack");
        assert_eq!(cache.put(b"acme pack").unwrap(), digest);
        assert_eq!(fs::read_dir(cache.root()).unwrap().count(), 1);
        assert_eq!(
            cache.get(&digest::sha256_hex(b"other"), 1024).unwrap(),
            None
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(cache.root()), 0o700);
            assert_eq!(mode(&cache.root().join(&digest)), 0o600);
        }
    }

    #[test]
    fn expected_digests_corruption_and_limits_refuse() {
        let home = TempDir::new().unwrap();
        let cache = ArtifactCache::new(home.path());
        let digest = digest::sha256_hex(b"acme");
        assert!(matches!(
            cache.put_expected(&digest, b"other"),
            Err(CacheError::Mismatch)
        ));
        assert!(matches!(cache.get("ABC", 10), Err(CacheError::Digest)));
        assert!(matches!(cache.contains("../x"), Err(CacheError::Digest)));
        cache.put_expected(&digest, b"acme").unwrap();
        assert!(matches!(cache.get(&digest, 3), Err(CacheError::TooLarge)));
        home.child(&digest).write_str("tampered").unwrap();
        assert!(matches!(
            cache.get(&digest, 64),
            Err(CacheError::Corrupt(_))
        ));
        // Storing the true bytes again repairs a corrupt entry.
        cache.put_expected(&digest, b"acme").unwrap();
        assert_eq!(cache.get(&digest, 64).unwrap().unwrap(), b"acme");
    }

    #[cfg(unix)]
    #[test]
    fn a_stored_entry_whose_directory_cannot_be_synced_is_still_stored() {
        use std::os::unix::fs::PermissionsExt;
        let home = TempDir::new().unwrap();
        let cache = ArtifactCache::new(home.path().join("cache"));
        cache.put(b"first").unwrap();
        // Writable and searchable, but not readable: the directory cannot be opened to sync.
        fs::set_permissions(cache.root(), fs::Permissions::from_mode(0o300)).unwrap();
        if fs::File::open(cache.root()).is_ok() {
            // Privileges that bypass permissions make the failure impossible to provoke.
            fs::set_permissions(cache.root(), fs::Permissions::from_mode(0o700)).unwrap();
            return;
        }
        let stored = cache.put(b"acme");
        let read = cache.get(&digest::sha256_hex(b"acme"), 64);
        fs::set_permissions(cache.root(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(stored.unwrap(), digest::sha256_hex(b"acme"));
        assert_eq!(read.unwrap().unwrap(), b"acme");
    }

    #[cfg(unix)]
    #[test]
    fn linked_entries_and_roots_refuse() {
        let home = TempDir::new().unwrap();
        home.child("real").create_dir_all().unwrap();
        std::os::unix::fs::symlink(home.path().join("real"), home.path().join("linked")).unwrap();
        let linked = ArtifactCache::new(home.path().join("linked"));
        let some_digest = digest::sha256_hex(b"x");
        assert!(matches!(linked.put(b"x"), Err(CacheError::NotRegular(_))));
        assert!(matches!(
            linked.get(&some_digest, 64),
            Err(CacheError::NotRegular(_))
        ));
        assert!(matches!(
            linked.contains(&some_digest),
            Err(CacheError::NotRegular(_))
        ));
        let missing = ArtifactCache::new(home.path().join("missing"));
        assert_eq!(missing.get(&some_digest, 64).unwrap(), None);
        assert!(!missing.contains(&some_digest).unwrap());
        let cache = ArtifactCache::new(home.path().join("real"));
        let digest = digest::sha256_hex(b"x");
        home.child("elsewhere").write_str("x").unwrap();
        std::os::unix::fs::symlink(home.path().join("elsewhere"), cache.root().join(&digest))
            .unwrap();
        assert!(matches!(
            cache.get(&digest, 64),
            Err(CacheError::NotRegular(_))
        ));
        assert!(matches!(
            cache.contains(&digest),
            Err(CacheError::NotRegular(_))
        ));
        // Writes refuse a linked entry instead of replacing it, even with the true bytes.
        assert!(matches!(
            cache.put_expected(&digest, b"x"),
            Err(CacheError::NotRegular(_))
        ));
        assert!(matches!(cache.put(b"x"), Err(CacheError::NotRegular(_))));
        assert!(
            fs::symlink_metadata(cache.root().join(&digest))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(home.path().join("elsewhere")).unwrap(), b"x");
        // A directory in place of an entry is refused the same way.
        let other = digest::sha256_hex(b"y");
        fs::create_dir(cache.root().join(&other)).unwrap();
        assert!(matches!(cache.put(b"y"), Err(CacheError::NotRegular(_))));
        assert!(matches!(
            cache.contains(&other),
            Err(CacheError::NotRegular(_))
        ));
    }
}
