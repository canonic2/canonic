//! Sweep-local checkpoint reuse; a retained handle prevents file-ID recycling.
use super::{Error, File, Path, checked, read_file};
use serde_json::Value;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;
use std::{io::Read as _, path::PathBuf, time::SystemTime};

#[derive(PartialEq, Eq)]
struct Version {
    length: u64,
    modified: SystemTime,
    identity: (u64, u64),
    #[cfg(unix)]
    changed: (i64, i64),
}
impl Version {
    fn of(file: &File) -> Result<Self, Error> {
        let metadata = file.metadata()?;
        #[cfg(unix)]
        let identity = (metadata.dev(), metadata.ino());
        #[cfg(windows)]
        let identity = {
            let information = winapi_util::file::information(file)?;
            (
                u64::from(information.volume_serial_number()),
                information.file_index(),
            )
        };
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified()?,
            identity,
            #[cfg(unix)]
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }
}

#[derive(Default)]
pub(in crate::review) struct Checkpoint {
    retained: Option<Retained>,
    #[cfg(test)]
    reads: usize,
}
struct Retained {
    path: PathBuf,
    version: Version,
    _file: File,
    value: Value,
    expected: Option<String>,
}
impl Checkpoint {
    pub(in crate::review) fn read(
        &mut self,
        path: &Path,
        expected: Option<&str>,
    ) -> Result<Option<&Value>, Error> {
        let mut file = match read_file(path) {
            Ok(file) => file,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                self.retained = None;
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let version = Version::of(&file)?;
        if version.length > super::super::MAX_STATE as u64 {
            return Err(crate::policy::Error::Limit.into());
        }
        if !self.retained.as_ref().is_some_and(|old| {
            old.path == path && old.version == version && old.expected.as_deref() == expected
        }) {
            let mut bytes = Vec::new();
            (&mut file)
                .take(super::super::MAX_STATE as u64 + 1)
                .read_to_end(&mut bytes)?;
            checked(path, &file)?;
            if version != Version::of(&file)? {
                return Err(Error::Locked);
            }
            if bytes.len() > super::super::MAX_STATE
                || expected.is_some_and(|digest| crate::policy::fingerprint(&bytes) != digest)
            {
                return Err(Error::Invalid("retained checkpoint size or digest differs"));
            }
            let value = serde_json::from_slice(&bytes)
                .map_err(|_| Error::Invalid("retained checkpoint encoding"))?;
            crate::diagnostics::trace::count("objects.checkpoint_reads", 1);
            #[cfg(test)]
            {
                self.reads += 1;
            }
            self.retained = Some(Retained {
                path: path.into(),
                version,
                _file: file,
                value,
                expected: expected.map(Into::into),
            });
        }
        Ok(self.retained.as_ref().map(|retained| &retained.value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write as _};
    fn write(path: &Path, bytes: &[u8]) {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        super::super::private(&mut options);
        options.open(path).unwrap().write_all(bytes).unwrap();
    }
    #[test]
    fn storage_reclamation_checkpoint_reads_once_until_atomic_identity_changes() {
        let root = assert_fs::TempDir::new().unwrap();
        let path = root.path().join("state.json");
        write(&path, b"{\"version\":1}");
        let mut cache = Checkpoint::default();
        for _ in 0..20 {
            assert_eq!(cache.read(&path, None).unwrap().unwrap()["version"], 1);
        }
        assert_eq!(cache.reads, 1);
        let replacement = root.path().join("replacement.json");
        write(&replacement, b"{\"version\":2}");
        super::super::super::replace(&replacement, &path).unwrap();
        assert_eq!(cache.read(&path, None).unwrap().unwrap()["version"], 2);
        assert_eq!(cache.reads, 2);
        assert!(cache.read(&path, Some(&"0".repeat(64))).is_err());
    }
}
