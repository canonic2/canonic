//! Read-only native project/session observations. Never read transcript contents.
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) fn directory(host: &Value, project: &Path, runtime: &Path) -> Option<PathBuf> {
    let session = super::text(&host["session_id"], 128)?;
    if session.len() < 8
        || !session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let runtime = runtime.canonicalize().ok()?;
    let transcript = Path::new(super::text(&host["transcript_path"], 4096)?);
    let relative = transcript.strip_prefix(runtime.join("tmp")).ok()?;
    let parts: Vec<_> = relative.components().collect();
    if parts.len() != 3 || parts[1].as_os_str() != "chats" {
        return None;
    }
    let slug = parts[0].as_os_str().to_str()?;
    if slug.is_empty()
        || !slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return None;
    }
    let name = parts[2].as_os_str().to_str()?;
    let stem = name
        .strip_suffix(".jsonl")
        .or_else(|| name.strip_suffix(".json"))?;
    if !(stem.starts_with("session-") && stem.ends_with(&format!("-{}", &session[..8]))) {
        return None;
    }
    let expected = runtime.join("tmp").join(slug);
    let actual = expected.canonicalize().ok()?;
    if actual != expected || !actual.is_dir() {
        return None;
    }
    let marker = expected.join(".project_root");
    let metadata = std::fs::symlink_metadata(&marker).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
        return None;
    }
    let owner = std::fs::read_to_string(marker).ok()?;
    if Path::new(owner.trim()).canonicalize().ok()? != project.canonicalize().ok()? {
        return None;
    }
    Some(actual)
}

pub(super) fn outputs(host: &Value, project: &Path, runtime: &Path) -> Vec<PathBuf> {
    let Some(directory) = directory(host, project, runtime) else {
        return vec![];
    };
    let mut paths = vec![];
    if let Some(session) = host["session_id"].as_str() {
        let expected = directory.join(session);
        if expected
            .canonicalize()
            .is_ok_and(|actual| actual == expected && actual.is_dir())
        {
            paths.push(expected);
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    #[test]
    fn gemini_session_roots_bind_project_marker_transcript_and_session() {
        let temp = assert_fs::TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let project = root.join("project");
        let runtime = root.join("runtime");
        let directory = runtime.join("tmp/acme");
        let outputs = directory.join("12345678-acme/tasks");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&outputs).unwrap();
        std::fs::create_dir_all(directory.join("chats")).unwrap();
        std::fs::write(directory.join(".project_root"), project.to_str().unwrap()).unwrap();
        let transcript = directory.join("chats/session-2026-11-05T00-00-12345678.jsonl");
        std::fs::write(&transcript, "Acme transcript").unwrap();
        let mut host = json!({"session_id":"12345678-acme","transcript_path":transcript});
        assert_eq!(
            super::outputs(&host, &project, &runtime),
            vec![directory.join("12345678-acme")]
        );
        host["session_id"] = json!("87654321-other");
        assert_eq!(
            super::outputs(&host, &project, &runtime),
            [] as [std::path::PathBuf; 0]
        );
        host["session_id"] = json!("12345678-acme");
        std::fs::write(directory.join(".project_root"), root.to_str().unwrap()).unwrap();
        assert_eq!(
            super::outputs(&host, &project, &runtime),
            [] as [std::path::PathBuf; 0]
        );
    }
}
