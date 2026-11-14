//! Native callback regressions with real files and isolated agent homes.
use super::*;
mod host_scopes;

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn automatic_mcp_matching_host_scope_fixture() {
    host_scopes::check();
}

#[test]
fn automatic_mcp_matching_callbacks_and_configuration_freshness() {
    for fixture in [
        "automatic_mcp_matching_callbacks_fixture",
        "automatic_mcp_matching_scopes_fixture",
        "automatic_mcp_matching_cost_fixture",
        "automatic_mcp_matching_default_home_fixture",
        "automatic_mcp_matching_review_fixture",
        "automatic_mcp_matching_host_scope_fixture",
    ] {
        let environment = assert_fs::TempDir::new().unwrap();
        let home = environment.path().join("home");
        let claude = home.join("claude-config");
        let codex = home.join("codex-config");
        fs::create_dir_all(&claude).unwrap();
        fs::create_dir_all(&codex).unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                &format!("agents::hook::tests::automatic_mcp::{fixture}"),
                "--ignored",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("CLAUDE_CONFIG_DIR", &claude)
            .env("CODEX_HOME", &codex);
        if fixture.contains("default_home") {
            command.env_remove("CLAUDE_CONFIG_DIR");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        if fixture.contains("cost") {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }
    }
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn automatic_mcp_matching_review_fixture() {
    let (_root, context) = fixture("recommended");
    let root = Path::new(&context.project_root);
    for agent in ["claude", "codex"] {
        let path = if agent == "codex" {
            root.join(".codex/config.toml")
        } else {
            root.join(".mcp.json")
        };
        let malformed_entry = if agent == "codex" {
            "[mcp_servers.broken]\ncommand = 42\n".to_owned()
        } else {
            json!({"mcpServers":{"broken":{"command":42}}}).to_string()
        };
        fs::write(&path, malformed_entry).unwrap();
        assert_eq!(
            call(root, agent, "external", "docs_search", "rename"),
            json!({})
        );
        for broken_file in ["{".to_owned(), " ".repeat(1024 * 1024 + 1)] {
            fs::write(&path, broken_file).unwrap();
            wrong_project_before_discovery(root, agent);
            assert_decision(
                &call(root, agent, "external", "docs_search", "rename"),
                agent,
                "deny",
            );
            let status = review::mcp::call(
                &context,
                &review::mcp::Connection::new("discovery-failure".into()),
                "status",
                &json!({}),
                &|| Ok(()),
            );
            assert_eq!(
                status["data"]["coverage"]["repair_needed"], true,
                "{status}"
            );
            assert!(
                !status["data"]["coverage"]["diagnostics"]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                "{status}"
            );
        }
        fs::remove_file(path).unwrap();
    }
    claude_semantic_scope_checks(&context);
}

fn wrong_project_before_discovery(root: &Path, agent: &str) {
    use crate::enforcement::protocol::{NativeApproval, NativeContext, evaluate_native};
    let input = json!({"protocol_version":1,"request_id":"wrong-project",
        "expected_project_id":"another-project","cwd":root,
        "operation":{"kind":"mcp","alias":"external","tool":"docs_search","arguments":{"query":"rename"}}});
    let native = NativeContext {
        integration: agent,
        approval: NativeApproval::Fallback,
        context_complete: true,
        working_files: None,
    };
    let (result, code) = evaluate_native(
        root,
        &serde_json::to_vec(&input).unwrap(),
        &json!({}),
        native,
        None,
        &|_| {},
    );
    assert_eq!(code, 2, "{result}");
    assert_eq!(result["code"], "project-changed", "{result}");
}

fn claude_semantic_scope_checks(context: &review::Context) {
    let root = Path::new(&context.project_root);
    configure(root, "claude", "acme", "acme-connector");
    let user = std::path::PathBuf::from(std::env::var_os("CLAUDE_CONFIG_DIR").unwrap())
        .join(".claude.json");
    fs::write(
        &user,
        json!({"history":"x".repeat(1024 * 1024)}).to_string(),
    )
    .unwrap();
    assert_decision(
        &call(root, "claude", "external", "docs_search", "rename"),
        "claude",
        "deny",
    );
    let mut state = json!({"numStartups":1,"projects":{
        context.project_root.clone(): {"history":["old"],"disabledMcpServers":[]},
        root.join("another-project").to_str().unwrap(): {"mcpServers":{"other":{"command":"other"}}}
    }});
    fs::write(&user, state.to_string()).unwrap();
    let observation = find("claude")
        .unwrap()
        .discover_mcp(root, root)
        .unwrap()
        .unwrap();
    state["numStartups"] = json!(2);
    state["tips"] = json!(["new tip"]);
    state["projects"][&context.project_root]["history"] = json!(["new"]);
    state["projects"][root.join("another-project").to_str().unwrap()]["mcpServers"] = json!({});
    fs::write(&user, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    assert!(
        observation.verify().is_ok(),
        "non-MCP state must not invalidate the observation"
    );
    for key in ["disabledMcpServers", "disabledMcpjsonServers"] {
        state["projects"][&context.project_root][key] = json!(["acme"]);
        fs::write(&user, state.to_string()).unwrap();
        assert!(
            observation.verify().is_err(),
            "disabled state must invalidate the observation"
        );
        assert_decision(
            &call(root, "claude", "acme", "docs_search", "rename"),
            "claude",
            "deny",
        );
        let disabled = find("claude")
            .unwrap()
            .discover_mcp(root, root)
            .unwrap()
            .unwrap();
        assert!(
            disabled
                .connections(&std::collections::BTreeMap::new(), "claude")
                .is_empty(),
            "disabled servers cannot satisfy other integrations' preconditions"
        );
        let status = review::mcp::call(
            context,
            &review::mcp::Connection::new("disabled-project".into()),
            "status",
            &json!({}),
            &|| Ok(()),
        );
        let rows = status["data"]["coverage"]["mcp_connections"]
            .as_array()
            .unwrap();
        assert!(
            !rows.iter().any(|row| row["agent"] == "claude"
                && row["alias"] == "acme"
                && row["configuration"] == "matched pack criteria"),
            "{status}"
        );
        state["projects"][&context.project_root][key] = json!([]);
    }
    fs::write(&user, state.to_string()).unwrap();
    assert!(observation.verify().is_ok());
    state["projects"][&context.project_root]["mcpServers"] = json!({"acme":{"command":"another"}});
    fs::write(&user, state.to_string()).unwrap();
    assert!(
        observation.verify().is_err(),
        "a new competing alias must invalidate the observation"
    );
    fs::remove_file(user).unwrap();
    configure(root, "claude", "acme", "another-connector");
    assert!(
        observation.verify().is_err(),
        "transport edits must invalidate the observation"
    );
}

#[test]
#[ignore = "run by parent with isolated HOME and no Claude configuration override"]
fn automatic_mcp_matching_default_home_fixture() {
    let (_root, context) = fixture("recommended");
    let root = Path::new(&context.project_root);
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(
        home.join(".claude.json"),
        json!({"mcpServers":{"acme":{"command":"acme-connector","args":["mcp","serve"]}}})
            .to_string(),
    )
    .unwrap();
    assert_decision(
        &call(root, "claude", "acme", "docs_search", "rename"),
        "claude",
        "allow",
    );
    let settings = home.join(".claude/settings.json");
    let disabled = json!({"disabledMcpServers":["acme"]}).to_string();
    fs::write(&settings, &disabled).unwrap();
    assert_decision(
        &call(root, "claude", "acme", "docs_search", "rename"),
        "claude",
        "deny",
    );
    assert_eq!(fs::read_to_string(settings).unwrap(), disabled);
}

fn pack() -> String {
    json!({"schema_version":1,"kind":"pack","id":"acme/connector","version":"1.0.0",
        "description":"Acme permissions","license":"MIT","maintainers":["Acme"],"engines":{"shield":"*"},
        "capabilities":["structured-rules-v1","mcp-integrations-v1","regex-v1","requirements-v1"],
        "mcp_integrations":{"connector":{"transport":"stdio","fields":{
            "/command":{"text":{"regex":"acme-connector|/opt/acme/bin/acme-connector"}},
            "/args":{"exact":["mcp","serve"]}}},
            "remote":{"transport":"http","fields":{"/url":{"text":"https://cloud.example.com/mcp"}}}},
        "rules":[
            {"id":"docs","decision":{"loose":"allow","recommended":"allow","strict":"ask"},
                "conditions":{"domain":"mcp","server":{"one_of":["connector","remote"]},"tool":"docs_search"},
                "requirements":{"fields":{"/query":{"text":"rename"}}},"on_unmet":{"decision":"deny"}},
            {"id":"delete","decision":"deny","conditions":{"domain":"mcp","server":{"one_of":["connector","remote"]},"tool":"delete"}},
            {"id":"review","decision":"ask","conditions":{"domain":"mcp","server":{"one_of":["connector","remote"]},"tool":"review"}}
        ]}).to_string()
}

fn fixture(profile: &str) -> (assert_fs::TempDir, review::Context) {
    configured_fixture(profile, false)
}

fn configured_fixture(profile: &str, managed: bool) -> (assert_fs::TempDir, review::Context) {
    let root = assert_fs::TempDir::new().unwrap();
    fs::create_dir_all(root.path().join(".codex")).unwrap();
    let mut config = json!({"schema_version":1,"packs":[{"id":"acme/connector","version":"1.0.0","profile":profile,"path":"pack.yml"}]});
    if managed {
        config["mcp"] = json!({"stable":{"transport":{"type":"stdio","command":"acme-connector","args":["mcp","serve"]},"agents":{"codex":"acme"},"packs":{"acme/connector":"connector"}}});
    }
    let context = review::mcp::initialize_fixture(
        root.path(),
        &Bundle::capture(&config.to_string(), &[pack()], None).unwrap(),
    );
    trust_codex(Path::new(&context.project_root), "trusted");
    let settings = claude_home().join("settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        settings,
        json!({"enableAllProjectMcpServers":true}).to_string(),
    )
    .unwrap();
    (root, context)
}

fn claude_home() -> std::path::PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(
        || std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".claude"),
        std::path::PathBuf::from,
    )
}

fn trust_codex(root: &Path, level: &str) {
    let path =
        std::path::PathBuf::from(std::env::var_os("CODEX_HOME").unwrap()).join("config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut doc = fs::read_to_string(&path)
        .unwrap_or_default()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    doc["projects"][root.to_str().unwrap()]["trust_level"] = toml_edit::value(level);
    fs::write(path, doc.to_string()).unwrap();
}

fn configure(root: &Path, agent: &str, alias: &str, command: &str) -> (std::path::PathBuf, String) {
    let (path, text) = if agent == "codex" {
        (
            root.join(".codex/config.toml"),
            format!(
                "[mcp_servers.{alias}]\ncommand = {command:?}\nargs = [\"mcp\", \"serve\"]\ndefault_tools_approval_mode = \"prompt\"\n[mcp_servers.{alias}.tools.acme_docs_search]\napproval_mode = \"approve\"\n"
            ),
        )
    } else {
        (
            root.join(".mcp.json"),
            json!({"mcpServers":{alias:{"command":command,"args":["mcp","serve"]}}}).to_string(),
        )
    };
    fs::write(&path, &text).unwrap();
    (path, text)
}

fn call(root: &Path, agent: &str, alias: &str, tool: &str, query: &str) -> Value {
    call_at(root, root, agent, alias, tool, query)
}

fn call_at(root: &Path, cwd: &Path, agent: &str, alias: &str, tool: &str, query: &str) -> Value {
    let event = if agent == "codex" {
        "PermissionRequest"
    } else {
        "PreToolUse"
    };
    translate_for(root, agent, if agent == "codex" { Event::Permission } else { Event::Before }, &serde_json::to_vec(&json!({"hook_event_name":event,
        "cwd":cwd,"session_id":"acme-session","tool_use_id":"acme-call","tool_name":format!("mcp__{alias}__{tool}"),
        "tool_input":{"query":query}})).unwrap())
}

fn assert_decision(value: &Value, agent: &str, expected: &str) {
    if agent == "codex" {
        if expected == "ask" {
            assert_eq!(value, &json!({}));
        } else {
            assert_eq!(
                value["hookSpecificOutput"]["decision"]["behavior"], expected,
                "{value}"
            );
        }
    } else {
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], expected,
            "{value}"
        );
    }
}

