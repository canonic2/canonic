//! Bounded private JSON files and atomic publication under a caller-owned lock.
use super::{Error, objects, private, replace, sync};
use serde_json::Value;
use std::{
    fs,
    io::{Read as _, Write as _},
    path::Path,
};

pub(in crate::review) fn directory(path: &Path, create: bool) -> Result<(), Error> {
    if create && !path.exists() {
        #[cfg(windows)]
        crate::windows_security::check_private(
            path.parent().ok_or(Error::Invalid("private parent"))?,
        )?;
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            builder.mode(0o700);
        }
        match builder.create(path) {
            Ok(()) => {
                #[cfg(windows)]
                crate::windows_security::make_private(path)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Invalid("private directory cannot be a link or file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let parent = fs::metadata(path.parent().ok_or(Error::Invalid("private parent"))?)?;
        if metadata.uid() != parent.uid() || metadata.mode() & 0o077 != 0 {
            return Err(Error::Invalid("private directory owner or privacy differs"));
        }
    }
    #[cfg(windows)]
    crate::windows_security::check_private(path)?;
    Ok(())
}

pub(in crate::review) fn read(path: &Path, limit: usize) -> Result<Value, Error> {
    let file = objects::read_file(path)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(crate::policy::Error::Limit.into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(crate::policy::Error::Limit.into());
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("private JSON file invalid"))
}

/// The caller holds an exclusive namespace lock. A pending file is owned by
/// that namespace and can be recovered after its previous writer exits.
pub(in crate::review) fn write(path: &Path, value: &Value, limit: usize) -> Result<(), Error> {
    if !crate::json::fits(value, limit) {
        return Err(crate::policy::Error::Limit.into());
    }
    check_existing(path)?;
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Invalid("private JSON encoding"))?;
    let temporary = path.with_extension("pending");
    if check_existing(&temporary)? {
        fs::remove_file(&temporary)?;
    }
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    private(&mut options);
    let mut file = options.open(&temporary)?;
    let result = (|| -> Result<(), Error> {
        #[cfg(windows)]
        crate::windows_security::make_private(&temporary)?;
        file.write_all(&bytes)?;
        sync(&file)?;
        drop(file);
        replace(&temporary, path)?;
        #[cfg(unix)]
        sync(&fs::File::open(
            path.parent().ok_or(Error::Invalid("private parent"))?,
        )?)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn check_existing(path: &Path) -> Result<bool, Error> {
    match objects::read_file(path) {
        Ok(_) => Ok(true),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn private_files_publish_complete_bounded_private_values_and_recover_pending() {
        let root = assert_fs::TempDir::new().unwrap();
        let dir = root.path().join("records");
        directory(&dir, true).unwrap();
        let _guard = objects::Directory::try_enter(&dir).unwrap();
        let path = dir.join("record.json");
        write(&path, &json!({"acme":1}), 128).unwrap();
        assert_eq!(read(&path, 128).unwrap()["acme"], 1);
        assert!(read(&path, 1).is_err());
        assert!(write(&path, &json!("x".repeat(256)), 128).is_err());
        assert_eq!(read(&path, 128).unwrap()["acme"], 1);
        write(
            &dir.join("record.pending"),
            &json!({"incomplete":true}),
            128,
        )
        .unwrap();
        write(&path, &json!({"acme":2}), 128).unwrap();
        assert_eq!(read(&path, 128).unwrap()["acme"], 2);
        assert!(!dir.join("record.pending").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn private_files_refuse_links_without_modifying_their_targets() {
        let root = assert_fs::TempDir::new().unwrap();
        let path = root.path().join("record.json");
        let target = root.path().join("target.json");
        write(&target, &json!({"acme":1}), 128).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(read(&path, 128).is_err());
        assert!(write(&path, &json!({"acme":2}), 128).is_err());
        assert_eq!(read(&target, 128).unwrap()["acme"], 1);
        assert!(directory(&path, false).is_err());
    }
}
