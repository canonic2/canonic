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
fn native_hook_requires_a_pinned_supported_event() {
    let project = assert_fs::TempDir::new().unwrap();
    for event in [None, Some("UnknownEvent")] {
        let mut command = cargo_bin_cmd!("shield");
        command
            .args(["__internal", "native-hook", "--agent", "codex", "--project"])
            .arg(project.path());
        if let Some(event) = event {
            command.args(["--event", event]);
        }
        command.write_stdin("not JSON").assert().code(2).stdout("");
    }
    project.child(".shield").assert(predicate::path::missing());
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
