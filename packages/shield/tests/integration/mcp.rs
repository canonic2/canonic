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
        let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin!("shield"));
        let mut child = command
            .args(["mcp", "serve", "--project"])
            .arg(root.path())
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
        self.send(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme","version":"1"}}}));
        assert_eq!(
            self.receive()["result"]["capabilities"],
            json!({"tools":{"listChanged":false},"resources":{},"prompts":{"listChanged":false}})
        );
        self.send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    }
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
