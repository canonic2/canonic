//! One host-reported workspace/session's artifact read scope.
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) fn root(host: &Value, project: &Path, config: &Path) -> Option<PathBuf> {
    let session = super::text(&host["conversationId"], 128)?;
    if !session
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let project = project.canonicalize().ok()?;
    if !host["workspacePaths"].as_array()?.iter().any(|p| {
        p.as_str()
            .is_some_and(|p| Path::new(p).canonicalize().ok().as_ref() == Some(&project))
    }) {
        return None;
    }
    let expected = config
        .canonicalize()
        .ok()?
        .join("antigravity-cli/brain")
        .join(session);
    if host["artifactDirectoryPath"].as_str()? != expected.to_str()? {
        return None;
    }
    let transcript = Path::new(host["transcriptPath"].as_str()?);
    if !["transcript_full.jsonl", "transcript.jsonl"]
        .iter()
        .any(|name| transcript == expected.join(".system_generated/logs").join(name))
    {
        return None;
    }
    let actual = expected.canonicalize().ok()?;
    (actual == expected && actual.is_dir()).then_some(actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn antigravity_artifacts_require_workspace_session_and_native_layout() {
        let temp = assert_fs::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();
        let config = project.join("config");
        let artifacts = config.join("antigravity-cli/brain/acme-session");
        std::fs::create_dir_all(&artifacts).unwrap();
        let mut host = json!({"conversationId":"acme-session","workspacePaths":[project],"artifactDirectoryPath":artifacts,"transcriptPath":artifacts.join(".system_generated/logs/transcript_full.jsonl")});
        assert_eq!(root(&host, &project, &config), Some(artifacts));
        host["conversationId"] = json!("other-session");
        assert_eq!(root(&host, &project, &config), None);
        host["conversationId"] = json!("../acme-session");
        assert_eq!(root(&host, &project, &config), None);
        host["conversationId"] = json!("acme-session");
        host["workspacePaths"] = json!([]);
        assert_eq!(root(&host, &project, &config), None);
    }
}
