//! Checks supported commands and prevents unimplemented commands from executing.

use assert_cmd::cargo::cargo_bin_cmd;
use assert_fs::prelude::*;
use predicates::prelude::*;

#[test]
fn help_identifies_shield() {
    cargo_bin_cmd!("shield")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: shield"));
}

#[test]
fn version_matches_package() {
    cargo_bin_cmd!("shield")
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("shield {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn native_hook_rejects_an_unknown_pinned_event() {
    let project = assert_fs::TempDir::new().unwrap();
    cargo_bin_cmd!("shield")
        .args([
            "__internal",
            "native-hook",
            "--agent",
            "codex",
            "--event",
            "UnknownEvent",
            "--project",
        ])
        .arg(project.path())
        .write_stdin("not JSON")
        .assert()
        .code(2)
        .stdout("");
    project.child(".shield").assert(predicate::path::missing());
}

#[test]
fn relative_native_hook_project_is_anchored_before_vendor_cwd() {
    use serde_json::{Value, json};
    let launch = assert_fs::TempDir::new().unwrap();
    let other = assert_fs::TempDir::new().unwrap();
    other.child(".shield").create_dir_all().unwrap();
    other
        .child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: []\nrules: rules.yml\n")
        .unwrap();
    other.child(".shield/rules.yml").write_str(&json!({
        "schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],
        "rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"any"}}}]
    }).to_string()).unwrap();
    assert_eq!(
        super::operator::interactive(&other, &["init"], "yes\n")["code"],
        0
    );
    let host = json!({"hook_event_name":"PreToolUse","tool_name":"Bash","session_id":"acme-relative","tool_use_id":"acme-relative","cwd":other.path(),"tool_input":{"command":"git status"}});
    for (cwd, expected) in [(other.path(), "allow"), (launch.path(), "deny")] {
        let output = cargo_bin_cmd!("shield")
            .current_dir(cwd)
            .env("HOME", other.path())
            .args([
                "--no-debug-log",
                "__internal",
                "native-hook",
                "--project",
                ".",
                "--agent",
                "claude",
                "--event",
                "PreToolUse",
            ])
            .write_stdin(host.to_string())
            .assert()
            .success();
        let value: Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecision"], expected,
            "{value}"
        );
    }
}

