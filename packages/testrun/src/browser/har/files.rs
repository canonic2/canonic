//! Copy attachment bytes into bounded owned memory; never extract archives.
//! Caller exports write only new private files.
use super::*;
use std::{
    io::{Cursor, Read},
    path::{Component, Path},
};

fn read(mut input: impl Read, max: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::caused("HAR_IO", error))?;
    if bytes.len() > max {
        return Err("HAR_ARCHIVE_BOUNDS".into());
    }
    Ok(bytes)
}
fn private(path: &Path, max: usize) -> Result<Vec<u8>, Error> {
    if !path.is_absolute() {
        return Err("HAR_PRIVATE_FILE_REQUIRED".into());
    }
    crate::artifacts::check_private_directory(path.parent().ok_or("HAR_PRIVATE_FILE_REQUIRED")?)
        .map_err(|_| Error::from("HAR_PRIVATE_FILE_REQUIRED"))?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A replaced special file must not block before descriptor checks.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|error| Error::caused("HAR_IO", error))?;
    let metadata = file
        .metadata()
        .map_err(|error| Error::caused("HAR_IO", error))?;
    if !metadata.is_file() {
        return Err("HAR_PRIVATE_FILE_REQUIRED".into());
    }
    if metadata.len() > max as u64 {
        return Err("HAR_ARCHIVE_BOUNDS".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err("HAR_PRIVATE_FILE_REQUIRED".into());
        }
    }
    read(file, max)
}
fn attachment(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.len() > 1024
        || name.contains('\\')
        || name.chars().any(char::is_control)
        || Path::new(name)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("HAR_INVALID_ATTACHMENT".into());
    }
    Ok(())
}
/// A caller-chosen copy of a published capture: pinned `startHar(path)`/`recordHar.path`
/// with an optional `resourcesDir` for attached bodies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Export {
    path: std::path::PathBuf,
    resources: Option<std::path::PathBuf>,
}

fn private_export_directory(directory: &Path) -> Result<(), Error> {
    #[cfg(windows)]
    let result = canonic_process_kit::endpoint::check_private_directory(directory);
    #[cfg(not(windows))]
    let result = crate::artifacts::check_private_directory(directory);
    result.map_err(|_| Error::from("HAR_PRIVATE_FILE_REQUIRED"))
}

fn write_export(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    #[cfg(windows)]
    let result = crate::artifacts::write_private(path, bytes);
    #[cfg(not(windows))]
    let result = crate::artifacts::write(path, bytes);
    result.map_err(|_| Error::from("HAR_EXPORT_IO"))
}