fn http_checks(root: &Path, agent: &str, profile: &str) {
    for url in [
        "https://cloud.example.com/mcp",
        "https://other.example.com/mcp",
    ] {
        let (path, source) = if agent == "codex" {
            (
                root.join(".codex/config.toml"),
                format!(
                    "[mcp_servers.remote_alias]\nurl = {url:?}\ndefault_tools_approval_mode = 'prompt'\n"
                ),
            )
        } else {
            (
                root.join(".mcp.json"),
                json!({"mcpServers":{"remote_alias":{"type":"http","url":url}}}).to_string(),
            )
        };
        fs::write(&path, &source).unwrap();
        let result = call(root, agent, "remote_alias", "docs_search", "rename");
        if url == "https://cloud.example.com/mcp" {
            assert_decision(
                &result,
                agent,
                if profile == "strict" { "ask" } else { "allow" },
            );
            assert_decision(
                &call(root, agent, "remote_alias", "delete", "rename"),
                agent,
                "deny",
            );
        } else {
            assert_eq!(result, json!({}), "{result}");
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
        fs::remove_file(path).unwrap();
    }
    let (path, unsupported) = if agent == "codex" {
        (root.join(".codex/config.toml"), "[mcp_servers.remote_alias]\nurl = 'https://cloud.example.com/mcp'\nhttp_headers = { X_Acme = 'fixture-header' }\n".to_owned())
    } else {
        (root.join(".mcp.json"), json!({"mcpServers":{"remote_alias":{"type":"http","url":"https://cloud.example.com/mcp","headers":{"X-Acme":"fixture-header"}}}}).to_string())
    };
    fs::write(&path, &unsupported).unwrap();
    assert_eq!(
        call(root, agent, "remote_alias", "docs_search", "rename"),
        json!({}),
        "unsupported transport must retain host permissions"
    );
    fs::write(&path, "{").unwrap();
    assert_decision(
        &call(root, agent, "remote_alias", "docs_search", "rename"),
        agent,
        "deny",
    );
    fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn automatic_mcp_matching_callbacks_fixture() {
    for profile in ["recommended", "strict"] {
        let (_root, context) = fixture(profile);
        let root = Path::new(&context.project_root);
        for agent in ["claude", "codex"] {
            http_checks(root, agent, profile);
            for alias in ["acme_first", "renamed_connector"] {
                let (path, before) = configure(root, agent, alias, "acme-connector");
                let expected = if profile == "strict" { "ask" } else { "allow" };
                assert_decision(
                    &call(root, agent, alias, "docs_search", "rename"),
                    agent,
                    expected,
                );
                let status = review::mcp::call(
                    &context,
                    &review::mcp::Connection::new("automatic-status".into()),
                    "status",
                    &json!({}),
                    &|| Ok(()),
                );
                let rows = status["data"]["coverage"]["mcp_connections"]
                    .as_array()
                    .unwrap();
                assert!(
                    rows.iter().any(|row| row["agent"] == agent
                        && row["alias"] == alias
                        && row["matching"] == "unchecked"
                        && row["configuration"] == "policy matching checked on interception"),
                    "{status}"
                );
                assert_decision(
                    &call(root, agent, alias, "docs_search", "outside"),
                    agent,
                    "deny",
                );
                assert_decision(&call(root, agent, alias, "delete", "rename"), agent, "deny");
                assert_decision(&call(root, agent, alias, "review", "rename"), agent, "ask");
                assert_decision(
                    &call(root, agent, alias, "unknown_tool", "rename"),
                    agent,
                    "ask",
                );
                assert_eq!(fs::read_to_string(&path).unwrap(), before);
                let history = review::mcp::call(
                    &context,
                    &review::mcp::Connection::new("history-check".into()),
                    "history_list",
                    &json!({}),
                    &|| Ok(()),
                );
                assert!(history["error"].is_null(), "{history}");
                let history = history.to_string();
                assert!(!history.contains("connection_digest"), "{history}");
                assert!(!history.contains("_mcp_binding"), "{history}");
                // The same name cannot authorize a different executable.
                configure(root, agent, alias, "another-connector");
                assert_eq!(call(root, agent, alias, "docs_search", "rename"), json!({}));
                // A supported absolute executable path uses the pack predicate.
                configure(root, agent, alias, "/opt/acme/bin/acme-connector");
                assert_decision(
                    &call(root, agent, alias, "docs_search", "rename"),
                    agent,
                    expected,
                );
                fs::remove_file(&path).unwrap();
                assert_eq!(call(root, agent, alias, "docs_search", "rename"), json!({}));
            }
        }
    }
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn automatic_mcp_matching_cost_fixture() {
    use clap::Parser as _;
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    for managed in [true, false] {
        let (_root, context) = configured_fixture("recommended", managed);
        let root = Path::new(&context.project_root);
        configure(root, "codex", "acme", "acme-connector");
        assert_decision(
            &call(root, "codex", "acme", "docs_search", "rename"),
            "codex",
            "allow",
        );
        let start = Instant::now();
        for _ in 0..5 {
            assert_decision(
                &call(root, "codex", "acme", "docs_search", "rename"),
                "codex",
                "allow",
            );
        }
        eprintln!(
            "automatic_mcp_matching managed={managed} warm_callback_mean_us={}",
            start.elapsed().as_micros() / 5
        );
        if managed {
            continue;
        }
        let logs = assert_fs::TempDir::new().unwrap();
        let cli = crate::cli::Cli::parse_from([
            "shield",
            "--debug-log-dir",
            logs.path().to_str().unwrap(),
            "status",
        ]);
        let log = Arc::new(crate::diagnostics::Log::open(&cli));
        let path = log.path().unwrap().to_owned();
        let request = log.request("hook", "automatic_mcp", None, None).unwrap();
        let scope = request.enter();
        let host = json!({"hook_event_name":"PermissionRequest","cwd":root,"session_id":"acme-session",
            "tool_name":"mcp__acme__docs_search","tool_input":{"query":"rename"}});
        let response = translate_event(
            root,
            find("codex").unwrap(),
            &serde_json::to_vec(&host).unwrap(),
            Event::Permission,
            &log,
        );
        assert_decision(&response, "codex", "allow");
        drop(scope);
        request.finish("success");
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let records = fs::read_to_string(&path).unwrap();
            let end = records
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .find(|r| r["event"] == "request_end");
            if let Some(end) = end {
                let counts = &end["fields"]["counts"];
                for key in [
                    "policy.preparations",
                    "policy.captures",
                    "projection.files_written",
                ] {
                    assert_eq!(counts[key], 0, "{end}");
                }
                assert_eq!(counts["pattern.compiled"].as_u64().unwrap_or(0), 0, "{end}");
                assert_eq!(counts["mcp.discovery.reads"], 4, "{end}");
                assert_eq!(counts["mcp.discovery.revalidations"], 2, "{end}");
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}

#[test]
#[ignore = "run by parent with isolated agent configuration homes"]
fn automatic_mcp_matching_scopes_fixture() {
    let (_root, context) = fixture("recommended");
    let root = Path::new(&context.project_root);
    for agent in ["claude", "codex"] {
        let (project, source) = configure(root, agent, "acme", "acme-connector");
        let home = std::path::PathBuf::from(
            std::env::var_os(if agent == "codex" {
                "CODEX_HOME"
            } else {
                "CLAUDE_CONFIG_DIR"
            })
            .unwrap(),
        );
        let user = home.join(if agent == "codex" {
            "config.toml"
        } else {
            ".claude.json"
        });
        fs::write(&user, &source).unwrap();
        if agent == "codex" {
            trust_codex(root, "trusted");
        }
        assert_decision(
            &call(root, agent, "acme", "docs_search", "rename"),
            agent,
            "deny",
        );
        fs::remove_file(&project).unwrap();
        assert_decision(
            &call(root, agent, "acme", "docs_search", "rename"),
            agent,
            "allow",
        );
        let observation = find(agent)
            .unwrap()
            .discover_mcp(root, root)
            .unwrap()
            .unwrap();
        assert!(observation.verify().is_ok());
        let unrelated_edit = if agent == "codex" {
            format!(
                "{}\n# unrelated formatting change\n",
                fs::read_to_string(&user).unwrap()
            )
        } else {
            let mut value: Value = serde_json::from_str(&source).unwrap();
            value["numStartups"] = json!(42);
            serde_json::to_string_pretty(&value).unwrap()
        };
        fs::write(&user, unrelated_edit).unwrap();
        assert!(observation.verify().is_ok());
        fs::write(&user, if agent == "codex" { "# changed" } else { "{}" }).unwrap();
        assert!(observation.verify().is_err());
        fs::remove_file(&user).unwrap();
        configure(root, agent, "acme", "acme-connector");
        if agent == "codex" {
            trust_codex(root, "trusted");
            fs::write(&project, "[mcp_servers.acme]\ncommand = 'acme-connector'\nargs = ['mcp','serve']\nenabled = false\n").unwrap();
        } else {
            fs::create_dir_all(root.join(".claude")).unwrap();
            fs::write(
                root.join(".claude/settings.local.json"),
                json!({"disabledMcpjsonServers":["acme"]}).to_string(),
            )
            .unwrap();
        }
        assert_decision(
            &call(root, agent, "acme", "docs_search", "rename"),
            agent,
            "deny",
        );
        fs::remove_file(&project).unwrap();
    }
    transport_approval_check(&context);
    observation_isolation_check(&context);
}

fn observation_isolation_check(context: &review::Context) {
    let root = Path::new(&context.project_root);
    configure(root, "codex", "acme", "acme-connector");
    let project = crate::runtime::Project::open(root).unwrap();
    let snapshot = project.snapshot().unwrap();
    let observation = find("codex")
        .unwrap()
        .discover_mcp(root, root)
        .unwrap()
        .unwrap();
    let connections = observation.connections(snapshot.mcp_connections(), "codex");
    let observed = snapshot
        .resolve_mcp("host:acme", &connections, "host-project")
        .unwrap();
    let (_other_root, other) = fixture("recommended");
    let other_project = crate::runtime::Project::open(Path::new(&other.project_root)).unwrap();
    let other_snapshot = other_project.snapshot().unwrap();
    let request = crate::policy::Request {
        executable: "shield:mcp".into(),
        arguments: vec![],
        cwd: other.project_root.clone(),
        project: other.project_root.clone(),
    };
    let result = other_snapshot.evaluate_resource(
        request,
        crate::review::runtime::consumer::ResourceRequest {
            input: crate::policy::normalized::Input::Mcp {
                server: "host:acme".into(),
                tool: "docs_search".into(),
                arguments: json!({"query":"rename"}),
                connection: None,
                original_input: json!({}),
            },
            context: crate::policy::normalized::Context {
                cwd: other.project_root,
                environment: None,
                working_files: None,
            },
            mcp: Some(std::sync::Arc::new(observed)),
        },
    );
    assert!(matches!(
        result,
        Err(crate::runtime::Error::Invalid(
            "MCP observation belongs to another project"
        ))
    ));
}

fn transport_approval_check(context: &review::Context) {
    let root = Path::new(&context.project_root);
    // In-session approval needs the agent's recorded integration.
    review::operator::integrations::configure_fixture(
        root,
        crate::cli::public::IntegrationChoice::agent("codex"),
        crate::review::integrations::AskFallback::Block,
    );
    // An exact one-time approval cannot transfer to another matched transport
    // just because its host alias and tool arguments stay the same.
    configure(root, "codex", "grant_connector", "acme-connector");
    let host = json!({"hook_event_name":"PreToolUse","cwd":root,"session_id":"acme-session",
        "tool_use_id":"acme-grant-call","tool_name":"mcp__grant_connector__delete","tool_input":{"query":"rename"}});
    let before = translate(root, "codex", &serde_json::to_vec(&host).unwrap());
    let text = before["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    let id = text
        .split("request_id ")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let mut mcp = context.clone();
    mcp.surface = "mcp".into();
    mcp.agent = Some("codex".into());
    let approved = review::mcp::call_with_confirmation(
        &mcp,
        &review::mcp::Connection::new("acme-approval".into()),
        "request_approve",
        &json!({"request_id":id}),
        &|| Ok(()),
        true,
    );
    assert_eq!(approved["data"]["status"], "approved", "{approved}");
    configure(
        root,
        "codex",
        "grant_connector",
        "/opt/acme/bin/acme-connector",
    );
    let changed = translate(root, "codex", &serde_json::to_vec(&host).unwrap());
    assert_eq!(
        changed["hookSpecificOutput"]["permissionDecision"], "deny",
        "{changed}"
    );
    configure(root, "codex", "grant_connector", "acme-connector");
    let original = translate(root, "codex", &serde_json::to_vec(&host).unwrap());
    assert!(
        original["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap_or_default()
            .contains("allowed once"),
        "{original}"
    );
}
