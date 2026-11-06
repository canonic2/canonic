use super::*;

fn input() -> Value {
    json!({"conversationId":"acme-session","workspacePaths":[std::env::temp_dir()],"stepIdx":2,
        "toolCall":{"name":"read_file","args":{"Path":std::env::temp_dir().join("acme.txt")}}})
}

#[test]
fn antigravity_observed_native_read_and_mcp_envelopes_preserve_identity() {
    let metadata = json!({"antigravity.google/conversation_id":"acme-session"});
    assert_eq!(
        Antigravity.mcp_session(metadata.as_object().unwrap()),
        Some("acme-session")
    );
    assert!(
        Antigravity
            .mcp_session(json!({"session_id":"invented"}).as_object().unwrap())
            .is_none()
    );
    let host = input();
    let request = Antigravity.request(&host).unwrap();
    assert_eq!(request.operation.as_ref().unwrap()["action"], "read");
    assert_eq!(
        request.operation.as_ref().unwrap()["path"],
        host["toolCall"]["args"]["Path"]
    );
    assert!(request.tool_id.is_none());
    let mut host = host;
    host["toolCall"] = json!({"name":"call_mcp_tool","args":{"ServerName":"acme_tools","ToolName":"read_file","Arguments":{"path":"acme.txt"}}});
    let operation = Antigravity.request(&host).unwrap().operation.unwrap();
    assert_eq!(
        operation,
        json!({"kind":"mcp","alias":"acme_tools","tool":"read_file","arguments":{"path":"acme.txt"}})
    );
    host["toolCall"]["args"]["ServerName"] = json!("shield");
    host["toolCall"]["args"]["ToolName"] = json!("policy_show");
    let request = Antigravity.request(&host).unwrap();
    assert_eq!(request.shield_tool, Some("policy_show"));
    assert_eq!(request.input, &host["toolCall"]["args"]["Arguments"]);
    for invalid in [json!("../acme"), json!(""), json!(false)] {
        host["toolCall"]["args"]["ServerName"] = invalid;
        assert!(Antigravity.request(&host).is_none());
    }
}

#[test]
fn antigravity_pinned_events_and_malformed_inputs_do_not_invent_context() {
    let host = input();
    assert!(
        Antigravity
            .request_for_event(&host, Event::Completed)
            .unwrap()
            .event
            == Event::Completed
    );
    let mut failed = host.clone();
    failed["error"] = json!("exit status 1");
    assert!(
        Antigravity
            .request_for_event(&failed, Event::Completed)
            .unwrap()
            .event
            == Event::Failed
    );
    for (field, value) in [
        ("conversationId", json!("")),
        ("workspacePaths", json!([])),
        ("workspacePaths", json!(["relative"])),
        ("stepIdx", json!(-1)),
        ("hook_event_name", json!("PreToolUse")),
    ] {
        let mut invalid = host.clone();
        invalid[field] = value;
        assert!(Antigravity.request(&invalid).is_none(), "{field}");
    }
    let mut relative = host;
    relative["toolCall"]["args"]["Path"] = json!("acme.txt");
    assert_eq!(
        Antigravity.request(&relative).unwrap().operation.unwrap()["path"],
        std::env::temp_dir().join("acme.txt").to_str().unwrap()
    );
}

#[test]
fn antigravity_relative_file_paths_need_an_unambiguous_workspace() {
    let mut host = input();
    for tool in ["read_file", "edit_file", "write_file"] {
        host["toolCall"] = json!({"name":tool,"args":{"Path":"acme.txt"}});
        let request = Antigravity.request(&host).unwrap();
        assert!(request.context_complete);
        assert_eq!(request.operation.unwrap()["kind"], "file");
        host["workspacePaths"] = json!([std::env::temp_dir(), std::env::temp_dir().join("other")]);
        host["toolCall"]["args"]["Cwd"] = json!(std::env::temp_dir());
        let request = Antigravity.request(&host).unwrap();
        assert!(!request.context_complete);
        assert_eq!(request.operation.unwrap()["unmodeled"], true);
        host["toolCall"]["args"]["Path"] = json!(std::env::temp_dir().join("acme.txt"));
        let request = Antigravity.request(&host).unwrap();
        assert!(request.context_complete);
        assert_eq!(request.operation.unwrap()["kind"], "file");
        host["workspacePaths"] = json!([std::env::temp_dir()]);
    }
}