impl Export {
    /// Validate before capture starts: absolute `.har`/`.zip` path in a private
    /// directory, not yet existing; `resourcesDir` only for attached `.har` output.
    pub(crate) fn new(
        path: &str,
        resources: Option<&str>,
        content: CaptureContent,
    ) -> Result<Self, Error> {
        let path = std::path::PathBuf::from(path);
        let zip = path.extension().is_some_and(|extension| extension == "zip");
        if path.as_os_str().len() > 4096
            || !path.is_absolute()
            || !(zip || path.extension().is_some_and(|extension| extension == "har"))
            || path.components().any(|c| matches!(c, Component::ParentDir))
        {
            return Err("HAR_EXPORT_PATH_INVALID".into());
        }
        private_export_directory(path.parent().ok_or("HAR_EXPORT_PATH_INVALID")?)?;
        if std::fs::symlink_metadata(&path).is_ok() {
            return Err("HAR_EXPORT_EXISTS".into());
        }
        let resources = resources
            .map(|directory| {
                let directory = std::path::PathBuf::from(directory);
                if zip || content != CaptureContent::Attach || !directory.is_absolute() {
                    return Err(Error::from("HAR_EXPORT_RESOURCES_INVALID"));
                }
                private_export_directory(&directory)?;
                Ok(directory)
            })
            .transpose()?;
        Ok(Self { path, resources })
    }
    /// Write the archive without replacing any file; attached `.har` output writes
    /// one private file per distinct body beside it or in `resourcesDir`.
    pub(crate) fn write(&self, archive: &Archive, content: CaptureContent) -> Result<Value, Error> {
        #[cfg(windows)]
        {
            private_export_directory(self.path.parent().expect("Validated parent"))?;
            if let Some(resources) = &self.resources {
                private_export_directory(resources)?;
            }
        }
        if self
            .path
            .extension()
            .is_some_and(|extension| extension == "zip")
        {
            write_export(&self.path, &archive.encode_attached()?)?;
            return Ok(json!({"path":self.path,"format":"zip","attachments":0}));
        }
        if content != CaptureContent::Attach {
            write_export(&self.path, &archive.encode()?)?;
            return Ok(json!({"path":self.path,"format":"har","attachments":0}));
        }
        let (value, members) = archive.encoded_members()?;
        let directory = self
            .resources
            .clone()
            .unwrap_or_else(|| self.path.parent().expect("Validated parent").to_path_buf());
        // Bodies first: a published `.har` never names a missing attachment.
        for (name, bytes) in &members {
            write_export(&directory.join(name), bytes)?;
        }
        let bytes = serde_json::to_vec(&value).map_err(|_| Error::from("HAR_INVALID_ARCHIVE"))?;
        write_export(&self.path, &bytes)?;
        Ok(
            json!({"path":self.path,"format":"har","attachments":members.len(),"resourcesDir":self.resources}),
        )
    }
}
impl Archive {
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        let bytes = private(path, MAX_INPUT)?;
        if path.extension().is_some_and(|extension| extension == "zip") {
            let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
                .map_err(|_| Error::from("HAR_INVALID_ARCHIVE"))?;
            if zip.len() > 512 {
                return Err("HAR_ENTRY_BOUNDS".into());
            }
            let mut names = BTreeSet::new();
            let mut har = None;
            for index in 0..zip.len() {
                let entry = zip
                    .by_index(index)
                    .map_err(|_| Error::from("HAR_INVALID_ARCHIVE"))?;
                let name = entry
                    .name()
                    .map_err(|_| Error::from("HAR_INVALID_ATTACHMENT"))?
                    .into_owned();
                if !names.insert(name.clone()) {
                    return Err("HAR_AMBIGUOUS_ATTACHMENT".into());
                }
                if name.ends_with(".har") && har.is_none() {
                    har = Some(index);
                }
            }
            let bytes = read(
                zip.by_index(har.ok_or("HAR_INVALID_ARCHIVE")?)
                    .map_err(|_| Error::from("HAR_INVALID_ARCHIVE"))?,
                MAX_INPUT,
            )?;
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| Error::from("HAR_INVALID_ARCHIVE"))?;
            Self::parse(&value, |name| {
                attachment(name)?;
                let entry = zip
                    .by_name(name)
                    .map_err(|_| Error::from("HAR_ATTACHMENT_UNAVAILABLE"))?;
                if entry.is_dir()
                    || entry
                        .unix_mode()
                        .is_some_and(|mode| mode & 0o170000 == 0o120000)
                {
                    return Err("HAR_INVALID_ATTACHMENT".into());
                }
                read(entry, MAX_BODY)
            })
        } else {
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| Error::from("HAR_INVALID_ARCHIVE"))?;
            let parent = path.parent().ok_or("HAR_PRIVATE_FILE_REQUIRED")?;
            Self::parse(&value, |name| {
                attachment(name)?;
                let target = parent.join(name);
                // Reject each link before resolving; an attachment must remain
                // beneath its private source directory without traversal.
                let mut current = parent.to_path_buf();
                for component in Path::new(name).components() {
                    current.push(component);
                    if std::fs::symlink_metadata(&current)
                        .map_err(|error| Error::caused("HAR_IO", error))?
                        .file_type()
                        .is_symlink()
                    {
                        return Err("HAR_INVALID_ATTACHMENT".into());
                    }
                }
                private(&target, MAX_BODY)
            })
        }
    }
}
