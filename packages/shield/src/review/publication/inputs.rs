//! Native version fences around exact, unlocked confirmation-file verification.
//! These short-lived observations never establish policy from metadata alone.
use super::{BTreeMap, BTreeSet, Context, Error, Path, State, configuration};

pub(super) struct Fence {
    #[cfg(unix)]
    observations: BTreeMap<std::path::PathBuf, Observation>,
}

#[cfg(unix)]
#[derive(PartialEq, Eq)]
struct Version {
    device: u64,
    inode: u64,
    length: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    mode: u32,
}
#[cfg(unix)]
impl Version {
    fn of(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt as _;
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            mode: metadata.mode(),
        }
    }
}
#[cfg(unix)]
struct Observation {
    version: Option<Version>,
    // Hold existing files to prevent inode reuse throughout the unlocked proof.
    file: Option<std::fs::File>,
}
#[cfg(unix)]
fn version(path: &Path) -> Result<Option<Version>, Error> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) =>
        {
            Err(Error::Invalid(
                "confirmation input cannot be a link or special file",
            ))
        }
        Ok(metadata) => Ok(Some(Version::of(&metadata))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

impl Fence {
    /// A platform without trustworthy native change time keeps the exact-hash
    /// locked path. Unusually deep input closures do too, rather than imposing
    /// a new policy-path or file-descriptor limit.
    pub(super) fn capture(
        context: &Context,
        before: &State,
        files: &BTreeMap<String, String>,
    ) -> Result<Option<Self>, Error> {
        #[cfg(not(unix))]
        {
            let _ = (context, before, files);
            Ok(None)
        }
        #[cfg(unix)]
        {
            let mut names =
                super::projections::paths(&before.candidates["project"], &before.operator)?;
            names.extend(files.keys().cloned());
            names.extend(["config.local.yml".into(), "rules.local.yml".into()]);
            if names.len() > 160 {
                return Ok(None);
            }
            let shield = Path::new(&context.project_root).join(".shield");
            let mut paths = BTreeSet::from([shield.clone()]);
            for name in &names {
                configuration::portable_path(name)?;
                let path = shield.join(name);
                for parent in path
                    .ancestors()
                    .take_while(|parent| parent.starts_with(&shield))
                {
                    paths.insert(parent.to_path_buf());
                    if paths.len() > 512 {
                        return Ok(None);
                    }
                }
            }
            let mut observations = BTreeMap::new();
            let mut retained = 0;
            for path in paths {
                let stamp = version(&path)?;
                let file = if stamp
                    .as_ref()
                    .is_some_and(|stamp| stamp.mode & 0o170_000 == 0o100_000)
                {
                    retained += 1;
                    if retained > 160 {
                        return Ok(None);
                    }
                    let file = match std::fs::File::open(&path) {
                        Ok(file) => file,
                        Err(error)
                            if error.raw_os_error().is_some_and(|code| {
                                [nix::libc::EMFILE, nix::libc::ENFILE].contains(&code)
                            }) =>
                        {
                            return Ok(None);
                        }
                        Err(error) => return Err(error.into()),
                    };
                    if Some(Version::of(&file.metadata()?)).as_ref() != stamp.as_ref()
                        || version(&path)?.as_ref() != stamp.as_ref()
                    {
                        return Err(Error::StaleSnapshot);
                    }
                    Some(file)
                } else {
                    None
                };
                observations.insert(
                    path,
                    Observation {
                        version: stamp,
                        file,
                    },
                );
            }
            let result = Self { observations };
            result.check()?;
            Ok(Some(result))
        }
    }
    pub(super) fn check(&self) -> Result<(), Error> {
        #[cfg(unix)]
        for (path, observation) in &self.observations {
            crate::diagnostics::trace::count("publication.input_version_probes", 1);
            if version(path)?.as_ref() != observation.version.as_ref()
                || observation.file.as_ref().is_some_and(|file| {
                    file.metadata()
                        .map(|metadata| Version::of(&metadata))
                        .ok()
                        .as_ref()
                        != observation.version.as_ref()
                })
            {
                return Err(Error::StaleSnapshot);
            }
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::review::publication::projections::projections;
    use serde_json::json;
    #[test]
    fn publication_path_inventory_uses_headers_without_rendering_bodies() {
        let candidate = json!({"config":json!({"schema_version":1,"packs":[{"id":"acme/tool","version":"1.0.0","profile":"recommended","path":"packs/tool.yml"}]}).to_string(),"packs":[null],"rules":"[invalid body"});
        let metadata =
            json!({"installed":[{"id":"acme/tool","version":"1.0.0","profile":"recommended"}]});
        assert_eq!(
            super::super::projections::paths(&candidate, &metadata).unwrap(),
            BTreeSet::from([
                "config.yml".into(),
                "rules.yml".into(),
                "shield.lock".into(),
                "packs/tool.yml".into()
            ])
        );
        assert!(projections(&candidate, &metadata).is_err());
    }
    #[test]
    fn publication_input_fence_rejects_same_length_edits_and_new_optional_files() {
        let (root, context) = crate::review::publication::tests::fixture();
        let before = crate::review::project::load(&context).unwrap();
        let files = projections(&before.candidates["project"], &before.operator).unwrap();
        let fence = Fence::capture(&context, &before, &files).unwrap().unwrap();
        fence.check().unwrap();
        let path = root.path().join(".shield/rules.yml");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        std::fs::write(&path, &changed).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert!(matches!(fence.check(), Err(Error::StaleSnapshot)));
        std::fs::write(&path, &bytes).unwrap();
        let fence = Fence::capture(&context, &before, &files).unwrap().unwrap();
        std::fs::write(
            root.path().join(".shield/config.local.yml"),
            "schema_version: 1\n",
        )
        .unwrap();
        assert!(matches!(fence.check(), Err(Error::StaleSnapshot)));
    }
    #[test]
    fn publication_input_fence_refuses_parent_links_and_atomic_replacements() {
        let (root, context) = crate::review::publication::tests::fixture();
        let before = crate::review::project::load(&context).unwrap();
        let files = projections(&before.candidates["project"], &before.operator).unwrap();
        let fence = Fence::capture(&context, &before, &files).unwrap().unwrap();
        let path = root.path().join(".shield/rules.yml");
        let replacement = root.path().join("replacement.yml");
        std::fs::write(&replacement, std::fs::read(&path).unwrap()).unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        assert!(matches!(fence.check(), Err(Error::StaleSnapshot)));
        std::fs::rename(
            root.path().join(".shield"),
            root.path().join("retained-shield"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            root.path().join("retained-shield"),
            root.path().join(".shield"),
        )
        .unwrap();
        assert!(Fence::capture(&context, &before, &files).is_err());
    }
}
