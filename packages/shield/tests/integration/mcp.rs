//! Verifies real stdio sessions, stable startup capture, and CLI/core parity.

use assert_cmd::cargo::cargo_bin_cmd;
use assert_fs::prelude::*;
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    process::{Child, Stdio},
    sync::mpsc::{Receiver, sync_channel},
    thread::JoinHandle,
    time::Duration,
};

struct Server {
    child: Child,
    output: Receiver<io::Result<String>>,
    reader: Option<JoinHandle<()>>,
}

pub(super) fn settle_startup(root: &assert_fs::TempDir) {
    cargo_bin_cmd!("shield")
        .args(["__internal", "history-events", "--project"])
        .arg(root.path())
        .env("HOME", root.path())
        .assert()
        .success();
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Server {
    fn start(root: &assert_fs::TempDir) -> Self {
        Self::start_with(root, &["mcp", "serve"])
    }
    fn start_with(root: &assert_fs::TempDir, launch: &[&str]) -> Self {
        Self::start_at(root.path(), root.path(), launch)
    }
    fn start_at(cwd: &std::path::Path, selected: &std::path::Path, launch: &[&str]) -> Self {
        let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin!("shield"));
        let mut child = command
            .env("HOME", super::fixture_home(cwd))
            .env("LOCALAPPDATA", super::fixture_home(cwd))
            .current_dir(cwd)
            .args(launch)
            .arg("--project")
            .arg(selected)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn owned MCP server");
        let mut reader = BufReader::new(child.stdout.take().expect("piped server stdout"));
        let (sender, output) = sync_channel(8);
        let reader = std::thread::spawn(move || {
            loop {
                let mut line = String::new();
                let result = match (&mut reader).take(1_048_577).read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if line.len() <= 1_048_576 => Ok(line),
                    Ok(_) => Err(io::Error::other("owned MCP response exceeds fixture bound")),
                    Err(error) => Err(error),
                };
                let failed = result.is_err();
                if sender.try_send(result).is_err() || failed {
                    break;
                }
            }
        });
        Self {
            child,
            output,
            reader: Some(reader),
        }
    }
    fn send(&mut self, message: &Value) {
        let stdin = self.child.stdin.as_mut().expect("open server stdin");
        writeln!(stdin, "{message}").expect("write MCP message");
        stdin.flush().expect("flush MCP message");
    }
    fn receive(&mut self) -> Value {
        let line = self
            .output
            .recv_timeout(Duration::from_secs(10))
            .expect("owned MCP server response before deadline")
            .expect("read server response");
        serde_json::from_str(&line).expect("JSON-RPC server response")
    }
    fn initialize(&mut self) {
        self.initialize_with(&json!({}));
    }
    fn initialize_with(&mut self, capabilities: &Value) {
        self.send(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-11-25","capabilities":capabilities,"clientInfo":{"name":"Acme","version":"1"}}}));
        assert_eq!(
            self.receive()["result"]["capabilities"],
            json!({"tools":{"listChanged":false},"resources":{},"prompts":{"listChanged":false}})
        );
        self.send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    }
}

#[test]
fn historical_pinned_launcher_reuses_policy_after_repeated_moves() {
    if super::isolated_home() {
        return;
    }
    let original = assert_fs::TempDir::new().unwrap();
    original.child(".shield").create_dir_all().unwrap();
    original
        .child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: []\n")
        .unwrap();
    enroll(&original);
    let selected = original.path().canonicalize().unwrap();
    let destinations = assert_fs::TempDir::new().unwrap();
    let mut previous = selected.clone();
    let mut policy = None;
    for name in ["first", "second"] {
        let target = destinations.path().join(name);
        std::fs::rename(&previous, &target).unwrap();
        let target = target.canonicalize().unwrap();
        let before: Value = serde_json::from_slice(
            &std::fs::read(target.join(".shield/review/state.json")).unwrap(),
        )
        .unwrap();
        let mut server = Server::start_at(&target, &selected, &["mcp", "serve"]);
        server.initialize_with(&json!({"elicitation":{"form":{}}}));
        server.send(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"status","arguments":{}}}));
        let status = server.receive();
        assert_eq!(status["result"]["isError"], false, "{status}");
        let current = &status["result"]["structuredContent"]["data"];
        // The moved files are the same policy.
        let digest = current["policy_files"]["digest"].clone();
        assert!(digest.is_string(), "{status}");
        assert_eq!(*policy.get_or_insert_with(|| digest.clone()), digest);
        assert_eq!(current["pending_approvals"], 0);
        // A moved project is the same project.
        assert_eq!(current["project_id"], before["project_id"], "{status}");
        server.send(&json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"status","arguments":{}}}));
        let continued = server.receive();
        assert_eq!(continued["result"]["isError"], false, "{continued}");
        assert_eq!(
            continued["result"]["structuredContent"]["data"]["project_root"],
            json!(target),
            "{continued}"
        );
        drop(server);
        previous = target;
    }
}

