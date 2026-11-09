//! Selected-source advisory snapshots and bounded private presentation storage.
use super::{Context, Snapshot, follows};
use crate::review::{
    Error,
    storage::{objects::Directory, private_files as files},
};
use canonic_registry_kit::source::Source;
pub(super) use files::{read, write};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub(super) const FILE_BYTES: usize = 2 * 1024 * 1024;
const FILES: usize = 256;

#[cfg_attr(
    test,
    expect(
        clippy::unnecessary_wraps,
        reason = "production home resolution is fallible; unit tests select fixture-owned storage"
    )
)]
fn root(context: &Context) -> Result<PathBuf, Error> {
    #[cfg(test)]
    {
        Ok(Path::new(&context.project_root).join(".pack-update-test-cache"))
    }
    #[cfg(not(test))]
    {
        let _ = context;
        std::env::home_dir()
            .filter(|home| home.is_absolute())
            .map(|home| home.join(".shield/pack-updates"))
            .ok_or(Error::Invalid("home directory unavailable"))
    }
}

pub(super) fn directory(context: &Context, kind: &str, create: bool) -> Result<PathBuf, Error> {
    let root = root(context)?;
    #[cfg(not(test))]
    files::directory(
        root.parent().ok_or(Error::Invalid("advisory parent"))?,
        create,
    )?;
    files::directory(&root, create)?;
    let directory = root.join(kind);
    files::directory(&directory, create)?;
    Ok(directory)
}

fn name(identity: &str) -> String {
    format!("{}.json", crate::policy::fingerprint(identity.as_bytes()))
}

pub(super) fn load(context: &Context, source: &Source) -> Result<Snapshot, Error> {
    let directory = directory(context, "catalogs", false)?;
    crate::diagnostics::trace::count("pack_updates.snapshot_reads", 1);
    let snapshot: Snapshot =
        serde_json::from_value(read(&directory.join(name(source.identity())), FILE_BYTES)?)
            .map_err(|_| Error::Invalid("advisory snapshot invalid"))?;
    if &snapshot.source != source {
        return Err(Error::Invalid("advisory source differs"));
    }
    Ok(snapshot)
}

pub(super) fn retain(context: &Context, snapshot: &Snapshot) -> Result<(), Error> {
    let directory = directory(context, "catalogs", true)?;
    let _lock = Directory::try_enter(&directory)?;
    let path = directory.join(name(snapshot.source.identity()));
    if let Ok(previous) = load(context, &snapshot.source)
        && !follows(&snapshot.checkpoint, &previous.checkpoint)
    {
        return Ok(());
    }
    // Advisory bytes are disposable. The safe writer still refuses an
    // unsafe file, but explicit authenticated acquisition can heal corrupt JSON.
    room(&directory, &path)?;
    let value = serde_json::to_value(snapshot).map_err(|_| Error::Invalid("advisory encoding"))?;
    write(&path, &value, FILE_BYTES)
}

/// Only writers scan the bounded namespace. File times choose eviction, never
/// metadata freshness. Unknown neighbors are refused rather than removed.
pub(super) fn room(directory: &Path, target: &Path) -> Result<(), Error> {
    let mut entries = Vec::new();
    for (index, entry) in fs::read_dir(directory)?.take(FILES * 3 + 2).enumerate() {
        if index > FILES * 3 {
            return Err(Error::Invalid("advisory directory exceeds scan bound"));
        }
        let entry = entry?;
        if entry.file_name() == ".objects.lock" {
            continue;
        }
        let path = entry.path();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if stem.len() != 64
            || !stem.bytes().all(|b| b.is_ascii_hexdigit())
            || !matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("json" | "pending" | "ready")
            )
        {
            return Err(Error::Invalid("unknown advisory cache entry"));
        }
        let file = crate::review::storage::objects::read_file(&path)?;
        if path
            .extension()
            .is_some_and(|e| e == "pending" || e == "ready")
        {
            fs::remove_file(&path)?;
        } else {
            entries.push((file.metadata()?.modified()?, path));
        }
        if entries.len() > FILES {
            return Err(Error::Invalid("advisory capacity exceeded"));
        }
    }
    if !target.exists() && entries.len() >= FILES {
        entries.sort();
        fs::remove_file(&entries[0].1)?;
    }
    Ok(())
}

pub(super) fn receipt_path(directory: &Path, project: &str, audience: &str) -> PathBuf {
    directory.join(name(&format!("{project}:{audience}")))
}
