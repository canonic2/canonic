//! Native environment overrides cannot turn agent configuration into writable scratch.
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value, json};
use std::fs;

pub(super) fn initialize(project: &assert_fs::TempDir) {
    let initialized = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/integration/operator_terminal.py"
        ))
        .arg(assert_cmd::cargo::cargo_bin!("shield"))
        .arg(project.path())
        .arg("apply")
        .output()
        .expect("owned initialization terminal runs");
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let initialized: Value = serde_json::from_slice(&initialized.stdout)
        .expect("initialization terminal returns a JSON receipt");
    assert_eq!(initialized["code"], 0, "{initialized}");
}

#[test]
fn relocated_agent_configuration_is_protected_with_temporary_access() {
    let project = assert_fs::TempDir::new().unwrap();
    let environment = assert_fs::TempDir::new().unwrap();
    let home = environment.path().join("home");
    let scratch = environment.path().join("scratch");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&scratch).unwrap();
    fs::create_dir(project.path().join(".shield")).unwrap();
    fs::write(
        project.path().join(".shield/config.yml"),
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
    )
    .unwrap();
    fs::write(project.path().join(".shield/rules.yml"), json!({
        "schema_version":1,"kind":"custom",
        "capabilities":["structured-rules-v1","argument-rules-v1","location-v1","requirements-v1","access-paths-v1"],
        "rules":[
            {"id":"print","decision":"allow","conditions":{"domain":"argv","executable":"printf","arguments":{"kind":"any"}}},
            {"id":"write","decision":"allow","conditions":{"domain":"file","action":"write"},
                "requirements":{"destination":{"within":{"root":"write"}}},"on_unmet":{"decision":"deny"}}
        ]
    }).to_string()).unwrap();
    initialize(&project);
    for (agent, variable) in [("claude", "CLAUDE_CONFIG_DIR"), ("codex", "CODEX_HOME")] {
        // The relocated config need not exist yet: creation is protected too.
        let config = scratch.join(format!("acme-{agent}"));
        for (path, missing_home, expected) in [
            (config.join("settings.json"), false, "deny"),
            (config.join("config.toml"), false, "deny"),
            (
                scratch.join("ordinary.txt"),
                false,
                if agent == "claude" { "allow" } else { "" },
            ),
            (scratch.join("ordinary.txt"), true, "deny"),
        ] {
            let mut command = cargo_bin_cmd!("shield");
            command
                .args([
                    "--no-debug-log",
                    "__internal",
                    "native-hook",
                    "--agent",
                    agent,
                    "--event",
                    "PreToolUse",
                    "--project",
                ])
                .arg(project.path())
                .env("HOME", &home)
                .env("TMPDIR", &scratch)
                .env(variable, &config);
            if missing_home {
                command.env_remove("HOME");
            }
            let output = command.write_stdin(json!({
                "hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"acme-working-files",
                "session_id":"acme-working-files","cwd":project.path(),
                "tool_input":{"command":format!("printf acme > '{}'",path.display())}
            }).to_string()).assert().success().get_output().stdout.clone();
            let response: Value = serde_json::from_slice(&output).unwrap();
            assert_eq!(
                response["hookSpecificOutput"]["permissionDecision"]
                    .as_str()
                    .unwrap_or(""),
                expected,
                "{agent}, missing home: {missing_home}, path: {}: {response}",
                path.display()
            );
            assert!(!path.exists());
        }
    }
}
