//! Recognize administration receipts written by an earlier format, read only,
//! for one conversion. That format keyed each folder's administration settings
//! and registration to a per-user receipt; a checkpoint whose receipt is
//! committed and matches exactly carries this user's earlier approval of
//! those settings. Afterwards consent is keyed by content in `authority`.
use super::{Error, State, portability, private_files};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const LIMIT: usize = 512 * 1024;

fn directory() -> Result<PathBuf, Error> {
    #[cfg(not(test))]
    let parent = portability::directory()?;
    #[cfg(test)]
    let parent = if let Some(path) = portability::consent::fixture_directory() {
        path.with_extension("administration")
    } else {
        let executable = std::env::current_exe()?;
        std::env::temp_dir().canonicalize()?.join(format!(
            "shield-administration-{}",
            crate::policy::fingerprint(executable.to_string_lossy().as_bytes())
        ))
    };
    private_files::directory(&parent, false)?;
    let path = parent.join("administration");
    private_files::directory(&path, false)?;
    Ok(path)
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn settings(state: &State) -> Value {
    json!({"enabled":state.enforcement.enabled,"path_access":state.operator["path_access"],
        "integration":state.operator["integration"],"pending":!state.operator["integration_pending"].is_null()})
}

fn read(directory: &Path, origin: &str) -> Option<Value> {
    if !digest(origin) {
        return None;
    }
    private_files::read(
        &directory.join("origins").join(origin).join("state.json"),
        LIMIT,
    )
    .or_else(|_| private_files::read(&directory.join(format!("{origin}.json")), LIMIT))
    .ok()
}

/// Whether the checkpoint's earlier-format administration receipt is this
/// user's committed approval of exactly the settings it holds.
pub(super) fn committed(state: &State) -> bool {
    let reference = &state.operator["administration_origin"];
    let (Some(origin), Some(token)) = (reference["origin"].as_str(), reference["token"].as_str())
    else {
        return false;
    };
    let Ok(directory) = directory() else {
        return false;
    };
    read(&directory, origin).is_some_and(|record| {
        reference["schema_version"] == 1
            && record["schema_version"] == 1
            && record["token"] == token
            && record["project"] == state.project_id.as_str()
            && record["committed"] == true
            && record["administration"] == settings(state)
    })
}
