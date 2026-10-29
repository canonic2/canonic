//! Scoped MCP approval installation, repair, conflict detection and removal.
use super::*;

#[test]
fn shield_mcp_approval_upgrade_preserves_other_servers_and_settings() {
    let desired = mcp(
        Path::new("/opt/acme/shield"),
        Path::new("/work/acme"),
        &crate::review::mcp::default_approved(),
    );
    let mut old: DocumentMut = desired.parse().unwrap();
    old["default_tools_approval_mode"] = value("approve");
    old.remove("tools");
    let old = old.to_string();
    let other = "# Acme settings\nmodel = 'acme'\n[mcp_servers.other]\ncommand = 'other-server'\ndefault_tools_approval_mode = 'prompt'\n[mcp_servers.other.tools.inspect]\napproval_mode = 'prompt'\n";
    let before = format!("{other}\n[mcp_servers.shield]\n{old}");
    let installed = merge_mcp(Some(&before), Some(&old), Some(&desired)).unwrap();
    assert!(installed.starts_with("# Acme settings\nmodel = 'acme'\n"));
    let doc: DocumentMut = installed.parse().unwrap();
    assert_eq!(
        doc["mcp_servers"]["shield"]["default_tools_approval_mode"].as_str(),
        Some("prompt")
    );
    assert_eq!(
        doc["mcp_servers"]["other"]["default_tools_approval_mode"].as_str(),
        Some("prompt")
    );
    assert_eq!(
        doc["mcp_servers"]["other"]["tools"]["inspect"]["approval_mode"].as_str(),
        Some("prompt")
    );
    assert_eq!(
        merge_mcp(Some(&installed), Some(&desired), Some(&desired)).unwrap(),
        installed
    );
    let removed = merge_mcp(Some(&installed), Some(&desired), None).unwrap();
    assert_eq!(
        removed.parse::<DocumentMut>().unwrap()["mcp_servers"]["other"].to_string(),
        other.parse::<DocumentMut>().unwrap()["mcp_servers"]["other"].to_string()
    );
    assert!(!has_mcp_entry(&removed).unwrap());
    let changed = installed.replace(
        "default_tools_approval_mode = \"prompt\"",
        "default_tools_approval_mode = \"approve\"",
    );
    assert!(merge_mcp(Some(&changed), Some(&desired), Some(&desired)).is_err());
    let mut diagnostics = Vec::new();
    diagnose(Some(&before), &mut diagnostics).unwrap();
    assert!(diagnostics.contains(&"Shield MCP mutations need default approval prompts in Codex"));
    assert_eq!(
        doc["mcp_servers"]["shield"]["tools"]["policy_apply"]["approval_mode"].as_str(),
        Some("prompt")
    );
    assert_eq!(
        doc["mcp_servers"]["shield"]["tools"]["policy_propose"]["approval_mode"].as_str(),
        Some("approve")
    );
    diagnostics.clear();
    diagnose(Some(&installed), &mut diagnostics).unwrap();
    assert_eq!(diagnostics, Vec::<&str>::new());
}

#[test]
fn marking_apply_still_prompts_in_codex() {
    let every: Vec<String> = crate::review::mcp::tool_summaries()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let desired = mcp(
        Path::new("/opt/acme/shield"),
        Path::new("/work/acme"),
        &every,
    );
    let doc: DocumentMut = desired.parse().unwrap();
    let tools = &doc["tools"];
    assert_eq!(
        tools["policy_apply"]["approval_mode"].as_str(),
        Some("prompt")
    );
    assert_eq!(
        tools["policy_propose"]["approval_mode"].as_str(),
        Some("approve")
    );
    let installed = merge_mcp(None, None, Some(&desired)).unwrap();
    let approving = installed.replace(
        "[mcp_servers.shield.tools.policy_apply]\napproval_mode = \"prompt\"",
        "[mcp_servers.shield.tools.policy_apply]\napproval_mode = \"approve\"",
    );
    assert_ne!(approving, installed);
    let mut diagnostics = Vec::new();
    diagnose(Some(&installed), &mut diagnostics).unwrap();
    assert_eq!(diagnostics, Vec::<&str>::new());
    diagnose(Some(&approving), &mut diagnostics).unwrap();
    assert!(diagnostics.contains(&"Shield MCP tool approvals need repair in Codex"));
}

#[test]
fn prefix_rules_list_each_pattern_once_with_its_sources() {
    use crate::policy::agent_outputs::CodexPrefix;
    assert_eq!(prefix_rules(&[]), None);
    let prefix = |rule: &str, pattern: Vec<Vec<&str>>| CodexPrefix {
        owner: "acme/tools".into(),
        rule: rule.into(),
        pattern: pattern
            .into_iter()
            .map(|p| p.into_iter().map(str::to_owned).collect())
            .collect(),
    };
    let text = prefix_rules(&[
        prefix(
            "tail",
            vec![vec!["acme"], vec!["logs"], vec!["tail", "filter"]],
        ),
        prefix(
            "again",
            vec![vec!["acme"], vec!["logs"], vec!["tail", "filter"]],
        ),
        prefix("quoted", vec![vec!["acme"], vec!["say \"hi\""]]),
        prefix("either", vec![vec!["acme", "acme-cli"], vec!["logs"]]),
    ])
    .unwrap();
    assert!(text.starts_with(PREFIX_RULES_HEADER));
    assert_eq!(text.matches("prefix_rule(").count(), 3, "{text}");
    // Codex accepts a union in the executable position too.
    assert!(
        text.contains(r#"pattern = [["acme", "acme-cli"], "logs"],"#),
        "{text}"
    );
    assert!(
        text.contains(r#"pattern = ["acme", "logs", ["tail", "filter"]],"#),
        "{text}"
    );
    assert!(
        text.contains(r#"match = [["acme", "logs", "tail"]],"#),
        "{text}"
    );
    assert!(
        text.contains("# acme/tools tail, acme/tools again"),
        "{text}"
    );
    assert!(
        text.contains(r#"pattern = ["acme", "say \"hi\""],"#),
        "{text}"
    );
}
