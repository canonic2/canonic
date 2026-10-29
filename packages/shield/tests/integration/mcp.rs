//! Verifies real stdio sessions, stable startup capture, and CLI/core parity.

use assert_cmd::cargo::cargo_bin_cmd;
use assert_fs::prelude::*;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdout, Stdio},
};

struct Server {
    child: Child,
    output: BufReader<ChildStdout>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
        let output = BufReader::new(child.stdout.take().expect("piped server stdout"));
        Self { child, output }
    }
    fn send(&mut self, message: &Value) {
        let stdin = self.child.stdin.as_mut().expect("open server stdin");
        writeln!(stdin, "{message}").expect("write MCP message");
        stdin.flush().expect("flush MCP message");
    }
    fn receive(&mut self) -> Value {
        let mut line = String::new();
        assert!(
            self.output
                .read_line(&mut line)
                .expect("read server response")
                > 0
        );
        serde_json::from_str(&line).expect("JSON-RPC server response")
    }
    fn initialize(&mut self) {
        self.send(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme","version":"1"}}}));
        assert_eq!(
            self.receive()["result"]["capabilities"],
            json!({"tools":{"listChanged":false}})
        );
        self.send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    }
}

#[cfg(unix)]
fn enroll(root: &assert_fs::TempDir) {
    assert_eq!(
        super::operator::interactive(root, &["init"], "yes\n")["code"],
        0
    );
}
#[test]
#[cfg(unix)]
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
#[cfg(unix)]
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
