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
fn codex_mcp_permission_requests_follow_bound_policy_and_leave_external_servers_to_host() {
    let root = assert_fs::TempDir::new().unwrap();
    let config = json!({"schema_version":1,"packs":[],"rules":"rules.json","mcp":{"cloud":{
        "transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"codex":"acme"}}}}).to_string();
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[
        {"id":"docs","decision":"allow","conditions":{"domain":"mcp","server":"cloud","tool":"docs_search"}},
        {"id":"review","decision":"ask","conditions":{"domain":"mcp","server":"cloud","tool":"review"}},
        {"id":"delete","decision":"deny","conditions":{"domain":"mcp","server":"cloud","tool":"delete"}}
    ]}).to_string();
    let context = review::mcp::initialize_fixture(
        root.path(),
        &Bundle::capture(&config, &[], Some(&rules)).unwrap(),
    );
    let project = Path::new(&context.project_root);
    review::operator::integrations::configure_fixture(
        project,
        crate::cli::public::IntegrationChoice::all(),
        review::integrations::AskFallback::Block,
    );
    for (alias, name, expected) in [
        ("acme", "docs_search", Some("allow")),
        ("acme", "review", None),
        ("acme", "delete", Some("deny")),
        ("external", "docs_search", None),
    ] {
        let host = json!({"hook_event_name":"PermissionRequest","session_id":"acme-session",
            "cwd":project,"tool_name":format!("mcp__{alias}__{name}"),"tool_input":{"query":"rename"}});
        let response = translate_for(
            project,
            "codex",
            Event::Permission,
            &serde_json::to_vec(&host).unwrap(),
        );
        if let Some(expected) = expected {
            assert_eq!(
                response["hookSpecificOutput"]["hookEventName"],
                "PermissionRequest"
            );
            assert_eq!(
                response["hookSpecificOutput"]["decision"]["behavior"], expected,
                "{response}"
            );
        } else {
            assert_eq!(response, json!({}), "{alias}: {name}");
        }
    }
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
            log.flush();
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
    // Invalid policy files leave no policy to check the bindings against.
    let path = root.path().join(".shield/config.yml");
    let original = fs::read(&path).unwrap();
    fs::write(&path, "schema_version: 1\npacks: [\n").unwrap();
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
fn mcp_completions_use_retained_call_identity_after_policy_and_binding_changes() {
    let (_root, context) = snapshot_fixture();
    let project = Path::new(&context.project_root);
    let name = "mcp__acme_snapshot__inspect";
    let arguments = json!({"query":"acme-managed-input"});
    for agent in ["claude", "codex"] {
        assert_permitted(&tool(project, agent, name, &arguments), agent);
    }
    fs::write(
        project.join(".shield/config.yml"),
        "schema_version: 1\npacks: [\n",
    )
    .unwrap();
    fs::write(project.join(".mcp.json"), "{").unwrap();
    let log = std::sync::Arc::new(crate::diagnostics::Log::create(&project.join("logs")).unwrap());
    let trace = log.request("hook", "mcp-completions", None, None).unwrap();
    let private = "acme-unverified-callback-input";
    {
        let _scope = trace.enter();
        for (agent, event, phase, arguments) in [
            ("claude", "PostToolUse", Event::Completed, arguments.clone()),
            ("codex", "PostToolUse", Event::Completed, arguments.clone()),
            (
                "claude",
                "PostToolUseFailure",
                Event::Failed,
                json!({"query":private}),
            ),
        ] {
            let host = native_host(project, agent, event, name, &arguments);
            assert_eq!(
                translate_for(project, agent, phase, &serde_json::to_vec(&host).unwrap()),
                json!({}),
                "{agent}: {event}"
            );
        }
    }
    assert_eq!(trace.recorded_count("integration.rebind_attempts"), 0);
    assert_eq!(trace.recorded_count("policy.preparations"), 0);
    assert_eq!(trace.recorded_count("history.appended_records"), 3);
    trace.finish("success");
    assert!(!journal(project).contains(private));
    let history = review::history_events(project).unwrap();
    let events = history["events"].as_array().unwrap();
    for agent in ["claude", "codex"] {
        let event = |name| {
            events
                .iter()
                .find(|event| {
                    event["provenance"]["agent"] == agent
                        && event["provenance"]["host_event_name"] == name
                })
                .unwrap()
        };
        let decision = event("PreToolUse");
        let completed = event("PostToolUse");
        assert_eq!(
            completed["links"]["decision_event_id"],
            decision["event_id"]
        );
        assert_eq!(completed["details"]["operation"]["arguments"], arguments);
    }
    let failed = events
        .iter()
        .find(|event| event["provenance"]["host_event_name"] == "PostToolUseFailure")
        .unwrap();
    assert!(failed["links"]["decision_event_id"].is_null());
    assert_eq!(failed["details"]["operation"]["arguments"], "[REDACTED]");
}

#[test]
fn external_native_mcp_completion_retains_only_unlinked_evidence() {
    let (root, _) = fixture();
    let name = "mcp__external__inspect";
    let arguments = json!({"query":"acme-private-callback-input"});
    assert_eq!(tool(root.path(), "claude", name, &arguments), json!({}));
    let host = native_host(root.path(), "claude", "PostToolUse", name, &arguments);
    assert_eq!(
        translate_for(
            root.path(),
            "claude",
            Event::Completed,
            &serde_json::to_vec(&host).unwrap()
        ),
        json!({})
    );
    assert!(!journal(root.path()).contains("acme-private-callback-input"));
    let history = review::history_events(root.path()).unwrap();
    let events = history["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{history}");
    assert_eq!(events[0]["kind"], "completion");
    assert!(events[0]["links"]["decision_event_id"].is_null());
    assert!(events[0]["links"]["request_id"].is_null());
    assert_eq!(
        events[0]["details"]["operation"],
        json!({"kind":"mcp","alias":"external","tool":"inspect","arguments":"[REDACTED]"})
    );
}

#[test]
fn mcp_completion_does_not_open_policy_to_initialize_missing_history() {
    let (_source, context) = snapshot_fixture();
    let root = assert_fs::TempDir::new().unwrap();
    let source = Path::new(&context.project_root).join(".shield");
    let shield = root.path().join(".shield");
    fs::create_dir(&shield).unwrap();
    for name in ["config.yml", "rules.json"] {
        fs::copy(source.join(name), shield.join(name)).unwrap();
    }
    let host = native_host(
        root.path(),
        "codex",
        "PostToolUse",
        "mcp__acme_snapshot__inspect",
        &json!({}),
    );
    assert_eq!(
        translate_for(
            root.path(),
            "codex",
            Event::Completed,
            &serde_json::to_vec(&host).unwrap()
        ),
        json!({"systemMessage":"Shield could not record the tool completion in history."})
    );
    assert!(!shield.join("state").exists());
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
fn mcp_sql_arguments_are_redacted_from_preview_and_retained_evaluation() {
    let root = assert_fs::TempDir::new().unwrap();
    let config = json!({"schema_version":1,"packs":[],"rules":"rules.json","mcp":{"cloud":{
        "transport":{"type":"http","url":"https://example.com/mcp"},"agents":{"codex":"acme"},
        "command_fields":{"execute_sql":{"executable":"acme","pointer":"/query","encoding":"argv-v1"}}}}}).to_string();
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[
        {"id":"sql-tool","decision":"allow","conditions":{"domain":"mcp","server":"cloud","tool":"execute_sql"}},
        {"id":"mapped-command","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}
    ]}).to_string();
    let context = review::mcp::initialize_fixture(
        root.path(),
        &Bundle::capture(&config, &[], Some(&rules)).unwrap(),
    );
    let project = Path::new(&context.project_root);
    review::operator::integrations::configure_fixture(
        project,
        crate::cli::public::IntegrationChoice::all(),
        review::integrations::AskFallback::Block,
    );
    let marker = "fixture-private-sql-argument";
    let args = json!({"query":["acme",marker]});
    assert_permitted(
        &tool(project, "codex", "mcp__acme__execute_sql", &args),
        "codex",
    );
    let stored = journal(project);
    assert!(
        stored.contains("mapped_commands") && stored.contains("mapped-command"),
        "{stored}"
    );
    assert!(!stored.contains(marker), "{stored}");

    let preview = review::mcp::call(
        &context,
        &review::mcp::Connection::new("preview-sql".into()),
        "policy_evaluate",
        &json!({"input":{"kind":"mcp","server":"cloud","tool":"execute_sql","arguments":args,"original_input":args},
            "context":{"cwd":project,"environment":null}}),
        &|| Ok(()),
    );
    assert_eq!(preview["data"]["decision"], "allow", "{preview}");
    assert_eq!(
        preview["data"]["explanation"]["mapped_commands"][0]["arguments"], "[REDACTED]",
        "{preview}"
    );
    assert!(!preview.to_string().contains(marker), "{preview}");
}

#[test]
fn mcp_connections_native_calls_use_stable_identity_full_arguments_and_drift_checks() {
    let root = assert_fs::TempDir::new().unwrap();
    let config=json!({"schema_version":1,"packs":[],"rules":"rules.json","mcp":{"project-cloud":{
        "transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"claude":"alias","codex":"alias"}}}}).to_string();
    let rules=json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","requirements-v1","mcp-connection-facts-v1"],"rules":[
        {"id":"inspect","decision":"allow","conditions":{"domain":"mcp","server":"project-cloud","tool":"inspect"},
            "requirements":{"fields":{"/payload/name":{"text":"Acme Ω \"quotes\""}},"connection":{"/id":{"exact":"project-cloud"},"/transport/url":{"exact":"https://cloud.example.com/mcp"}}},"on_unmet":{"decision":"deny"}},
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
