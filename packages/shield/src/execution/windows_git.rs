//! Fixed Git-for-Windows inspections with protected application images and held repository inputs.
use super::{Error, git, windows_binding as native, windows_pe};
use crate::policy;
use native::{Directory, DirectoryMetadata, Identity, Image, ImageMetadata};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::Read,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const MODEL: &str = if cfg!(target_arch = "aarch64") {
    "windows-clangarm64-git-inspection-v1"
} else {
    "windows-ucrt64-git-inspection-v1"
};
const VERSION: &str = "2.56.0.windows.3";
const LIBRARIES: &str = if cfg!(target_arch = "aarch64") {
    "protected-clangarm64-application-and-system32-v1"
} else {
    "protected-ucrt64-application-and-system32-v1"
};

fn runtime_directory() -> Option<&'static str> {
    match windows_pe::native_machine()? {
        windows_pe::Machine::Amd64 => Some("ucrt64"),
        windows_pe::Machine::Arm64 => Some("clangarm64"),
    }
}
const APPLICATION_DLLS: [&str; 4] = [
    "libiconv-2.dll",
    "libintl-8.dll",
    "libpcre2-8-0.dll",
    "zlib1.dll",
];

/// This identifies the supported layout, not publisher authenticity. Opening
/// the binding separately verifies protected owners/ACLs, PE bytes and version.
pub(super) fn application_directory(request: &policy::Request) -> Option<&Path> {
    let executable = Path::new(&request.executable);
    let bin = executable.parent()?;
    let runtime = bin.parent()?;
    (windows_pe::native_machine().is_some()
        && executable.is_absolute()
        && executable.file_name()?.eq_ignore_ascii_case("git.exe")
        && bin.file_name()?.eq_ignore_ascii_case("bin")
        && runtime
            .file_name()?
            .eq_ignore_ascii_case(runtime_directory()?))
    .then_some(bin)
}

fn helpers(request: &policy::Request) -> Result<PathBuf, Error> {
    Ok(application_directory(request)
        .and_then(Path::parent)
        .ok_or(Error::Unsupported)?
        .join("libexec")
        .join("git-core"))
}

fn git_path(path: &Path) -> Result<String, Error> {
    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            let name = name.to_str().ok_or(Error::Unsupported)?;
            let stem = name
                .split('.')
                .next()
                .unwrap_or_default()
                .trim_end_matches(' ')
                .to_ascii_uppercase();
            let device = matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            ) || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|number| {
                    matches!(
                        number,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            });
            if device
                || name.ends_with(['.', ' '])
                || name
                    .chars()
                    .any(|character| character.is_control() || "<>:\"|?*".contains(character))
            {
                return Err(Error::Unsupported);
            }
        }
    }
    let path = path.to_str().ok_or(Error::Unsupported)?;
    let path = path
        .strip_prefix(r"\\?\")
        .unwrap_or(path)
        .replace('\\', "/");
    if !path.as_bytes().get(..3).is_some_and(|prefix| {
        prefix[0].is_ascii_alphabetic() && prefix[1] == b':' && prefix[2] == b'/'
    }) {
        return Err(Error::Unsupported);
    }
    Ok(path)
}

