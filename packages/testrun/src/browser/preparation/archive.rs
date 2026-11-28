//! Bounded archive extraction and complete installed-tree integrity checks.
use super::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Read, path::Component};

const MAX_FILES: usize = 100_000;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub(super) fn io(error: std::io::Error) -> Error {
    Error::new("BUNDLE_IO", error.to_string())
}
fn invalid() -> Error {
    Error::new(
        "BUNDLE_INVALID",
        "Browser archive contains an invalid path, type, link, size or identity",
    )
}

fn digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(io)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(io)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub(super) fn verify_download(path: &Path, artifact: &Artifact) -> Result<()> {
    if fs::symlink_metadata(path).map_err(io)?.len() != artifact.bytes
        || digest(path)? != artifact.sha256
    {
        return Err(Error::new(
            "BUNDLE_CHECKSUM",
            "Downloaded browser archive differs from its pinned size or SHA-256",
        ));
    }
    Ok(())
}
fn relative(path: &Path) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                #[cfg(windows)]
                windows_component(part)?;
                result.push(part);
            }
            Component::CurDir => {}
            _ => return Err(invalid()),
        }
    }
    if result.as_os_str().is_empty()
        || result.as_os_str().len() > 4096
        || result.components().count() > 64
    {
        return Err(invalid());
    }
    Ok(result)
}

#[cfg(windows)]
fn windows_component(part: &std::ffi::OsStr) -> Result<()> {
    let part = part.to_str().ok_or_else(invalid)?;
    if !part.is_ascii()
        || part.ends_with([' ', '.'])
        || part
            .bytes()
            .any(|byte| byte < 32 || b"<>:\"/\\|?*".contains(&byte))
    {
        return Err(invalid());
    }
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    // An 8.3 alias could resolve an entry under a second spelling on NTFS.
    if stem.rsplit_once('~').is_some_and(|(prefix, serial)| {
        !prefix.is_empty() && !serial.is_empty() && serial.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        return Err(invalid());
    }
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.len() == 4
        && stem.as_bytes()[3].is_ascii_digit())
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(all(test, windows))]
#[test]
fn windows_archive_refuses_dos_alias_components() {
    use std::ffi::OsStr;
    assert!(windows_component(OsStr::new("ACMELO~1.DLL")).is_err());
    assert!(windows_component(OsStr::new("acme~notes.txt")).is_ok());
}
fn safe_link(path: &Path, target: &Path) -> Result<()> {
    let mut depth = path.parent().map_or(0, |p| p.components().count());
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return Err(invalid()),
        }
    }
    Ok(())
}

