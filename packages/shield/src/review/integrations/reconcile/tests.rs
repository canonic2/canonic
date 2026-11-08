use super::*;
use serde_json::json;

fn text(value: impl Into<Value>) -> String {
    value.into().to_string()
}

fn hook(command: &str) -> Value {
    json!({"matcher":".*","hooks":[{"type":"command","command":command,"statusMessage":"Shield project enforcement"}]})
}

#[test]
fn json_reconciliation_preserves_existing_top_level_and_nested_key_order() {
    let before =
        r#"{"zebra":{"z":1,"a":2},"mcpServers":{"shield":{"command":"old"}},"alpha":true}"#;
    let after = before.replace("old", "new");
    for current in [
        before.to_owned(),
        before.replace("\"alpha\":true", "\"alpha\":true,\"neighbor\":42"),
    ] {
        let output = reconcile(".mcp.json", Some(before), Some(&after), Some(&current))
            .unwrap()
            .unwrap();
        assert!(output.find("\"zebra\"").unwrap() < output.find("\"alpha\"").unwrap());
        assert!(output.find("\"z\"").unwrap() < output.find("\"a\"").unwrap());
        let actual: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(actual["mcpServers"]["shield"]["command"], "new");
        assert_eq!(actual["zebra"], json!({"z":1,"a":2}));
    }
}

#[test]
fn codex_inline_current_servers_preserve_foreign_alias_during_recovery_and_review() {
    let old = "[mcp_servers.shield]\ncommand = 'old'\n";
    let new = "[mcp_servers.shield]\ncommand = 'new'\n";
    let current =
        "mcp_servers = { shield = { command = 'old' }, acme = { command = 'foreign' } }\n";
    let merged = reconcile(".codex/config.toml", Some(old), Some(new), Some(current))
        .unwrap()
        .unwrap();
    let parsed: toml_edit::DocumentMut = merged.parse().unwrap();
    assert!(parsed["mcp_servers"].as_inline_table().is_some());
    assert_eq!(
        parsed["mcp_servers"]["shield"]["command"].as_str(),
        Some("new")
    );
    assert_eq!(
        parsed["mcp_servers"]["acme"]["command"].as_str(),
        Some("foreign")
    );

    let edited = current.replace("'old'", "'manual'");
    let (reviewed, kept) = resolve(
        ".codex/config.toml",
        Some(old),
        Some(new),
        Some(&edited),
        Conflict::ReplaceOwned,
    )
    .unwrap();
    assert!(!kept);
    let reviewed: toml_edit::DocumentMut = reviewed.unwrap().parse().unwrap();
    assert_eq!(
        reviewed["mcp_servers"]["shield"]["command"].as_str(),
        Some("new")
    );
    assert_eq!(
        reviewed["mcp_servers"]["acme"]["command"].as_str(),
        Some("foreign")
    );
    let (cancelled, kept) = resolve(
        ".codex/config.toml",
        Some(new),
        Some(old),
        Some(&edited),
        Conflict::KeepCurrent,
    )
    .unwrap();
    assert!(kept);
    assert_eq!(cancelled.as_deref(), Some(edited.as_str()));

    let collision = current.replace("acme", "SHIELD");
    assert!(reconcile(".codex/config.toml", Some(old), Some(new), Some(&collision)).is_err());
}

