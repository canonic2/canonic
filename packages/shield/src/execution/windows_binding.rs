//! Protected System32 leaf images, held namespace/identity and explicit cleared environment.
use super::{Error, windows_pe};
use crate::policy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::Read,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const MAX_IMAGE: u64 = 16_777_216;
const MAX_IMAGES: usize = 64;
const MAX_BYTES: u64 = 67_108_864;
const MODEL: &str = "windows-protected-system-leaf-v1";
const LIBRARIES: &str = "protected-system32-and-os-api-sets-v1";
const DLL_SEARCH: &str = "protected-system32-no-path-v1";

pub(super) struct Binding(Kind);
enum Kind {
    Leaf(LeafBinding),
    Git(super::windows_git::Binding),
}
impl Binding {
    pub(super) fn open(request: &policy::Request) -> Result<Self, Error> {
        if super::windows_git::application_directory(request).is_some() {
            super::windows_git::Binding::open(request).map(|binding| Self(Kind::Git(binding)))
        } else {
            LeafBinding::open(request).map(|binding| Self(Kind::Leaf(binding)))
        }
    }
    pub(super) fn verify(&self) -> Result<(), Error> {
        match &self.0 {
            Kind::Leaf(binding) => binding.verify(),
            Kind::Git(binding) => binding.verify(),
        }
    }
    pub(super) fn metadata(&self) -> Value {
        match &self.0 {
            Kind::Leaf(binding) => binding.metadata(),
            Kind::Git(binding) => binding.metadata(),
        }
    }
}

pub(super) fn environment_for(
    request: &policy::Request,
) -> Result<BTreeMap<String, String>, Error> {
    let mut result = environment()?;
    if super::windows_git::application_directory(request).is_some() {
        result.extend(super::windows_git::environment(request)?);
    }
    Ok(result)
}

pub(super) fn system_directory() -> Result<PathBuf, Error> {
    winsafe::GetSystemDirectory()
        .map(PathBuf::from)
        .map_err(|error| std::io::Error::other(error.to_string()).into())
}

pub(super) fn environment() -> Result<BTreeMap<String, String>, Error> {
    let directory = system_directory()?;
    let root = directory
        .parent()
        .and_then(Path::to_str)
        .ok_or(Error::Unsupported)?;
    Ok(BTreeMap::from([("SystemRoot".into(), root.into())]))
}

pub(super) fn normalized(path: &str) -> String {
    let path = path.replace('/', "\\");
    path.strip_prefix(r"\\?\")
        .unwrap_or(&path)
        .to_ascii_lowercase()
}

pub(super) fn supported(request: &policy::Request) -> bool {
    if windows_pe::native_machine().is_none() {
        return false;
    }
    let Ok(system) = system_directory() else {
        return false;
    };
    for name in ["hostname.exe", "whoami.exe", "timeout.exe"] {
        if normalized(&request.executable) != normalized(&system.join(name).to_string_lossy()) {
            continue;
        }
        return if name == "timeout.exe" {
            request.arguments.len() == 3
                && request.arguments[0].eq_ignore_ascii_case("/t")
                && request.arguments[2].eq_ignore_ascii_case("/nobreak")
                && !request.arguments[1].is_empty()
                && request.arguments[1]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit())
                && request.arguments[1]
                    .parse::<u32>()
                    .is_ok_and(|value| value <= 86_400)
        } else {
            request.arguments.is_empty()
        };
    }
    false
}

