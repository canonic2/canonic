//! Launcher locators stay local while reviewed policy survives relocation.
use super::*;

#[test]
fn portable_launcher_copy_requires_new_registration_and_preserves_original() {
    let (original, context) = fixture(true);
    let previous = registered(&context);
    let old = launch(&previous, "claude", "cloud");
    let checkpoint = original.path().join(".shield/review/state.json");
    let original_bytes = std::fs::read(&checkpoint).unwrap();
    let original_host = std::fs::read(original.path().join(".mcp.json")).unwrap();
    let copied = assert_fs::TempDir::new().unwrap();
    // A new machine retains project policy/state but starts with its own host settings.
    std::fs::create_dir(copied.path().join(".shield")).unwrap();
    crate::test_support::copy_fixture_directory(
        &original.path().join(".shield"),
        &copied.path().join(".shield"),
    );
    let manual = json!({"type":"http","url":"https://manual.example.com/mcp"});
    std::fs::write(
        copied.path().join(".mcp.json"),
        json!({"mcpServers":{"manual":manual}}).to_string(),
    )
    .unwrap();
    let child = review::open(copied.path()).unwrap();
    let before = state(&child);
    // Private origin preserves the review, while missing host entries cannot
    // acquire replacement tokens until their normal setup plan is applied.
    assert!(registration(&before.operator).unwrap().is_some());
    for agent in ["claude", "codex"] {
        for id in ["cloud", "neighbor"] {
            assert!(
                integrations::resolve_mcp_token(copied.path(), &launch(&previous, agent, id).token)
                    .is_err()
            );
        }
    }
    let snapshot = crate::runtime::Project::open(copied.path())
        .unwrap()
        .snapshot()
        .unwrap();
    assert_eq!(
        snapshot.identity().policy_revision(),
        before.policy_revision
    );
    assert!(
        integrations::mcp_projection(
            copied.path(),
            "claude",
            "cloud",
            &snapshot.mcp_connections()["cloud"]
        )
        .is_err()
    );
    let plan = setup::prepare(
        copied.path(),
        &before.operator,
        integrations::IntegrationChoice::Agents(
            integrations::IntegrationChoice::bits(&["claude", "codex"]).unwrap(),
        ),
    )
    .unwrap()
    .unwrap();
    assert!(integrations::resolve_mcp_token(copied.path(), &old.token).is_err());
    assert_eq!(
        std::fs::read(copied.path().join(".mcp.json")).unwrap(),
        json!({"mcpServers":{"manual":manual}})
            .to_string()
            .as_bytes()
    );
    integrations::apply(&child, &before.operator, &plan).unwrap();
    let current = registered(&child);
    for agent in ["claude", "codex"] {
        let next = launch(&current, agent, "cloud");
        assert_ne!(next.token, launch(&previous, agent, "cloud").token);
        let (id, binding) = integrations::resolve_mcp_token(copied.path(), &next.token).unwrap();
        assert_eq!(id, "cloud");
        assert!(binding.matches(&snapshot.mcp_connections()["cloud"]));
        let projected = integrations::mcp_projection(
            copied.path(),
            agent,
            "cloud",
            &snapshot.mcp_connections()["cloud"],
        )
        .unwrap();
        integrations::verify_mcp_connection(copied.path(), agent, "acme_cloud", &projected)
            .unwrap();
    }
    assert!(integrations::resolve_mcp_token(copied.path(), &old.token).is_err());
    assert_eq!(state(&child).policy_revision, before.policy_revision);
    let host: Value =
        serde_json::from_slice(&std::fs::read(copied.path().join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(host["mcpServers"]["manual"], manual);
    assert_eq!(std::fs::read(&checkpoint).unwrap(), original_bytes);
    assert_eq!(
        std::fs::read(original.path().join(".mcp.json")).unwrap(),
        original_host
    );
    assert!(integrations::resolve_mcp_token(original.path(), &old.token).is_ok());
}

#[test]
fn portable_launcher_rename_rotates_tokens_without_policy_or_neighbor_changes() {
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
    assert!(!original.path().exists());
    let reopened = review::open(&moved).unwrap();
    let current = registered(&reopened);
    assert_eq!(state(&reopened).policy_revision, revision);
    for agent in ["claude", "codex"] {
        let old = launch(&previous, agent, "cloud");
        let next = launch(&current, agent, "cloud");
        assert_ne!(old.token, next.token);
        assert!(integrations::resolve_mcp_token(&moved, &old.token).is_err());
        assert!(integrations::resolve_mcp_token(&moved, &next.token).is_ok());
    }
    assert!(current.connections["claude"]["launches"]["neighbor"].is_null());
    let host: Value =
        serde_json::from_slice(&std::fs::read(moved.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(host["mcpServers"]["acme_neighbor"], neighbor);
    assert_eq!(host["mcpServers"]["manual"], manual);
}
