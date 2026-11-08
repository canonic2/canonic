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
            .current_dir(cwd)
            .args(launch)
            .arg("--project")
            .arg(selected)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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
fn bootstrap_real_pinned_launcher_reaches_consent_after_repeated_moves() {
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
    let mut identities = Vec::new();
    for name in ["first", "second"] {
        let target = destinations.path().join(name);
        std::fs::rename(&previous, &target).unwrap();
        let target = target.canonicalize().unwrap();
        let bytes = std::fs::read(target.join(".shield/review/state.json")).unwrap();
        let mut server = Server::start_at(&target, &selected, &["mcp", "serve"]);
        server.initialize_with(&json!({"elicitation":{"form":{}}}));
        server.send(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"status","arguments":{}}}));
        let status = server.receive();
        assert_eq!(
            status["result"]["structuredContent"]["data"]["bootstrap_required"], true,
            "{status}"
        );
        server.send(&json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"project_enroll","arguments":{}}}));
        let form = server.receive();
        assert_eq!(form["method"], "elicitation/create", "{form}");
        assert_eq!(
            std::fs::read(target.join(".shield/review/state.json")).unwrap(),
            bytes
        );
        server.send(
            &json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":"accept","content":{}}}),
        );
        let enrolled = server.receive();
        assert_eq!(enrolled["id"], 3, "{enrolled}");
        assert_eq!(enrolled["result"]["isError"], false, "{enrolled}");
        let id = enrolled["result"]["structuredContent"]["data"]["project_id"].clone();
        assert!(id.is_string(), "{enrolled}");
        assert!(!identities.contains(&id));
        identities.push(id);
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
fn bootstrap_cli_init_requires_confirmation_and_preserves_state_on_decline() {
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
    let declined = super::operator::interactive(&destination, &["init"], "no\n");
    assert_eq!(declined["code"], 130, "{declined}");
    assert_eq!(std::fs::read(&state_path).unwrap(), bytes);
    let accepted = super::operator::interactive(&destination, &["init"], "yes\n");
    assert_eq!(accepted["code"], 0, "{accepted}");
    let after: Value = serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    assert_ne!(after["project_id"], before["project_id"]);
    assert_eq!(
        after["project_root"],
        json!(destination.path().canonicalize().unwrap())
    );
    assert!(after["read_requests"].as_array().unwrap().is_empty());
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
            assert_eq!(
                evaluated["result"]["structuredContent"]["policy_revision"],
                original["policy_revision"]
            );
        }
        server.child.stdin.take();
        assert!(server.child.wait().unwrap().success());
        let target: Value = serde_json::from_slice(
            &std::fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            target["active"]["activation_id"],
            original["active"]["activation_id"]
        );
        assert_eq!(target["active"]["binding"], original["active"]["binding"]);
        assert!(target["active"]["compiled"].is_object());
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
    ] {
        raw.as_object_mut()
            .expect("checkpoint object")
            .remove(field);
    }
    raw["active"]
        .as_object_mut()
        .expect("active checkpoint")
        .remove("compiled");
    raw["history_generation"] = Value::Null;
    raw["authorization_events"]
        .as_array_mut()
        .expect("authorization events")
        .retain(|event| event["kind"] != "compiled_cache_prepared");
    for event in raw["authorization_events"]
        .as_array_mut()
        .expect("authorization events")
    {
        if event["kind"] == "runtime_activated" {
            event["details"]
                .as_object_mut()
                .expect("activation details")
                .remove("compiled_digest");
        }
    }
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
