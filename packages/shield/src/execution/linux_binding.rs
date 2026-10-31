//! GNU/Linux native images, protected loader cache and root-owned library closure.
use super::{Error, elf, git, linux_cache};
use crate::policy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{self, File, Metadata},
    os::unix::fs::{FileExt, MetadataExt},
    path::{Path, PathBuf},
};

const MODEL: &str = "linux-gnu-native-v1";
const CONTRACT: &str = "protected-glibc-native-cache-and-library-closure-v1";
const MAX_IMAGE: u64 = 67_108_864;
const GIT_VERSION: &str = "2.53.0";
type Identity = (u64, u64, u64, i64, i64, i64, i64);

fn architecture() -> Result<(elf::Machine, &'static str, u32), Error> {
    if cfg!(target_arch = "x86_64") {
        Ok((elf::Machine::X86_64, "/lib64/ld-linux-x86-64.so.2", 0x303))
    } else if cfg!(target_arch = "aarch64") {
        Ok((elf::Machine::Aarch64, "/lib/ld-linux-aarch64.so.1", 0xa03))
    } else {
        Err(Error::Unsupported)
    }
}

fn identity(metadata: &Metadata) -> Identity {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

fn protected_parents(path: &Path) -> Result<(), Error> {
    for parent in path.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(parent)?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(Error::Unsupported);
        }
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    path: PathBuf,
    target: PathBuf,
    identity: Identity,
}

