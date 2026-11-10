//! Per-project cache admission, serialized publication and in-flight plan pins.
use super::{Artifact, Error, atomic_write, digest, temporary_writer};
use crate::review::storage::{
    self,
    objects::{Directory, Pins},
};
use std::{collections::BTreeSet, path::Path};

fn keep_active(
    root: &str,
    artifact: &Artifact,
    keep: &mut BTreeSet<String>,
    checkpoint: &mut storage::objects::Checkpoint,
) -> Result<(), Error> {
    let path = Path::new(root).join(".shield/review/state.json");
    let Some(state) = checkpoint.read(&path, None)? else {
        return Ok(());
    };
    let current = &state["active"]["compiled"];
    if current["directory"].as_str() == artifact.directory.to_str()
        && current["project_key"] == artifact.project_key
        && let Some(plan) = current["plan_key"].as_str().filter(|s| digest(s))
    {
        keep.insert(format!("{plan}.bin"));
        keep.insert(format!("{plan}.inputs.json"));
        if let Some(name) = super::inputs::object_name(current) {
            keep.insert(name);
        }
    }
    Ok(())
}

pub(super) fn publish(
    root: &str,
    artifact: &Artifact,
    bytes: &[u8],
    manifest: &[u8],
    recovery: bool,
) -> Result<Pins, Error> {
    let partition = artifact
        .path()
        .parent()
        .ok_or(Error::Invalid("cache partition"))?
        .to_path_buf();
    let mut directory = Directory::enter(&partition)?;
    let binary = format!("{}.bin", artifact.plan_key);
    let inputs = format!("{}.inputs.json", artifact.plan_key);
    let keep = BTreeSet::from([binary.clone(), inputs.clone()]);
    let mut protected = keep.clone();
    let mut checkpoint = storage::objects::Checkpoint::default();
    keep_active(root, artifact, &mut protected, &mut checkpoint)?;
    directory.sweep_refresh(
        &protected,
        |name| {
            name.strip_suffix(".bin")
                .or_else(|| name.strip_suffix(".inputs.json"))
                .or_else(|| name.strip_suffix(".probe.json"))
                .is_some_and(digest)
                || temporary_writer(name).is_some_and(|pid| !storage::writer_running(pid))
        },
        |keep| keep_active(root, artifact, keep, &mut checkpoint),
    )?;
    let writes = [(binary, bytes), (inputs, manifest)]
        .into_iter()
        .map(|(name, bytes)| {
            storage::objects::same_bytes(&partition.join(&name), bytes)
                // This is the selected disposable object, rather than an
                // unrelated neighbor or an authoritative source checkpoint.
                .map_err(|_| Error::CacheUnavailable)
                .map(|same| (name, bytes, !same))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let needed = writes
        .iter()
        .filter(|(_, _, write)| *write)
        .map(|(_, bytes, _)| bytes.len() as u64)
        .sum();
    let count = writes.iter().filter(|(_, _, write)| *write).count();
    directory.admit(needed, count, recovery)?;
    for (name, bytes, write) in writes {
        if write {
            // The namespace mutex excludes new pins until replacement. Close
            // the exclusively checked old handle before Windows replacement.
            drop(directory.unpinned(&name)?);
            atomic_write(&partition.join(name), bytes)?;
        }
    }
    directory.pins(&keep)
}

/// Optional immutable metadata joins the same physical namespace admission and
/// pin protocol as plans. Publication itself never scans source files.
pub(super) fn publish_probe(
    root: &str,
    artifact: &Artifact,
    name: &str,
    bytes: &[u8],
) -> Result<Pins, Error> {
    if !name.strip_suffix(".probe.json").is_some_and(digest) {
        return Err(Error::Invalid("input metadata object name"));
    }
    let partition = artifact
        .path()
        .parent()
        .ok_or(Error::Missing)?
        .to_path_buf();
    let mut directory = Directory::enter(&partition)?;
    let keep = BTreeSet::from([name.to_owned()]);
    let mut protected = keep.clone();
    let mut checkpoint = storage::objects::Checkpoint::default();
    keep_active(root, artifact, &mut protected, &mut checkpoint)?;
    directory.sweep_refresh(
        &protected,
        |name| {
            name.strip_suffix(".bin")
                .or_else(|| name.strip_suffix(".inputs.json"))
                .or_else(|| name.strip_suffix(".probe.json"))
                .is_some_and(digest)
                || temporary_writer(name).is_some_and(|pid| !storage::writer_running(pid))
        },
        |keep| keep_active(root, artifact, keep, &mut checkpoint),
    )?;
    if !storage::objects::same_bytes(&partition.join(name), bytes)? {
        directory.admit(bytes.len() as u64, 1, false)?;
        drop(directory.unpinned(name)?);
        atomic_write(&partition.join(name), bytes)?;
    }
    directory.pins(&keep)
}
