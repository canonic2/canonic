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
    // Using the registered binary must not weaken launcher-drift detection.
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