#[test]
fn historical_hooks_continue_without_an_event_argument_and_fail_closed() {
    let project = assert_fs::TempDir::new().unwrap();
    for agent in ["claude", "codex"] {
        for (input, completion) in [
            (r#"{"hook_event_name":"PostToolUse"}"#, true),
            (r#"{"hook_event_name":"PostToolUseFailure"}"#, true),
            (r#"{"hook_event_name":"PreToolUse"}"#, false),
            ("not JSON", false),
            (r#"{"hook_event_name":"UnknownEvent"}"#, false),
            (
                r#"{"hook_event_name":"PreToolUse","hook_event_name":"PostToolUse"}"#,
                false,
            ),
        ] {
            let output = cargo_bin_cmd!("shield")
                .args(["__internal", "native-hook", "--agent", agent, "--project"])
                .arg(project.path())
                .write_stdin(input)
                .assert()
                .success();
            let value: serde_json::Value =
                serde_json::from_slice(&output.get_output().stdout).unwrap();
            if completion && !(agent == "codex" && input.contains("PostToolUseFailure")) {
                assert_eq!(value, serde_json::json!({}));
            } else {
                assert_eq!(
                    value["hookSpecificOutput"]["permissionDecision"], "deny",
                    "{agent}: {value}"
                );
            }
        }
        let output = cargo_bin_cmd!("shield")
            .args(["__internal", "native-hook", "--agent", agent, "--project"])
            .arg(project.path())
            .write_stdin(format!(
                r#"{{"hook_event_name":"PostToolUse","padding":"{}"}}"#,
                "x".repeat(8 * 1024 * 1024)
            ))
            .assert()
            .success();
        let value: serde_json::Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    }
    project.child(".shield").assert(predicate::path::missing());
}

#[test]
fn historical_hooks_preserve_active_policy_decisions() {
    use serde_json::{Value, json};
    let project = assert_fs::TempDir::new().unwrap();
    project.child(".shield").create_dir_all().unwrap();
    project
        .child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: []\nrules: rules.yml\n")
        .unwrap();
    project.child(".shield/rules.yml").write_str(&json!({
        "schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],
        "rules":[
            {"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["status"]}}},
            {"id":"destroy","decision":"deny","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["reset","--hard"]}}},
            {"id":"publish","decision":"ask","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["push"]}}}
        ]
    }).to_string()).unwrap();
    assert_eq!(
        super::operator::interactive(&project, &["init"], "yes\n")["code"],
        0
    );
    for agent in ["claude", "codex"] {
        for (source, decision) in [
            ("git status", "allow"),
            ("git reset --hard", "deny"),
            ("git push", "ask"),
        ] {
            let host = json!({"hook_event_name":"PreToolUse","tool_name":"Bash",
                "tool_use_id":"acme-historical","session_id":"acme-historical","cwd":project.path(),
                "tool_input":{"command":source}});
            for pinned in [false, true] {
                if !pinned {
                    super::mcp::historical_checkpoint(&project);
                }
                // 27343ffa installed the same native command without --event.
                let mut command = cargo_bin_cmd!("shield");
                command
                    .args(["__internal", "native-hook", "--agent", agent, "--project"])
                    .arg(project.path())
                    .env("HOME", project.path());
                if pinned {
                    command.args(["--event", "PreToolUse"]);
                }
                let output = command.write_stdin(host.to_string()).assert().success();
                let response: Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
                let expected = match (agent, decision) {
                    ("codex", "allow") => None,
                    ("codex", "ask") => Some("deny"),
                    _ => Some(decision),
                };
                assert_eq!(
                    response["hookSpecificOutput"]["permissionDecision"].as_str(),
                    expected,
                    "{agent}, {source}, pinned={pinned}: {response}"
                );
            }
        }
    }
}

#[test]
fn pinned_permission_event_blocks_malformed_and_oversized_subprocess_input() {
    let project = assert_fs::TempDir::new().unwrap();
    // A supplied authorization event that disagrees with the pinned one is
    // denied in the supplied event's format, so that host honors it.
    let output = cargo_bin_cmd!("shield")
        .args([
            "__internal",
            "native-hook",
            "--agent",
            "codex",
            "--event",
            "PermissionRequest",
            "--project",
        ])
        .arg(project.path())
        .write_stdin(r#"{"hook_event_name":"PreToolUse"}"#)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
    for input in [
        "not JSON".to_owned(),
        format!(
            r#"{{"hook_event_name":"PermissionRequest","padding":"{}"}}"#,
            "x".repeat(8 * 1024 * 1024)
        ),
    ] {
        let output = cargo_bin_cmd!("shield")
            .args([
                "__internal",
                "native-hook",
                "--agent",
                "codex",
                "--event",
                "PermissionRequest",
                "--project",
            ])
            .arg(project.path())
            .write_stdin(input)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            value["hookSpecificOutput"]["hookEventName"],
            "PermissionRequest"
        );
        assert_eq!(value["hookSpecificOutput"]["decision"]["behavior"], "deny");
    }
}

#[test]
fn unsupported_run_is_rejected_without_changing_project() {
    let project = assert_fs::TempDir::new().unwrap();
    let marker = project.child("marker.txt");
    marker.write_str("preserve me").unwrap();
    cargo_bin_cmd!("shield")
        .current_dir(project.path())
        .args(["__internal", "run", "--", "touch", "must-not-exist"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("blocked execution"));
    marker.assert("preserve me");
    project
        .child("must-not-exist")
        .assert(predicate::path::missing());
    project.child(".shield").assert(predicate::path::missing());
}

#[test]
fn missing_command_requests_a_command() {
    cargo_bin_cmd!("shield")
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("Usage: shield"));
}
