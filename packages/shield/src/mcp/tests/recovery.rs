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

fn installed_hooks(agent: &str, modern: bool, handover: bool) -> (Transport, Project) {
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
    if handover && !cfg!(windows) {
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
            let (transport, project) = installed_hooks(agent, modern, false);
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
            // Without a session folder (Claude here has no CLAUDE_PROJECT_DIR)
            // the stale launch cannot open policy; Codex's session folder is
            // the project, so its stale hook follows it the way Git would.
            let stale_hook = native_at(&stale, &project, agent, "Bash", input.clone());
            let decision = &stale_hook["hookSpecificOutput"]["permissionDecision"];
            if agent == "codex" {
                assert_ne!(decision, "deny", "{stale_hook}");
            } else {
                assert_eq!(decision, "deny", "{stale_hook}");
            }
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
            for protected in [".shield/state/state.json", ".shield/review/state.json"] {
                let denied = native(
                    &project,
                    agent,
                    "Bash",
                    json!({"command":format!("cat {protected}"),
                    "workdir":project.review.project_root}),
                );
                assert_eq!(
                    denied["hookSpecificOutput"]["permissionDecision"], "deny",
                    "{denied}"
                );
            }
            let doctor = transport.call(json!(30), "doctor", &json!({}));
            assert_eq!(doctor["result"]["isError"], false, "{doctor}");
            assert_eq!(
                doctor["result"]["structuredContent"]["data"]["coverage"]["repair_needed"], false,
                "{doctor}"
            );
        }
    }
}

