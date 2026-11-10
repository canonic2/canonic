//! Optional private provenance for reclaiming payloads of missing checkouts.
//! Neither provenance nor a cache inventory supplies policy authority.
use super::{
    Artifact, Error, Source, check_directory, digest, prepare_directory, project_key, storage,
    temporary_writer,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::{
    fs,
    path::{Path, PathBuf},
};

const INDEX_BYTES: usize = 16 * 1024 * 1024;
const OWNER_BYTES: usize = 8 * 1024;
const TRACKED_OWNERS: usize = 4096;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    schema_version: u8,
    cursor: Option<String>,
    owners: BTreeMap<String, Value>,
}
impl Default for Index {
    fn default() -> Self {
        Self {
            schema_version: 1,
            cursor: None,
            owners: BTreeMap::new(),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    schema_version: u8,
    project_root: String,
    project_id: String,
    root_identity: (u64, u64),
    review_identity: (u64, u64),
    uid: Option<u32>,
}

fn identity(path: &Path) -> Result<((u64, u64), Option<u32>), Error> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Invalid("cache owner directory is not regular"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(((metadata.dev(), metadata.ino()), Some(metadata.uid())))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let file = fs::OpenOptions::new()
            .read(true)
            .access_mode(0x0080)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(path)?;
        let information = winapi_util::file::information(&file)?;
        Ok((
            (
                u64::from(information.volume_serial_number()),
                information.file_index(),
            ),
            None,
        ))
    }
}
fn namespace(directory: &Path, owner: Option<u32>) -> Result<PathBuf, Error> {
    check_directory(directory, true, owner)?;
    let namespace = directory.join("ownership");
    prepare_directory(&namespace, owner)?;
    Ok(namespace)
}
fn read(namespace: &Path) -> Result<Index, Error> {
    let value = match storage::private_files::read(&namespace.join("state.json"), INDEX_BYTES) {
        Ok(value) => value,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Index::default());
        }
        Err(error) => return Err(error),
    };
    let index: Index = serde_json::from_value(value)
        .map_err(|_| Error::Invalid("cache ownership index invalid"))?;
    if index.schema_version != 1
        || index.owners.len() > TRACKED_OWNERS
        || index.owners.keys().any(|key| !digest(key))
        || index.cursor.as_ref().is_some_and(|key| !digest(key))
    {
        return Err(Error::Invalid("cache ownership index unsupported"));
    }
    Ok(index)
}
fn save(namespace: &Path, index: &Index) -> Result<(), Error> {
    storage::private_files::write(
        &namespace.join("state.json"),
        &serde_json::to_value(index).map_err(|_| Error::Invalid("cache ownership encoding"))?,
        INDEX_BYTES,
    )
}
fn decoded(key: &str, value: &Value) -> Result<Owner, Error> {
    if !crate::json::fits(value, OWNER_BYTES) {
        return Err(crate::policy::Error::Limit.into());
    }
    let owner: Owner = serde_json::from_value(value.clone())
        .map_err(|_| Error::Invalid("cache ownership receipt invalid"))?;
    let root = Path::new(&owner.project_root);
    if owner.schema_version != 1
        || !root.is_absolute()
        || !digest(&owner.project_id)
        || root.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
        || project_key(&owner.project_root, &owner.project_id) != key
    {
        return Err(Error::Invalid("cache ownership receipt unsupported"));
    }
    Ok(owner)
}

