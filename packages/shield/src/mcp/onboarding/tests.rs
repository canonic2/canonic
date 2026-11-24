use super::*;
use crate::mcp::tests::{Transport, initialize, test_options};

#[test]
fn first_run_setup_uses_isolated_agent_homes() {
    for fixture in [
        "first_run_initialization_confirms_then_uses_normal_tools_on_the_same_connection",
        "project_setup_detects_hub_and_repeats_without_confirmation",
        "project_setup_without_forms_returns_the_operator_action",
        "project_setup_with_large_native_settings_keeps_the_review_short",
        "first_run_decline_and_unavailable_confirmation_leave_no_shield_files",
        "first_run_modern_confirmations_bind_exact_arguments_and_are_single_use",
        "first_run_preserves_policy_that_appears_during_confirmation",
        "first_run_preserves_existing_authoring_files",
        "first_run_refuses_home_and_ancestors",
        "first_run_interrupted_publication_keeps_normal_inspection_available",
    ] {
        let environment = assert_fs::TempDir::new().unwrap();
        let home = environment.path().join("home");
        let claude = home.join("claude");
        let codex = home.join("codex");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::create_dir_all(&codex).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("mcp::onboarding::tests::{fixture}"),
                "--ignored",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("SHIELD_ISOLATED_HOME", &home)
            .env("TMPDIR", environment.path())
            .env("TMP", environment.path())
            .env("TEMP", environment.path())
            .env("CLAUDE_CONFIG_DIR", &claude)
            .env("CODEX_HOME", &codex)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{fixture}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}

fn fresh(capabilities: &Value) -> (assert_fs::TempDir, Transport) {
    fresh_at(capabilities, None)
}

fn fresh_at(
    capabilities: &Value,
    project: Option<&std::path::Path>,
) -> (assert_fs::TempDir, Transport) {
    let root = assert_fs::TempDir::new().expect("fixture project");
    std::fs::create_dir_all(root.path().join(".hub")).expect("fixture Hub folder");
    std::fs::write(root.path().join(".hub/config.yml"), "schema_version: 1\n")
        .expect("fixture Hub marker");
    let server = Bootstrap {
        input: Arc::new(ReviewContext {
            project: project
                .unwrap_or(root.path())
                .canonicalize()
                .expect("canonical fixture project"),
            agent: Some("codex".into()),
            session_id: None,
            tool_use_id: None,
        }),
        active: Arc::new(OnceLock::new()),
        confirmations: Mutex::new(Confirmations::default()),
        plans: Mutex::new(BTreeMap::new()),
        sequence: AtomicU64::new(1),
        log: Arc::new(crate::diagnostics::Log::disabled()),
        prepare: Onboarding::fixture,
    };
    let transport = Transport::with_runner(move |reader, writer| {
        serve_server(server, reader, writer, &test_options())
    });
    let [hello, initialized] = initialize(capabilities, "codex");
    assert!(
        transport.request(&hello)["result"]["instructions"]
            .as_str()
            .expect("bootstrap instructions")
            .contains("Call setup")
    );
    transport.send(&initialized);
    (root, transport)
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn first_run_refuses_home_and_ancestors() {
    let home = std::env::home_dir().unwrap().canonicalize().unwrap();
    for project in [&home, home.parent().unwrap()] {
        let (_root, transport) = fresh_at(&json!({"elicitation":{"form":{}}}), Some(project));
        let refused = transport.call(json!(2), "init", &json!({"agents":["hub"]}));
        assert_eq!(
            data(&refused)["error"]["code"],
            "invalid_project",
            "{refused}"
        );
        assert!(
            data(&refused)["error"]["message"]
                .as_str()
                .unwrap()
                .contains(project.to_str().unwrap()),
            "{refused}"
        );
        assert!(!project.join(".shield").exists());
        let (_, result) = transport.close();
        result.unwrap();
    }
    let project = home.join("project");
    std::fs::create_dir(&project).unwrap();
    assert!(first_run_target_allowed(&project, Some(&home)));
    let outside = assert_fs::TempDir::new().unwrap();
    assert!(first_run_target_allowed(outside.path(), Some(&home)));
    #[cfg(unix)]
    {
        let alias = home.join("alias");
        std::os::unix::fs::symlink(&home, &alias).unwrap();
        assert!(!first_run_target_allowed(&alias, Some(&home)));
    }
}

fn data(reply: &Value) -> &Value {
    &reply["result"]["structuredContent"]
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn project_setup_detects_hub_and_repeats_without_confirmation() {
    let (root, transport) = fresh(&json!({"elicitation":{"form":{}}}));
    let form = transport.call(json!(2), "setup", &json!({}));
    assert_eq!(form["method"], "elicitation/create", "{form}");
    assert!(!root.path().join(".shield").exists());
    transport.send(&json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":"accept","content":{"confirm":true}}}));
    let reply = transport.receive();
    let result = &data(&reply)["data"];
    assert_eq!(result["setup_status"], "initialized", "{reply}");
    assert_eq!(result["next"]["server"], "hub");
    assert_eq!(result["next"]["tool"], "setup");
    let definition = root.path().join(".hub/mcp/shield.yml");
    let before = std::fs::read(&definition).unwrap();
    assert!(!root.path().join(".codex/hooks.json").exists());
    let repeated = transport.call(json!(3), "setup", &json!({}));
    assert_eq!(
        data(&repeated)["data"]["setup_status"],
        "unchanged",
        "{repeated}"
    );
    assert_eq!(data(&repeated)["data"]["next"], result["next"]);
    assert_eq!(std::fs::read(definition).unwrap(), before);
    transport.close().1.unwrap();
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn project_setup_without_forms_returns_the_operator_action() {
    let (root, transport) = fresh(&json!({}));
    let reply = transport.call(
        json!(2),
        "setup",
        &json!({"agents":["hub"],"profile":"strict"}),
    );
    assert_eq!(
        data(&reply)["error"]["code"],
        "approval_unavailable",
        "{reply}"
    );
    assert_eq!(
        data(&reply)["error"]["operator_action"],
        json!({"command":"shield","argv":["--project",root.path().canonicalize().unwrap(),"setup","--agent","hub","--profile","strict"]})
    );
    assert!(!root.path().join(".shield").exists());
    assert!(!root.path().join(".hub/mcp/shield.yml").exists());
    transport.close().1.unwrap();
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn project_setup_with_large_native_settings_keeps_the_review_short() {
    let (root, transport) = fresh(&json!({"elicitation":{"form":{}}}));
    let path = root.path().join(".claude/settings.local.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let padding = "fixture setting".repeat(6000);
    std::fs::write(&path, json!({"env":{"ACME_PADDING":padding}}).to_string()).unwrap();
    let form = transport.call(json!(2), "setup", &json!({"agents":["claude"]}));
    assert_eq!(form["method"], "elicitation/create", "{form}");
    assert!(
        form.to_string().len() < 12_000,
        "{} bytes",
        form.to_string().len()
    );
    assert!(!form.to_string().contains("ACME_PADDING"));
    transport.send(&json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":"accept","content":{"confirm":true}}}));
    let reply = transport.receive();
    assert_eq!(
        data(&reply)["data"]["setup_status"],
        "initialized",
        "{reply}"
    );
    let settings: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(settings["env"]["ACME_PADDING"], padding);
    assert!(!root.path().join(".hub/mcp/shield.yml").exists());
    transport.close().1.unwrap();
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn first_run_initialization_confirms_then_uses_normal_tools_on_the_same_connection() {
    let (root, transport) = fresh(&json!({"elicitation":{"form":{}}}));
    let blocked = transport.call(json!(2), "enable", &json!({}));
    assert_eq!(
        data(&blocked)["error"]["code"],
        "not_initialized",
        "{blocked}"
    );
    let question = transport.call(json!(3), "init", &json!({"agents":["hub"]}));
    assert_eq!(question["method"], "elicitation/create", "{question}");
    assert!(!root.path().join(".shield").exists());
    assert!(!root.path().join(".hub/mcp/shield.yml").exists());
    transport.send(&json!({"jsonrpc":"2.0","id":question["id"],"result":{"action":"accept","content":{"confirm":true}}}));
    let reply = transport.receive();
    assert!(data(&reply)["error"].is_null(), "{reply}");
    assert_eq!(data(&reply)["data"]["enabled"], true, "{reply}");
    assert!(root.path().join(".hub/mcp/shield.yml").is_file());
    let status = transport.call(json!(4), "status", &json!({}));
    assert!(data(&status)["error"].is_null(), "{status}");
    assert_eq!(
        data(&status)["data"]["enforcement"],
        "defined-for-hub",
        "{status}"
    );
    let repeated = transport.call(json!(5), "init", &json!({"agents":["hub"]}));
    assert!(data(&repeated)["error"].is_null(), "{repeated}");
    let (_, result) = transport.close();
    result.unwrap();
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn first_run_decline_and_unavailable_confirmation_leave_no_shield_files() {
    for accept_forms in [false, true] {
        let (root, transport) = fresh(&if accept_forms {
            json!({"elicitation":{"form":{}}})
        } else {
            json!({})
        });
        let reply = transport.call(json!(2), "init", &json!({"agents":["hub"]}));
        let reply = if accept_forms {
            assert_eq!(reply["method"], "elicitation/create", "{reply}");
            transport
                .send(&json!({"jsonrpc":"2.0","id":reply["id"],"result":{"action":"decline"}}));
            transport.receive()
        } else {
            reply
        };
        assert_eq!(
            data(&reply)["error"]["code"],
            if accept_forms {
                "approval_declined"
            } else {
                "approval_unavailable"
            },
            "{reply}"
        );
        assert!(!root.path().join(".shield").exists());
        assert!(!root.path().join(".hub/mcp/shield.yml").exists());
        let (_, result) = transport.close();
        result.unwrap();
    }
}

fn modern(id: i64, arguments: &Value, state: Option<&Value>, action: Option<&str>) -> Value {
    let mut params = json!({"name":"init","arguments":arguments,"_meta":{
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
        "io.modelcontextprotocol/clientCapabilities":{"elicitation":{"form":{}}}
    }});
    if let Some(state) = state {
        params["requestState"] = state.clone();
    }
    if let Some(action) = action {
        params["inputResponses"] = json!({"confirmation":{"action":action}});
    }
    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params})
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn first_run_modern_confirmations_bind_exact_arguments_and_are_single_use() {
    for action in ["accept", "decline", "changed"] {
        let (root, transport) = fresh(&json!({}));
        let args = json!({"agents":["hub"]});
        let question = transport.request(&modern(2, &args, None, None));
        assert_eq!(
            question["result"]["resultType"], "input_required",
            "{question}"
        );
        assert!(!root.path().join(".shield").exists());
        let token = &question["result"]["requestState"];
        let submitted = if action == "changed" {
            json!({"agents":["claude"]})
        } else {
            args.clone()
        };
        let reply = transport.request(&modern(
            3,
            &submitted,
            Some(token),
            Some(if action == "decline" {
                "decline"
            } else {
                "accept"
            }),
        ));
        if action == "accept" {
            assert!(data(&reply)["error"].is_null(), "{reply}");
            assert!(root.path().join(".shield/config.yml").is_file());
            let status = transport.call(json!(4), "status", &json!({}));
            assert_eq!(data(&status)["data"]["enabled"], true, "{status}");
        } else {
            assert_eq!(
                data(&reply)["error"]["code"],
                if action == "changed" {
                    "invalid_confirmation"
                } else {
                    "approval_declined"
                },
                "{reply}"
            );
            assert!(!root.path().join(".shield").exists());
        }
        let repeated = transport.request(&modern(5, &args, Some(token), Some("accept")));
        assert_eq!(
            data(&repeated)["error"]["code"],
            "invalid_confirmation",
            "{repeated}"
        );
        let (_, result) = transport.close();
        result.unwrap();
    }
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn first_run_preserves_policy_that_appears_during_confirmation() {
    let (root, transport) = fresh(&json!({"elicitation":{"form":{}}}));
    let question = transport.call(json!(2), "init", &json!({"agents":["hub"]}));
    assert_eq!(question["method"], "elicitation/create", "{question}");
    std::fs::create_dir(root.path().join(".shield")).unwrap();
    let original = "schema_version: 1\npacks: []\n# concurrent policy\n";
    std::fs::write(root.path().join(".shield/config.yml"), original).unwrap();
    transport.send(
        &json!({"jsonrpc":"2.0","id":question["id"],"result":{"action":"accept","content":{}}}),
    );
    let reply = transport.receive();
    assert!(data(&reply)["error"].is_object(), "{reply}");
    assert_eq!(
        std::fs::read_to_string(root.path().join(".shield/config.yml")).unwrap(),
        original
    );
    assert!(!root.path().join(".hub/mcp/shield.yml").exists());
    let (_, result) = transport.close();
    result.unwrap();
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn first_run_preserves_existing_authoring_files() {
    for (name, original) in [
        ("config.yml", "schema_version: 1\npacks: []\n"),
        (
            "rules.yml",
            "schema_version: 1\nkind: custom\ncapabilities: []\nrules: []\n",
        ),
        (
            "config.local.yml",
            "schema_version: 1\n# preserved fixture\n",
        ),
    ] {
        let (root, transport) = fresh(&json!({"elicitation":{"form":{}}}));
        std::fs::create_dir(root.path().join(".shield")).unwrap();
        let path = root.path().join(".shield").join(name);
        std::fs::write(&path, original).unwrap();
        let response = transport.call(json!(2), "init", &json!({"agents":["hub"]}));
        assert!(
            data(&response)["error"]["message"]
                .as_str()
                .unwrap()
                .contains("existing Shield authoring files"),
            "{response}"
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
        // Valid project configuration may be enrolled by integration inspection;
        // orphan authoring files must still leave the project uninitialized.
        if name != "config.yml" {
            assert!(!root.path().join(".shield/state/state.json").exists());
        }
        assert!(!root.path().join(".hub/mcp/shield.yml").exists());
        let (_, result) = transport.close();
        result.unwrap();
    }
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn first_run_interrupted_publication_keeps_normal_inspection_available() {
    let (root, transport) = fresh(&json!({"elicitation":{"form":{}}}));
    let question = transport.call(json!(2), "init", &json!({"agents":["hub"]}));
    assert_eq!(question["method"], "elicitation/create", "{question}");
    std::fs::create_dir(root.path().join(".hub/mcp")).unwrap();
    let foreign = "schema_version: 1\nconnection:\n  transport: stdio\n  command: acme-helper\n";
    std::fs::write(root.path().join(".hub/mcp/shield.yml"), foreign).unwrap();
    transport.send(
        &json!({"jsonrpc":"2.0","id":question["id"],"result":{"action":"accept","content":{}}}),
    );
    let _reply = transport.receive();
    assert!(root.path().join(".shield/config.yml").is_file());
    assert_eq!(
        std::fs::read_to_string(root.path().join(".hub/mcp/shield.yml")).unwrap(),
        foreign
    );
    let status = transport.call(json!(3), "status", &json!({}));
    assert!(data(&status)["error"].is_null(), "{status}");
    assert_eq!(data(&status)["data"]["enabled"], true, "{status}");
    let (_, result) = transport.close();
    result.unwrap();
}
