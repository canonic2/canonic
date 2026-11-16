//! Host eligibility, stable project roots and host-owned namespaces.
use super::*;

pub(super) fn check() {
    claude();
    codex();
    reserved();
    collisions();
    hyphenated_aliases();
    unsupported_aliases();
}

fn unsupported_aliases() {
    for agent in ["claude", "codex"] {
        let (_root, context) = fixture("recommended");
        let root = Path::new(&context.project_root);
        configure(root, agent, "Acme", "acme-connector");
        assert_eq!(
            call(root, agent, "Acme", "docs_search", "rename"),
            json!({}),
            "an unsupported unique spelling retains host permissions, never pack Allow"
        );
        assert_eq!(
            call(root, agent, "acme", "docs_search", "rename"),
            json!({}),
            "an approximate spelling of one unsupported key cannot borrow pack Allow"
        );
        let doctor = review::mcp::call(
            &context,
            &review::mcp::Connection::new("unsupported-alias".into()),
            "doctor",
            &json!({}),
            &|| Ok(()),
        );
        let row = doctor["data"]["coverage"]["mcp_connections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["agent"] == agent && row["alias"] == "Acme")
            .unwrap();
        assert!(row["diagnostic"].is_string(), "{row}");
        assert_ne!(
            row["configuration"],
            "policy matching checked on interception"
        );
    }
}

fn hyphenated_aliases() {
    for agent in ["claude", "codex"] {
        let (_root, context) = fixture("recommended");
        let root = Path::new(&context.project_root);
        let (path, source) = configure(root, agent, "acme-browser", "acme-connector");
        let namespace = if agent == "codex" {
            "acme_browser"
        } else {
            "acme-browser"
        };
        assert_decision(
            &call(root, agent, namespace, "docs_search", "rename"),
            agent,
            "allow",
        );
        let history = review::history_events(root).unwrap();
        let evidence = history["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|event| event.pointer("/details/mcp_match/alias"))
            .collect::<Vec<_>>();
        assert!(
            evidence.iter().any(|alias| **alias == "acme-browser"),
            "{history}"
        );
        assert!(
            !evidence.iter().any(|alias| **alias == "acme_browser"),
            "{history}"
        );
        let doctor = review::mcp::call(
            &context,
            &review::mcp::Connection::new("alias-doctor".into()),
            "doctor",
            &json!({}),
            &|| Ok(()),
        );
        let rows = doctor["data"]["coverage"]["mcp_connections"]
            .as_array()
            .unwrap();
        let row = rows
            .iter()
            .find(|row| row["agent"] == agent && row["alias"] == "acme-browser")
            .unwrap();
        assert_eq!(row["tool_namespace"], namespace);
        assert!(row["diagnostic"].is_null());
        if agent == "codex" {
            let (_, other) = configure(root, agent, "acme_browser", "another-connector");
            fs::write(&path, format!("{source}\n{other}")).unwrap();
            assert_decision(
                &call(root, agent, namespace, "docs_search", "rename"),
                agent,
                "deny",
            );
            let doctor = review::mcp::call(
                &context,
                &review::mcp::Connection::new("collision-doctor".into()),
                "doctor",
                &json!({}),
                &|| Ok(()),
            );
            let rows = doctor["data"]["coverage"]["mcp_connections"]
                .as_array()
                .unwrap();
            for row in rows.iter().filter(|row| {
                row["agent"] == "codex"
                    && ["acme-browser", "acme_browser"]
                        .iter()
                        .any(|alias| row["alias"] == *alias)
            }) {
                assert!(row["diagnostic"].as_str().unwrap().contains("ambiguous"));
                assert_ne!(
                    row["configuration"],
                    "policy matching checked on interception"
                );
            }
        }
    }
    let (_root, context) = configured_alias_fixture("recommended", Some("acme-browser"));
    let root = Path::new(&context.project_root);
    configure(root, "codex", "acme-browser", "acme-connector");
    assert_decision(
        &call(root, "codex", "acme_browser", "docs_search", "rename"),
        "codex",
        "allow",
    );
    let history = review::history_events(root).unwrap();
    assert!(
        history["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event
                .pointer("/details/mcp_match/alias")
                .is_some_and(|alias| alias == "acme-browser")),
        "managed connection history retains its configured key: {history}"
    );
}

fn collisions() {
    let (_root, context) = fixture("recommended");
    let root = Path::new(&context.project_root);
    for agent in ["claude", "codex"] {
        // The controls prove the same hook call would otherwise allow docs.
        let (path, source) = configure(root, agent, "acme", "acme-connector");
        assert_decision(
            &call(root, agent, "acme", "docs_search", "rename"),
            agent,
            "allow",
        );
        for collision in ["acme_", "acme__x", "Acme", "acme-", "acme."] {
            let config_alias = if agent == "codex" {
                format!("{collision:?}")
            } else {
                collision.into()
            };
            let (_, other) = configure(root, agent, &config_alias, "another-connector");
            let combined = if agent == "codex" {
                format!("{source}\n{other}")
            } else {
                let mut value: Value = serde_json::from_str(&source).unwrap();
                value["mcpServers"][collision] =
                    serde_json::from_str::<Value>(&other).unwrap()["mcpServers"][collision].clone();
                value.to_string()
            };
            fs::write(&path, &combined).unwrap();
            if agent == "claude" && collision == "acme-" {
                // Claude preserves this supported hyphen: these are distinct
                // namespaces, unlike Codex's encoded trailing underscore.
                assert_decision(
                    &call(root, agent, "acme", "docs_search", "rename"),
                    agent,
                    "allow",
                );
                continue;
            }
            assert_decision(
                &call(root, agent, "acme", "docs_search", "rename"),
                agent,
                "deny",
            );
            if collision == "Acme" {
                assert_decision(
                    &call(root, agent, "Acme", "docs_search", "rename"),
                    agent,
                    "deny",
                );
            }
            // Delimiter-bearing aliases cannot borrow the shorter alias's Allow.
            if collision.starts_with("acme_") {
                assert_decision(
                    &call(root, agent, collision, "docs_search", "rename"),
                    agent,
                    "deny",
                );
            }
            assert_eq!(fs::read_to_string(&path).unwrap(), combined);
        }
        fs::write(&path, &source).unwrap();
        assert_decision(
            &call(root, agent, "acme", "docs_search", "rename"),
            agent,
            "allow",
        );
        fs::remove_file(path).unwrap();
    }
}

fn claude_state(root: &Path, trusted: bool) -> Value {
    json!({"projects":{root.to_str().unwrap():{"hasTrustDialogAccepted":trusted}}})
}

fn claude() {
    let (_root, context) = fixture("recommended");
    let root = Path::new(&context.project_root);
    let nested = root.join("sub");
    fs::create_dir_all(nested.join(".claude")).unwrap();
    fs::create_dir_all(root.join(".claude")).unwrap();
    let state_path = std::path::PathBuf::from(std::env::var_os("CLAUDE_CONFIG_DIR").unwrap())
        .join(".claude.json");
    let (_, source) = configure(root, "claude", "root_server", "acme-connector");
    fs::write(
        nested.join(".mcp.json"),
        source.replace("root_server", "acme"),
    )
    .unwrap();
    fs::write(
        nested.join(".claude/settings.local.json"),
        json!({"enableAllProjectMcpServers":true}).to_string(),
    )
    .unwrap();
    fs::write(&state_path,json!({"projects":{nested.to_str().unwrap():{
        "hasTrustDialogAccepted":true,"mcpServers":{"acme":{"command":"acme-connector","args":["mcp","serve"]}}
    }}}).to_string()).unwrap();
    assert_eq!(
        call_at(root, &nested, "claude", "acme", "docs_search", "rename"),
        json!({})
    );
    assert_decision(
        &call_at(
            root,
            &nested,
            "claude",
            "root_server",
            "docs_search",
            "rename",
        ),
        "claude",
        "allow",
    );
    claude_project_approvals(root);
    claude_user_approvals(root, &nested);
}

fn claude_project_approvals(root: &Path) {
    let state_path = std::path::PathBuf::from(std::env::var_os("CLAUDE_CONFIG_DIR").unwrap())
        .join(".claude.json");
    let shared = root.join(".claude/settings.json");
    let local = root.join(".claude/settings.local.json");
    let user = claude_home().join("settings.json");
    configure(root, "claude", "acme", "acme-connector");
    fs::write(&user, "{}").unwrap();
    fs::write(
        &shared,
        json!({"enableAllProjectMcpServers":true}).to_string(),
    )
    .unwrap();
    fs::write(
        &local,
        json!({"enabledMcpjsonServers":["acme"]}).to_string(),
    )
    .unwrap();
    fs::write(&state_path, claude_state(root, false).to_string()).unwrap();
    assert_eq!(
        call(root, "claude", "acme", "docs_search", "rename"),
        json!({}),
        "repository approval cannot establish workspace trust"
    );
    fs::write(&state_path, claude_state(root, true).to_string()).unwrap();
    fs::write(&shared, "{}").unwrap();
    assert_decision(
        &call(root, "claude", "acme", "docs_search", "rename"),
        "claude",
        "allow",
    );
    let observation = find("claude")
        .unwrap()
        .discover_mcp(root, root)
        .unwrap()
        .unwrap();
    fs::write(
        &local,
        json!({"enabledMcpjsonServers":["other"]}).to_string(),
    )
    .unwrap();
    assert!(
        observation.verify().is_err(),
        "approval removal invalidates the match"
    );
    assert_eq!(
        call(root, "claude", "acme", "docs_search", "rename"),
        json!({})
    );
    let pending = find("claude")
        .unwrap()
        .discover_mcp(root, root)
        .unwrap()
        .unwrap();
    assert!(
        pending
            .connections(&std::collections::BTreeMap::new(), "claude")
            .is_empty(),
        "unapproved alternatives cannot satisfy preconditions"
    );
    fs::write(
        &local,
        json!({"enableAllProjectMcpServers":true}).to_string(),
    )
    .unwrap();
    assert_decision(
        &call(root, "claude", "acme", "docs_search", "rename"),
        "claude",
        "allow",
    );
    let enabled = find("claude")
        .unwrap()
        .discover_mcp(root, root)
        .unwrap()
        .unwrap();
    fs::write(&state_path, claude_state(root, false).to_string()).unwrap();
    assert!(
        enabled.verify().is_err(),
        "trust removal invalidates project-derived approval"
    );
    assert_eq!(
        call(root, "claude", "acme", "docs_search", "rename"),
        json!({})
    );
}

fn claude_user_approvals(root: &Path, nested: &Path) {
    let state_path = std::path::PathBuf::from(std::env::var_os("CLAUDE_CONFIG_DIR").unwrap())
        .join(".claude.json");
    let shared = root.join(".claude/settings.json");
    let local = root.join(".claude/settings.local.json");
    let user = claude_home().join("settings.json");
    // User approvals are independent of project trust, but rejection wins.
    fs::write(&user, json!({"enabledMcpjsonServers":["acme"]}).to_string()).unwrap();
    assert_decision(
        &call(root, "claude", "acme", "docs_search", "rename"),
        "claude",
        "allow",
    );
    fs::write(
        &local,
        json!({"disabledMcpjsonServers":["acme"]}).to_string(),
    )
    .unwrap();
    assert_decision(
        &call(root, "claude", "acme", "docs_search", "rename"),
        "claude",
        "deny",
    );
    fs::write(&local, "{}").unwrap();
    fs::write(&user, "{}").unwrap();
    fs::write(&state_path, claude_state(root, true).to_string()).unwrap();
    assert_eq!(
        call_at(root, nested, "claude", "acme", "docs_search", "rename"),
        json!({}),
        "nested settings cannot approve a root server"
    );
    fs::write(
        &shared,
        json!({"enableAllProjectMcpServers":true}).to_string(),
    )
    .unwrap();
    let approved = find("claude")
        .unwrap()
        .discover_mcp(root, root)
        .unwrap()
        .unwrap();
    fs::write(
        &shared,
        json!({"enableAllProjectMcpServers":false}).to_string(),
    )
    .unwrap();
    assert!(approved.verify().is_err());
    assert_eq!(
        call(root, "claude", "acme", "docs_search", "rename"),
        json!({})
    );
    // Merely starting a native callback is not evidence of server approval.
    assert_eq!(
        fs::read_to_string(&shared).unwrap(),
        json!({"enableAllProjectMcpServers":false}).to_string()
    );
}

fn codex() {
    let (_root, context) = fixture("recommended");
    let root = Path::new(&context.project_root);
    let nested = root.join("sub");
    fs::create_dir_all(nested.join(".codex")).unwrap();
    let user =
        std::path::PathBuf::from(std::env::var_os("CODEX_HOME").unwrap()).join("config.toml");
    let (path, source) = configure(root, "codex", "acme", "acme-connector");
    trust_codex(root, "untrusted");
    assert_eq!(
        call(root, "codex", "acme", "docs_search", "rename"),
        json!({})
    );
    // A project cannot trust itself, and ignored malformed layers do not block a user server.
    fs::write(
        &path,
        format!(
            "{source}\n[projects.{}]\ntrust_level = 'trusted'\n",
            json!(root.to_str().unwrap())
        ),
    )
    .unwrap();
    assert_eq!(
        call(root, "codex", "acme", "docs_search", "rename"),
        json!({})
    );
    fs::write(&path, "malformed = [").unwrap();
    fs::write(&user, &source).unwrap();
    assert_decision(
        &call(root, "codex", "acme", "docs_search", "rename"),
        "codex",
        "allow",
    );
    fs::write(&user, "").unwrap();
    configure(root, "codex", "acme", "acme-connector");
    assert_eq!(
        call(root, "codex", "acme", "docs_search", "rename"),
        json!({}),
        "unknown trust grants no project allowance"
    );
    trust_codex(root, "trusted");
    assert_decision(
        &call(root, "codex", "acme", "docs_search", "rename"),
        "codex",
        "allow",
    );
    let observation = find("codex")
        .unwrap()
        .discover_mcp(root, root)
        .unwrap()
        .unwrap();
    trust_codex(root, "untrusted");
    assert!(
        observation.verify().is_err(),
        "trust revocation invalidates a project observation"
    );
    trust_codex(root, "trusted");
    fs::write(
        nested.join(".codex/config.toml"),
        source.replace("acme]", "nested]"),
    )
    .unwrap();
    assert_decision(
        &call_at(root, &nested, "codex", "nested", "docs_search", "rename"),
        "codex",
        "allow",
    );
    trust_codex(&nested, "untrusted");
    assert_eq!(
        call_at(root, &nested, "codex", "nested", "docs_search", "rename"),
        json!({})
    );
}

fn reserved() {
    let (_root, context) = fixture("recommended");
    let root = Path::new(&context.project_root);
    for agent in ["claude", "codex"] {
        for alias in [
            "plugin_acme",
            "claude_ai_gmail",
            "claude_ai_Gmail",
            "codex_apps",
        ] {
            configure(root, agent, alias, "acme-connector");
            assert_eq!(
                call(root, agent, alias, "docs_search", "rename"),
                json!({}),
                "file claims cannot suppress host connector prompts"
            );
            let observed = find(agent)
                .unwrap()
                .discover_mcp(root, root)
                .unwrap()
                .unwrap();
            assert!(
                observed
                    .connections(&std::collections::BTreeMap::new(), agent)
                    .is_empty()
            );
            let claimed = serde_json::from_value(json!({"claimed":{
                "transport":{"type":"stdio","command":"acme-connector","args":["mcp","serve"]},
                "agents":{agent:alias}
            }}))
            .unwrap();
            assert!(
                observed
                    .connections(&claimed, agent)
                    .values()
                    .all(|c| !c.enabled),
                "a managed namespace claim cannot satisfy another integration's precondition"
            );
        }
    }
}