pub(super) type Identity = (u64, u64, u64, u64);
pub(super) fn identity(file: &File) -> Result<Identity, Error> {
    let information = winapi_util::file::information(file)?;
    let metadata = file.metadata()?;
    Ok((
        information.volume_serial_number(),
        information.file_index(),
        metadata.len(),
        metadata.last_write_time(),
    ))
}
pub(super) fn directory_identity(file: &File) -> Result<(u64, u64), Error> {
    let information = winapi_util::file::information(file)?;
    Ok((information.volume_serial_number(), information.file_index()))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImageMetadata {
    pub(super) name: String,
    pub(super) sha256: String,
    pub(super) identity: Identity,
}
pub(super) struct Image {
    path: PathBuf,
    file: File,
    pub(super) metadata: ImageMetadata,
}
impl Image {
    pub(super) fn open(path: &Path, name: &str) -> Result<(Self, Vec<u8>), Error> {
        let file = crate::windows_security::open_system(path, false).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("Windows system image {name}: {error}"),
            )
        })?;
        let id = identity(&file)?;
        if id.2 == 0 || id.2 > MAX_IMAGE {
            return Err(Error::Unsupported);
        }
        let mut bytes = Vec::new();
        (&file).take(MAX_IMAGE + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != id.2 {
            return Err(Error::Changed);
        }
        let image = Self {
            path: path.canonicalize()?,
            file,
            metadata: ImageMetadata {
                name: name.into(),
                sha256: policy::fingerprint(&bytes),
                identity: id,
            },
        };
        image.verify()?;
        Ok((image, bytes))
    }
    pub(super) fn verify(&self) -> Result<(), Error> {
        let current = crate::windows_security::open_system(&self.path, false)?;
        if self.path.canonicalize()? != self.path
            || identity(&current)? != self.metadata.identity
            || identity(&self.file)? != self.metadata.identity
        {
            return Err(Error::Changed);
        }
        // The retained file denies write/delete sharing; no writer can change
        // bytes between this revalidation and CreateProcess opening the image.
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DirectoryMetadata {
    pub(super) path: PathBuf,
    pub(super) identity: (u64, u64),
}
pub(super) struct Directory {
    file: File,
    pub(super) metadata: DirectoryMetadata,
    system: bool,
}
impl Directory {
    pub(super) fn open(path: &Path, system: bool) -> Result<Self, Error> {
        let path = path.canonicalize()?;
        let file = if system {
            crate::windows_security::open_system(&path, true)?
        } else {
            OpenOptions::new()
                .read(true)
                .access_mode(0x80)
                .share_mode(3)
                .custom_flags(0x0200_0000 | 0x0020_0000)
                .open(&path)?
        };
        let metadata = file.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::Unsupported);
        }
        let identity = directory_identity(&file)?;
        Ok(Self {
            file,
            metadata: DirectoryMetadata { path, identity },
            system,
        })
    }
    pub(super) fn verify(&self) -> Result<(), Error> {
        let current = Self::open(&self.metadata.path, self.system)?;
        if current.metadata.path != self.metadata.path
            || current.metadata.identity != self.metadata.identity
            || directory_identity(&self.file)? != self.metadata.identity
        {
            return Err(Error::Changed);
        }
        Ok(())
    }
}

