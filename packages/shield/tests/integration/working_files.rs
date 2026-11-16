//! Native environment overrides cannot turn agent configuration into writable scratch.
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value, json};
use std::fs;

pub(super) fn initialize(project: &assert_fs::TempDir, home: &std::path::Path) {
    // Policy files are the policy: a folder that has them needs no review.
    if project.path().join(".shield/config.yml").exists() {
        cargo_bin_cmd!("shield")
            .args(["init", "--json", "--no-debug-log", "--project"])
            .arg(project.path())
            .env("HOME", home)
            .assert()
            .success();
        return;
    }
    let initialized = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/integration/operator_terminal.py"
        ))
        .arg(assert_cmd::cargo::cargo_bin!("shield"))
        .arg(project.path())
        .arg("apply")
        .env("SHIELD_TEST_HOME", home)
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
fn ripgrep_missing_operands_follow_policy_through_native_hooks() {
    let project = assert_fs::TempDir::new().unwrap();
    let environment = assert_fs::TempDir::new().unwrap();
    let home = environment.path().join("home");
    fs::create_dir(&home).unwrap();
    fs::create_dir(project.path().join(".shield")).unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    fs::write(project.path().join("src/app.ts"), "clientId").unwrap();
    fs::write(
        project.path().join(".shield/config.yml"),
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
    )
    .unwrap();
    fs::write(project.path().join(".shield/rules.yml"), json!({
        "schema_version":1,"kind":"custom",
        "capabilities":["structured-rules-v1","argument-rules-v1","location-v1","requirements-v1"],
        "rules":[{"id":"search","decision":"allow",
            "conditions":{"domain":"argv","executable":"rg","arguments":{"kind":"any"}},
            "requirements":{"targets":{"action":"read","all":{"within":{"root":"project"},"exclude":[
                {"root":"project","path":"secret"},
                {"root":"project","path":"future/secret"}
            ]}}},"on_unmet":{"decision":"deny"}},
            {"id":"create","decision":"allow","conditions":{"domain":"argv","executable":{"one_of":["ln","mkdir"]},"arguments":{"kind":"any"}},
                "requirements":{"targets":{"action":"write","all":{"within":{"root":"project"}}}},"on_unmet":{"decision":"deny"}}]
    }).to_string()).unwrap();
    initialize(&project, &home);
    std::os::unix::fs::symlink("src", project.path().join("inside-link")).unwrap();
    std::os::unix::fs::symlink(environment.path(), project.path().join("outside-link")).unwrap();
    std::os::unix::fs::symlink("absent", project.path().join("dangling-link")).unwrap();
    for agent in ["claude", "codex"] {
        let allowed = if agent == "claude" { "allow" } else { "" };
        for (source, denied) in [
            ("rg clientId src/missing.ts", false),
            ("rg clientId missing/parents/file.ts", false),
            ("rg --files src missing", false),
            ("rg clientId missing src/app.ts", false),
            ("rg -f missing-patterns src/app.ts", false),
            ("rg --ignore-file missing-ignore clientId src", false),
            ("mkdir missing", false),
            ("ln -s /outside/secret created-link", false),
            ("rg clientId inside-link/missing.ts", false),
            ("rg clientId outside-link/missing.ts", true),
            ("rg clientId dangling-link/child", true),
            ("rg clientId secret/missing.ts", true),
            ("rg clientId missing secret/missing.ts", true),
            ("rg clientId ../missing.ts", true),
            ("rg --hidden clientId future", true),
            ("rg --files future", true),
            ("rg clientId src/app.ts/child", true),
            ("rg --follow clientId missing", true),
            ("rg clientId .shield/state/state.json", true),
            ("mkdir missing && rg --files missing", true),
            (
                "ln -s /outside/secret created-link && rg Acme created-link",
                true,
            ),
        ] {
            let output = cargo_bin_cmd!("shield")
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
                .write_stdin(
                    json!({
                        "hook_event_name":"PreToolUse","tool_name":"Bash",
                        "session_id":"acme-missing-search","tool_use_id":"acme-missing-search",
                        "cwd":project.path(),"tool_input":{"command":source}
                    })
                    .to_string(),
                )
                .assert()
                .success();
            let response: Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
            let decision = response["hookSpecificOutput"]["permissionDecision"]
                .as_str()
                .unwrap_or("");
            assert_eq!(
                decision,
                if denied { "deny" } else { allowed },
                "{agent}: {source}: {response}"
            );
            assert!(!response.to_string().contains("Remove it from the search"));
        }
    }
    assert!(!project.path().join("missing").exists());
    assert!(fs::symlink_metadata(project.path().join("created-link")).is_err());
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
    initialize(&project, &home);
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
                // Unsetting HOME permits the native account-database fallback.
                // Use an absent directory to exercise an unverified boundary.
                command.env("HOME", environment.path().join("missing-home"));
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
