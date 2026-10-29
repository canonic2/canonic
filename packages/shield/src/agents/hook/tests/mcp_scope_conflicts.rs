//! Native scope checks use isolated homes, never the developer's agent settings.
use super::*;

#[test]
fn mcp_connections_scope_conflicts_are_blocked() {
    for fixture in [
        "claude_scope_conflicts_fixture",
        "codex_scope_conflicts_fixture",
    ] {
        let environment = assert_fs::TempDir::new().unwrap();
        let home = environment.path().join("home");
        let claude = home.join("claude-config");
        let codex = home.join("codex-config");
        fs::create_dir_all(&claude).unwrap();
        fs::create_dir_all(&codex).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("agents::hook::tests::mcp_scope_conflicts::{fixture}"),
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
}

fn fixture() -> assert_fs::TempDir {
    let root = assert_fs::TempDir::new().unwrap();
    let config=json!({"schema_version":1,"packs":[],"rules":"rules.json","mcp":{"cloud":{
        "transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"claude":"acme","codex":"acme"}}}}).to_string();
    let rules=json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[
        {"id":"inspect","decision":"allow","conditions":{"domain":"mcp","server":"cloud","tool":"inspect"}}
    ]}).to_string();
    review::mcp::initialize_fixture(
        root.path(),
        &Bundle::capture(&config, &[], Some(&rules)).unwrap(),
    );
    review::operator::integrations::configure_fixture(
        root.path(),
        crate::cli::public::IntegrationChoice::all(),
        review::integrations::AskFallback::Block,
    );
    root
}

fn scope_call(root: &Path, cwd: &Path, agent: &str) -> Value {
    translate(root,agent,&serde_json::to_vec(&json!({"hook_event_name":"PreToolUse","tool_name":"mcp__acme__inspect","tool_use_id":"acme-tool","session_id":"acme-session","cwd":cwd,"tool_input":{}})).unwrap())
}

fn assert_blocked(root: &Path, cwd: &Path, agent: &str) {
    let response = scope_call(root, cwd, agent);
    assert_eq!(
        response["hookSpecificOutput"]["permissionDecision"], "deny",
        "{response}"
    );
    assert!(!response.to_string().contains("other.example.com"));
}

#[test]
#[ignore = "run by parent with isolated native configuration homes"]
fn claude_scope_conflicts_fixture() {
    let root = fixture();
    let path = Path::new(&std::env::var_os("CLAUDE_CONFIG_DIR").unwrap()).join(".claude.json");
    let project = root.path().canonicalize().unwrap();
    assert_permitted(&scope_call(root.path(), root.path(), "claude"), "claude");
    for value in [
        json!({"projects":{project.to_str().unwrap():{"mcpServers":{"acme":{"type":"http","url":"https://other.example.com/mcp"}}}}}),
        // Even identical entries in another scope require explicit resolution.
        json!({"projects":{project.to_str().unwrap():{"mcpServers":{"acme":{"type":"http","url":"https://cloud.example.com/mcp"}}}}}),
        json!({"mcpServers":{"acme":{"type":"http","url":"https://other.example.com/mcp"}}}),
        json!({"mcpServers":{"ACME":{"type":"http","url":"https://other.example.com/mcp"}}}),
    ] {
        let source = value.to_string();
        fs::write(&path, &source).unwrap();
        assert_blocked(root.path(), root.path(), "claude");
        assert!(
            find("claude")
                .unwrap()
                .check_mcp_scopes(root.path(), root.path(), "acme")
                .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
    }
    fs::write(&path,json!({"projects":{"/unrelated/acme":{"mcpServers":{"acme":{}}}},"mcpServers":{"unrelated":{}}}).to_string()).unwrap();
    assert_permitted(&scope_call(root.path(), root.path(), "claude"), "claude");
    fs::write(&path, "{").unwrap();
    assert_blocked(root.path(), root.path(), "claude");
    fs::write(&path, "{}").unwrap();
    let nested = root.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(
        nested.join(".mcp.json"),
        json!({"mcpServers":{"acme":{"type":"http","url":"https://other.example.com/mcp"}}})
            .to_string(),
    )
    .unwrap();
    assert_blocked(root.path(), &nested, "claude");
    assert_permitted(&scope_call(root.path(), root.path(), "claude"), "claude");
}

#[test]
#[ignore = "run by parent with isolated native configuration homes"]
fn codex_scope_conflicts_fixture() {
    let root = fixture();
    let path = Path::new(&std::env::var_os("CODEX_HOME").unwrap()).join("config.toml");
    assert_permitted(&scope_call(root.path(), root.path(), "codex"), "codex");
    let source = "[mcp_servers.acme]\nurl = 'https://other.example.com/mcp'\n";
    fs::write(&path, source).unwrap();
    assert_blocked(root.path(), root.path(), "codex");
    assert!(
        find("codex")
            .unwrap()
            .check_mcp_scopes(root.path(), root.path(), "acme")
            .is_err()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    fs::write(
        &path,
        "[mcp_servers.unrelated]\nurl = 'https://other.example.com/mcp'\n",
    )
    .unwrap();
    assert_permitted(&scope_call(root.path(), root.path(), "codex"), "codex");
    let nested = root.path().join("nested");
    fs::create_dir_all(nested.join(".codex")).unwrap();
    let closer = nested.join(".codex/config.toml");
    fs::write(&closer, source).unwrap();
    assert_blocked(root.path(), &nested, "codex");
    fs::write(&closer, "[").unwrap();
    assert_blocked(root.path(), &nested, "codex");
    assert_permitted(&scope_call(root.path(), root.path(), "codex"), "codex");
}