#[cfg(not(windows))]
#[test]
fn recovery_box_managed_hooks_route_to_box_without_writing() {
    for agent in ["claude", "codex"] {
        for modern in [false, true] {
            let (transport, project) = installed_hooks(agent, modern, true);
            let adapter = crate::agents::find(agent).unwrap();
            let hooks_path = project.root.path().join(adapter.spec().hooks_path);
            let mut hooks: Value =
                serde_json::from_slice(&std::fs::read(&hooks_path).unwrap()).unwrap();
            hooks["hooks"]["PreToolUse"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "matcher":".*","hooks":[{"type":"command",
                        "command":"shield __internal native-hook --project /acme/old-project"}]
                }));
            let broken = hooks.to_string();
            std::fs::write(&hooks_path, &broken).unwrap();
            let mcp_path = project
                .root
                .path()
                .join(adapter.spec().mcp_path.unwrap_or(".mcp.json"));
            let before = std::fs::read(&mcp_path).unwrap();
            let revision = project.revision();
            let result = transport.call(
                json!(20),
                "mcp_setup",
                &json!({"agents":[agent],"repair":true}),
            );
            assert_eq!(result["result"]["isError"], true, "{result}");
            let message = result["result"]["structuredContent"]["error"]["message"]
                .as_str()
                .unwrap();
            assert!(message.contains("config_sync"), "{result}");
            assert!(message.contains("repair_hooks: true"), "{result}");
            assert_eq!(std::fs::read_to_string(&hooks_path).unwrap(), broken);
            assert_eq!(std::fs::read(&mcp_path).unwrap(), before);
            assert_eq!(project.revision(), revision);
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
fn recovery_policy_denial_reaches_exact_approval_and_consumed_native_retry() {
    for agent in ["claude", "codex"] {
        for modern in [false, true] {
            let (server, project) = fixture(Some(agent));
            // In-session approval needs the agent's recorded integration.
            crate::review::operator::integrations::configure_fixture(
                project.root.path(),
                crate::review::integrations::IntegrationChoice::agent(agent),
                crate::review::integrations::AskFallback::Block,
            );
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

#[test]
fn git_restoration_stdio_review_keeps_both_warnings_through_decline_and_accept() {
    for agent in ["claude", "codex"] {
        for modern in [false, true] {
            let (server, project) = fixture(Some(agent));
            crate::review::operator::integrations::configure_fixture(
                project.root.path(),
                crate::review::integrations::IntegrationChoice::agent(agent),
                crate::review::integrations::AskFallback::Block,
            );
            assert!(
                crate::test_support::git()
                    .current_dir(project.root.path())
                    .args(["init", "-q"])
                    .status()
                    .unwrap()
                    .success()
            );
            let mut context = (*server.review()).clone();
            context.session_id = Some("acme-session".into());
            drop(server);
            let transport = Transport::start(Server::for_review(context).unwrap());
            for message in initialize(&json!({"elicitation":{"form":{}}}), agent) {
                transport.send(&message);
            }
            assert!(transport.receive()["result"].is_object());
            let input = json!({"command":"git checkout --ours -- .shield/config.yml .codex/config.toml", "workdir":project.review.project_root});
            for action in ["decline", "accept"] {
                let refused = native(&project, agent, "Bash", input.clone());
                assert_eq!(
                    refused["hookSpecificOutput"]["permissionDecision"], "deny",
                    "{refused}"
                );
                let listed = transport.call(json!(10), "request_list", &json!({}));
                let requests = listed["result"]["structuredContent"]["data"]["requests"]
                    .as_array()
                    .unwrap();
                let args = json!({"request_id":requests.last().unwrap()["request_id"]});
                let form = begin(&transport, "request_approve", &args, modern);
                let text = form.to_string();
                assert!(text.contains("Changing agent settings"), "{form}");
                assert!(text.contains("replace Shield's policy files"), "{form}");
                assert!(text.contains("Git target worktrees:"), "{form}");
                let result = answer(&transport, "request_approve", &args, &form, action, modern);
                assert_eq!(result["result"]["isError"], action == "decline", "{result}");
                let retry = native(&project, agent, "Bash", input.clone());
                assert_eq!(
                    retry["hookSpecificOutput"]["permissionDecision"] == "deny",
                    action == "decline",
                    "{retry}"
                );
            }
            assert_eq!(
                native(&project, agent, "Bash", input)["hookSpecificOutput"]["permissionDecision"],
                "deny"
            );
        }
    }
}

#[test]
fn invalid_policy_recovery_stdio_retains_diagnostics_confirmation_and_exact_retry() {
    for agent in ["claude", "codex"] {
        for modern in [false, true] {
            let (server, project) = fixture(Some(agent));
            crate::review::operator::integrations::configure_fixture(
                project.root.path(),
                crate::review::integrations::IntegrationChoice::agent(agent),
                crate::review::integrations::AskFallback::Block,
            );
            assert!(
                crate::test_support::git()
                    .current_dir(project.root.path())
                    .args(["init", "-q"])
                    .status()
                    .unwrap()
                    .success()
            );
            let mut context = (*server.review()).clone();
            context.session_id = Some("acme-session".into());
            drop(server);
            let config = project.root.path().join(".shield/config.yml");
            let broken = "packs: [\n";
            std::fs::write(&config, broken).unwrap();
            // A new connection must start while the active config is broken.
            let transport = Transport::start(Server::for_review(context).unwrap());
            for message in initialize(&json!({"elicitation":{"form":{}}}), agent) {
                transport.send(&message);
            }
            assert!(transport.receive()["result"].is_object());
            for tool in ["status", "doctor"] {
                let diagnostic = transport.call(json!(20), tool, &json!({}));
                assert_eq!(diagnostic["result"]["isError"], false, "{diagnostic}");
            }
            let input = json!({"command":"git checkout --ours -- .shield/config.yml .codex/config.toml", "workdir":project.review.project_root});
            for action in ["decline", "accept"] {
                let refused = native(&project, agent, "Bash", input.clone());
                assert_eq!(
                    refused["hookSpecificOutput"]["permissionDecision"], "deny",
                    "{refused}"
                );
                let listed = transport.call(json!(21), "request_list", &json!({}));
                let requests = listed["result"]["structuredContent"]["data"]["requests"]
                    .as_array()
                    .unwrap();
                let args = json!({"request_id":requests.last().unwrap()["request_id"]});
                let shield = native(
                    &project,
                    agent,
                    "mcp__shield__request_approve",
                    args.clone(),
                );
                assert_ne!(
                    shield["hookSpecificOutput"]["permissionDecision"], "deny",
                    "{shield}"
                );
                let form = begin(&transport, "request_approve", &args, modern);
                for warning in [
                    "current policy files are invalid",
                    "Changing agent settings",
                    "replace Shield's policy files",
                ] {
                    assert!(form.to_string().contains(warning), "{form}");
                }
                assert_eq!(std::fs::read_to_string(&config).unwrap(), broken);
                let result = answer(&transport, "request_approve", &args, &form, action, modern);
                assert_eq!(result["result"]["isError"], action == "decline", "{result}");
                let retry = native(&project, agent, "Bash", input.clone());
                assert_eq!(
                    retry["hookSpecificOutput"]["permissionDecision"] == "deny",
                    action == "decline",
                    "{retry}"
                );
                assert_eq!(std::fs::read_to_string(&config).unwrap(), broken);
            }
            let replay = native(&project, agent, "Bash", input);
            assert_eq!(
                replay["hookSpecificOutput"]["permissionDecision"], "deny",
                "{replay}"
            );
        }
    }
}

#[test]
fn invalid_policy_recovery_without_forms_keeps_requests_pending() {
    for agent in ["claude", "codex"] {
        let (server, project) = fixture(Some(agent));
        crate::review::operator::integrations::configure_fixture(
            project.root.path(),
            crate::review::integrations::IntegrationChoice::agent(agent),
            crate::review::integrations::AskFallback::Block,
        );
        let config = project.root.path().join(".shield/config.yml");
        std::fs::write(&config, "packs: [\n").unwrap();
        let transport = Transport::start(server);
        for message in initialize(&json!({}), agent) {
            transport.send(&message);
        }
        assert!(transport.receive()["result"].is_object());
        let input = json!({"command":"pwd","workdir":project.review.project_root});
        let refused = native(&project, agent, "Bash", input.clone());
        assert_eq!(
            refused["hookSpecificOutput"]["permissionDecision"], "deny",
            "{refused}"
        );
        let listed = transport.call(json!(30), "request_list", &json!({}));
        let args = json!({"request_id":listed["result"]["structuredContent"]["data"]["requests"][0]["request_id"]});
        let response = transport.call(json!(31), "request_approve", &args);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"], "approval_unavailable",
            "{response}"
        );
        let listed = transport.call(json!(32), "request_list", &json!({}));
        assert_eq!(
            listed["result"]["structuredContent"]["data"]["requests"][0]["status"], "pending",
            "{listed}"
        );
        let retry = native(&project, agent, "Bash", input);
        assert_eq!(
            retry["hookSpecificOutput"]["permissionDecision"], "deny",
            "{retry}"
        );
        assert_eq!(std::fs::read_to_string(config).unwrap(), "packs: [\n");
    }
}