/// Called only after the evaluator revision and selected cache binding have
/// validated. Failure only disables optional cleanup tracking.
pub(super) fn register(source: Source<'_>, artifact: &Artifact) -> Result<(), Error> {
    if artifact.project_key != project_key(source.project_root, source.project_id) {
        return Err(Error::StaleSnapshot);
    }
    let (root_identity, root_uid) = identity(Path::new(source.project_root))?;
    let (review_identity, uid) = identity(&Path::new(source.project_root).join(".shield/review"))?;
    if root_uid != uid {
        return Err(Error::Invalid("cache owner identity differs"));
    }
    let owner = Owner {
        schema_version: 1,
        project_root: source.project_root.into(),
        project_id: source.project_id.into(),
        root_identity,
        review_identity,
        uid,
    };
    let value =
        serde_json::to_value(owner).map_err(|_| Error::Invalid("cache ownership encoding"))?;
    decoded(&artifact.project_key, &value)?;
    let namespace = namespace(&artifact.directory, uid)?;
    let _guard = storage::objects::Directory::enter(&namespace)?;
    if fs::metadata(namespace.join(".objects.lock"))?.len() != 0 {
        return Err(Error::Invalid(
            "cache ownership mutex contains unexpected data",
        ));
    }
    let mut index = read(&namespace)?;
    if let Some(existing) = index.owners.get(&artifact.project_key) {
        if *existing != value {
            return Err(Error::Invalid("cache ownership receipt differs"));
        }
    } else {
        if index.owners.len() >= TRACKED_OWNERS {
            return Err(Error::CleanupPending);
        }
        index.owners.insert(artifact.project_key.clone(), value);
        save(&namespace, &index)?;
    }
    Ok(())
}
fn missing(owner: &Owner) -> bool {
    let root = Path::new(&owner.project_root);
    match identity(root) {
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return true,
        Ok((identity, uid)) if identity == owner.root_identity && uid == owner.uid => {}
        _ => return false,
    }
    match identity(&root.join(".shield/review")) {
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => true,
        // An existing or replaced review tree may carry active/recovery data.
        // Preserve it without opening another checkout's checkpoint or sources.
        _ => false,
    }
}
fn obsolete_or_missing(owner: &Owner, current_root: &str) -> bool {
    if missing(owner) {
        return true;
    }
    // Only the caller's own checkout may use live checkpoint roots as proof.
    // Another existing workspace remains opaque, including obsolete epochs.
    owner.project_root == current_root
        && identity(Path::new(current_root))
            .is_ok_and(|(identity, uid)| identity == owner.root_identity && uid == owner.uid)
        && identity(&Path::new(current_root).join(".shield/review"))
            .is_ok_and(|(identity, uid)| identity == owner.review_identity && uid == owner.uid)
}
fn payload(name: &str) -> bool {
    name.strip_suffix(".bin")
        .or_else(|| name.strip_suffix(".inputs.json"))
        .or_else(|| name.strip_suffix(".probe.json"))
        .is_some_and(digest)
        || temporary_writer(name).is_some_and(|pid| !storage::writer_running(pid))
}
fn empty_shell(partition: &Path) -> Result<bool, Error> {
    for entry in fs::read_dir(partition)?.take(2) {
        if entry?.file_name() != ".objects.lock" {
            return Ok(false);
        }
    }
    Ok(true)
}
#[derive(Default)]
struct Protected {
    checkpoints: BTreeMap<String, storage::objects::Checkpoint>,
    namespaces: BTreeSet<String>,
}
impl Protected {
    fn read(&mut self, root: &str, directory: &Path) -> Result<(), Error> {
        let review = Path::new(root).join(".shield/review");
        let main = self
            .checkpoints
            .entry("state.json".into())
            .or_default()
            .read(&review.join("state.json"), None)?
            .ok_or(Error::Missing)?;
        if main["schema_version"] != 1 || !main["operator"].is_object() {
            return Err(Error::Invalid(
                "cache owner checkpoint metadata unsupported",
            ));
        }
        let mut recoveries = Vec::new();
        for (prefix, pins) in [
            ("upgrade", &main["operator"]["upgrade_recovery"]),
            (
                "bootstrap",
                &main["operator"]["bootstrap"]["checkpoint_recovery"],
            ),
        ] {
            if pins.is_null() {
                continue;
            }
            let pins = pins
                .as_array()
                .filter(|pins| pins.len() <= 2)
                .ok_or(Error::Invalid("cache owner recovery metadata unsupported"))?;
            for pin in pins {
                let digest = pin
                    .as_str()
                    .filter(|value| super::digest(value))
                    .ok_or(Error::Invalid("cache owner recovery digest invalid"))?;
                recoveries.push((format!("{prefix}-{digest}.json"), digest.to_owned()));
            }
        }
        protect(main, directory, &mut self.namespaces)?;
        for (name, digest) in recoveries {
            let recovered = self
                .checkpoints
                .entry(name.clone())
                .or_default()
                .read(&review.join(name), Some(&digest))?
                .ok_or(Error::Missing)?;
            protect(recovered, directory, &mut self.namespaces)?;
        }
        Ok(())
    }
}
fn protect(state: &Value, directory: &Path, keep: &mut BTreeSet<String>) -> Result<(), Error> {
    if state["active"].is_null() {
        return Ok(());
    }
    let compiled = &state["active"]["compiled"];
    let root = compiled["directory"]
        .as_str()
        .ok_or(Error::Invalid("cache recovery binding unsupported"))?;
    let key = compiled["project_key"]
        .as_str()
        .filter(|key| digest(key))
        .ok_or(Error::Invalid("cache recovery namespace unsupported"))?;
    if Path::new(root) == directory {
        keep.insert(key.into());
    }
    Ok(())
}
/// One progressive registered owner per invocation. Root tracking is bounded
/// optional metadata, not an aggregate cache or host admission limit.
pub(super) fn maintain(
    directory: &Path,
    current: &str,
    uid: Option<u32>,
    root: &str,
) -> Result<(), Error> {
    let namespace = namespace(directory, uid)?;
    let _guard = storage::objects::Directory::try_enter(&namespace)?;
    if fs::metadata(namespace.join(".objects.lock"))?.len() != 0 {
        return Err(Error::Invalid(
            "cache ownership mutex contains unexpected data",
        ));
    }
    let mut index = read(&namespace)?;
    let next = index
        .owners
        .keys()
        .find(|key| index.cursor.as_ref().is_none_or(|cursor| *key > cursor))
        .or_else(|| index.owners.keys().next())
        .cloned();
    let Some(key) = next else {
        return Ok(());
    };
    let progress = index.cursor.as_ref() != Some(&key);
    index.cursor = Some(key.clone());
    let mut retired = false;
    let mut protected = Protected::default();
    protected.read(root, directory)?;
    if key != current
        && !protected.namespaces.contains(&key)
        && let Ok(owner) = decoded(&key, &index.owners[&key])
        && owner.uid == uid
        && obsolete_or_missing(&owner, root)
    {
        let compiled = directory.join("compiled");
        check_directory(&compiled, true, uid)?;
        let partition = compiled.join(&key);
        match fs::symlink_metadata(&partition) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                retired = true;
            }
            Err(error) => return Err(error.into()),
            Ok(_) => {
                check_directory(&partition, true, uid)?;
                // A live partition writer or reader pin excludes deletion.
                let mut partition_owner = storage::objects::Directory::try_enter(&partition)?;
                if fs::metadata(partition.join(".objects.lock"))?.len() != 0 {
                    return Err(Error::Invalid(
                        "cache partition mutex contains unexpected data",
                    ));
                }
                partition_owner.sweep_refresh(&BTreeSet::new(), payload, |_| {
                    protected.read(root, directory)?;
                    if protected.namespaces.contains(&key) {
                        return Err(Error::StaleSnapshot);
                    }
                    if obsolete_or_missing(&owner, root) {
                        Ok(())
                    } else {
                        Err(Error::StaleSnapshot)
                    }
                })?;
                retired = empty_shell(&partition)? && obsolete_or_missing(&owner, root);
            }
        }
    }
    if retired {
        index.owners.remove(&key);
        crate::diagnostics::trace::count("cache.orphan_retired", 1);
    }
    if progress || retired {
        save(&namespace, &index)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
