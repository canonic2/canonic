//! Both Google hosts enter the shared evaluator without sharing wire formats.
use super::*;

fn input(agent: &str, root: &Path, tool: &str, args: Value) -> Value {
    let mut host = if agent == "gemini" {
        json!({"hook_event_name":"BeforeTool","session_id":"acme-session","cwd":root,"tool_name":tool})
    } else {
        json!({"conversationId":"acme-session","workspacePaths":[root],"stepIdx":2,"toolCall":{"name":tool}})
    };
    if agent == "gemini" {
        host["tool_input"] = args;
    } else {
        host["toolCall"]["args"] = args;
    }
    host
}

#[test]
fn google_adapters_reach_policy_and_protected_paths_with_native_replies() {
    let (root, _) = fixture();
    fs::write(root.path().join("acme.txt"), "Acme").unwrap();
    for agent in ["gemini", "agy"] {
        let allowed = if agent == "agy" {
            "auto_approve"
        } else {
            "allow"
        };
        let shell = |source: &str| {
            if agent == "gemini" {
                input(
                    agent,
                    root.path(),
                    "run_shell_command",
                    json!({"command":source,"dir_path":root.path()}),
                )
            } else {
                input(
                    agent,
                    root.path(),
                    "run_command",
                    json!({"CommandLine":source,"Cwd":root.path()}),
                )
            }
        };
        let send = |host: &Value| {
            translate_for(
                root.path(),
                agent,
                Event::Before,
                &serde_json::to_vec(host).unwrap(),
            )
        };
        if !cfg!(windows) || agent == "agy" {
            assert_eq!(send(&shell("git status"))["decision"], allowed);
            assert_eq!(send(&shell("git reset --hard"))["decision"], "deny");
            assert_eq!(
                send(&shell("git push"))["decision"],
                if agent == "gemini" {
                    "ask"
                } else {
                    "force_ask"
                }
            );
        }
        for (path, expected) in [
            (root.path().join("acme.txt"), allowed),
            (root.path().join(".shield/state/state.json"), "deny"),
            (root.path().join(".shield/review/state.json"), "deny"),
        ] {
            let args = if agent == "gemini" {
                json!({"file_path":path})
            } else {
                json!({"Path":path})
            };
            let value = send(&input(agent, root.path(), "read_file", args));
            assert_eq!(value["decision"], expected, "{agent}: {value}");
        }
        // Invalid mapped calls return actionable context, not host abstention.
        let args = if agent == "gemini" {
            json!({"file_path":""})
        } else {
            json!({"Path":""})
        };
        let value = send(&input(agent, root.path(), "read_file", args));
        assert_eq!(value["decision"], "deny");
        assert!(!value["reason"].as_str().unwrap().is_empty(), "{value}");
        let mut own = input(
            agent,
            root.path(),
            "call_mcp_tool",
            json!({"ServerName":"shield","ToolName":"policy_show","Arguments":{}}),
        );
        if agent == "gemini" {
            own = input(agent, root.path(), "mcp_shield_policy_show", json!({}));
            own["mcp_context"] = json!({"server_name":"shield","tool_name":"policy_show"});
        }
        assert_eq!(send(&own)["decision"], allowed);
        if agent == "gemini" {
            own["mcp_context"]["tool_name"] = json!("acme_unknown_tool");
        } else {
            own["toolCall"]["args"]["ToolName"] = json!("acme_unknown_tool");
        }
        assert_eq!(send(&own)["decision"], "deny");
    }
}

