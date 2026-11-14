//! Private launcher tokens are folder state: a renamed or copied project keeps
//! them, and reviewed policy and neighboring host entries stay unchanged.
use super::*;

#[test]
fn portable_launcher_rename_keeps_tokens_policy_and_neighbors() {
    let (original, context) = fixture(true);
    let previous = registered(&context);
    let revision = state(&context).policy_revision;
    let host_path = original.path().join(".mcp.json");
    let mut host: Value = serde_json::from_slice(&std::fs::read(&host_path).unwrap()).unwrap();
    host["mcpServers"]["acme_neighbor"]["command"] = json!("acme-user-custom");
    let neighbor = host["mcpServers"]["acme_neighbor"].clone();
    let manual = host["mcpServers"]["manual"].clone();
    std::fs::write(&host_path, serde_json::to_vec(&host).unwrap()).unwrap();
    let target = assert_fs::TempDir::new().unwrap();
    let moved = target.path().join("renamed project");
    std::fs::rename(original.path(), &moved).unwrap();
    let reopened = review::open(&moved).unwrap();
    let current = registered(&reopened);
    assert_eq!(state(&reopened).policy_revision, revision);
    for agent in ["claude", "codex"] {
        let old = launch(&previous, agent, "cloud");
        assert_eq!(launch(&current, agent, "cloud").token, old.token);
        assert!(integrations::resolve_mcp_token(&moved, &old.token).is_ok());
    }
    let host: Value =
        serde_json::from_slice(&std::fs::read(moved.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(host["mcpServers"]["acme_neighbor"], neighbor);
    assert_eq!(host["mcpServers"]["manual"], manual);
    assert_eq!(host["mcpServers"]["acme_cloud"]["args"][3], ".");
}

#[test]
fn portable_launcher_copy_keeps_its_own_tokens_and_leaves_the_original_alone() {
    let (original, context) = fixture(true);
    let previous = registered(&context);
    let checkpoint = original.path().join(".shield/state/state.json");
    let original_bytes = std::fs::read(&checkpoint).unwrap();
    let original_host = std::fs::read(original.path().join(".mcp.json")).unwrap();
    let copied = assert_fs::TempDir::new().unwrap();
    copy_project(original.path(), copied.path());
    let child = review::open(copied.path()).unwrap();
    assert_eq!(child.project_id, context.project_id);
    let current = registered(&child);
    for agent in ["claude", "codex"] {
        let token = launch(&previous, agent, "cloud").token;
        assert_eq!(launch(&current, agent, "cloud").token, token);
        assert!(integrations::resolve_mcp_token(copied.path(), &token).is_ok());
        assert!(integrations::resolve_mcp_token(original.path(), &token).is_ok());
    }
    assert_eq!(std::fs::read(&checkpoint).unwrap(), original_bytes);
    assert_eq!(
        std::fs::read(original.path().join(".mcp.json")).unwrap(),
        original_host
    );
}