#[cfg(unix)]
#[test]
fn cli_init_in_a_moved_project_reports_it_set_up_without_writing() {
    let original = assert_fs::TempDir::new().unwrap();
    original.child(".shield").create_dir_all().unwrap();
    original
        .child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: []\n")
        .unwrap();
    enroll(&original);
    let destination = assert_fs::TempDir::new().unwrap();
    std::fs::rename(original.path(), destination.path()).unwrap();
    let state_path = destination.path().join(".shield/review/state.json");
    let bytes = std::fs::read(&state_path).unwrap();
    let before: Value = serde_json::from_slice(&bytes).unwrap();
    let init = super::operator::interactive(&destination, &["init"], "no\n");
    assert_eq!(init["code"], 0, "{init}");
    assert!(
        init["stdout"]
            .as_str()
            .unwrap()
            .contains("Shield is set up"),
        "{init}"
    );
    assert_eq!(std::fs::read(&state_path).unwrap(), bytes);
    assert!(before.get("project_root").is_none());
}

#[test]
#[cfg(unix)]
fn a_hand_edit_is_the_next_decision_for_a_running_mcp_server_and_the_cli() {
    let root = assert_fs::TempDir::new().unwrap();
    root.child(".shield").create_dir_all().unwrap();
    root.child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: []\nrules: rules.yml\n")
        .unwrap();
    let rules = |decision: &str| {
        json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],
            "rules":[{"id":"acme","decision":decision,"conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}]})
        .to_string()
    };
    root.child(".shield/rules.yml")
        .write_str(&rules("allow"))
        .unwrap();
    enroll(&root);
    let input = json!({"input":{"kind":"argv","executable":"acme","arguments":["read"],"original_input":{"command":"acme read"}},
        "context":{"cwd":root.path(),"environment":{}}});
    let mut server = Server::start(&root);
    server.initialize();
    let mut evaluate = |id: &str| {
        server.send(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":"policy_evaluate","arguments":input}}));
        let reply = server.receive();
        reply["result"]["structuredContent"]["data"]["decision"].clone()
    };
    assert_eq!(evaluate("before"), "allow");
    // A person edits the rule while the server runs; no command follows.
    root.child(".shield/rules.yml")
        .write_str(&rules("deny"))
        .unwrap();
    assert_eq!(evaluate("after"), "deny");
    server.child.stdin.take();
    assert!(server.child.wait().unwrap().success());
    let request = root.child("request.json");
    request.write_str(&input.to_string()).unwrap();
    let cli = super::operator::query(
        &root,
        &["policy", "evaluate", request.path().to_str().unwrap()],
    );
    assert_eq!(cli["decision"], "deny", "{cli}");
}

/// A folder 0.28.0 left converts when the MCP server first opens it, the
/// server decides by its files, and the CLI reports the conversion after.
#[test]
#[cfg(unix)]
fn a_0_28_0_folder_converts_on_mcp_start_and_the_cli_follows_its_files() {
    use std::os::unix::fs::PermissionsExt;
    let root = assert_fs::TempDir::new().unwrap();
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/checkpoints/0.28.0");
    root.copy_from(&fixture, &["**/*"]).unwrap();
    std::fs::rename(root.path().join("shield"), root.path().join(".shield")).unwrap();
    root.child(".shield/.gitignore")
        .write_str("config.local.yml\nrules.local.yml\nshield.local.lock\nreview/\ncache/\n")
        .unwrap();
    let review = root.path().join(".shield/review");
    std::fs::set_permissions(&review, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(
        review.join("state.json"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let mut server = Server::start(&root);
    server.initialize();
    let mut evaluate = |id: &str, argument: &str| {
        let input = json!({"input":{"kind":"argv","executable":"acme","arguments":[argument],
            "original_input":{"command":format!("acme {argument}")}},
            "context":{"cwd":root.path(),"environment":{}}});
        server.send(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":"policy_evaluate","arguments":input}}));
        server.receive()["result"]["structuredContent"]["data"]["decision"].clone()
    };
    assert_eq!(evaluate("read", "read"), "allow");
    assert_eq!(evaluate("remove", "remove"), "deny");
    server.child.stdin.take();
    assert!(server.child.wait().unwrap().success());
    let local = std::fs::read_to_string(root.path().join(".shield/config.local.yml")).unwrap();
    assert!(local.contains("paused"), "{local}");
    let status = super::operator::query(&root, &["status"]);
    assert_eq!(status["checkpoint_upgrade_pending"], false, "{status}");
    assert_eq!(status["upgrade"]["policy"]["same"], true, "{status}");
}

#[test]
fn historical_mcp_launch_forms_continue_to_serve_project_tools() {
    let root = assert_fs::TempDir::new().unwrap();
    root.child(".shield").create_dir_all().unwrap();
    root.child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: []\nrules: rules.yml\n")
        .unwrap();
    root.child(".shield/rules.yml").write_str(&json!({
        "schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],
        "rules":[
            {"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}},
            {"id":"destroy","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["remove"]}}},
            {"id":"publish","decision":"ask","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["publish"]}}}
        ]
    }).to_string()).unwrap();
    enroll(&root);
    for launch in [
        vec!["__internal", "mcp", "--agent", "claude-code"],
        vec!["__internal", "mcp", "--agent", "codex"],
        vec!["mcp", "serve", "--agent", "claude-code"],
    ] {
        let original = historical_checkpoint(&root);
        let path = root.path().join(".shield/review/state.json");
        let original_bytes = std::fs::read(&path).unwrap();
        let mut server = Server::start_with(&root, &launch);
        server.initialize();
        server.send(&json!({"jsonrpc":"2.0","id":"status","method":"tools/call",
            "params":{"name":"status","arguments":{}}}));
        let response = server.receive();
        assert_eq!(
            response["result"]["isError"], false,
            "{launch:?}: {response}"
        );
        assert_eq!(
            response["result"]["structuredContent"]["data"]["checkpoint_upgrade_pending"],
            true
        );
        assert!(
            std::fs::read(&path).unwrap() == original_bytes,
            "status must not publish checkpoint conversion"
        );
        // Every evaluation decides by the same policy files.
        let mut revision = None;
        for (operation, decision) in [("read", "allow"), ("remove", "deny"), ("publish", "ask")] {
            server.send(&json!({"jsonrpc":"2.0","id":operation,"method":"tools/call",
                "params":{"name":"policy_evaluate","arguments":{
                    "input":{"kind":"argv","executable":"acme","arguments":[operation],"original_input":{"command":format!("acme {operation}")}},
                    "context":{"cwd":root.path(),"environment":{}}
                }}}));
            let evaluated = server.receive();
            assert_eq!(
                evaluated["result"]["isError"], false,
                "{launch:?}: {evaluated}"
            );
            assert_eq!(
                evaluated["result"]["structuredContent"]["data"]["decision"], decision,
                "{launch:?}: {evaluated}"
            );
            let current = evaluated["result"]["structuredContent"]["policy_revision"].clone();
            assert!(current.is_string(), "{evaluated}");
            assert_eq!(revision.get_or_insert_with(|| current.clone()), &current);
        }
        server.child.stdin.take();
        assert!(server.child.wait().unwrap().success());
        let target: Value = serde_json::from_slice(
            &std::fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        )
        .unwrap();
        // Conversion drops the recorded activation and the folder's root; the
        // files decide.
        assert!(target.get("active").is_none(), "{target}");
        assert!(target.get("project_root").is_none());
        assert_eq!(target["policy_revision"], original["policy_revision"]);
    }
}

#[test]
fn historical_cli_inspection_is_read_only_without_manual_setup() {
    let root = assert_fs::TempDir::new().unwrap();
    enroll(&root);
    let original = historical_checkpoint(&root);
    let path = root.path().join(".shield/review/state.json");
    let original_bytes = std::fs::read(&path).unwrap();
    let output = cargo_bin_cmd!("shield")
        .args(["status", "--json", "--project"])
        .arg(root.path())
        .env("HOME", root.path())
        .assert()
        .success();
    let response: Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
    assert!(response["error"].is_null(), "{response}");
    assert_eq!(response["checkpoint_upgrade_pending"], true);
    let target: Value = serde_json::from_slice(
        &std::fs::read(root.path().join(".shield/review/state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        target["active"]["activation_id"],
        original["active"]["activation_id"]
    );
    assert_eq!(target["active"]["binding"], original["active"]["binding"]);
    assert!(
        std::fs::read(path).unwrap() == original_bytes,
        "status must not publish checkpoint conversion"
    );
}

#[test]
fn historical_mcp_refuses_future_state_with_format_details_and_preserves_bytes() {
    let root = assert_fs::TempDir::new().unwrap();
    enroll(&root);
    let path = root.path().join(".shield/review/state.json");
    let mut future: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    future["schema_version"] = json!(2);
    let bytes = serde_json::to_vec(&future).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let output = cargo_bin_cmd!("shield")
        .args(["__internal", "mcp", "--project"])
        .arg(root.path())
        .assert()
        .code(1);
    assert!(output.get_output().stdout.is_empty());
    let message = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(message.contains("observed schema 2"), "{message}");
    assert!(message.contains("supported schema 1"), "{message}");
    assert!(message.contains("state.json"), "{message}");
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

/// Synthetic optional-cache checkpoint for isolating outer-format conversion.
/// Source references and activation evidence come from current test enrollment.
pub(super) fn historical_checkpoint(root: &assert_fs::TempDir) -> Value {
    let path = root.path().join(".shield/review/state.json");
    let mut raw: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read enrolled checkpoint"))
            .expect("decode enrolled checkpoint");
    for field in [
        "hook_requests",
        "session_grants",
        "policy_drafts",
        "dismissed_reviews",
        "enforcement",
        "history_owner",
        "configuration_changes",
        "terminal_receipts",
        "followed_files",
    ] {
        raw.as_object_mut()
            .expect("checkpoint object")
            .remove(field);
    }
    // The earlier format always recorded its absolute root and a root-bound
    // activation, which the reader accepts and ignores.
    raw["project_root"] = json!(
        root.path()
            .canonicalize()
            .expect("fixture root")
            .to_str()
            .expect("UTF-8 fixture root")
    );
    let binding = super::operator::fixture_digest(
        json!([
            raw["project_id"],
            raw["project_root"],
            "acme-original-activation",
            null,
            raw["policy_revision"],
            "acme-candidate-digest"
        ])
        .to_string()
        .as_bytes(),
    );
    raw["active"] = json!({"activation_id":"acme-original-activation",
        "previous_activation_id":null,"policy_revision":raw["policy_revision"],
        "candidate_digest":"acme-candidate-digest","candidates":raw["candidates"],
        "binding":binding});
    raw["history_generation"] = Value::Null;
    raw["authorization_events"]
        .as_array_mut()
        .expect("authorization events")
        .push(
            json!({"schema_version":1,"event_id":"acme-original-activation-event",
            "kind":"runtime_activated","links":{"activation_id":"acme-original-activation"},
            "details":{"binding":binding}}),
        );
    std::fs::write(path, raw.to_string()).expect("write historical checkpoint");
    raw
}

fn enroll(root: &assert_fs::TempDir) {
    #[cfg(windows)]
    super::windows::enroll(root.path(), false);
    #[cfg(unix)]
    assert_eq!(
        super::operator::interactive(root, &["init"], "yes\n")["code"],
        0
    );
}
#[test]
fn live_session_retains_diagnostic_handles_without_execution() {
    let root = assert_fs::TempDir::new().unwrap();
    enroll(&root);
    let mut server = Server::start(&root);
    server.initialize();
    let args = json!({"input":{"kind":"argv","executable":"acme","arguments":["write","$(touch write)"],"original_input":{"command":"acme write"}},"context":{"cwd":root.path(),"environment":{}}});
    let mut call = json!({"jsonrpc":"2.0","id":"preview","method":"tools/call","params":{"name":"policy_evaluate","arguments":args}});
    server.send(&call);
    let first = server.receive();
    assert_eq!(
        first["result"]["structuredContent"]["data"]["decision"],
        "ask"
    );
    assert_eq!(first["result"]["isError"], false);
    call["params"]["arguments"]["candidate_id"] =
        first["result"]["structuredContent"]["data"]["candidate_id"].clone();
    root.child(".shield/config.yml")
        .write_str("PRIVATE_SECRET: [")
        .unwrap();
    server.send(&call);
    assert_eq!(server.receive(), first);
    root.child("write").assert(predicates::path::missing());
    server.child.stdin.take();
    assert!(server.child.wait().unwrap().success());
}

#[test]
fn startup_and_frame_failures_are_sanitized_and_never_emit_non_protocol_stdout() {
    let root = assert_fs::TempDir::new().unwrap();
    let output = cargo_bin_cmd!("shield")
        .args(["mcp", "serve", "--project"])
        .arg(root.path())
        .assert()
        .code(1);
    assert_eq!(output.get_output().stdout, [] as [u8; 0]);
    assert!(!String::from_utf8_lossy(&output.get_output().stderr).contains("PRIVATE_SECRET"));
    enroll(&root);
    let output = cargo_bin_cmd!("shield")
        .args(["mcp", "serve", "--project"])
        .arg(root.path())
        .write_stdin("{PRIVATE_SECRET")
        .assert()
        .code(1);
    let report: Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
    assert_eq!(report["error"]["code"], -32700);
    assert!(!report.to_string().contains("PRIVATE_SECRET"));
}

#[test]
fn copied_executable_serves_skill_bytes_without_a_project_or_source_files() {
    let root = assert_fs::TempDir::new().unwrap();
    let executable = root.child(if cfg!(windows) {
        "shield.exe"
    } else {
        "shield"
    });
    std::fs::copy(assert_cmd::cargo::cargo_bin!("shield"), executable.path()).unwrap();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"resources/read","params":{
        "uri":"skill://shield/policy-review/SKILL.md","_meta":{
            "io.modelcontextprotocol/protocolVersion":"2026-07-28",
            "io.modelcontextprotocol/clientCapabilities":{}
        }
    }});
    let result = assert_cmd::Command::new(executable.path())
        .current_dir(root.path())
        .args(["mcp", "serve", "--skills-only"])
        .timeout(Duration::from_secs(10))
        .write_stdin(format!("{request}\n"))
        .assert()
        .success();
    let response: Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(
        response["result"]["contents"][0]["text"],
        include_str!("../../skills/policy-review/SKILL.md")
    );
    root.child(".shield").assert(predicates::path::missing());
}

#[test]
fn deadline_limits_are_rejected_before_startup_or_side_effects() {
    for deadline in ["0", "60001", "not-a-number"] {
        let output = cargo_bin_cmd!("shield")
            .args([
                "mcp",
                "serve",
                "--project",
                "/workspace/acme",
                "--request-timeout-ms",
                deadline,
            ])
            .assert()
            .code(2);
        assert_eq!(output.get_output().stdout, [] as [u8; 0]);
    }
}