#[test]
fn google_adapters_json_mcp_transports_keep_vendor_keys_and_exact_ownership() {
    use crate::mcp_connections::Transport;
    for (agent, key) in [("gemini", "url"), ("agy", "serverUrl")] {
        let adapter = crate::agents::find(agent).unwrap();
        let names = r#"{"mcpServers":{"acme_tools":{"command":"first-mcp"},"acme-tools":{"command":"second-mcp"}}}"#;
        assert_ne!(
            adapter.connection(names, "acme_tools").unwrap(),
            adapter.connection(names, "acme-tools").unwrap()
        );
        let transport = Transport::Http {
            url: "https://example.com/mcp".into(),
        };
        let alternative = if agent == "gemini" {
            json!({"httpUrl":"https://example.com/mcp"})
        } else {
            json!({"url":"https://example.com/mcp"})
        };
        assert_eq!(
            adapter
                .connection(
                    &json!({"mcpServers":{"acme":alternative}}).to_string(),
                    "acme"
                )
                .unwrap(),
            Some(transport.clone())
        );
        let mixed = if agent == "gemini" {
            json!({"url":"https://example.com/mcp","type":"sse"})
        } else {
            json!({"url":"https://example.com/mcp","serverUrl":"https://other.example.com/mcp"})
        };
        assert!(
            adapter
                .connection(&json!({"mcpServers":{"acme":mixed}}).to_string(), "acme")
                .is_err()
        );
        let desired = [("acme".into(), transport.clone())].into_iter().collect();
        let original =
            "{\"mcpServers\":{\"other\":{\"command\":\"other-mcp\"}},\"ui\":{\"theme\":\"acme\"}}";
        let added = adapter
            .merge_connections(original, &json!({}), &desired)
            .unwrap();
        let document: Value = serde_json::from_str(&added.source).unwrap();
        if agent == "gemini" {
            assert_eq!(document["mcpServers"]["acme"]["type"], "http");
        }
        assert_eq!(
            document["mcpServers"]["acme"][key],
            "https://example.com/mcp"
        );
        assert_eq!(
            adapter.connection(&added.source, "acme").unwrap(),
            Some(transport)
        );
        let removed = adapter
            .merge_connections(
                &added.source,
                added.ownership.as_ref().unwrap(),
                &std::collections::BTreeMap::default(),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&removed.source).unwrap(),
            serde_json::from_str::<Value>(original).unwrap()
        );
        let edited = added
            .source
            .replace("https://example.com/mcp", "https://other.example.com/mcp");
        assert!(
            adapter
                .merge_connections(
                    &edited,
                    added.ownership.as_ref().unwrap(),
                    &std::collections::BTreeMap::default()
                )
                .is_err()
        );
        assert!(adapter.connection("{\"mcpServers\":{\"acme\":{\"command\":\"acme\",\"url\":\"https://example.com\"}}}","acme").is_err());
    }
}

#[test]
fn google_known_effectful_tools_cannot_bypass_policy() {
    let (root, _) = fixture();
    for (tool, field) in [
        ("view_code_item", "File"),
        ("view_file_outline", "AbsolutePath"),
    ] {
        let host = input(
            "agy",
            root.path(),
            tool,
            json!({field:root.path().join(".shield/state/state.json")}),
        );
        let response = translate_for(
            root.path(),
            "agy",
            Event::Before,
            &serde_json::to_vec(&host).unwrap(),
        );
        assert_eq!(response["decision"], "deny", "{response}");
    }
    for (agent, tool, args) in [
        (
            "agy",
            "send_command_input",
            json!({"Input":"rm -rf .agents","Id":"existing-shell"}),
        ),
        ("gemini", "save_memory", json!({"fact":"Acme"})),
    ] {
        let host = input(agent, root.path(), tool, args);
        let response = translate_for(
            root.path(),
            agent,
            Event::Before,
            &serde_json::to_vec(&host).unwrap(),
        );
        assert_eq!(response["decision"], "deny", "{response}");
    }
}

#[test]
fn google_antigravity_relative_file_paths_in_multiple_workspaces_never_auto_approve() {
    let (root, context) = fixture();
    let project = Path::new(&context.project_root);
    let other = project.join("other");
    fs::create_dir(&other).unwrap();
    for tool in ["read_file", "edit_file", "write_file"] {
        for path in ["acme.txt", ".agents/hooks.json"] {
            let mut host = input("agy", project, tool, json!({"Path":path}));
            // Cwd is not a documented base for file tools.
            host["toolCall"]["args"]["Cwd"] = json!(project);
            for workspaces in [json!([other, project]), json!([project, other])] {
                host["workspacePaths"] = workspaces;
                let response = translate_for(
                    root.path(),
                    "agy",
                    Event::Before,
                    &serde_json::to_vec(&host).unwrap(),
                );
                assert_ne!(response["decision"], "auto_approve", "{tool}: {response}");
            }
        }
        for (path, expected) in [
            (project.join("acme.txt"), "auto_approve"),
            (
                project.join(".agents/hooks.json"),
                if tool == "read_file" {
                    "force_ask"
                } else {
                    "deny"
                },
            ),
        ] {
            let mut host = input("agy", project, tool, json!({"Path":path}));
            host["workspacePaths"] = json!([other, project]);
            let response = translate_for(
                root.path(),
                "agy",
                Event::Before,
                &serde_json::to_vec(&host).unwrap(),
            );
            assert_eq!(response["decision"], expected, "{tool}: {response}");
        }
    }
}

#[test]
fn google_antigravity_reused_terminal_keeps_host_permissions_but_known_denials_apply() {
    let (root, _) = fixture();
    for (source, denied) in [("git status", false), ("git reset --hard", true)] {
        let host = input(
            "agy",
            root.path(),
            "run_command",
            json!({
                "CommandLine":source,"RequestedTerminalID":"acme-terminal"
            }),
        );
        let value = translate_for(
            root.path(),
            "agy",
            Event::Before,
            &serde_json::to_vec(&host).unwrap(),
        );
        if denied {
            assert_eq!(value["decision"], "deny", "{value}");
        } else {
            assert_eq!(value, json!({}));
        }
    }
}
