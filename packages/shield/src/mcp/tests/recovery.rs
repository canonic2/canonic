//! Recovery starts at a native refusal and completes through the same MCP session.
use super::*;

fn native(project: &Project, agent: &str, tool: &str, input: Value) -> Value {
    native_at(project.root.path(), project, agent, tool, input)
}

fn native_at(
    root: &std::path::Path,
    project: &Project,
    agent: &str,
    tool: &str,
    mut input: Value,
) -> Value {
    if agent == "claude" && tool == "Bash" {
        input.as_object_mut().unwrap().remove("workdir");
    }
    crate::agents::translate_event(
        root,
        crate::agents::find(agent).unwrap(),
        &serde_json::to_vec(&json!({"hook_event_name":"PreToolUse",
            "tool_name":tool,"tool_use_id":"acme-recovery","session_id":"acme-session",
            "cwd":project.review.project_root,"tool_input":input}))
        .unwrap(),
        crate::agents::Event::Before,
        &crate::diagnostics::Log::disabled(),
    )
}

fn installed_hooks_with_handover(agent: &str, modern: bool) -> (Transport, Project) {
    let (server, project) = fixture(Some(agent));
    let transport = Transport::start(server);
    for message in initialize(&json!({"elicitation":{"form":{}}}), agent) {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    pwd_rule(&transport, agent, modern, "allow");
    let args = json!({"agents":[agent]});
    let form = begin(&transport, "mcp_setup", &args, modern);
    let result = answer(&transport, "mcp_setup", &args, &form, "accept", modern);
    assert_eq!(result["result"]["isError"], false, "{result}");
    if !cfg!(windows) {
        // Model a project handover: Box keeps publishing these native
        // files after Shield drops its direct-install ownership.
        std::fs::create_dir_all(project.root.path().join(".box")).unwrap();
        std::fs::write(
            project.root.path().join(".box/config.yml"),
            "schema_version: 1\n",
        )
        .unwrap();
        let args = json!({"agents":["box"]});
        let form = begin(&transport, "mcp_setup", &args, modern);
        let result = answer(&transport, "mcp_setup", &args, &form, "accept", modern);
        assert_eq!(result["result"]["isError"], false, "{result}");
    }
    (transport, project)
}

#[test]
fn recovery_stale_hooks_reach_reviewed_reconciliation_and_native_retry() {
    for agent in ["claude", "codex"] {
        for modern in [false, true] {
            let (transport, project) = installed_hooks_with_handover(agent, modern);
            let args = json!({"agents":[agent]});
            let path = project
                .root
                .path()
                .join(crate::agents::find(agent).unwrap().spec().hooks_path);
            let mut hooks: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            let stale = project.root.path().join("old-task");
            std::fs::create_dir(&stale).unwrap();
            hooks["hooks"]["PreToolUse"].as_array_mut().unwrap().push(json!({
                "matcher":".*","hooks":[
                    {"type":"command","command":format!("shield __internal native-hook --project '{}'", stale.display()),
                        "statusMessage":"Shield project enforcement"},
                    {"type":"command","command":"echo acme hook"}
                ]}));
            let broken = hooks.to_string();
            std::fs::write(&path, &broken).unwrap();
            let adapter = crate::agents::find(agent).unwrap();
            let mcp_path = project
                .root
                .path()
                .join(adapter.spec().mcp_path.unwrap_or(".mcp.json"));
            let source = std::fs::read_to_string(&mcp_path).unwrap();
            if agent == "codex" {
                let mut doc: toml_edit::DocumentMut = source.parse().unwrap();
                doc["mcp_servers"]["acme"]["command"] = toml_edit::value("acme-server");
                std::fs::write(&mcp_path, doc.to_string()).unwrap();
            } else {
                let mut doc: Value = serde_json::from_str(&source).unwrap();
                doc["mcpServers"]["acme"] = json!({"command":"acme-server"});
                std::fs::write(&mcp_path, doc.to_string()).unwrap();
            }
            let input = json!({"command":"pwd","workdir":project.review.project_root});
            let denied = native_at(&stale, &project, agent, "Bash", input.clone());
            assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
            let recovery = native_at(&stale, &project, agent, "mcp__shield__mcp_setup", json!({}));
            assert_ne!(recovery["hookSpecificOutput"]["permissionDecision"], "deny");
            let conflict = transport.call(json!(8), "mcp_setup", &args);
            assert_eq!(conflict["result"]["isError"], true, "{conflict}");
            let args = conflict["result"]["structuredContent"]["error"]["retry_arguments"].clone();
            assert_eq!(args, json!({"agents":[agent],"repair":true}), "{conflict}");
            for action in ["decline", "accept"] {
                let form = begin(&transport, "mcp_setup", &args, modern);
                assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
                let result = answer(&transport, "mcp_setup", &args, &form, action, modern);
                assert_eq!(result["result"]["isError"], action == "decline", "{result}");
                if action == "decline" {
                    assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
                }
            }
            let repaired: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            let groups = repaired["hooks"]["PreToolUse"].as_array().unwrap();
            let commands = groups
                .iter()
                .flat_map(|group| group["hooks"].as_array().unwrap())
                .filter_map(|entry| entry["command"].as_str())
                .collect::<Vec<_>>();
            assert!(commands.contains(&"echo acme hook"));
            assert!(
                !commands
                    .iter()
                    .any(|command| command.contains(&stale.to_string_lossy().to_string()))
            );
            assert_eq!(groups.len(), 2);
            let source = std::fs::read_to_string(&mcp_path).unwrap();
            let retained = adapter.connection(&source, "acme").unwrap().unwrap();
            assert_eq!(json!(retained)["command"], "acme-server");
            // Ordinary work reaches normal policy again, and protected-state
            // access remains blocked after the confirmed repair.
            let retry = native(&project, agent, "Bash", input);
            assert_ne!(
                retry["hookSpecificOutput"]["permissionDecision"], "deny",
                "{retry}"
            );
            let denied = native(
                &project,
                agent,
                "Bash",
                json!({"command":"cat .shield/review/state.json",
                "workdir":project.review.project_root}),
            );
            assert_eq!(
                denied["hookSpecificOutput"]["permissionDecision"], "deny",
                "{denied}"
            );
            let doctor = transport.call(json!(30), "doctor", &json!({}));
            assert_eq!(doctor["result"]["isError"], false, "{doctor}");
            assert_eq!(
                doctor["result"]["structuredContent"]["data"]["coverage"]["repair_needed"], false,
                "{doctor}"
            );
        }
    }
}

fn begin(transport: &Transport, name: &str, args: &Value, modern: bool) -> Value {
    let form = if modern {
        transport.request(&super::skills::modern(
            20,
            "tools/call",
            json!({"name":name,"arguments":args}),
            json!({"elicitation":{"form":{}}}),
        ))
    } else {
        transport.call(json!(20), name, args)
    };
    if modern {
        assert_eq!(form["result"]["resultType"], "input_required", "{form}");
    } else {
        assert_eq!(form["method"], "elicitation/create", "{form}");
    }
    form
}

fn answer(
    transport: &Transport,
    name: &str,
    args: &Value,
    form: &Value,
    action: &str,
    modern: bool,
) -> Value {
    if modern {
        transport.request(&super::skills::modern(
            21,
            "tools/call",
            json!({"name":name,"arguments":args,"requestState":form["result"]["requestState"],
                "inputResponses":{"confirmation":{"action":action}}}),
            json!({"elicitation":{"form":{}}}),
        ))
    } else {
        transport.request(&json!({"jsonrpc":"2.0","id":form["id"],
            "result":{"action":action,"content":{}}}))
    }
}

fn pwd_rule(transport: &Transport, agent: &str, modern: bool, decision: &str) {
    let proposal = transport.call(
        json!(2),
        "rule_add",
        &json!({"rule":{
        "id":"pwd","decision":decision,"conditions":{"domain":"argv",
            "executable":"pwd","arguments":{"kind":"exact","values":[]}}}}),
    );
    assert_eq!(proposal["result"]["isError"], false, "{proposal}");
    let args = &proposal["result"]["structuredContent"]["data"]["apply_arguments"];
    let applied = if agent == "claude" {
        let form = begin(transport, "policy_apply", args, modern);
        answer(transport, "policy_apply", args, &form, "accept", modern)
    } else {
        transport.call(json!(3), "policy_apply", args)
    };
    assert_eq!(applied["result"]["isError"], false, "{applied}");
}

#[test]
fn recovery_unusable_cache_location_reaches_approved_repair_and_native_retry() {
    for agent in ["codex", "claude"] {
        for modern in [false, true] {
            let (server, project) = fixture(Some(agent));
            let transport = Transport::start(server);
            for message in initialize(&json!({"elicitation":{"form":{}}}), agent) {
                transport.send(&message);
            }
            assert!(transport.receive()["result"].is_object());
            // Establish the ordinary operation through its real proposal/approval path.
            pwd_rule(&transport, agent, modern, "allow");
            let before = project.revision();
            let scratch = assert_fs::TempDir::new().unwrap();
            let old_cache = scratch.path().join("old");
            let args = json!({"cache_dir":old_cache});
            let form = begin(&transport, "cache_prepare", &args, modern);
            assert!(!old_cache.exists());
            let result = answer(&transport, "cache_prepare", &args, &form, "accept", modern);
            assert_eq!(result["result"]["isError"], false, "{result}");
            std::fs::rename(&old_cache, scratch.path().join("retained-cache")).unwrap();
            std::fs::write(&old_cache, "unusable cache root").unwrap();
            let command = json!({"command":"pwd","workdir":project.review.project_root});
            let denied = native(&project, agent, "Bash", command.clone());
            assert_eq!(
                denied["hookSpecificOutput"]["permissionDecision"], "deny",
                "{denied}"
            );
            assert!(denied.to_string().contains("cache_prepare"), "{denied}");
            assert!(!denied.to_string().contains("terminal"), "{denied}");
            for tool in ["doctor", "cache_prepare"] {
                let reply = native(&project, agent, &format!("mcp__shield__{tool}"), json!({}));
                assert_ne!(
                    reply["hookSpecificOutput"]["permissionDecision"], "deny",
                    "{reply}"
                );
            }
            let doctor = transport.call(json!(4), "doctor", &json!({}));
            assert_eq!(doctor["result"]["isError"], false, "{doctor}");
            let rebuilt = transport.call(json!(5), "cache_prepare", &json!({}));
            assert_eq!(rebuilt["result"]["isError"], true, "{rebuilt}");
            let new_cache = scratch.path().join("recovered");
            let args = json!({"cache_dir":new_cache});
            for action in ["decline", "accept"] {
                let form = begin(&transport, "cache_prepare", &args, modern);
                assert!(!new_cache.exists());
                let result = answer(&transport, "cache_prepare", &args, &form, action, modern);
                assert_eq!(result["result"]["isError"], action == "decline", "{result}");
                let retry = native(&project, agent, "Bash", command.clone());
                assert_eq!(
                    retry["hookSpecificOutput"]["permissionDecision"] == "deny",
                    action == "decline",
                    "{retry}"
                );
            }
            assert_eq!(project.revision(), before);
            assert_eq!(
                std::fs::read_to_string(&old_cache).unwrap(),
                "unusable cache root"
            );
        }
    }
}

#[test]
fn recovery_policy_denial_reaches_exact_approval_and_consumed_native_retry() {
    for agent in ["claude", "codex"] {
        for modern in [false, true] {
            let (server, project) = fixture(Some(agent));
            let mut context = (*server.review()).clone();
            context.session_id = Some("acme-session".into());
            drop(server);
            let transport = Transport::start(Server::for_review(context).unwrap());
            for message in initialize(&json!({"elicitation":{"form":{}}}), agent) {
                transport.send(&message);
            }
            assert!(transport.receive()["result"].is_object());
            pwd_rule(&transport, agent, modern, "deny");
            let before = project.revision();
            let input = json!({"command":"pwd","workdir":project.review.project_root});
            let denied = native(&project, agent, "Bash", input.clone());
            assert_eq!(
                denied["hookSpecificOutput"]["permissionDecision"], "deny",
                "{denied}"
            );
            assert!(denied.to_string().contains("request_approve"), "{denied}");
            for action in ["decline", "accept"] {
                let listed = transport.call(json!(10), "request_list", &json!({}));
                let requests = listed["result"]["structuredContent"]["data"]["requests"]
                    .as_array()
                    .unwrap();
                assert_eq!(requests.len(), 1, "{listed}");
                let args = json!({"request_id":requests[0]["request_id"]});
                let reply = native(
                    &project,
                    agent,
                    "mcp__shield__request_approve",
                    args.clone(),
                );
                assert_ne!(
                    reply["hookSpecificOutput"]["permissionDecision"], "deny",
                    "{reply}"
                );
                let form = begin(&transport, "request_approve", &args, modern);
                let result = answer(&transport, "request_approve", &args, &form, action, modern);
                assert_eq!(result["result"]["isError"], action == "decline", "{result}");
                let retry = native(&project, agent, "Bash", input.clone());
                assert_eq!(
                    retry["hookSpecificOutput"]["permissionDecision"] == "deny",
                    action == "decline",
                    "{retry}"
                );
            }
            let consumed = native(&project, agent, "Bash", input);
            assert_eq!(
                consumed["hookSpecificOutput"]["permissionDecision"], "deny",
                "{consumed}"
            );
            assert_eq!(project.revision(), before);
        }
    }
}
