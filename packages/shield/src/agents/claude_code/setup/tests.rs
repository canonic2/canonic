//! Persistent MCP approval, user ownership and malformed-input boundaries.
use super::*;

fn parsed(update: &SettingsUpdate) -> Value {
    serde_json::from_str(&update.source).unwrap()
}

fn approved() -> Vec<String> {
    vec!["status".into(), "policy_propose".into()]
}

#[test]
fn claude_approval_install_repair_and_removal_preserve_other_settings() {
    let original = json!({"theme":"acme","permissions":{"allow":["Read","mcp__other__inspect"],"ask":["Bash"],"deny":["Read(.env)"]},"enabledMcpjsonServers":["other"],"disabledMcpjsonServers":["unrelated"],"enableAllProjectMcpServers":false});
    let tools = approved();
    let installed = merge(Some(&original.to_string()), None, Some(tools.as_slice())).unwrap();
    let config = parsed(&installed);
    assert_eq!(
        config["permissions"]["allow"],
        json!([
            "Read",
            "mcp__other__inspect",
            "mcp__shield__status",
            "mcp__shield__policy_propose"
        ])
    );
    assert_eq!(config["enabledMcpjsonServers"], json!(["other", SERVER]));
    assert_eq!(config["permissions"]["ask"], json!(["Bash"]));
    let repaired = merge(
        Some(&installed.source),
        installed.ownership.as_ref(),
        Some(tools.as_slice()),
    )
    .unwrap();
    assert_eq!(parsed(&repaired), config);
    assert_eq!(repaired.ownership, installed.ownership);
    let removed = merge(Some(&repaired.source), repaired.ownership.as_ref(), None).unwrap();
    assert_eq!(parsed(&removed), original);
    assert!(removed.ownership.is_none());
    let mut diagnostics = Vec::new();
    diagnose(&config, &mut diagnostics);
    assert_eq!(diagnostics, Vec::<&str>::new());
}

#[test]
fn changed_approvals_drop_only_owned_rules() {
    let original = json!({"permissions":{"allow":["mcp__shield__status"]}});
    let installed = merge(
        Some(&original.to_string()),
        None,
        Some(approved().as_slice()),
    )
    .unwrap();
    let narrowed = merge(
        Some(&installed.source),
        installed.ownership.as_ref(),
        Some(&["status".to_owned()][..]),
    )
    .unwrap();
    assert_eq!(
        parsed(&narrowed)["permissions"]["allow"],
        json!(["mcp__shield__status"])
    );
    let none = merge(
        Some(&narrowed.source),
        narrowed.ownership.as_ref(),
        Some(&[][..]),
    )
    .unwrap();
    assert_eq!(
        parsed(&none)["permissions"]["allow"],
        original["permissions"]["allow"]
    );
}

#[test]
fn removal_keeps_preexisting_approvals_and_empty_containers() {
    let status = "mcp__shield__status";
    for original in [
        json!({}),
        json!({"permissions":{}}),
        json!({"permissions":{"allow":[]},"enabledMcpjsonServers":[]}),
        json!({"permissions":{"allow":[status]},"enabledMcpjsonServers":[SERVER]}),
        json!({"permissions":{"allow":[status,status]},"enabledMcpjsonServers":[SERVER,SERVER]}),
    ] {
        let installed = merge(
            Some(&original.to_string()),
            None,
            Some(approved().as_slice()),
        )
        .unwrap();
        let removed = merge(Some(&installed.source), installed.ownership.as_ref(), None).unwrap();
        assert_eq!(parsed(&removed), original);
    }
}

#[test]
fn repair_restores_missing_approvals_and_removal_keeps_later_user_additions() {
    let installed = merge(None, None, Some(approved().as_slice())).unwrap();
    let edited = json!({"permissions":{"allow":["Read"]},"enabledMcpjsonServers":["other"]});
    let repaired = merge(
        Some(&edited.to_string()),
        installed.ownership.as_ref(),
        Some(approved().as_slice()),
    )
    .unwrap();
    assert_eq!(
        parsed(&repaired)["permissions"]["allow"],
        json!(["Read", "mcp__shield__status", "mcp__shield__policy_propose"])
    );
    let removed = merge(Some(&repaired.source), repaired.ownership.as_ref(), None).unwrap();
    assert_eq!(parsed(&removed), edited);
    let mut diagnostics = Vec::new();
    diagnose(&edited, &mut diagnostics);
    assert_eq!(diagnostics.len(), 1);
}

#[test]
fn malformed_settings_and_ownership_refuse_before_publication() {
    for source in [
        "[]",
        "{",
        r#"{"permissions":null}"#,
        r#"{"permissions":{"allow":true}}"#,
        r#"{"permissions":{"allow":[1]}}"#,
        r#"{"enabledMcpjsonServers":"shield"}"#,
    ] {
        assert!(
            merge(Some(source), None, Some(approved().as_slice())).is_err(),
            "{source}"
        );
    }
    assert!(
        merge(
            None,
            Some(&json!({"unknown":true})),
            Some(approved().as_slice())
        )
        .is_err()
    );
}

#[test]
fn upgrade_removes_only_owned_apply_ask_entries() {
    let original =
        json!({"permissions":{"allow":[],"ask":["Bash",APPLY]},"enabledMcpjsonServers":[SERVER]});
    let ownership = json!({"ask":"existing","permissions_created":false});
    let upgraded = merge(
        Some(&original.to_string()),
        Some(&ownership),
        Some(approved().as_slice()),
    )
    .unwrap();
    assert_eq!(parsed(&upgraded)["permissions"]["ask"], json!(["Bash"]));
    let user_owned = merge(
        Some(&original.to_string()),
        None,
        Some(approved().as_slice()),
    )
    .unwrap();
    assert_eq!(
        parsed(&user_owned)["permissions"]["ask"],
        original["permissions"]["ask"]
    );
}
