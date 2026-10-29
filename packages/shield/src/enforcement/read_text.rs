//! Verified bounded text-read resource binding, shared by transport and operator approval.
use crate::policy;
use serde_json::json;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
const MAX_TEXT: u64 = 65_536;

pub(crate) struct Binding {
    pub(crate) path: PathBuf,
    cwd: PathBuf,
    file: fs::File,
    metadata: fs::Metadata,
    pub(crate) digest: String,
    project: PathBuf,
    access_paths: crate::configuration::AccessPaths,
    roots: Vec<PathBuf>,
}
impl Binding {
    #[cfg(test)]
    pub(crate) fn open(root: &Path, cwd: &Path, raw: &str) -> Result<Self, ()> {
        Self::open_scoped(
            root,
            cwd,
            raw,
            &crate::configuration::AccessPaths::default(),
        )
    }
    pub(crate) fn open_scoped(
        root: &Path,
        cwd: &Path,
        raw: &str,
        paths: &crate::configuration::AccessPaths,
    ) -> Result<Self, ()> {
        let path = Path::new(raw);
        if raw.len() > 4096
            || !cwd.is_absolute()
            || !cwd.starts_with(root)
            || !path.is_absolute()
            || path.extension().is_none_or(|ext| ext != "txt")
        {
            return Err(());
        }
        let canonical = path.canonicalize().map_err(|_| ())?;
        let roots = crate::location::Observer::readable_roots(root, cwd, paths).ok_or(())?;
        if canonical != path
            || !roots.iter().any(|root| canonical.starts_with(root))
            || roots
                .iter()
                .any(|root| canonical.starts_with(root.join(".shield")))
        {
            return Err(());
        }
        let metadata = fs::symlink_metadata(&canonical).map_err(|_| ())?;
        if !metadata.is_file() || metadata.len() > MAX_TEXT || !same_file(&metadata, &metadata) {
            return Err(());
        }
        let file = fs::File::open(&canonical).map_err(|_| ())?;
        let actual = file.metadata().map_err(|_| ())?;
        if !same_file(&metadata, &actual) || !actual.is_file() || actual.len() > MAX_TEXT {
            return Err(());
        }
        let mut bytes = Vec::new();
        (&file)
            .take(MAX_TEXT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ())?;
        if bytes.len() as u64 > MAX_TEXT
            || bytes.contains(&0)
            || std::str::from_utf8(&bytes).is_err()
            || bytes.starts_with(b"%PDF-")
        {
            return Err(());
        }
        let digest = resource_digest(&canonical, cwd, &actual, &bytes);
        let binding = Self {
            path: canonical,
            cwd: cwd.to_owned(),
            file,
            metadata: actual,
            digest,
            project: root.to_owned(),
            access_paths: paths.clone(),
            roots,
        };
        binding.verify()?;
        Ok(binding)
    }

    pub(crate) fn verify(&self) -> Result<(), ()> {
        if crate::location::Observer::readable_roots(&self.project, &self.cwd, &self.access_paths)
            .as_ref()
            != Some(&self.roots)
            || self.path.canonicalize().map_err(|_| ())? != self.path
            || self.cwd.canonicalize().map_err(|_| ())? != self.cwd
            || !self.cwd.is_dir()
        {
            return Err(());
        }
        let current = fs::symlink_metadata(&self.path).map_err(|_| ())?;
        let actual = self.file.metadata().map_err(|_| ())?;
        if !same_file(&current, &actual)
            || !same_file(&self.metadata, &actual)
            || actual.len() != self.metadata.len()
            || actual.modified().map_err(|_| ())? != self.metadata.modified().map_err(|_| ())?
        {
            return Err(());
        }
        let mut file = &self.file;
        file.seek(SeekFrom::Start(0)).map_err(|_| ())?;
        let mut bytes = Vec::new();
        file.take(MAX_TEXT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ())?;
        if bytes.len() as u64 > MAX_TEXT
            || resource_digest(&self.path, &self.cwd, &actual, &bytes) != self.digest
        {
            return Err(());
        }
        Ok(())
    }
}

#[cfg(unix)]
fn resource_digest(path: &Path, cwd: &Path, metadata: &fs::Metadata, bytes: &[u8]) -> String {
    use std::os::unix::fs::MetadataExt;
    policy::fingerprint(
        json!([
            path,
            cwd,
            metadata.dev(),
            metadata.ino(),
            metadata.len(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            policy::fingerprint(bytes)
        ])
        .to_string()
        .as_bytes(),
    )
}
#[cfg(not(unix))]
fn resource_digest(_: &Path, _: &Path, _: &fs::Metadata, _: &[u8]) -> String {
    String::new()
}

pub(crate) fn verify_approval_read(
    request: &policy::Request,
    digest: &str,
    paths: &crate::configuration::AccessPaths,
) -> Result<(), crate::review::Error> {
    if !valid_request(request) {
        return Err(crate::review::Error::Invalid("invalid read request"));
    }
    let path = request
        .arguments
        .first()
        .ok_or(crate::review::Error::Invalid("read path missing"))?;
    let binding = Binding::open_scoped(
        Path::new(&request.project),
        Path::new(&request.cwd),
        path,
        paths,
    )
    .map_err(|()| crate::review::Error::Invalid("read binding changed"))?;
    if binding.digest != digest {
        return Err(crate::review::Error::Invalid("read binding changed"));
    }
    Ok(())
}

pub(crate) fn valid_request(request: &policy::Request) -> bool {
    if request.executable != "shield:read-text"
        || request.arguments.is_empty()
        || !Path::new(&request.arguments[0]).is_absolute()
        || !Path::new(&request.cwd).starts_with(&request.project)
    {
        return false;
    }
    let mut tail = &request.arguments[1..];
    for name in ["--offset", "--limit"] {
        if tail.first().is_some_and(|v| v == name) {
            if tail.get(1).is_none_or(|v| {
                v.parse::<u64>().map_or(true, |n| {
                    !(1..=1_000_000).contains(&n) || n.to_string() != *v
                })
            }) {
                return false;
            }
            tail = &tail[2..];
        }
    }
    tail.is_empty()
}

#[cfg(unix)]
fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.is_file()
        && b.is_file()
        && a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.nlink() == 1
        && b.nlink() == 1
}
#[cfg(not(unix))]
fn same_file(_: &fs::Metadata, _: &fs::Metadata) -> bool {
    false
}
