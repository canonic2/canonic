//! Repository layout never decides connection identity or launcher privacy.
use super::*;

fn native_fixture() -> (assert_fs::TempDir, Context) {
    let (root, context) = cloud_fixture(true);
    fs::write(root.path().join(".mcp.json"),
        "{\"mcpServers\":{\"acme\":{\"type\":\"http\",\"url\":\"https://cloud.example.com/mcp\"}}}\n").unwrap();
    fs::create_dir(root.path().join(".codex")).unwrap();
    fs::write(root.path().join(".codex/config.toml"),
        "# Native connection owned by the user\n[mcp_servers.acme]\nurl = 'https://cloud.example.com/mcp'\ndefault_tools_approval_mode = 'prompt'\n").unwrap();
    (root, context)
}

#[test]
fn mcp_connections_native_maintenance_preserves_entries_and_never_claims_them() {
    let (root, context) = native_fixture();
    let original = config(root.path())["mcpServers"]["acme"].clone();
    let codex = fs::read_to_string(root.path().join(".codex/config.toml")).unwrap();
    for _ in 0..2 {
        let before = meta(&context);
        let plan = prepare(root.path(), &before, IntegrationChoice::all())
            .unwrap()
            .unwrap();
        let desired = plan.desired.as_ref().unwrap();
        for agent in ["claude", "codex"] {
            let owned = &desired.connections[agent];
            assert!(owned["launches"].as_object().unwrap().is_empty());
            assert!(owned["entries"].is_null());
            assert_eq!(owned["review"][0]["projection"], "existing native entry");
        }
        apply(&context, &before, &plan).unwrap();
        let settings: Value = serde_json::from_slice(
            &fs::read(root.path().join(".claude/settings.local.json")).unwrap(),
        )
        .unwrap();
        assert!(
            settings["enabledMcpjsonServers"]
                .as_array()
                .unwrap()
                .contains(&json!("acme"))
        );
        assert!(
            meta(&context)["integration"]["connections"]["claude"]["settings"]
                .as_array()
                .unwrap()
                .contains(&json!("acme"))
        );
        assert_eq!(config(root.path())["mcpServers"]["acme"], original);
        assert!(
            fs::read_to_string(root.path().join(".codex/config.toml"))
                .unwrap()
                .contains(&codex)
        );
    }
    let before = meta(&context);
    let removal = prepare(root.path(), &before, IntegrationChoice::remove_all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &removal).unwrap();
    let settings: Value =
        serde_json::from_slice(&fs::read(root.path().join(".claude/settings.local.json")).unwrap())
            .unwrap();
    assert!(
        !settings["enabledMcpjsonServers"]
            .as_array()
            .unwrap()
            .contains(&json!("acme"))
    );
    assert_eq!(config(root.path())["mcpServers"]["acme"], original);
    assert!(
        fs::read_to_string(root.path().join(".codex/config.toml"))
            .unwrap()
            .contains(&codex)
    );
}

#[test]
fn mcp_connections_native_maintenance_rechecks_transport_before_any_publication() {
    let (root, context) = native_fixture();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::all())
        .unwrap()
        .unwrap();
    let path = root.path().join(".mcp.json");
    let changed = fs::read_to_string(&path)
        .unwrap()
        .replace("cloud.example.com", "other.example.com");
    fs::write(&path, &changed).unwrap();
    assert!(apply(&context, &before, &plan).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), changed);
    assert!(!root.path().join(".claude/settings.local.json").exists());
    assert_eq!(meta(&context), before);
    assert!(prepare(root.path(), &before, IntegrationChoice::all()).is_err());
}

#[test]
fn mcp_connections_native_maintenance_rechecks_competing_scopes() {
    let (root, context) = native_fixture();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::all())
        .unwrap()
        .unwrap();
    let nested = root.path().join("nested");
    fs::create_dir_all(nested.join(".codex")).unwrap();
    fs::write(
        nested.join(".codex/config.toml"),
        "[mcp_servers.acme]\nurl = 'https://other.example.com/mcp'\n",
    )
    .unwrap();
    let transport = crate::mcp_connections::Transport::Http {
        url: "https://cloud.example.com/mcp".into(),
    };
    assert!(
        crate::review::integrations::verify_mcp_connection_at(
            root.path(),
            &nested,
            "codex",
            "acme",
            &transport
        )
        .is_err()
    );
    // The root-scoped setup still owns only its root, not the nested file.
    apply(&context, &before, &plan).unwrap();
    assert!(
        fs::read_to_string(nested.join(".codex/config.toml"))
            .unwrap()
            .contains("other.example.com")
    );
}

#[test]
fn mcp_connections_native_stdio_preserves_exact_arguments_environment_and_ownership() {
    let (root, context) = private_stdio_fixture();
    let native = json!({"type":"stdio","command":"/acme/private/bin/connector",
        "args":["--profile","acme-personal"],"env":{"ACME_REGION":"acme-private-region"}});
    fs::write(
        root.path().join(".mcp.json"),
        json!({"mcpServers":{"acme":native}}).to_string(),
    )
    .unwrap();
    fs::create_dir(root.path().join(".codex")).unwrap();
    let codex = "# Existing private native entry\n[mcp_servers.acme]\ncommand = '/acme/private/bin/connector'\nargs = ['--profile', 'acme-personal']\nenv = { ACME_REGION = 'acme-private-region' }\n";
    fs::write(root.path().join(".codex/config.toml"), codex).unwrap();
    fixture_git(
        root.path(),
        &["add", "-f", "--", ".mcp.json", ".codex/config.toml"],
    );
    let index = fs::read(root.path().join(".git/index")).unwrap();
    let before = meta(&context);
    let plan = prepare(root.path(), &before, IntegrationChoice::all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &plan).unwrap();
    assert_eq!(config(root.path())["mcpServers"]["acme"], native);
    assert!(
        fs::read_to_string(root.path().join(".codex/config.toml"))
            .unwrap()
            .contains(codex)
    );
    for agent in ["claude", "codex"] {
        assert!(
            meta(&context)["integration"]["connections"][agent]["launches"]
                .as_object()
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    let before = meta(&context);
    let removal = prepare(root.path(), &before, IntegrationChoice::remove_all())
        .unwrap()
        .unwrap();
    apply(&context, &before, &removal).unwrap();
    assert_eq!(config(root.path())["mcpServers"]["acme"], native);
    assert!(
        fs::read_to_string(root.path().join(".codex/config.toml"))
            .unwrap()
            .contains(codex)
    );
}
