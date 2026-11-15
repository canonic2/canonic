//! An omitted adapter's damaged configuration cannot block another adapter's review.
use super::*;

fn state(project: &Project) -> Value {
    serde_json::from_slice(
        &std::fs::read(project.root.path().join(".shield/state/state.json")).unwrap(),
    )
    .unwrap()
}

fn omitted(project: &Project) -> Value {
    let state = state(project);
    let registered = &state["operator"]["integration"];
    json!({"agent":registered["agents"]["codex"],
        "connection":registered["connections"]["codex"],
        "fallback":registered["ask_fallback"]["codex"],
        "hooks":std::fs::read(project.root.path().join(".codex/hooks.json")).unwrap(),
        "config":std::fs::read(project.root.path().join(".codex/config.toml")).unwrap()})
}

fn answer(transport: &Transport, form: &Value, action: &str) -> Value {
    assert_eq!(form["method"], "elicitation/create", "{form}");
    transport.request(&json!({"jsonrpc":"2.0","id":form["id"],
        "result":{"action":action,"content":{}}}))
}

fn installed() -> (Transport, Project) {
    let (server, project) = fixture(Some("claude"));
    let transport = Transport::start(server);
    for message in initialize(&json!({"elicitation":{"form":{}}}), "claude") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    let form = transport.call(json!(2), "mcp_setup", &json!({"agents":["claude","codex"]}));
    let installed = answer(&transport, &form, "accept");
    assert_eq!(installed["result"]["isError"], false, "{installed}");
    (transport, project)
}

#[test]
fn explicit_project_repair_uses_the_published_schema_and_original_connection_confirmation() {
    let (transport, source) = installed();
    let (_server, target) = fixture(Some("claude"));
    let source_before = state(&source);
    let target_before = state(&target);
    let path = target.root.path().join(".mcp.json");
    let host_before = std::fs::read(&path).ok();
    let args = json!({"project":target.root.path(),"agents":["claude"],"repair":true});
    let mut preview = args.clone();
    preview["dry_run"] = json!(true);
    let response = transport.call(json!(20), "mcp_setup", &preview);
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(state(&target), target_before);
    assert_eq!(std::fs::read(&path).ok(), host_before);
    let form = transport.call(json!(21), "mcp_setup", &args);
    let declined = answer(&transport, &form, "decline");
    assert_eq!(declined["result"]["isError"], true, "{declined}");
    assert_eq!(state(&target), target_before);
    assert_eq!(std::fs::read(&path).ok(), host_before);
    let form = transport.call(json!(22), "mcp_setup", &args);
    let applied = answer(&transport, &form, "accept");
    assert_eq!(applied["result"]["isError"], false, "{applied}");
    let entry: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(entry["mcpServers"]["shield"]["command"], "shield");
    assert_eq!(entry["mcpServers"]["shield"]["args"][3], ".");
    assert_eq!(state(&source), source_before);
    let status = transport.call(json!(23), "status", &json!({}));
    assert_eq!(
        status["result"]["structuredContent"]["project_id"],
        source.review.project_id
    );
    transport.close().1.unwrap();
}

fn pending(project: &Project) -> String {
    use crate::review::hook_approval::{Identity, Key, Outcome, Summary, refused};
    let active = &crate::review::runtime::current_identity(&project.review);
    let identity = Identity {
        activation_id: active["activation_id"].as_str().unwrap(),
        policy_revision: active["policy_revision"].as_str().unwrap(),
        candidate_digest: active["candidate_digest"].as_str().unwrap(),
    };
    let key = Key {
        agent: "claude",
        session: "acme-session",
        cwd: &project.review.project_root,
        tool: "Bash",
        input_digest: crate::policy::fingerprint(b"acme selected action"),
    };
    let summary = Summary {
        action: "Run acme selected action".into(),
        reason: "the policy asks the operator".into(),
        overrides_deny: false,
        deny_rules: Vec::new(),
        settings_change: false,
        policy_change: false,
    };
    match refused(project.root.path(), &key, &identity, &summary).unwrap() {
        Outcome::Pending(id, _) => id,
        Outcome::Allowed => panic!("request must wait for actual human approval"),
    }
}