pub(super) fn environment(request: &policy::Request) -> Result<BTreeMap<String, String>, Error> {
    let mut result: BTreeMap<String, String> = [
        ("LC_ALL", "C"),
        ("GIT_CONFIG_GLOBAL", "NUL"),
        ("GIT_CONFIG_SYSTEM", "NUL"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_OPTIONAL_LOCKS", "0"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_NO_LAZY_FETCH", "1"),
        ("GIT_ATTR_NOSYSTEM", "1"),
        (
            "GIT_ATTR_SOURCE",
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
        ),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect();
    for (key, path) in [
        ("GIT_DIR", Path::new(&request.project).join(".git")),
        ("GIT_WORK_TREE", PathBuf::from(&request.project)),
        ("GIT_EXEC_PATH", helpers(request)?),
    ] {
        result.insert(key.into(), git_path(&path)?);
    }
    Ok(result)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepositoryFileMetadata {
    name: String,
    identity: Identity,
    sha256: String,
}
struct RepositoryFile {
    path: PathBuf,
    file: File,
    metadata: RepositoryFileMetadata,
}
impl RepositoryFile {
    fn open(directory: &Path, name: &str) -> Result<Self, Error> {
        let path = directory.join(name);
        let file = Self::read_handle(&path)?;
        let identity = native::identity(&file)?;
        if identity.2 > 1_048_576 {
            return Err(Error::Unsupported);
        }
        let mut bytes = Vec::new();
        (&file).take(1_048_577).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != identity.2 || native::identity(&file)? != identity {
            return Err(Error::Changed);
        }
        let result = Self {
            path,
            file,
            metadata: RepositoryFileMetadata {
                name: name.into(),
                identity,
                sha256: policy::fingerprint(&bytes),
            },
        };
        result.verify()?;
        Ok(result)
    }
    fn read_handle(path: &Path) -> Result<File, Error> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x0020_0000)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.file_attributes() & 0x400 != 0
            || path.canonicalize()? != path
        {
            return Err(Error::Unsupported);
        }
        Ok(file)
    }
    fn verify(&self) -> Result<(), Error> {
        let current = Self::read_handle(&self.path)?;
        if native::identity(&self.file)? != self.metadata.identity
            || native::identity(&current)? != self.metadata.identity
        {
            return Err(Error::Changed);
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundImageMetadata {
    application: bool,
    image: ImageMetadata,
}
struct BoundImage {
    application: bool,
    image: Image,
}

fn hold(path: &Path, protected: bool, directories: &mut Vec<Directory>) -> Result<(), Error> {
    if directories.len() >= 128 {
        return Err(Error::Unsupported);
    }
    let directory = Directory::open(path, protected)?;
    if native::normalized(&directory.metadata.path.to_string_lossy())
        != native::normalized(&path.to_string_lossy())
    {
        return Err(Error::Unsupported);
    }
    directories.push(directory);
    Ok(())
}

fn namespace(
    request: &policy::Request,
    system: &Path,
    application: &Path,
) -> Result<Vec<Directory>, Error> {
    let cwd = Path::new(&request.cwd);
    if native::normalized(&request.cwd).get(..3)
        != native::normalized(&system.to_string_lossy()).get(..3)
        || native::normalized(&application.to_string_lossy()).get(..3)
            != native::normalized(&system.to_string_lossy()).get(..3)
        || cwd.canonicalize()? != cwd
        || !cwd.starts_with(&request.project)
        || super::protected_cwd(cwd, Path::new(&request.project))
    {
        return Err(Error::Unsupported);
    }
    let mut directories = Vec::new();
    let windows = system.parent().ok_or(Error::Unsupported)?;
    hold(windows, true, &mut directories)?;
    hold(system, true, &mut directories)?;
    if windows.join("System").try_exists()? {
        hold(&windows.join("System"), true, &mut directories)?;
    }
    for parent in cwd.ancestors() {
        hold(parent, false, &mut directories)?;
    }
    for directory in [application.to_path_buf(), helpers(request)?] {
        for (depth, parent) in directory.ancestors().enumerate() {
            hold(parent, depth == 0, &mut directories)?;
        }
    }
    hold(
        &Path::new(&request.project).join(".git"),
        false,
        &mut directories,
    )?;
    Ok(directories)
}

fn application_image(name: &str) -> bool {
    name == "git.exe" || APPLICATION_DLLS.contains(&name)
}

fn images(system: &Path, application: &Path) -> Result<(Vec<BoundImage>, BTreeSet<String>), Error> {
    let mut pending = BTreeSet::from([
        "git.exe".to_owned(),
        "ntdll.dll".into(),
        "kernel32.dll".into(),
        "kernelbase.dll".into(),
        "apisetschema.dll".into(),
    ]);
    let mut retained = BTreeMap::new();
    let mut api_sets = BTreeSet::new();
    let mut total = 0_u64;
    while let Some(name) = pending.pop_first() {
        if retained.contains_key(&name) {
            continue;
        }
        if windows_pe::api_set(&name) {
            api_sets.insert(name);
            if api_sets.len() > 512 {
                return Err(Error::Unsupported);
            }
            continue;
        }
        if retained.len() >= 64 {
            return Err(Error::Unsupported);
        }
        let application_image = application_image(&name);
        let path = if application_image {
            application.join(&name)
        } else {
            if application.join(&name).try_exists()? {
                return Err(Error::Unsupported);
            }
            system.join(&name)
        };
        let (image, bytes) = Image::open(&path, &name)?;
        total += bytes.len() as u64;
        if total > 67_108_864 {
            return Err(Error::Unsupported);
        }
        if name == "git.exe"
            && !bytes.windows(VERSION.len() + 1).any(|part| {
                &part[..VERSION.len()] == VERSION.as_bytes() && part[VERSION.len()] == 0
            })
        {
            return Err(Error::Unsupported);
        }
        let dependencies = windows_pe::dependencies(
            &bytes,
            name == "git.exe",
            windows_pe::native_machine().ok_or(Error::Unsupported)?,
        )?;
        if application_image {
            pending.extend(dependencies);
        }
        retained.insert(
            name,
            BoundImage {
                application: application_image,
                image,
            },
        );
    }
    if !APPLICATION_DLLS
        .iter()
        .all(|name| retained.contains_key(*name))
    {
        return Err(Error::Unsupported);
    }
    Ok((retained.into_values().collect(), api_sets))
}

pub(super) struct Binding {
    request: policy::Request,
    system: PathBuf,
    application: PathBuf,
    directories: Vec<Directory>,
    images: Vec<BoundImage>,
    api_sets: BTreeSet<String>,
    files: Vec<RepositoryFile>,
    environment: BTreeMap<String, String>,
}
impl Binding {
    pub(super) fn open(request: &policy::Request) -> Result<Self, Error> {
        if !git::supported(request) {
            return Err(Error::Unsupported);
        }
        let application = application_directory(request)
            .ok_or(Error::Unsupported)?
            .canonicalize()?;
        if native::normalized(&application.to_string_lossy())
            != native::normalized(
                &application_directory(request)
                    .ok_or(Error::Unsupported)?
                    .to_string_lossy(),
            )
        {
            return Err(Error::Unsupported);
        }
        let system = native::system_directory()?.canonicalize()?;
        let directories = namespace(request, &system, &application)?;
        let (images, api_sets) = images(&system, &application)?;
        let repository = Path::new(&request.project).join(".git");
        let files = ["config", "HEAD"]
            .into_iter()
            .map(|name| RepositoryFile::open(&repository, name))
            .collect::<Result<_, _>>()?;
        let result = Self {
            request: request.clone(),
            system,
            application,
            directories,
            images,
            api_sets,
            files,
            environment: native::environment_for(request)?,
        };
        result.verify()?;
        Ok(result)
    }
    pub(super) fn verify(&self) -> Result<(), Error> {
        if !git::supported(&self.request)
            || self.environment != native::environment_for(&self.request)?
        {
            return Err(Error::Changed);
        }
        let repository = Path::new(&self.request.project).join(".git");
        if repository.join("info/attributes").try_exists()?
            || repository.join("commondir").try_exists()?
        {
            return Err(Error::Unsupported);
        }
        for directory in &self.directories {
            directory.verify()?;
        }
        for image in &self.images {
            image.image.verify()?;
            if !image.application
                && self
                    .application
                    .join(&image.image.metadata.name)
                    .try_exists()?
            {
                return Err(Error::Changed);
            }
        }
        for file in &self.files {
            file.verify()?;
        }
        let selected =
            crate::windows_security::open_system(Path::new(&self.request.executable), false)?;
        let executable = self
            .images
            .iter()
            .find(|image| image.image.metadata.name == "git.exe")
            .ok_or(Error::Changed)?;
        if native::identity(&selected)? != executable.image.metadata.identity {
            return Err(Error::Changed);
        }
        Ok(())
    }
    pub(super) fn metadata(&self) -> Value {
        json!({"model":MODEL,"version":VERSION,"library_contract":LIBRARIES,
            "executable":self.request.executable,"system_directory":self.system,"application_directory":self.application,
            "environment":self.environment,"launch_arguments":git::launch(&self.request).arguments,
            "images":self.images.iter().map(|image| BoundImageMetadata { application:image.application,image:image.image.metadata.clone() }).collect::<Vec<_>>(),
            "api_sets":self.api_sets,"directories":self.directories.iter().map(|directory| &directory.metadata).collect::<Vec<_>>(),
            "files":self.files.iter().map(|file| &file.metadata).collect::<Vec<_>>()})
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resources {
    model: String,
    version: String,
    library_contract: String,
    executable: String,
    system_directory: PathBuf,
    application_directory: PathBuf,
    environment: BTreeMap<String, String>,
    launch_arguments: Vec<String>,
    images: Vec<BoundImageMetadata>,
    api_sets: BTreeSet<String>,
    directories: Vec<DirectoryMetadata>,
    files: Vec<RepositoryFileMetadata>,
}
pub(super) fn valid(request: &policy::Request, resource: &Value) -> bool {
    let Ok(resources) = serde_json::from_value::<Resources>(resource.clone()) else {
        return false;
    };
    let Some(application) = application_directory(request) else {
        return false;
    };
    if resources.images.len() > 64 {
        return false;
    }
    let images: Vec<_> = resources
        .images
        .iter()
        .map(|image| image.image.clone())
        .collect();
    resources.model == MODEL
        && resources.version == VERSION
        && resources.library_contract == LIBRARIES
        && git::supported(request)
        && resources.executable == request.executable
        && native::normalized(&resources.application_directory.to_string_lossy())
            == native::normalized(&application.to_string_lossy())
        && native::system_directory().is_ok_and(|directory| {
            native::normalized(&directory.to_string_lossy())
                == native::normalized(&resources.system_directory.to_string_lossy())
        })
        && native::environment_for(request)
            .is_ok_and(|environment| resources.environment == environment)
        && resources.launch_arguments == git::launch(request).arguments
        && native::valid_images(request, &images)
        && APPLICATION_DLLS.iter().all(|name| {
            resources
                .images
                .iter()
                .any(|image| image.application && image.image.name == *name)
        })
        && resources
            .images
            .iter()
            .all(|image| image.application == application_image(&image.image.name))
        && resources.api_sets.len() <= 512
        && resources
            .api_sets
            .iter()
            .all(|name| windows_pe::api_set(name))
        && resources.files.len() == 2
        && resources
            .files
            .iter()
            .zip(["config", "HEAD"])
            .all(|(file, name)| {
                file.name == name
                    && file.identity.1 != 0
                    && file.identity.2 <= 1_048_576
                    && file.sha256.len() == 64
                    && file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        && !resources.directories.is_empty()
        && resources.directories.len() <= 128
        && resources.directories.iter().all(|directory| {
            directory.identity.1 != 0
                && directory.path.is_absolute()
                && directory
                    .path
                    .to_str()
                    .is_some_and(|path| !path.contains('\0'))
        })
        && [
            Path::new(&request.cwd),
            resources.application_directory.as_path(),
            Path::new(&request.project).join(".git").as_path(),
        ]
        .iter()
        .all(|path| {
            resources
                .directories
                .iter()
                .any(|directory| directory.path == **path)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_git_layout_requires_the_native_runtime_directory() {
        for runtime in ["ucrt64", "clangarm64", "mingw64"] {
            let request = policy::Request {
                executable: format!(r"C:\Program Files\Git\{runtime}\bin\git.exe"),
                arguments: vec!["status".into()],
                cwd: r"C:\Acme".into(),
                project: r"C:\Acme".into(),
            };
            assert_eq!(
                application_directory(&request).is_some(),
                Some(runtime) == runtime_directory(),
                "{runtime}"
            );
        }
    }

    #[test]
    fn git_paths_preserve_drive_names_and_refuse_dos_aliases() {
        for path in [r"C:\Acme Tools\project", r"\\?\C:\Acme Tools\project"] {
            assert_eq!(git_path(Path::new(path)).unwrap(), "C:/Acme Tools/project");
        }
        for path in [
            r"\\server\share\project",
            r"\\?\UNC\server\share",
            r"C:\Acme.\project",
            r"C:\Acme \project",
            r"C:\NUL\project",
            r"C:\NUL .txt\project",
            r"C:\LPT¹\project",
            r"C:\Acme:stream\project",
        ] {
            assert!(git_path(Path::new(path)).is_err(), "{path}");
        }
    }
}