struct LeafBinding {
    request: policy::Request,
    system_directory: PathBuf,
    environment: BTreeMap<String, String>,
    images: BTreeMap<String, Image>,
    api_sets: BTreeSet<String>,
    directories: Vec<Directory>,
    cwd_identity: (u64, u64),
}
impl LeafBinding {
    pub(super) fn open(request: &policy::Request) -> Result<Self, Error> {
        if !supported(request) {
            return Err(Error::Unsupported);
        }
        let system = system_directory()?.canonicalize()?;
        let cwd = Path::new(&request.cwd);
        // Boot-time drive mappings are protected from ordinary-user mutation.
        // Other drive maps and network cwd bindings need their own verified model.
        if normalized(&request.cwd).get(..3) != normalized(&system.to_string_lossy()).get(..3)
            || cwd.canonicalize()? != cwd
            || !cwd.starts_with(&request.project)
            || cwd
                .strip_prefix(&request.project)
                .ok()
                .and_then(|path| path.components().next())
                .is_some_and(|part| {
                    crate::location::native_name(part.as_os_str(), std::ffi::OsStr::new(".shield"))
                })
        {
            return Err(Error::Unsupported);
        }
        let mut directories = vec![
            Directory::open(system.parent().ok_or(Error::Unsupported)?, true)?,
            Directory::open(&system, true)?,
        ];
        let legacy_system = system.parent().ok_or(Error::Unsupported)?.join("System");
        if legacy_system.try_exists()? {
            directories.push(Directory::open(&legacy_system, true)?);
        }
        let held_cwd = Directory::open(cwd, false)?;
        let cwd_identity = held_cwd.metadata.identity;
        directories.push(held_cwd);
        for parent in cwd.ancestors().skip(1) {
            if directories.len() >= 128 {
                return Err(Error::Unsupported);
            }
            directories.push(Directory::open(parent, false)?);
        }
        let executable = Path::new(&request.executable)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(Error::Unsupported)?
            .to_ascii_lowercase();
        let mut pending = BTreeSet::from([
            executable.clone(),
            "ntdll.dll".into(),
            "kernel32.dll".into(),
            "kernelbase.dll".into(),
            "apisetschema.dll".into(),
        ]);
        let mut images = BTreeMap::new();
        let mut api_sets = BTreeSet::new();
        let mut total = 0;
        while let Some(name) = pending.pop_first() {
            if images.contains_key(&name) {
                continue;
            }
            if windows_pe::api_set(&name) {
                api_sets.insert(name);
                if api_sets.len() > 512 {
                    return Err(Error::Unsupported);
                }
                continue;
            }
            if images.len() >= MAX_IMAGES {
                return Err(Error::Unsupported);
            }
            let (image, bytes) = Image::open(&system.join(&name), &name)?;
            total += bytes.len() as u64;
            if total > MAX_BYTES {
                return Err(Error::Unsupported);
            }
            let dependencies = windows_pe::dependencies(
                &bytes,
                name == executable,
                windows_pe::native_machine().ok_or(Error::Unsupported)?,
            )
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    format!("unverified Windows image load contract: {name}"),
                )
            })?;
            // Pin the executable's declared libraries and the OS loader/API
            // schema. Their transitive implementation belongs to the protected
            // OS search contract, like macOS's libSystem/shared-cache contract;
            // enumerating unused DLL exports would require unrelated optional
            // Windows features even when this leaf never calls them.
            if name == executable {
                pending.extend(dependencies);
            }
            images.insert(name, image);
        }
        let result = Self {
            request: request.clone(),
            system_directory: system,
            environment: environment()?,
            images,
            api_sets,
            directories,
            cwd_identity,
        };
        result.verify()?;
        Ok(result)
    }
    pub(super) fn verify(&self) -> Result<(), Error> {
        if !supported(&self.request) || self.environment != environment()? {
            return Err(Error::Changed);
        }
        for directory in &self.directories {
            directory.verify()?;
        }
        for image in self.images.values() {
            image.verify()?;
        }
        let executable = Path::new(&self.request.executable)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(Error::Changed)?
            .to_ascii_lowercase();
        let selected =
            crate::windows_security::open_system(Path::new(&self.request.executable), false)?;
        let selected = identity(&selected)?;
        if self
            .images
            .get(&executable)
            .is_none_or(|image| image.metadata.identity != selected)
        {
            return Err(Error::Changed);
        }
        Ok(())
    }
    pub(super) fn metadata(&self) -> Value {
        json!({"model":MODEL,"library_contract":LIBRARIES,"executable":self.request.executable,
            "system_directory":self.system_directory,"environment":self.environment,"dll_search":DLL_SEARCH,
            "images":self.images.values().map(|image| &image.metadata).collect::<Vec<_>>(),
            "api_sets":self.api_sets,"cwd_identity":self.cwd_identity,
            "directories":self.directories.iter().map(|directory| &directory.metadata).collect::<Vec<_>>()})
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resources {
    model: String,
    library_contract: String,
    executable: String,
    system_directory: PathBuf,
    environment: BTreeMap<String, String>,
    dll_search: String,
    images: Vec<ImageMetadata>,
    api_sets: BTreeSet<String>,
    cwd_identity: (u64, u64),
    directories: Vec<DirectoryMetadata>,
}

pub(super) fn valid_images(request: &policy::Request, images: &[ImageMetadata]) -> bool {
    let Some(executable) = Path::new(&request.executable)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return false;
    };
    let names: BTreeSet<_> = images.iter().map(|image| image.name.as_str()).collect();
    names.len() == images.len()
        && images.len() <= MAX_IMAGES
        && [
            "ntdll.dll",
            "kernel32.dll",
            "kernelbase.dll",
            "apisetschema.dll",
        ]
        .iter()
        .all(|name| names.contains(name))
        && names.contains(executable.to_ascii_lowercase().as_str())
        && images.iter().all(|image| {
            !image.name.is_empty()
                && image.name.len() <= 128
                && image.name.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-~".contains(&byte)
                })
                && image.sha256.len() == 64
                && image.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                && image.identity.1 != 0
                && (1..=MAX_IMAGE).contains(&image.identity.2)
        })
        && images.iter().map(|image| image.identity.2).sum::<u64>() <= MAX_BYTES
}