struct Extractor<'a> {
    root: &'a Path,
    seen: BTreeSet<PathBuf>,
    links: Vec<(PathBuf, PathBuf)>,
    bytes: u64,
}
impl Extractor<'_> {
    fn entry(
        &mut self,
        path: &Path,
        kind: u32,
        size: u64,
        mode: u32,
        mut data: impl Read,
    ) -> Result<()> {
        let path = relative(path)?;
        #[cfg(unix)]
        let key = path.clone();
        #[cfg(windows)]
        let key = PathBuf::from(path.to_str().ok_or_else(invalid)?.to_ascii_lowercase());
        if !self.seen.insert(key) || self.seen.len() > MAX_FILES {
            return Err(invalid());
        }
        self.bytes = self.bytes.checked_add(size).ok_or_else(invalid)?;
        if self.bytes > MAX_BYTES || size > 1024 * 1024 * 1024 {
            return Err(invalid());
        }
        let output = self.root.join(&path);
        if kind == 0o040000 {
            fs::create_dir_all(&output).map_err(io)?;
            return Ok(());
        }
        if kind == 0o120000 {
            if size > 4096 {
                return Err(invalid());
            }
            let mut bytes = Vec::new();
            data.read_to_end(&mut bytes).map_err(io)?;
            let target = PathBuf::from(String::from_utf8(bytes).map_err(|_| invalid())?);
            safe_link(&path, &target)?;
            self.links.push((path, target));
            return Ok(());
        }
        if kind != 0 && kind != 0o100000 {
            return Err(invalid());
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&output)
            .map_err(io)?;
        if std::io::copy(&mut data.by_ref().take(size + 1), &mut file).map_err(io)? != size {
            return Err(invalid());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(if mode & 0o111 != 0 {
                0o700
            } else {
                0o600
            }))
            .map_err(io)?;
        }
        #[cfg(not(unix))]
        let _ = mode;
        crate::artifacts::sync(&file).map_err(io)?;
        Ok(())
    }
    #[cfg(unix)]
    fn finish(self) -> Result<()> {
        for (path, target) in &self.links {
            let output = self.root.join(path);
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(io)?;
            }
            std::os::unix::fs::symlink(target, output).map_err(io)?;
        }
        for (path, _) in &self.links {
            if !fs::canonicalize(self.root.join(path))
                .map_err(io)?
                .starts_with(self.root)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn finish(self) -> Result<()> {
        if self.links.is_empty() {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}
pub(super) fn extract(source: &Path, root: &Path, is_zip: bool) -> Result<()> {
    let root = fs::canonicalize(root).map_err(io)?;
    let mut output = Extractor {
        root: &root,
        seen: BTreeSet::new(),
        links: Vec::new(),
        bytes: 0,
    };
    if is_zip {
        let mut archive =
            zip::ZipArchive::new(fs::File::open(source).map_err(io)?).map_err(|_| invalid())?;
        if archive.len() > MAX_FILES {
            return Err(invalid());
        }
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|_| invalid())?;
            let path = PathBuf::from(entry.name().map_err(|_| invalid())?.as_ref());
            let mode = entry.unix_mode().unwrap_or(0o100600);
            let kind = if entry.is_dir() {
                0o040000
            } else {
                mode & 0o170000
            };
            output.entry(&path, kind, entry.size(), mode, &mut entry)?;
        }
    } else {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(
            fs::File::open(source).map_err(io)?,
        ));
        for entry in archive.entries().map_err(io)? {
            let mut entry = entry.map_err(io)?;
            let path = entry.path().map_err(io)?.into_owned();
            let kind = entry.header().entry_type();
            let mode = entry.header().mode().map_err(io)?;
            if kind.is_symlink() {
                let target = entry
                    .link_name()
                    .map_err(io)?
                    .ok_or_else(invalid)?
                    .into_owned();
                let bytes = target.to_str().ok_or_else(invalid)?.as_bytes();
                output.entry(&path, 0o120000, bytes.len() as u64, mode, bytes)?;
            } else {
                let kind = if kind.is_dir() {
                    0o040000
                } else if kind.is_file() {
                    0o100000
                } else {
                    return Err(invalid());
                };
                output.entry(&path, kind, entry.size(), mode, &mut entry)?;
            }
        }
    }
    output.finish()
}
fn tree(root: &Path, artifact: &Artifact) -> Result<String> {
    let canonical = fs::canonicalize(root).map_err(io)?;
    let root = canonical.as_path();
    let mut hash = Sha256::new();
    let mut pending = vec![root.to_path_buf()];
    let mut count = 0;
    let mut bytes = 0u64;
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(io)?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(io)?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path == root.join("verified.json") {
                continue;
            }
            if manifest::external_extensions_directory(artifact)
                .is_some_and(|directory| path == root.join(directory))
            {
                // Linux Chrome creates this directory when resolving its external
                // extension path. Its presence is harmless only while empty.
                let metadata = fs::symlink_metadata(&path).map_err(io)?;
                if !metadata.is_dir() || fs::read_dir(&path).map_err(io)?.next().is_some() {
                    return Err(invalid());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    // SAFETY: geteuid has no arguments or caller preconditions.
                    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0
                    {
                        return Err(invalid());
                    }
                }
                continue;
            }
            if manifest::has_dependency_marker(artifact)
                && path == root.join("DEPENDENCIES_VALIDATED")
            {
                let metadata = fs::symlink_metadata(&path).map_err(io)?;
                if !metadata.is_file() || metadata.len() != 0 {
                    return Err(invalid());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    // SAFETY: geteuid has no arguments or caller preconditions.
                    if metadata.uid() != unsafe { libc::geteuid() }
                        || metadata.mode() & 0o022 != 0
                        || metadata.nlink() != 1
                    {
                        return Err(invalid());
                    }
                }
                // The pinned Playwright driver writes this empty cache marker
                // after host dependency validation; browser bytes stay hashed.
                continue;
            }
            count += 1;
            if count > MAX_FILES {
                return Err(invalid());
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid())?
                .to_str()
                .ok_or_else(invalid)?;
            hash.update((relative.len() as u64).to_le_bytes());
            hash.update(relative.as_bytes());
            let metadata = fs::symlink_metadata(&path).map_err(io)?;
            #[cfg(windows)]
            if crate::artifacts::is_reparse_point(&metadata) {
                return Err(invalid());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                hash.update(metadata.permissions().mode().to_le_bytes());
            }
            if metadata.is_dir() {
                hash.update(b"d");
                pending.push(path);
            } else if metadata.file_type().is_symlink() {
                hash.update(b"l");
                let target = fs::read_link(&path).map_err(io)?;
                safe_link(Path::new(relative), &target)?;
                if !fs::canonicalize(&path).map_err(io)?.starts_with(root) {
                    return Err(invalid());
                }
                hash.update(target.to_str().ok_or_else(invalid)?.as_bytes());
            } else if metadata.is_file() {
                bytes = bytes.checked_add(metadata.len()).ok_or_else(invalid)?;
                if bytes > MAX_BYTES {
                    return Err(invalid());
                }
                hash.update(b"f");
                hash.update(digest(&path)?.as_bytes());
            } else {
                return Err(invalid());
            }
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub(super) fn seal(root: &Path, artifact: &Artifact) -> Result<()> {
    if !root.join(artifact.entry).is_file() {
        return Err(invalid());
    }
    let record = json!({"id":artifact.id,"archiveSha256":artifact.sha256,"archiveBytes":artifact.bytes,"treeSha256":tree(root, artifact)?,"source":artifact.url,"license":artifact.license});
    crate::artifacts::write(
        &root.join("verified.json"),
        &serde_json::to_vec(&record).map_err(|_| invalid())?,
    )?;
    #[cfg(unix)]
    sync_directories(root)?;
    Ok(())
}
#[cfg(unix)]
fn sync_directories(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root).map_err(io)? {
        let path = entry.map_err(io)?.path();
        if fs::symlink_metadata(&path).map_err(io)?.is_dir() {
            sync_directories(&path)?;
        }
    }
    crate::artifacts::sync(&fs::File::open(root).map_err(io)?).map_err(io)
}

pub(super) fn verify_install(root: &Path, artifact: &Artifact) -> Result<()> {
    #[cfg(unix)]
    crate::artifacts::check_private_directory(root)?;
    #[cfg(windows)]
    {
        let metadata = fs::symlink_metadata(root.join("verified.json")).map_err(io)?;
        if !metadata.is_file() || crate::artifacts::is_reparse_point(&metadata) {
            return Err(invalid());
        }
        private_tree(root)?;
    }
    let record: Value = serde_json::from_slice(&fs::read(root.join("verified.json")).map_err(io)?)
        .map_err(|_| invalid())?;
    if record["id"] != artifact.id
        || record["archiveSha256"] != artifact.sha256
        || record["archiveBytes"] != artifact.bytes
        || record["treeSha256"] != tree(root, artifact)?
    {
        return Err(Error::new(
            "BUNDLE_CORRUPT",
            "The private browser installation changed; preserve active sessions and replace this exact cache build before reconnecting",
        ));
    }
    Ok(())
}