#[test]
fn claude_setup_and_request_review_ignore_omitted_codex_damage_on_one_connection() {
    for malformed in [false, true] {
        let (transport, project) = installed();
        let request_id = pending(&project);
        for path in [".codex/hooks.json", ".codex/config.toml"] {
            let path = project.root.path().join(path);
            let original = std::fs::read_to_string(&path).unwrap();
            let damaged = if malformed {
                "{ acme malformed host configuration".into()
            } else {
                original.replace("shield", "acme-stale-shield")
            };
            assert_ne!(damaged, original);
            std::fs::write(path, damaged).unwrap();
        }
        let before = omitted(&project);
        let checkpoint = state(&project);
        let args = json!({"agents":["claude"]});
        let form = transport.call(json!(3), "mcp_setup", &args);
        let declined = answer(&transport, &form, "decline");
        assert_eq!(declined["result"]["isError"], true, "{declined}");
        assert_eq!(state(&project), checkpoint);
        assert_eq!(omitted(&project), before);
        let form = transport.call(json!(4), "mcp_setup", &args);
        let applied = answer(&transport, &form, "accept");
        assert_eq!(applied["result"]["isError"], false, "{applied}");
        assert_eq!(omitted(&project), before);
        for (id, action) in [(5, "decline"), (6, "accept")] {
            let form = transport.call(
                json!(id),
                "request_approve",
                &json!({"request_id":request_id}),
            );
            let result = answer(&transport, &form, action);
            assert_eq!(result["result"]["isError"], action == "decline", "{result}");
            assert_eq!(omitted(&project), before);
        }
        let status = transport.call(json!(7), "status", &json!({}));
        assert_eq!(status["result"]["isError"], false, "{status}");
        transport.close().1.unwrap();
    }
}

#[test]
fn selected_malformed_claude_configuration_refuses_without_changing_any_adapter() {
    let (transport, project) = installed();
    let path = project.root.path().join(".claude/settings.local.json");
    std::fs::write(&path, "{ acme malformed selected host configuration").unwrap();
    let checkpoint = state(&project);
    let before = omitted(&project);
    let response = transport.call(json!(3), "mcp_setup", &json!({"agents":["claude"]}));
    assert!(response["method"].is_null(), "{response}");
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(state(&project), checkpoint);
    assert_eq!(omitted(&project), before);
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "{ acme malformed selected host configuration"
    );
    transport.close().1.unwrap();
}

#[test]
fn pending_codex_setup_cannot_write_omitted_files_or_block_claude_human_approval() {
    let (transport, project) = installed();
    let request_id = pending(&project);
    crate::review::mcp::interrupted_setup_fixture(&project.review, "codex");
    for path in [".codex/hooks.json", ".codex/config.toml"] {
        std::fs::write(
            project.root.path().join(path),
            "{ acme broken pending Codex file",
        )
        .unwrap();
    }
    let before = omitted(&project);
    let checkpoint = state(&project);
    let status = transport.call(json!(3), "status", &json!({}));
    assert_eq!(status["result"]["isError"], false, "{status}");
    assert_eq!(state(&project), checkpoint);
    assert_eq!(omitted(&project), before);
    for (id, action) in [(4, "decline"), (5, "accept")] {
        let form = transport.call(
            json!(id),
            "request_approve",
            &json!({"request_id":request_id}),
        );
        let result = answer(&transport, &form, action);
        assert_eq!(result["result"]["isError"], action == "decline", "{result}");
        assert_eq!(omitted(&project), before);
        assert_eq!(
            state(&project)["operator"]["integration_pending"],
            checkpoint["operator"]["integration_pending"]
        );
    }
    transport.close().1.unwrap();
}
