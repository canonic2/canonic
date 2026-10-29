use super::*;

fn snapshot_fixture() -> (assert_fs::TempDir, review::Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let config=json!({"schema_version":1,"packs":[],"rules":"rules.json","mcp":{"cloud":{
        "transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"claude":"acme_snapshot","codex":"acme_snapshot"}}}}).to_string();
    let rules=json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[
        {"id":"inspect","decision":"allow","conditions":{"domain":"mcp","server":"cloud","tool":"inspect"}}
    ]}).to_string();
    let context = review::mcp::initialize_fixture(
        root.path(),
        &Bundle::capture(&config, &[], Some(&rules)).unwrap(),
    );
    review::operator::integrations::configure_fixture(
        Path::new(&context.project_root),
        crate::cli::public::IntegrationChoice::all(),
        review::integrations::AskFallback::Block,
    );
    (root, context)
}

#[test]
fn mcp_connections_classification_and_enforcement_share_one_snapshot_load() {
    use clap::Parser;
    let (root, context) = snapshot_fixture();
    for agent in ["claude", "codex"] {
        for (alias, bound) in [("acme_snapshot", true), ("external_snapshot", false)] {
            let logs = root.path().join(format!("{agent}-{alias}-logs"));
            let cli = crate::cli::Cli::parse_from([
                "shield",
                "--debug-log-dir",
                logs.to_str().unwrap(),
                "status",
            ]);
            let log = crate::diagnostics::Log::open(&cli);
            let host = json!({"hook_event_name":"PreToolUse","tool_name":format!("mcp__{alias}__inspect"),"tool_use_id":"snapshot-tool","session_id":"snapshot-session","cwd":context.project_root,"tool_input":{}});
            let response = translate_logged(
                Path::new(&context.project_root),
                agent,
                &serde_json::to_vec(&host).unwrap(),
                &log,
            );
            if bound {
                assert_permitted(&response, agent);
            } else {
                assert_eq!(response, json!({}));
            }
            let records = fs::read_to_string(log.path().unwrap())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>();
            for stage in ["snapshot_load_start", "snapshot_load_ready"] {
                assert_eq!(
                    records
                        .iter()
                        .filter(
                            |row| row["event"] == "hook_stage" && row["fields"]["stage"] == stage
                        )
                        .count(),
                    1,
                    "{records:?}"
                );
            }
        }
    }
}

#[test]
fn mcp_connections_unknown_bindings_fail_closed_when_snapshot_is_unavailable() {
    let (root, context) = snapshot_fixture();
    let project = Path::new(&context.project_root);
    let path = root.path().join(".shield/review/state.json");
    let original = fs::read(&path).unwrap();
    let mut state: Value = serde_json::from_slice(&original).unwrap();
    state["active"] = Value::Null;
    fs::write(&path, state.to_string()).unwrap();
    assert!(review::open(project).is_ok());
    assert!(Project::open(project).unwrap().snapshot().is_err());
    for agent in ["claude", "codex"] {
        for alias in ["acme_snapshot", "external_snapshot"] {
            let response = tool(
                project,
                agent,
                &format!("mcp__{alias}__inspect"),
                &json!({}),
            );
            assert_eq!(
                response["hookSpecificOutput"]["permissionDecision"], "deny",
                "{response}"
            );
            assert!(
                response
                    .to_string()
                    .contains("MCP bindings could not be checked"),
                "{response}"
            );
            let completion = translate_for(
                project,
                agent,
                Event::Completed,
                &serde_json::to_vec(&json!({"hook_event_name":"PostToolUse","tool_name":format!("mcp__{alias}__inspect"),
                    "tool_use_id":"acme-tool","session_id":"acme-session","cwd":project,"tool_input":{}}))
                .unwrap(),
            );
            assert_eq!(
                completion,
                json!({"systemMessage":"Shield completion history unavailable; MCP bindings could not be checked."}),
                "a completion cannot be denied, so it reports the gap instead"
            );
        }
    }
    fs::write(&path, original).unwrap();
    for agent in ["claude", "codex"] {
        assert_eq!(
            tool(
                project,
                agent,
                "mcp__external_snapshot__inspect",
                &json!({})
            ),
            json!({})
        );
    }
}

#[test]
fn mcp_connections_retained_snapshot_still_requires_current_activation_at_handoff() {
    let (root, context) = snapshot_fixture();
    let project = Path::new(&context.project_root);
    let prepared = protocol::Prepared::load(project, &|_| {}).unwrap();
    let path = root.path().join(".shield/review/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["active"] = Value::Null;
    fs::write(&path, state.to_string()).unwrap();
    let input = json!({"protocol_version":1,"request_id":"snapshot-reuse","expected_project_id":context.project_id,"cwd":project,
        "operation":{"kind":"mcp","alias":"acme_snapshot","tool":"inspect","arguments":{}},"host":null,"approval_request_id":null});
    let (receipt, exit) = protocol::evaluate_native(
        project,
        &serde_json::to_vec(&input).unwrap(),
        &json!({}),
        protocol::NativeContext {
            integration: "claude",
            approval: protocol::NativeApproval::Prompt,
            context_complete: true,
            working_files: None,
        },
        Some(prepared),
        &|_| {},
    );
    assert_eq!(exit, 2, "{receipt}");
    assert_eq!(receipt["code"], "handoff-failed", "{receipt}");
    assert_eq!(receipt["decision"], "deny");
}

#[test]
fn mcp_connections_disabled_bindings_do_not_become_host_allowances() {
    let root = assert_fs::TempDir::new().unwrap();
    let config=json!({"schema_version":1,"packs":[],"mcp":{"cloud":{"enabled":false,"transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"claude":"acme","codex":"acme"}}}}).to_string();
    review::mcp::initialize_fixture(root.path(), &Bundle::capture(&config, &[], None).unwrap());
    for agent in ["claude", "codex"] {
        assert_eq!(
            tool(root.path(), agent, "mcp__acme__inspect", &json!({}))["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        assert_eq!(
            tool(root.path(), agent, "mcp__outside__inspect", &json!({})),
            json!({})
        );
    }
}

#[test]
fn mcp_connections_native_calls_use_stable_identity_full_arguments_and_drift_checks() {
    let root = assert_fs::TempDir::new().unwrap();
    let config=json!({"schema_version":1,"packs":[],"rules":"rules.json","mcp":{"project-cloud":{
        "transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"claude":"alias","codex":"alias"}}}}).to_string();
    let rules=json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","requirements-v1","mcp-connection-facts-v1"],"rules":[
        {"id":"inspect","decision":"allow","conditions":{"domain":"mcp","server":"project-cloud","tool":"inspect"},
            "requirements":{"fields":{"/payload/name":{"text":"Acme Ω \"quotes\""}},"connection":{"/transport/url":{"exact":"https://cloud.example.com/mcp"}}},"on_unmet":{"decision":"deny"}},
        {"id":"delete","decision":"deny","conditions":{"domain":"mcp","server":"project-cloud","tool":"delete"}},
        {"id":"review","decision":"ask","conditions":{"domain":"mcp","server":"project-cloud","tool":"review"}}
    ]}).to_string();
    let bundle = Bundle::capture(&config, &[], Some(&rules)).unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    review::operator::integrations::configure_fixture(
        root.path(),
        crate::cli::public::IntegrationChoice::all(),
        review::integrations::AskFallback::Block,
    );
    let project = Path::new(&context.project_root);
    let arguments =
        json!({"payload":{"name":"Acme Ω \"quotes\"","nested":[null,true,{"key":"value"}]}});
    for agent in ["claude", "codex"] {
        assert_permitted(
            &tool(project, agent, "mcp__alias__inspect", &arguments),
            agent,
        );
        assert_eq!(
            tool(project, agent, "mcp__alias__delete", &arguments)["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        assert_eq!(
            tool(project, agent, "mcp__alias__inspect", &json!({}))["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        assert_eq!(
            tool(project, agent, "mcp__alias__review", &arguments)["hookSpecificOutput"]["permissionDecision"],
            if agent == "claude" { "ask" } else { "deny" }
        );
        assert_eq!(
            tool(project, agent, "mcp__unbound__inspect", &arguments),
            json!({})
        );
    }
    let path = project.join(".mcp.json");
    let original = fs::read_to_string(&path).unwrap();
    let mut ambiguous: Value = serde_json::from_str(&original).unwrap();
    ambiguous["mcpServers"]["alias__shadow"] =
        json!({"type":"http","url":"https://other.example.com/mcp"});
    fs::write(&path, ambiguous.to_string()).unwrap();
    assert_eq!(
        tool(project, "claude", "mcp__alias__inspect", &arguments)["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    fs::write(&path, &original).unwrap();
    let changed = fs::read_to_string(&path)
        .unwrap()
        .replace("cloud.example.com", "other.example.com");
    fs::write(&path, changed).unwrap();
    assert_eq!(
        tool(project, "claude", "mcp__alias__inspect", &arguments)["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
}