fn protected_path(path: &Path) -> Result<(PathBuf, Vec<Link>), Error> {
    use std::path::Component;
    if !path.is_absolute() {
        return Err(Error::Unsupported);
    }
    let mut pending =
        VecDeque::from_iter(path.components().map(|part| part.as_os_str().to_owned()));
    let mut resolved = PathBuf::new();
    let mut links = Vec::new();
    let mut steps = 0;
    while let Some(part) = pending.pop_front() {
        steps += 1;
        if steps > 512 {
            return Err(Error::Unsupported);
        }
        match Path::new(&part)
            .components()
            .next()
            .ok_or(Error::Unsupported)?
        {
            Component::RootDir => resolved = PathBuf::from("/"),
            Component::CurDir => continue,
            Component::ParentDir => {
                resolved.pop();
                continue;
            }
            Component::Normal(name) => resolved.push(name),
            Component::Prefix(_) => return Err(Error::Unsupported),
        }
        let metadata = fs::symlink_metadata(&resolved)?;
        if metadata.uid() != 0 {
            return Err(Error::Unsupported);
        }
        if metadata.is_symlink() {
            if links.len() >= 40 {
                return Err(Error::Unsupported);
            }
            let target = fs::read_link(&resolved)?;
            links.push(Link {
                path: resolved.clone(),
                target: target.clone(),
                identity: identity(&metadata),
            });
            resolved.pop();
            for part in target.components().rev() {
                pending.push_front(part.as_os_str().to_owned());
            }
        } else if !pending.is_empty() && (!metadata.is_dir() || metadata.mode() & 0o022 != 0) {
            return Err(Error::Unsupported);
        }
    }
    Ok((resolved, links))
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageMetadata {
    path: PathBuf,
    links: Vec<Link>,
    identity: Identity,
    sha256: String,
}
struct Resource {
    file: File,
    metadata: ImageMetadata,
}
impl Resource {
    fn open(path: &Path) -> Result<(Self, Vec<u8>), Error> {
        let (path, links) = protected_path(path)?;
        protected_parents(&path)?;
        let file = File::open(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != 0
            || metadata.mode() & 0o6022 != 0
            || metadata.len() > MAX_IMAGE
        {
            return Err(Error::Unsupported);
        }
        let mut bytes = vec![0; usize::try_from(metadata.len()).map_err(|_| Error::Unsupported)?];
        file.read_exact_at(&mut bytes, 0)?;
        if identity(&metadata) != identity(&file.metadata()?)
            || identity(&metadata) != identity(&fs::symlink_metadata(&path)?)
        {
            return Err(Error::Changed);
        }
        let metadata = ImageMetadata {
            path,
            links,
            identity: identity(&metadata),
            sha256: policy::fingerprint(&bytes),
        };
        Ok((Self { file, metadata }, bytes))
    }
    fn verify(&self) -> Result<(), Error> {
        // Binding::verify performs one fresh capture of paths and content below.
        // Separately retain the identity of the handles acquired by this binding.
        if identity(&self.file.metadata()?) != self.metadata.identity {
            return Err(Error::Changed);
        }
        Ok(())
    }
}

fn preload() -> Result<Option<Resource>, Error> {
    let path = Path::new("/etc/ld.so.preload");
    protected_parents(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
        Ok(_) => {
            let (resource, bytes) = Resource::open(path)?;
            if !bytes.is_empty() {
                return Err(Error::Unsupported);
            }
            Ok(Some(resource))
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Resources {
    model: String,
    library_contract: String,
    executable: String,
    loader: PathBuf,
    cache: ImageMetadata,
    images: Vec<ImageMetadata>,
    preload: Option<ImageMetadata>,
    cwd_identity: (u64, u64),
    git: Option<Value>,
    launch_arguments: Vec<String>,
}

fn capture_images(
    request: &policy::Request,
    loader: &Path,
    libraries: &linux_cache::Libraries,
    machine: elf::Machine,
) -> Result<Vec<Resource>, Error> {
    let executable = protected_path(Path::new(&request.executable))?.0;
    let loader_image = protected_path(loader)?.0;
    let mut pending = BTreeSet::from([PathBuf::from(&request.executable), loader.to_path_buf()]);
    let mut images = BTreeMap::new();
    let mut total = 0_u64;
    while let Some(path) = pending.pop_first() {
        if images.contains_key(&path) {
            continue;
        }
        if images.len() >= 64 {
            return Err(Error::Unsupported);
        }
        let (resource, bytes) = Resource::open(&path)?;
        total += bytes.len() as u64;
        if total > 268_435_456 {
            return Err(Error::Unsupported);
        }
        let image = elf::inspect(&bytes, machine)?;
        if (resource.metadata.path == executable || resource.metadata.path == loader_image)
            && resource.file.metadata()?.mode() & 0o111 == 0
        {
            return Err(Error::Unsupported);
        }
        if resource.metadata.path == executable
            && request.executable == git::EXECUTABLE
            && !bytes.windows(GIT_VERSION.len() + 1).any(|part| {
                &part[..GIT_VERSION.len()] == GIT_VERSION.as_bytes() && part[GIT_VERSION.len()] == 0
            })
        {
            return Err(Error::Unsupported);
        }
        if resource.metadata.path == executable && image.interpreter.is_none() {
            return Err(Error::Unsupported);
        }
        if let Some(interpreter) = image.interpreter {
            if protected_path(Path::new(&interpreter))?.0 != loader_image {
                return Err(Error::Unsupported);
            }
            pending.insert(PathBuf::from(interpreter));
        }
        for name in image.needed {
            for path in libraries.get(&name).ok_or(Error::Unsupported)? {
                pending.insert(path.clone());
            }
        }
        images.insert(path, resource);
    }
    Ok(images.into_values().collect())
}

pub(super) struct Binding {
    request: policy::Request,
    resources: Resources,
    images: Vec<Resource>,
    cache: Resource,
    preload: Option<Resource>,
}
impl Binding {
    pub(super) fn open(request: &policy::Request) -> Result<Self, Error> {
        if !super::supported(request) {
            return Err(Error::Unsupported);
        }
        let (machine, interpreter, flags) = architecture()?;
        let loader = PathBuf::from(interpreter);
        let (cache, bytes) = Resource::open(Path::new("/etc/ld.so.cache"))?;
        let libraries = linux_cache::libraries(&bytes, flags)?;
        let images = capture_images(request, &loader, &libraries, machine)?;
        let cwd = fs::metadata(&request.cwd)?;
        if !cwd.is_dir() || Path::new(&request.cwd).canonicalize()? != Path::new(&request.cwd) {
            return Err(Error::Unsupported);
        }
        let preload = preload()?;
        let launch = if request.executable == git::EXECUTABLE {
            git::launch(request)
        } else {
            request.clone()
        };
        let resources = Resources {
            model: MODEL.into(),
            library_contract: CONTRACT.into(),
            executable: request.executable.clone(),
            loader: loader.clone(),
            cache: cache.metadata.clone(),
            images: images.iter().map(|image| image.metadata.clone()).collect(),
            preload: preload.as_ref().map(|image| image.metadata.clone()),
            cwd_identity: (cwd.dev(), cwd.ino()),
            git: if request.executable == git::EXECUTABLE {
                Some(git::capture(request)?)
            } else {
                None
            },
            launch_arguments: launch.arguments,
        };
        Ok(Self {
            request: request.clone(),
            resources,
            images,
            cache,
            preload,
        })
    }

    pub(super) fn verify(&self) -> Result<(), Error> {
        self.cache.verify()?;
        for image in &self.images {
            image.verify()?;
        }
        if let Some(preload) = &self.preload {
            preload.verify()?;
        }
        let current = Self::open(&self.request)?;
        if self.metadata() != current.metadata() {
            return Err(Error::Changed);
        }
        Ok(())
    }

    pub(super) fn metadata(&self) -> Value {
        serde_json::to_value(&self.resources).expect("native resource metadata serializes")
    }
}

pub(super) fn valid(request: &policy::Request, value: &Value) -> bool {
    let Ok(resources) = serde_json::from_value::<Resources>(value.clone()) else {
        return false;
    };
    super::supported(request)
        && resources.model == MODEL
        && resources.library_contract == CONTRACT
        && resources.executable == request.executable
        && resources.cwd_identity.1 != 0
        && resources.git.is_some() == (request.executable == git::EXECUTABLE)
        && (2..=64).contains(&resources.images.len())
        && resources.images.iter().all(|image| {
            image.path.is_absolute()
                && image.identity.1 != 0
                && image.identity.2 <= MAX_IMAGE
                && image.sha256.len() == 64
                && image.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        && resources.cache.path.is_absolute()
        && resources.cache.sha256.len() == 64
        && resources.cache.identity.1 != 0
        && resources.loader.is_absolute()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn linux_native_path_refuses_a_writable_alias_to_a_protected_image() {
        let root = assert_fs::TempDir::new().unwrap();
        let alias = root.path().join("leaf");
        std::os::unix::fs::symlink("/usr/bin/true", &alias).unwrap();
        assert_eq!(
            alias.canonicalize().unwrap(),
            Path::new("/usr/bin/true").canonicalize().unwrap()
        );
        assert!(protected_path(&alias).is_err());
        assert!(protected_path(Path::new("/usr/bin/true")).is_ok());
    }

    #[test]
    fn linux_native_binding_captures_system_leaves_and_refuses_local_copies() {
        let root = assert_fs::TempDir::new().unwrap();
        let cwd = root
            .path()
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        for executable in ["/bin/echo", "/usr/bin/true", "/usr/bin/false", "/bin/sleep"] {
            let request = policy::Request {
                executable: executable.into(),
                arguments: if executable.ends_with("sleep") {
                    vec!["0".into()]
                } else {
                    vec![]
                },
                cwd: cwd.clone(),
                project: cwd.clone(),
            };
            let binding =
                Binding::open(&request).unwrap_or_else(|error| panic!("{executable}: {error}"));
            binding.verify().unwrap();
            assert!(valid(&request, &binding.metadata()));
            let copy = root.path().join("leaf");
            fs::copy(executable, &copy).unwrap();
            let mut local = request.clone();
            local.executable = copy.to_str().unwrap().into();
            assert!(Binding::open(&local).is_err());
            let mut altered = binding.metadata();
            altered["model"] = json!("unbound");
            assert!(!valid(&request, &altered));
        }
    }
}
