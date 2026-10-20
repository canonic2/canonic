//! The page-index request for a space: where its saved index lives in the
//! system temporary folder, and the identity that invalidates it when the
//! binary, the evaluator or the preview settings change.
//!
//! The saved indexes are kept for at most 32 projects; one unused for seven
//! days is removed. Removal touches only this capability's folder.
use crate::model::Request;
use anyhow::Result;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const KEPT: usize = 31;
const UNUSED: Duration = Duration::from_hours(7 * 24);

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn is_project_folder(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// The saved index file for `root`, under `base`, after removing the
/// folders of other projects past the limits.
///
/// # Errors
/// Returns an error when the folder cannot be made.
pub(crate) fn saved_path(base: &Path, root: &Path) -> Result<PathBuf> {
    let project = digest(&root.to_string_lossy());
    std::fs::create_dir_all(base)?;
    let mut others: Vec<(PathBuf, SystemTime)> = std::fs::read_dir(base)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (is_project_folder(&name) && name != project)
                .then(|| Some((entry.path(), entry.metadata().ok()?.modified().ok()?)))
                .flatten()
        })
        .collect();
    others.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    let now = SystemTime::now();
    for (at, (folder, modified)) in others.iter().enumerate() {
        let old = now.duration_since(*modified).unwrap_or_default() > UNUSED;
        if at >= KEPT || old {
            let _removed = std::fs::remove_dir_all(folder);
        }
    }
    let folder = base.join(project);
    std::fs::create_dir_all(&folder)?;
    let _touched = std::fs::File::open(&folder).and_then(|file| file.set_modified(now));
    Ok(folder.join("index.postcard"))
}

/// What the saved index is valid for: these files as they are, and the
/// preview settings.
pub(crate) fn identity(files: &[PathBuf], options: &Value) -> String {
    let stamps: Vec<Value> = files
        .iter()
        .map(|file| {
            let metadata = std::fs::metadata(file).ok();
            let modified = metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|since| since.as_millis());
            json!([
                file.to_string_lossy(),
                modified.map(|ms| ms.to_string()),
                metadata.map(|metadata| metadata.len())
            ])
        })
        .collect();
    digest(&json!([stamps, options]).to_string())
}

/// One request for the resident index.
pub(crate) fn request(
    root: &Path,
    saved: PathBuf,
    identity: String,
    evaluator: PathBuf,
    node: Option<&Path>,
    options: &Value,
    cached_generation: &str,
) -> Request {
    Request {
        root: root.to_path_buf(),
        saved,
        identity,
        evaluator,
        node: node.map(Path::to_path_buf).unwrap_or_default(),
        options: options.clone(),
        walk: false,
        cached_generation: cached_generation.to_owned(),
        deadline_ms: 120_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_keeps_one_folder_and_another_project_gets_its_own() {
        let base = std::env::temp_dir().join(format!("wb-catalog-{}", std::process::id()));
        let _clean = std::fs::remove_dir_all(&base);
        let acme = saved_path(&base, Path::new("/acme")).unwrap();
        assert_eq!(saved_path(&base, Path::new("/acme")).unwrap(), acme);
        let other = saved_path(&base, Path::new("/example")).unwrap();
        assert_ne!(other.parent(), acme.parent());
        assert!(acme.parent().unwrap().is_dir() && other.parent().unwrap().is_dir());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn the_identity_changes_with_the_settings() {
        let files = [PathBuf::from("/nowhere/workbench-server")];
        assert_eq!(identity(&files, &json!({})), identity(&files, &json!({})));
        assert_ne!(
            identity(&files, &json!({})),
            identity(&files, &json!({ "include": ["src"] }))
        );
    }
}
