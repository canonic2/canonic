//! Exact setup/mint content pin with conservative per-launch verification.
use crate::review::Error;
use ring::digest::{Context as DigestContext, SHA256};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read as _, path::PathBuf};

#[cfg(test)]
thread_local! {
    static TEST_EXECUTABLE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn executable() -> Result<PathBuf, Error> {
    #[cfg(test)]
    if let Some(path) = TEST_EXECUTABLE.with(|slot| slot.borrow().clone()) {
        return Ok(path);
    }
    Ok(std::env::current_exe()?.canonicalize()?)
}

#[cfg(test)]
pub(super) fn with_executable<T>(
    path: &std::path::Path,
    operation: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_EXECUTABLE.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(TEST_EXECUTABLE.with(|slot| slot.replace(Some(path.into()))));
    operation()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    path: PathBuf,
    digest: String,
    stamp: Stamp,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    size: u64,
    device: u64,
    inode: u64,
    #[cfg(unix)]
    modified: (i64, i64),
    #[cfg(unix)]
    changed: (i64, i64),
    #[cfg(unix)]
    mode: u32,
}

fn stamp(file: &fs::File) -> Result<Stamp, Error> {
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt as _;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::Invalid("SDK read binary must be a regular file"));
    }
    #[cfg(windows)]
    let identity = winapi_util::file::information(file)?;
    Ok(Stamp {
        size: metadata.len(),
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(windows)]
        device: u64::from(identity.volume_serial_number()),
        #[cfg(unix)]
        inode: metadata.ino(),
        #[cfg(windows)]
        inode: identity.file_index(),
        #[cfg(unix)]
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        #[cfg(unix)]
        changed: (metadata.ctime(), metadata.ctime_nsec()),
        #[cfg(unix)]
        mode: metadata.mode(),
    })
}

fn open(path: &std::path::Path) -> Result<fs::File, Error> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        // Prevent writes and replacement while checking the executable.
        options.share_mode(1);
    }
    Ok(options.open(path)?)
}

fn content(file: &mut fs::File) -> Result<String, Error> {
    const LIMIT: u64 = 128 * 1024 * 1024;
    if file.metadata()?.len() > LIMIT {
        return Err(Error::Invalid("SDK read binary exceeds setup limit"));
    }
    let mut hash = DigestContext::new(&SHA256);
    let mut bytes = vec![0_u8; 64 * 1024].into_boxed_slice();
    let mut total = 0_u64;
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > LIMIT {
            return Err(Error::Invalid("SDK read binary exceeds setup limit"));
        }
        hash.update(&bytes[..count]);
    }
    Ok(canonic_registry_kit::digest::encode(hash.finish().as_ref()))
}

impl Receipt {
    pub(super) fn capture() -> Result<Self, Error> {
        let path = executable()?;
        Self::capture_path(path)
    }

    fn capture_path(path: PathBuf) -> Result<Self, Error> {
        let mut file = open(&path)?;
        let before = stamp(&file)?;
        let digest = content(&mut file)?;
        if before != stamp(&file)? || before != stamp(&open(&path)?)? {
            return Err(Error::StaleSnapshot);
        }
        Ok(Self {
            path,
            digest,
            stamp: before,
        })
    }

    pub(super) fn digest(&self) -> &str {
        &self.digest
    }

    pub(super) fn check_current(&self) -> Result<(), Error> {
        self.admit_current().map(drop)
    }

    pub(super) fn admit_current(&self) -> Result<fs::File, Error> {
        let path = executable()?;
        self.check_path(&path)
    }

    fn check_path(&self, path: &std::path::Path) -> Result<fs::File, Error> {
        let file = open(path)?;
        #[cfg(windows)]
        let mut file = file;
        if !self.path.is_absolute()
            || path != self.path.as_path()
            || !super::model::digest(&self.digest)
            || self.stamp != stamp(&file)?
        {
            return Err(Error::Invalid("SDK read binary identity differs"));
        }
        // Windows has no adopted safe change-time wrapper here. Rehash rather
        // than treating size/modified-time alone as content authorization.
        #[cfg(windows)]
        {
            if content(&mut file)? != self.digest {
                return Err(Error::Invalid("SDK read binary content differs"));
            }
        }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_receipt_detects_same_size_edits_and_path_replacement() {
        let fixture = assert_fs::TempDir::new().unwrap();
        let path = fixture.path().join("shield");
        fs::write(&path, b"acme-binary").unwrap();
        let receipt = Receipt::capture_path(path.clone()).unwrap();
        receipt.check_path(&path).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, b"evil-binary").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert!(receipt.check_path(&path).is_err());
        let replacement = fixture.path().join("replacement");
        fs::write(&replacement, b"acme-binary").unwrap();
        assert!(receipt.check_path(&replacement).is_err());
        fs::remove_file(&path).unwrap();
        fs::rename(&replacement, &path).unwrap();
        assert!(receipt.check_path(&path).is_err());
        assert_eq!(
            receipt.digest(),
            Receipt::capture_path(path).unwrap().digest()
        );
    }
}
