//! Native leaf identity is fixed to canonical read-only boot resources and dyld contract.
use super::Error;
use crate::policy;
use serde_json::{Value, json};
#[cfg(target_os = "macos")]
use std::os::unix::fs::MetadataExt;
use std::{fs, io::Read, path::Path};

#[cfg(target_os = "macos")]
struct Resource {
    path: String,
    file: fs::File,
    identity: (u64, u64, u64, i64, i64),
    digest: String,
    read_only: bool,
}

#[cfg(target_os = "macos")]
impl Resource {
    fn open(path: &str, read_only: bool) -> Result<(Self, Vec<u8>), Error> {
        if Path::new(path).canonicalize()? != Path::new(path) {
            return Err(Error::Unsupported);
        }
        let file = fs::File::open(path)?;
        let metadata = file.metadata()?;
        let volume = nix::sys::statvfs::fstatvfs(&file).map_err(std::io::Error::from)?;
        let root = nix::sys::statvfs::statvfs("/").map_err(std::io::Error::from)?;
        if !metadata.is_file()
            || metadata.uid() != 0
            || metadata.mode() & 0o6022 != 0
            || metadata.mode() & 0o111 == 0
            || metadata.len() > 16_777_216
            || (read_only && volume.filesystem_id() != root.filesystem_id())
            || (read_only
                && !volume
                    .flags()
                    .contains(nix::sys::statvfs::FsFlags::ST_RDONLY))
        {
            return Err(Error::Unsupported);
        }
        let mut bytes = Vec::new();
        (&file).take(16_777_217).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != metadata.len() {
            return Err(Error::Changed);
        }
        let resource = Self {
            path: path.into(),
            file,
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.len(),
                metadata.mtime(),
                metadata.mtime_nsec(),
            ),
            digest: policy::fingerprint(&bytes),
            read_only,
        };
        resource.verify()?;
        Ok((resource, bytes))
    }
    fn verify(&self) -> Result<(), Error> {
        let path = fs::symlink_metadata(&self.path)?;
        let opened = self.file.metadata()?;
        if !path.is_file()
            || path.uid() != 0
            || path.mode() & 0o6022 != 0
            || path.mode() & 0o111 == 0
            || self.identity
                != (
                    path.dev(),
                    path.ino(),
                    path.len(),
                    path.mtime(),
                    path.mtime_nsec(),
                )
            || opened.dev() != path.dev()
            || opened.ino() != path.ino()
            || Path::new(&self.path).canonicalize()? != Path::new(&self.path)
            || (self.read_only
                && !nix::sys::statvfs::fstatvfs(&self.file)
                    .map_err(std::io::Error::from)?
                    .flags()
                    .contains(nix::sys::statvfs::FsFlags::ST_RDONLY))
        {
            return Err(Error::Changed);
        }
        if !self.read_only {
            for parent in Path::new(&self.path).ancestors().skip(1) {
                let metadata = fs::metadata(parent)?;
                if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                    return Err(Error::Unsupported);
                }
            }
            let mut bytes = Vec::new();
            fs::File::open(&self.path)?
                .take(16_777_217)
                .read_to_end(&mut bytes)?;
            if policy::fingerprint(&bytes) != self.digest {
                return Err(Error::Changed);
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub(super) struct Binding {
    executable: Resource,
    loader: Resource,
    cwd: String,
    cwd_identity: (u64, u64),
    request: policy::Request,
    git: Option<Value>,
}

#[cfg(target_os = "macos")]
impl Binding {
    pub(super) fn open(request: &policy::Request) -> Result<Self, Error> {
        if !super::supported(request) {
            return Err(Error::Unsupported);
        }
        let is_git = request.executable == super::git::EXECUTABLE;
        let (executable, bytes) = Resource::open(&request.executable, !is_git)?;
        let cpu = if cfg!(target_arch = "aarch64") {
            0x0100_000c
        } else if cfg!(target_arch = "x86_64") {
            0x0100_0007
        } else {
            return Err(Error::Unsupported);
        };
        super::macho::verify_family(&bytes, cpu, is_git)?;
        let (loader, _) = Resource::open("/usr/lib/dyld", true)?;
        let cwd = fs::metadata(&request.cwd)?;
        Ok(Self {
            executable,
            loader,
            cwd: request.cwd.clone(),
            cwd_identity: (cwd.dev(), cwd.ino()),
            request: request.clone(),
            git: if is_git {
                Some(super::git::capture(request)?)
            } else {
                None
            },
        })
    }
    pub(super) fn verify(&self) -> Result<(), Error> {
        self.executable.verify()?;
        self.loader.verify()?;
        if let Some(git) = &self.git
            && *git != super::git::capture(&self.request)?
        {
            return Err(Error::Changed);
        }
        let cwd = fs::metadata(&self.cwd)?;
        if !cwd.is_dir()
            || Path::new(&self.cwd).canonicalize()? != Path::new(&self.cwd)
            || self.cwd_identity != (cwd.dev(), cwd.ino())
        {
            return Err(Error::Changed);
        }
        Ok(())
    }
    pub(super) fn metadata(&self) -> Value {
        let mut value = json!({"model":"macos-readonly-system-leaf-v1","executable":self.executable.path,"sha256":self.executable.digest,"executable_identity":self.executable.identity,"loader":self.loader.path,"loader_sha256":self.loader.digest,"loader_identity":self.loader.identity,"cwd_identity":self.cwd_identity,"library_contract":"OS libSystem/shared cache","environment":"cleared-lc-all-c"});
        if let Some(git) = &self.git {
            value["model"] = json!("macos-clt-git-inspection-v1");
            value["library_contract"] = json!("OS Git libraries/shared cache");
            value["environment"] = json!("cleared-git-inspection-v1");
            value["git"] = git.clone();
        }
        value
    }
}