#[test]
fn pending_json_setup_keeps_foreign_hooks_permissions_servers_and_settings() {
    let old = text(
        json!({"hooks":{"PreToolUse":[hook("shield-old __internal native-hook")]},"permissions":{"allow":["Read"]}}),
    );
    let new = text(
        json!({"hooks":{"PreToolUse":[hook("shield-new __internal native-hook")]},"permissions":{"allow":["Read","mcp__shield__status"]}}),
    );
    let actual = text(
        json!({"hooks":{"PreToolUse":[hook("shield-old __internal native-hook"),{"matcher":"Write","hooks":[{"type":"command","command":"acme-format"}]}]},"permissions":{"allow":["Read","Bash(acme-check)"]},"theme":"light"}),
    );
    let merged = reconcile(
        ".claude/settings.local.json",
        Some(&old),
        Some(&new),
        Some(&actual),
    )
    .unwrap()
    .unwrap();
    let result: Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(result["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
    assert!(
        result["hooks"]["PreToolUse"]
            .as_array()
            .unwrap()
            .contains(&hook("shield-new __internal native-hook"))
    );
    assert_eq!(result["theme"], "light");
    assert!(
        result["permissions"]["allow"]
            .as_array()
            .unwrap()
            .contains(&json!("Bash(acme-check)"))
    );
    assert!(
        result["permissions"]["allow"]
            .as_array()
            .unwrap()
            .contains(&json!("mcp__shield__status"))
    );
}

#[test]
fn manual_owned_hook_changes_require_review_and_cancellation_preserves_them() {
    let old = text(json!({"hooks":{"PreToolUse":[hook("shield-old __internal native-hook")]}}));
    let new = text(
        json!({"hooks":{"PreToolUse":[hook("shield-new __internal native-hook")]},"enabledMcpjsonServers":["shield"]}),
    );
    let actual = text(
        json!({"hooks":{"PreToolUse":[hook("manual __internal native-hook"),{"hooks":[{"command":"acme-watch"}]}]},"enabledMcpjsonServers":["shield","acme"]}),
    );
    assert!(
        reconcile(
            ".claude/settings.local.json",
            Some(&old),
            Some(&new),
            Some(&actual)
        )
        .is_err()
    );
    let (cancelled, kept) = resolve(
        ".claude/settings.local.json",
        Some(&new),
        Some(&old),
        Some(&actual),
        Conflict::KeepCurrent,
    )
    .unwrap();
    assert!(kept);
    let cancelled: Value = serde_json::from_str(cancelled.as_deref().unwrap()).unwrap();
    assert!(
        cancelled["hooks"]["PreToolUse"]
            .as_array()
            .unwrap()
            .contains(&hook("manual __internal native-hook"))
    );
    assert_eq!(cancelled["enabledMcpjsonServers"], json!(["acme"]));
    let (replaced, kept) = resolve(
        ".claude/settings.local.json",
        Some(&old),
        Some(&new),
        Some(&actual),
        Conflict::ReplaceOwned,
    )
    .unwrap();
    assert!(!kept);
    let replaced: Value = serde_json::from_str(replaced.as_deref().unwrap()).unwrap();
    assert!(
        replaced["hooks"]["PreToolUse"]
            .as_array()
            .unwrap()
            .contains(&hook("shield-new __internal native-hook"))
    );
    assert_eq!(replaced["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
}

#[test]
fn no_op_owned_hook_plan_still_detects_manual_drift() {
    let planned = text(json!({"hooks":{"PreToolUse":[hook("shield __internal native-hook")]}}));
    let actual = text(
        json!({"hooks":{"PreToolUse":[hook("manual __internal native-hook")]},"theme":"dark"}),
    );
    assert!(
        reconcile(
            ".claude/settings.local.json",
            Some(&planned),
            Some(&planned),
            Some(&actual)
        )
        .is_err()
    );
}

#[test]
fn reserved_mcp_entry_is_atomic_while_foreign_entries_survive() {
    let old = text(json!({"mcpServers":{"shield":{"command":"old","args":["mcp"]}}}));
    let new = text(json!({"mcpServers":{"shield":{"command":"new","args":["mcp"]}}}));
    let actual = text(
        json!({"mcpServers":{"shield":{"command":"old","args":["mcp"]},"acme":{"command":"acme-server"}}}),
    );
    let merged = reconcile(".mcp.json", Some(&old), Some(&new), Some(&actual))
        .unwrap()
        .unwrap();
    let result: Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(result["mcpServers"]["acme"]["command"], "acme-server");
    assert_eq!(result["mcpServers"]["shield"]["command"], "new");
    let changed =
        text(json!({"mcpServers":{"shield":{"command":"old","args":["mcp"],"extra":true}}}));
    assert!(reconcile(".mcp.json", Some(&old), Some(&new), Some(&changed)).is_err());
}

#[test]
fn dedicated_assets_remain_atomic_and_manual_bytes_survive_cancellation() {
    for path in [
        ".box/mcp/shield.yml",
        ".codex/rules/shield.rules",
        ".claude/skills/shield/register.mjs",
    ] {
        assert!(reconcile(path, Some("old"), Some("new"), Some("manual")).is_err());
        assert_eq!(
            resolve(
                path,
                Some("old"),
                Some("new"),
                Some("manual"),
                Conflict::ReplaceOwned
            )
            .unwrap(),
            (Some("new".into()), false)
        );
        assert_eq!(
            resolve(
                path,
                Some("new"),
                Some("old"),
                Some("manual"),
                Conflict::KeepCurrent
            )
            .unwrap(),
            (Some("manual".into()), true)
        );
    }
}

#[test]
fn foreign_callbacks_in_the_owned_group_survive_before_and_after_partial_publication() {
    let old_group = hook("shield-old __internal native-hook");
    let new_group = hook("shield-new __internal native-hook");
    let old = text(json!({"hooks":{"PreToolUse":[old_group]}}));
    let new = text(json!({"hooks":{"PreToolUse":[new_group]}}));
    let foreign = json!({"type":"command","command":"acme-observe"});
    for mut group in [old_group, new_group.clone()] {
        group["hooks"].as_array_mut().unwrap().push(foreign.clone());
        let current = text(json!({"hooks":{"PreToolUse":[group]}}));
        let merged = reconcile(
            ".claude/settings.local.json",
            Some(&old),
            Some(&new),
            Some(&current),
        )
        .unwrap()
        .unwrap();
        let parsed: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(
            parsed["hooks"]["PreToolUse"][0]["hooks"],
            json!([new_group["hooks"][0], foreign])
        );
        assert_eq!(
            reconcile(
                ".claude/settings.local.json",
                Some(&old),
                Some(&new),
                Some(&merged)
            )
            .unwrap(),
            Some(merged)
        );
    }
}

#[test]
fn foreign_container_type_changes_do_not_conflict_with_shield_delta() {
    let old = text(json!({"mcpServers":{"shield":{"command":"old"}},"acme":{"theme":"dark"}}));
    let new = text(json!({"mcpServers":{"shield":{"command":"new"}},"acme":{"theme":"dark"}}));
    for current_foreign in [Some(json!("custom")), Some(Value::Null), None] {
        let mut current = json!({"mcpServers":{"shield":{"command":"old"}}});
        if let Some(value) = &current_foreign {
            current["acme"] = value.clone();
        }
        let merged = reconcile(
            ".mcp.json",
            Some(&old),
            Some(&new),
            Some(&current.to_string()),
        )
        .unwrap()
        .unwrap();
        let parsed: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(parsed.get("acme"), current_foreign.as_ref());
        assert_eq!(parsed["mcpServers"]["shield"]["command"], "new");
    }
}

#[test]
fn new_recognizable_shield_callbacks_require_review_instead_of_clearing_pending() {
    let old = text(json!({"hooks":{"PreToolUse":[hook("shield-old __internal native-hook")]}}));
    let new = text(json!({"hooks":{"PreToolUse":[hook("shield-new __internal native-hook")]}}));
    let mut group = hook("shield-old __internal native-hook");
    group["hooks"]
        .as_array_mut()
        .unwrap()
        .push(hook("manual __internal native-hook")["hooks"][0].clone());
    let actual = text(json!({"hooks":{"PreToolUse":[group]}}));
    assert!(
        reconcile(
            ".claude/settings.local.json",
            Some(&old),
            Some(&new),
            Some(&actual)
        )
        .is_err()
    );
    let (merged, kept) = resolve(
        ".claude/settings.local.json",
        Some(&old),
        Some(&new),
        Some(&actual),
        Conflict::ReplaceOwned,
    )
    .unwrap();
    assert!(!kept);
    let parsed: Value = serde_json::from_str(merged.as_deref().unwrap()).unwrap();
    assert_eq!(
        parsed["hooks"]["PreToolUse"][0]["hooks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn codex_toml_recovery_preserves_foreign_tables_comments_and_current_layout() {
    let old = "model = 'acme'\n[mcp_servers.shield]\ncommand = 'old'\nargs = ['mcp']\n";
    let new = old.replace("'old'", "'new'");
    let actual = "model = 'acme'\ntheme = 'light' # user\n# provider\n[mcp_servers.acme]\ncommand = 'acme-provider'\n\n[mcp_servers.shield]\ncommand = 'old'\nargs = ['mcp']\n";
    let merged = reconcile(".codex/config.toml", Some(old), Some(&new), Some(actual))
        .unwrap()
        .unwrap();
    assert!(merged.contains("theme = 'light' # user"));
    assert!(merged.contains("# provider\n[mcp_servers.acme]\ncommand = 'acme-provider'"));
    let parsed: toml_edit::DocumentMut = merged.parse().unwrap();
    assert_eq!(
        parsed["mcp_servers"]["shield"]["command"].as_str(),
        Some("new")
    );
    assert_eq!(
        reconcile(".codex/config.toml", Some(&new), Some(&new), Some(&merged)).unwrap(),
        Some(merged)
    );
}

#[test]
fn codex_owned_toml_conflicts_are_reviewed_and_cancelled_without_foreign_loss() {
    let old = "[mcp_servers.shield]\ncommand = 'old'\n";
    let new = "[mcp_servers.shield]\ncommand = 'new'\n";
    let actual = "theme = 'light'\n[mcp_servers.shield]\ncommand = 'manual'\n";
    assert!(reconcile(".codex/config.toml", Some(old), Some(new), Some(actual)).is_err());
    assert_eq!(
        resolve(
            ".codex/config.toml",
            Some(new),
            Some(old),
            Some(actual),
            Conflict::KeepCurrent
        )
        .unwrap(),
        (Some(actual.into()), true)
    );
    let (merged, _) = resolve(
        ".codex/config.toml",
        Some(old),
        Some(new),
        Some(actual),
        Conflict::ReplaceOwned,
    )
    .unwrap();
    assert!(merged.unwrap().contains("theme = 'light'"));
}

#[test]
fn merged_output_limit_is_checked_after_foreign_values_are_preserved() {
    let half = usize::try_from(MAX_CONFIG).unwrap() / 2;
    let old = "{}";
    let new = json!({"owned":"x".repeat(half)}).to_string();
    let actual = json!({"foreign":"y".repeat(half)}).to_string();
    assert!(new.len() as u64 <= MAX_CONFIG && actual.len() as u64 <= MAX_CONFIG);
    assert!(reconcile(".mcp.json", Some(old), Some(&new), Some(&actual)).is_err());
}

#[test]
fn new_json_alias_collisions_refuse_publication_and_preserve_foreign_alias_on_cancel() {
    let old = text(json!({"mcpServers":{"shield":{"command":"old"}}}));
    let new = text(json!({"mcpServers":{"shield":{"command":"new"}}}));
    let actual =
        text(json!({"mcpServers":{"shield":{"command":"old"},"SHIELD":{"command":"foreign"}}}));
    assert!(reconcile(".mcp.json", Some(&old), Some(&new), Some(&actual)).is_err());
    assert!(
        resolve(
            ".mcp.json",
            Some(&old),
            Some(&new),
            Some(&actual),
            Conflict::ReplaceOwned
        )
        .is_err()
    );
    assert_eq!(
        resolve(
            ".mcp.json",
            Some(&new),
            Some(&old),
            Some(&actual),
            Conflict::KeepCurrent
        )
        .unwrap(),
        (Some(actual), true)
    );
    let already_published =
        text(json!({"mcpServers":{"shield":{"command":"new"},"SHIELD":{"command":"foreign"}}}));
    assert!(
        reconcile(
            ".mcp.json",
            Some(&old),
            Some(&already_published),
            Some(&already_published)
        )
        .is_err(),
        "the exact-after fast path still checks namespace collisions"
    );
}

#[test]
fn codex_punctuation_alias_collisions_do_not_silently_delete_foreign_servers() {
    let old = "[mcp_servers.shield]\ncommand = 'old'\n";
    let new = "[mcp_servers.shield]\ncommand = 'new'\n";
    let actual =
        "[mcp_servers.shield]\ncommand = 'old'\n[mcp_servers.shield-]\ncommand = 'foreign'\n";
    assert!(reconcile(".codex/config.toml", Some(old), Some(new), Some(actual)).is_err());
    assert!(
        resolve(
            ".codex/config.toml",
            Some(old),
            Some(new),
            Some(actual),
            Conflict::ReplaceOwned
        )
        .is_err()
    );
    assert_eq!(
        resolve(
            ".codex/config.toml",
            Some(new),
            Some(old),
            Some(actual),
            Conflict::KeepCurrent
        )
        .unwrap(),
        (Some(actual.into()), true)
    );
}

#[test]
fn codex_nested_tool_approval_delta_is_not_hidden_by_unchanged_parent_values() {
    let old = "[mcp_servers.shield]\ncommand = 'shield'\n[mcp_servers.shield.tools.status]\napproval_mode = 'prompt'\n";
    let new = old.replace("'prompt'", "'approve'");
    let actual = format!("theme = 'light'\n{old}");
    let merged = reconcile(".codex/config.toml", Some(old), Some(&new), Some(&actual))
        .unwrap()
        .unwrap();
    let parsed: toml_edit::DocumentMut = merged.parse().unwrap();
    assert_eq!(
        parsed["mcp_servers"]["shield"]["tools"]["status"]["approval_mode"].as_str(),
        Some("approve")
    );
    assert_eq!(parsed["theme"].as_str(), Some("light"));
}
