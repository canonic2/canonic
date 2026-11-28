//! Private package snapshots preserve submitted bytes and bound filesystem traversal.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
};

pub(super) struct Snapshot {
    directory: PathBuf,
    pub path: PathBuf,
    pub sha256: String,
}
impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
impl Snapshot {
    pub(super) fn retain(self, destination: &Path) -> Result<()> {
        fs::rename(&self.directory, destination)
            .map_err(|_| Error::new("PACKAGE_IO", "Cannot retain immutable build artifact"))
    }
    pub fn create(source: &Path) -> Result<Self> {
        Self::create_checked(source, &crate::process::Cancellation::default())
    }
    pub(super) fn create_checked(
        source: &Path,
        cancel: &crate::process::Cancellation,
    ) -> Result<Self> {
        cancel.check()?;
        if !source.is_absolute() {
            return Err(Error::invalid(
                "App path must be absolute on the Testrun host",
            ));
        }
        let source = source
            .canonicalize()
            .map_err(|_| Error::new("PACKAGE_NOT_FOUND", "App package path is unavailable"))?;
        let extension = source.extension().and_then(|s| s.to_str());
        if !(extension == Some("apk") && source.is_file()
            || extension == Some("app") && source.is_dir())
        {
            return Err(Error::invalid(
                "Choose a single .apk file or an .app directory",
            ));
        }
        let directory = std::env::temp_dir().join(format!("testrun-app-{}", uuid::Uuid::new_v4()));
        crate::artifacts::directory(&directory)?;
        let mut snapshot = Self {
            path: directory.join(source.file_name().unwrap()),
            directory,
            sha256: String::new(),
        };
        let mut hash = Sha256::new();
        let mut total = 0;
        let mut count = 0;
        if source.is_file() {
            copy_file(
                &source,
                &snapshot.path,
                &fs::metadata(&source)
                    .map_err(|_| Error::new("PACKAGE_IO", "Cannot read APK metadata"))?,
                &mut hash,
                &mut total,
                cancel,
            )?;
        } else {
            copy(
                &source,
                &snapshot.path,
                &source,
                &mut hash,
                &mut total,
                &mut count,
                cancel,
            )?;
        }
        snapshot.sha256 = format!("{:x}", hash.finalize());
        Ok(snapshot)
    }
}
fn copy(
    source: &Path,
    destination: &Path,
    root: &Path,
    hash: &mut Sha256,
    total: &mut u64,
    count: &mut usize,
    cancel: &crate::process::Cancellation,
) -> Result<()> {
    cancel.check()?;
    *count += 1;
    if *count > 20000
        || source
            .strip_prefix(root)
            .unwrap_or(source)
            .components()
            .count()
            > 128
    {
        return Err(Error::invalid("App package has too many entries"));
    }
    let metadata = fs::symlink_metadata(source)
        .map_err(|_| Error::new("PACKAGE_IO", "Cannot read app package"))?;
    hash.update(
        source
            .strip_prefix(root)
            .unwrap_or(Path::new(""))
            .as_os_str()
            .as_encoded_bytes(),
    );
    hash.update([0]);
    if metadata.file_type().is_symlink() {
        let link = fs::read_link(source).map_err(|_| Error::invalid("Invalid package symlink"))?;
        if link.is_absolute()
            || !source
                .canonicalize()
                .is_ok_and(|path| path.starts_with(root))
        {
            return Err(Error::invalid(
                "Package symlinks must remain inside the app bundle",
            ));
        }
        hash.update(b"link");
        hash.update((link.as_os_str().as_encoded_bytes().len() as u64).to_be_bytes());
        hash.update(link.as_os_str().as_encoded_bytes());
        #[cfg(unix)]
        std::os::unix::fs::symlink(link, destination)
            .map_err(|_| Error::new("PACKAGE_IO", "Cannot snapshot package symlink"))?;
        #[cfg(not(unix))]
        return Err(Error::new(
            "UNSUPPORTED_HOST",
            "App bundle symlinks require a Unix host",
        ));
    } else if metadata.is_dir() {
        hash.update(b"directory");
        fs::create_dir(destination)
            .map_err(|_| Error::new("PACKAGE_IO", "Cannot snapshot app directory"))?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(source)
            .map_err(|_| Error::new("PACKAGE_IO", "Cannot read app directory"))?
            .take(20001)
        {
            cancel.check()?;
            entries.push(entry.map_err(|_| Error::new("PACKAGE_IO", "Cannot read app entry"))?);
        }
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            copy(
                &entry.path(),
                &destination.join(entry.file_name()),
                root,
                hash,
                total,
                count,
                cancel,
            )?;
        }
    } else if metadata.is_file() {
        hash.update(b"file");
        hash.update(metadata.len().to_be_bytes());
        copy_file(source, destination, &metadata, hash, total, cancel)?;
    } else {
        return Err(Error::invalid("App package contains a special file"));
    }
    Ok(())
}
fn copy_file(
    source: &Path,
    destination: &Path,
    metadata: &fs::Metadata,
    hash: &mut Sha256,
    total: &mut u64,
    cancel: &crate::process::Cancellation,
) -> Result<()> {
    if metadata.len() > 2 * 1024 * 1024 * 1024 - *total {
        return Err(Error::invalid("App package exceeds 2 GiB"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut input = options
        .open(source)
        .map_err(|_| Error::new("PACKAGE_IO", "Cannot open app file"))?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| Error::new("PACKAGE_IO", "Cannot snapshot app file"))?;
    let mut buffer = [0; 65536];
    let mut copied = 0;
    loop {
        cancel.check()?;
        let n = input
            .read(&mut buffer)
            .map_err(|_| Error::new("PACKAGE_IO", "Cannot read app bytes"))?;
        if n == 0 {
            break;
        }
        *total += n as u64;
        copied += n as u64;
        if *total > 2 * 1024 * 1024 * 1024 {
            return Err(Error::invalid("App package exceeds 2 GiB"));
        }
        output
            .write_all(&buffer[..n])
            .map_err(|_| Error::new("PACKAGE_IO", "Cannot snapshot app bytes"))?;
        hash.update(&buffer[..n]);
    }
    if copied != metadata.len() {
        return Err(Error::new(
            "PACKAGE_CHANGED",
            "App package changed while being prepared",
        ));
    }
    fs::set_permissions(destination, metadata.permissions())
        .map_err(|_| Error::new("PACKAGE_IO", "Cannot preserve app permissions"))?;
    Ok(())
}
pub(super) fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 256
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(Error::invalid(
            "Invalid application bundle/package identifier",
        ));
    }
    Ok(())
}
pub(super) fn ios_identity(path: &Path, simulator: bool) -> Result<Identity> {
    if path.extension().and_then(|s| s.to_str()) != Some("app") {
        return Err(Error::invalid("iOS requires an .app directory"));
    }
    if std::fs::metadata(path.join("Info.plist"))
        .map_err(|_| Error::invalid("App bundle is missing Info.plist"))?
        .len()
        > 1024 * 1024
    {
        return Err(Error::invalid("App Info.plist is too large"));
    }
    let bytes = std::fs::read(path.join("Info.plist"))
        .map_err(|_| Error::invalid("App bundle is missing Info.plist"))?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::invalid("App Info.plist is too large"));
    }
    let plist: plist::Value =
        plist::from_bytes(&bytes).map_err(|_| Error::invalid("Invalid app Info.plist"))?;
    let plist = plist
        .as_dictionary()
        .ok_or_else(|| Error::invalid("App Info.plist must be a dictionary"))?;
    let string = |key| {
        plist
            .get(key)
            .and_then(plist::Value::as_string)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .ok_or_else(|| Error::invalid(format!("App Info.plist omitted {key}")))
    };
    let platform = if simulator {
        "iPhoneSimulator"
    } else {
        "iPhoneOS"
    };
    if !plist
        .get("CFBundleSupportedPlatforms")
        .and_then(plist::Value::as_array)
        .is_some_and(|values| values.iter().any(|v| v.as_string() == Some(platform)))
    {
        return Err(Error::new(
            "PACKAGE_PLATFORM_MISMATCH",
            "App bundle was built for another platform",
        ));
    }
    let executable = string("CFBundleExecutable")?;
    if Path::new(&executable).components().count() != 1 || !path.join(executable).is_file() {
        return Err(Error::invalid("App bundle executable is unavailable"));
    }
    Ok(Identity {
        bundle_id: string("CFBundleIdentifier")?,
        version: string("CFBundleShortVersionString").unwrap_or_default(),
        build: string("CFBundleVersion")?,
    })
}