pub(super) fn valid(request: &policy::Request, resource: &Value) -> bool {
    if super::windows_git::application_directory(request).is_some() {
        return super::windows_git::valid(request, resource);
    }
    let Ok(resources) = serde_json::from_value::<Resources>(resource.clone()) else {
        return false;
    };
    let Some(system) = resources.system_directory.to_str() else {
        return false;
    };
    supported(request)
        && resources.model == MODEL
        && resources.library_contract == LIBRARIES
        && resources.dll_search == DLL_SEARCH
        && resources.executable == request.executable
        && environment().is_ok_and(|value| value == resources.environment)
        && system_directory()
            .is_ok_and(|value| normalized(&value.to_string_lossy()) == normalized(system))
        && valid_images(request, &resources.images)
        && resources.api_sets.len() <= 512
        && resources
            .api_sets
            .iter()
            .all(|name| windows_pe::api_set(name) && name.len() <= 128)
        && resources.cwd_identity.1 != 0
        && resources.directories.iter().any(|directory| {
            directory.path == Path::new(&request.cwd)
                && directory.identity == resources.cwd_identity
        })
        && !resources.directories.is_empty()
        && resources.directories.len() <= 128
        && resources.directories.iter().all(|directory| {
            directory.path.is_absolute()
                && directory.identity.1 != 0
                && directory
                    .path
                    .to_str()
                    .is_some_and(|path| !path.contains('\0'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_system_leaf_binding_retains_images_and_refuses_changed_resources_or_other_programs()
    {
        let root = assert_fs::TempDir::new().unwrap();
        let cwd = root
            .path()
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        for (name, arguments) in [
            ("hostname.exe", vec![]),
            ("whoami.exe", vec![]),
            (
                "timeout.exe",
                vec!["/t".into(), "1".into(), "/nobreak".into()],
            ),
        ] {
            let request = policy::Request {
                executable: system_directory()
                    .unwrap()
                    .join(name)
                    .to_str()
                    .unwrap()
                    .into(),
                arguments,
                cwd: cwd.clone(),
                project: cwd.clone(),
            };
            let binding = Binding::open(&request).unwrap_or_else(|error| panic!("{name}: {error}"));
            binding.verify().unwrap();
            let metadata = binding.metadata();
            assert!(valid(&request, &metadata));
            assert!(metadata["images"].as_array().unwrap().len() >= 5);
            let mut changed = metadata.clone();
            changed["environment"]["PATH"] = json!("C:\\Acme");
            assert!(!valid(&request, &changed));
            let mut missing = metadata.clone();
            missing["images"]
                .as_array_mut()
                .unwrap()
                .retain(|image| image["name"] != "ntdll.dll");
            assert!(!valid(&request, &missing));
            let mut duplicate = metadata.clone();
            duplicate["images"]
                .as_array_mut()
                .unwrap()
                .push(metadata["images"][0].clone());
            assert!(!valid(&request, &duplicate));
            let mut foreign_cwd = metadata.clone();
            foreign_cwd["cwd_identity"][1] = json!(u64::MAX);
            assert!(!valid(&request, &foreign_cwd));
            let mut changed_request = request.clone();
            changed_request.executable = root.path().join(name).to_str().unwrap().into();
            assert!(!supported(&changed_request));
        }
    }
}