#[test]
fn antigravity_maps_documented_shell_files_and_web_without_rewrites() {
    let cwd = std::env::temp_dir();
    assert_eq!(
        operation("run_command", &json!({"CommandLine":"pwd","Cwd":cwd}))
            .unwrap()
            .into_operation()
            .unwrap()["source"],
        "pwd"
    );
    let mut host = input();
    host["toolCall"] = json!({"name":"run_command","args":{"CommandLine":"pwd"}});
    assert!(Antigravity.request(&host).is_some());
    host["workspacePaths"] = json!([cwd, cwd.join("other")]);
    let request = Antigravity.request(&host).unwrap();
    assert!(!request.context_complete);
    assert_eq!(request.operation.unwrap()["unmodeled"], true);
    host["workspacePaths"] = json!([cwd]);
    host["toolCall"]["args"]["RequestedTerminalID"] = json!("acme-terminal");
    let request = Antigravity.request(&host).unwrap();
    assert!(!request.context_complete);
    assert_eq!(request.operation.unwrap()["unmodeled"], true);
    for tool in [
        "write_to_file",
        "replace_file_content",
        "multi_replace_file_content",
    ] {
        assert_eq!(
            operation(tool, &json!({"TargetFile":cwd.join("acme.txt")}))
                .unwrap()
                .into_operation()
                .unwrap()["action"],
            "write"
        );
    }
    assert!(
        operation(
            "find_by_name",
            &json!({"SearchDirectory":cwd,"Pattern":"../**"})
        )
        .is_none()
    );
    assert_eq!(
        operation("read_url_content", &json!({"Url":"https://example.com"}))
            .unwrap()
            .into_operation()
            .unwrap()["action"],
        "fetch"
    );
    assert!(operation("mcp_acme_read", &json!({})).is_none());
    assert!(
        operation("ask_question", &json!({}))
            .unwrap()
            .into_operation()
            .is_none()
    );
}

#[test]
fn antigravity_decisions_force_fresh_prompts_and_post_hooks_return_empty() {
    for (decision, wire) in [
        (Decision::Allow, "auto_approve"),
        (Decision::Ask, "force_ask"),
        (Decision::Deny, "deny"),
    ] {
        let value = Antigravity.reply(
            Event::Before,
            &Reply {
                decision,
                reason: "Shield: Acme decision.".into(),
                agent_context: Some("Read the skill folder and retry.".into()),
            },
        );
        assert_eq!(value["decision"], wire);
        assert!(value["reason"].as_str().unwrap().contains("skill folder"));
        assert!(value.get("overwrite").is_none());
        assert!(value.get("permissionOverrides").is_none());
    }
    assert_eq!(
        Antigravity.completion("PostToolUse", Some("Acme")),
        json!({})
    );
}

#[test]
fn antigravity_named_hook_and_json_mcp_ownership_preserve_user_configuration() {
    let empty = super::super::setup::HookGroups::new();
    let groups = [(
        "PreToolUse".into(),
        Antigravity.hook_group("shield __internal native-hook --agent agy"),
    )]
    .into_iter()
    .collect();
    let original =
        json!({"acme-lint":{"PostToolUse":[{"matcher":"*","hooks":[{"command":"acme-lint"}]}]}})
            .to_string();
    let merged = Antigravity
        .merge_hooks(Some(&original), &empty, &groups)
        .unwrap();
    assert_eq!(
        Antigravity.capture_hooks(Some(&merged), &groups).unwrap(),
        groups
    );
    let removed = Antigravity
        .merge_hooks(Some(&merged), &groups, &empty)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&removed).unwrap(),
        serde_json::from_str::<Value>(&original).unwrap()
    );
    let edited = merged.replace("\"enabled\": true", "\"enabled\": false");
    assert!(
        Antigravity
            .merge_hooks(Some(&edited), &groups, &empty)
            .is_err()
    );
    let source = json!({"mcpServers":{"acme":{"command":"acme-mcp"}},"theme":"acme"}).to_string();
    let entry = json!({"command":"shield","args":["mcp","serve"]}).to_string();
    let merged = Antigravity
        .merge_mcp(Some(&source), None, Some(&entry))
        .unwrap();
    assert!(Antigravity.has_mcp_entry(&merged).unwrap());
    assert!(
        Antigravity
            .merge_mcp(Some(&merged), None, Some(&entry))
            .is_err()
    );
    assert!(
        Antigravity
            .merge_mcp(Some(&merged), Some("{}"), None)
            .is_err()
    );
    let removed = Antigravity
        .merge_mcp(Some(&merged), Some(&entry), None)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&removed).unwrap(),
        serde_json::from_str::<Value>(&source).unwrap()
    );
}
