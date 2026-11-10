use super::*;
use std::collections::BTreeMap;

#[test]
fn copilot_requires_explicit_selection_even_when_its_cli_is_present() {
    let root = assert_fs::TempDir::new().unwrap();
    std::fs::write(root.path().join("copilot"), "fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.path().join("copilot"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    std::fs::write(root.path().join("copilot.exe"), "fixture").unwrap();
    assert!(!Copilot.is_installed(&[root.path().to_owned()]));
}

fn envelope(tool: &str, input: Value) -> Value {
    let mut host = json!({"sessionId":"copilot-fixture-session","timestamp":1_794_205_347_620_u64,"cwd":std::env::temp_dir(),"toolName":tool});
    host["toolArgs"] = input;
    host
}

#[test]
fn copilot_pinned_events_keep_complete_inputs_without_inventing_correlation() {
    let host = envelope(
        "bash",
        json!({"command":"printf fixture","description":"Print fixture"}),
    );
    let request = Copilot.request(&host).unwrap();
    assert_eq!(request.session, "copilot-fixture-session");
    assert!(request.tool_id.is_none());
    assert!(request.context_complete);
    assert_eq!(request.operation.unwrap()["source"], "printf fixture");
    assert!(Copilot.input_event(&host).is_none());
    let mut completed = host.clone();
    completed["toolResult"] = json!({"resultType":"success","textResultForLlm":"fixture"});
    assert!(
        Copilot
            .request_for_event(&completed, Event::Completed)
            .is_some()
    );
    assert!(Copilot.request_for_event(&host, Event::Completed).is_none());
    completed["hook_event_name"] = json!("PreToolUse");
    assert!(
        Copilot
            .request_for_event(&completed, Event::Completed)
            .is_none()
    );
    let mut failed = host;
    failed["error"] = json!("fixture failure");
    assert!(Copilot.request_for_event(&failed, Event::Failed).is_some());
}

#[test]
fn copilot_reused_shell_context_and_unknown_arguments_do_not_gain_known_cwd() {
    let mut host = envelope(
        "bash",
        json!({"command":"pwd","description":"Print folder","shellId":"earlier-shell"}),
    );
    assert!(!Copilot.request(&host).unwrap().context_complete);
    assert_eq!(
        Copilot.request(&host).unwrap().operation.unwrap()["unmodeled"],
        true
    );
    host["toolArgs"]["workdir"] = json!(std::env::temp_dir());
    assert!(Copilot.request(&host).is_none());
    host["toolArgs"] = json!({"command":"pwd","description":"Print folder","detach":"yes"});
    assert!(Copilot.request(&host).is_none());
}

#[test]
fn copilot_file_search_and_mcp_limits_stay_explicit() {
    let path = std::env::temp_dir().join("copilot-fixture.txt");
    let host = envelope("create", json!({"path":path,"file_text":"fixture"}));
    assert_eq!(
        Copilot.request(&host).unwrap().operation.unwrap()["action"],
        "write"
    );
    let host = envelope("view", json!({"path":"relative.txt"}));
    assert!(Copilot.request(&host).is_none());
    for pattern in ["../secret", "{..,safe}/secret", "@(..)/secret"] {
        let host = envelope("glob", json!({"pattern":pattern}));
        assert!(Copilot.request(&host).is_none());
    }
    let host = envelope("grep", json!({"pattern":"fixture","paths":["src","docs"]}));
    assert_eq!(
        Copilot.request(&host).unwrap().operation.unwrap()["operations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for tool in [
        "acme-fixture_echo",
        "prod-db-read",
        "shield-unknown",
        "apply_patch",
        "web_fetch",
        "write_bash",
        "write_powershell",
        "future_effectful_tool",
    ] {
        assert!(Copilot.request(&envelope(tool, json!({}))).is_none());
    }
    let host = envelope("shield-status", json!({}));
    assert_eq!(Copilot.request(&host).unwrap().shield_tool, Some("status"));
}

#[test]
fn copilot_decisions_match_top_level_native_wire_contract() {
    for (decision, expected) in [
        (Decision::Allow, "allow"),
        (Decision::Ask, "ask"),
        (Decision::Deny, "deny"),
    ] {
        let reply = Copilot.reply(
            Event::Before,
            &Reply {
                decision,
                reason: "Fixture reason".into(),
                agent_context: Some("Fixture guidance".into()),
            },
        );
        assert_eq!(reply["permissionDecision"], expected);
        assert_eq!(
            reply["permissionDecisionReason"],
            "Fixture reason\nFixture guidance"
        );
        assert!(reply.get("hookSpecificOutput").is_none());
    }
    assert_eq!(
        Copilot.completion("postToolUse", Some("Fixture note")),
        json!({"additionalContext":"Fixture note"})
    );
}

#[test]
fn copilot_hook_codec_preserves_unrelated_callbacks_and_requires_exact_ownership() {
    let group = Copilot
        .direct_hook_group(Path::new("ignored"), Path::new("ignored"), "preToolUse")
        .unwrap();
    assert_eq!(group["exec"], "shield");
    assert_eq!(group["args"][3], ".");
    let desired = BTreeMap::from([("preToolUse".into(), group.clone())]);
    let source = json!({"version":1,"hooks":{"preToolUse":[{"type":"command","command":"unrelated-check"}],"sessionStart":[{"type":"command","command":"unrelated-start"}]},"custom":"retained"}).to_string();
    let installed = Copilot
        .merge_hooks(Some(&source), &BTreeMap::new(), &desired)
        .unwrap();
    assert_eq!(
        Copilot.capture_hooks(Some(&installed), &desired).unwrap(),
        desired
    );
    let removed = Copilot
        .merge_hooks(Some(&installed), &desired, &BTreeMap::new())
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&removed).unwrap(),
        serde_json::from_str::<Value>(&source).unwrap()
    );
    let duplicated = json!({"version":1,"hooks":{"preToolUse":[group.clone(), group]}}).to_string();
    assert!(Copilot.capture_hooks(Some(&duplicated), &desired).is_err());
    let repaired = Copilot.repair_hooks(Some(&duplicated), &desired).unwrap();
    assert_eq!(
        Copilot.capture_hooks(Some(&repaired), &desired).unwrap(),
        desired
    );
    assert!(
        Copilot
            .merge_hooks(Some("{\"version\":2}"), &BTreeMap::new(), &desired)
            .is_err()
    );
}

#[test]
fn copilot_setup_refuses_imported_claude_hooks_including_pending_publication() {
    let root = assert_fs::TempDir::new().unwrap();
    let source = json!({"hooks":{"PreToolUse":[{"matcher":"*","hooks":[{"type":"command","command":"shield __internal native-hook --project . --agent claude --event PreToolUse"}]}]}}).to_string();
    let pending =
        |name: &str| (name == ".claude/settings.local.json").then(|| Some(source.clone()));
    let error = Copilot
        .check_native_setup(root.path(), &pending)
        .err()
        .unwrap();
    assert!(error.to_string().contains("separate trusted Copilot"));
    std::fs::create_dir(root.path().join(".claude")).unwrap();
    std::fs::write(root.path().join(".claude/settings.local.json"), &source).unwrap();
    assert!(Copilot.check_native_setup(root.path(), &|_| None).is_err());
    // Explicitly planned removal wins over the old bytes on disk, without
    // rewriting the source or reading the developer's user settings.
    assert!(
        setup::check_project(root.path(), &|name| (name == ".claude/settings.local.json")
            .then_some(None))
        .is_ok()
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join(".claude/settings.local.json")).unwrap(),
        source
    );
}
