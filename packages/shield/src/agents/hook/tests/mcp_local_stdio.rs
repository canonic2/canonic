//! Private launcher verification when native hooks start through a binary alias.
use super::*;

#[test]
fn mcp_connections_local_stdio_hooks_use_registered_binary_through_symlink() {
    let environment = assert_fs::TempDir::new().unwrap();
    let home = environment.path().join("home");
    let claude = home.join("claude-config");
    let codex = home.join("codex-config");
    fs::create_dir_all(&claude).unwrap();
    fs::create_dir_all(&codex).unwrap();
    let alias = environment.path().join("shield-hook-alias");
    std::os::unix::fs::symlink(std::env::current_exe().unwrap(), &alias).unwrap();
    let output = std::process::Command::new(&alias)
        .args([
            "--exact",
            "agents::hook::tests::mcp_local_stdio::local_stdio_symlink_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("HOME", &home)
        .env("CLAUDE_CONFIG_DIR", &claude)
        .env("CODEX_HOME", &codex)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[test]
#[ignore = "run by parent through a symlink with isolated native homes"]
fn local_stdio_symlink_fixture() {
    let invoked = std::path::PathBuf::from(std::env::args_os().next().unwrap());
    let registered_binary = std::env::current_exe().unwrap().canonicalize().unwrap();
    assert_ne!(invoked, registered_binary);
    assert_eq!(invoked.canonicalize().unwrap(), registered_binary);
    let (_root, context) = configured_project();
    let project = Path::new(&context.project_root);
    assert_registered_launchers(project, &registered_binary);
    assert_launcher_drift_is_blocked(project);
    assert_existing_native_connections(&context);
}

fn configured_project() -> (assert_fs::TempDir, review::Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let git = std::process::Command::new("/usr/bin/git")
        .current_dir(root.path())
        .args(["init", "-q"])
        .output()
        .unwrap();
    assert!(
        git.status.success(),
        "{}",
        String::from_utf8_lossy(&git.stderr)
    );
    let local = json!({"schema_version":1,"packs":[],"mcp":{"cloud":{
        "transport":{"type":"stdio","command":"acme-connector","args":["--profile","acme-ro"],"env":{"ACME_REGION":"acme-region"}},
        "agents":{"claude":"acme","codex":"acme"}}}}).to_string();
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[
        {"id":"inspect","decision":"allow","conditions":{"domain":"mcp","server":"cloud","tool":"inspect"}},
        {"id":"delete","decision":"deny","conditions":{"domain":"mcp","server":"cloud","tool":"delete"}}
    ]}).to_string();
    let bundle = Bundle::capture_layers(
        "schema_version: 1\npacks: []\nrules: rules.json\n",
        Some(&local),
        &[],
        Some(&rules),
        None,
        &[],
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    // Match the terminal workflow's canonical enrolled project root, including
    // hosts where the system temporary directory itself has a symlink spelling.
    let project = Path::new(&context.project_root);
    review::operator::integrations::configure_fixture(
        project,
        crate::cli::public::IntegrationChoice::all(),
        review::integrations::AskFallback::Block,
    );
    let snapshot = Project::open(root.path()).unwrap().snapshot().unwrap();
    assert!(snapshot.mcp_connections()["cloud"].local);
    (root, context)
}

fn assert_registered_launchers(project: &Path, registered_binary: &Path) {
    for agent in ["claude", "codex"] {
        let source = fs::read_to_string(project.join(if agent == "claude" {
            ".mcp.json"
        } else {
            ".codex/config.toml"
        }))
        .unwrap();
        let transport = find(agent)
            .unwrap()
            .connection(&source, "acme")
            .unwrap()
            .unwrap();
        let crate::mcp_connections::Transport::Stdio { command, args, env } = transport else {
            panic!("expected private stdio launcher")
        };
        assert_eq!(Path::new(&command), registered_binary);
        assert!(args.contains(&"mcp-connect".into()));
        assert!(args.contains(&"--token".into()));
        assert!(env.is_empty());
        assert_permitted(
            &tool(project, agent, "mcp__acme__inspect", &json!({})),
            agent,
        );
        assert_eq!(
            tool(project, agent, "mcp__acme__delete", &json!({}))["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
    }
}

fn assert_launcher_drift_is_blocked(project: &Path) {
    // Keep the original launcher-drift boundary covered before exercising
    // existing native transports without launch bindings.
    let original = fs::read_to_string(project.join(".mcp.json")).unwrap();
    let mut changed: Value = serde_json::from_str(&original).unwrap();
    changed["mcpServers"]["acme"]["command"] = json!("/acme/changed-shield");
    fs::write(project.join(".mcp.json"), changed.to_string()).unwrap();
    let codex_original = fs::read_to_string(project.join(".codex/config.toml")).unwrap();
    let mut codex_changed: toml_edit::DocumentMut = codex_original.parse().unwrap();
    codex_changed["mcp_servers"]["acme"]["command"] = toml_edit::value("/acme/changed-shield");
    fs::write(
        project.join(".codex/config.toml"),
        codex_changed.to_string(),
    )
    .unwrap();
    for agent in ["claude", "codex"] {
        assert_eq!(
            tool(project, agent, "mcp__acme__inspect", &json!({}))["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
    }
    fs::write(project.join(".mcp.json"), original).unwrap();
    fs::write(project.join(".codex/config.toml"), codex_original).unwrap();
}

fn assert_existing_native_connections(context: &review::Context) {
    let project = Path::new(&context.project_root);
    // A pre-existing native connection can match reviewed local policy without
    // any generated binding. This also works in projects with no Git metadata.
    fs::rename(project.join(".git"), project.join("saved-index")).unwrap();
    review::update_operator_fixture(context, |operator| {
        operator["integration"]["connections"] = json!({});
        operator["integration"]["version"] = json!("0.0.0");
        operator["integration"]["binary_digest"] = json!("0".repeat(64));
    })
    .unwrap();
    let path = project.join(".mcp.json");
    let mut claude: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    claude["mcpServers"]["acme"] = json!({"command":"acme-connector",
        "args":["--profile","acme-ro"],"env":{"ACME_REGION":"acme-region"}});
    fs::write(&path, claude.to_string()).unwrap();
    let path = project.join(".codex/config.toml");
    let mut codex: toml_edit::DocumentMut = fs::read_to_string(&path).unwrap().parse().unwrap();
    codex["mcp_servers"]["acme"]["command"] = toml_edit::value("acme-connector");
    codex["mcp_servers"]["acme"]["args"] = toml_edit::value(
        ["--profile", "acme-ro"]
            .into_iter()
            .collect::<toml_edit::Array>(),
    );
    codex["mcp_servers"]["acme"]["env"]["ACME_REGION"] = toml_edit::value("acme-region");
    fs::write(&path, codex.to_string()).unwrap();
    let before =
        [".mcp.json", ".codex/config.toml"].map(|path| fs::read(project.join(path)).unwrap());
    for agent in ["claude", "codex"] {
        assert_permitted(
            &tool(project, agent, "mcp__acme__inspect", &json!({})),
            agent,
        );
        assert_eq!(
            tool(project, agent, "mcp__acme__delete", &json!({}))["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
    }
    // Hooks may append history, but do not rewrite connection settings.
    for (index, path) in [".mcp.json", ".codex/config.toml"].iter().enumerate() {
        assert_eq!(before[index], fs::read(project.join(path)).unwrap());
    }
    // Native matching must not weaken transport-drift detection.
    let path = project.join(".mcp.json");
    let mut claude: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    claude["mcpServers"]["acme"]["command"] = json!("/acme/changed-shield");
    fs::write(&path, claude.to_string()).unwrap();
    let path = project.join(".codex/config.toml");
    let mut codex: toml_edit::DocumentMut = fs::read_to_string(&path).unwrap().parse().unwrap();
    codex["mcp_servers"]["acme"]["command"] = toml_edit::value("/acme/changed-shield");
    fs::write(&path, codex.to_string()).unwrap();
    for agent in ["claude", "codex"] {
        assert_eq!(
            tool(project, agent, "mcp__acme__inspect", &json!({}))["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
    }
}
