//! Published startup, finite-stdio replies and managed Android control without hardware.
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

fn binary() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_testrun").into()
}

#[test]
fn docs_cli_queries_and_continues_without_runtime_storage() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("unusable-data-root");
    std::fs::write(&data, "runtime must not open this as a directory").unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(binary())
            .current_dir(root.path())
            .env("TESTRUN_DATA_ROOT", &data)
            .env("TESTRUN_ADB", root.path().join("missing-adb"))
            .args(args)
            .output()
            .unwrap()
    };
    let first = run(&["--json", "docs", "list", "--limit", "1"]);
    assert!(first.status.success(), "{first:?}");
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    let cursor = first["nextCursor"].as_str().unwrap();
    let next = run(&["--json", "docs", "list", "--limit", "1", "--cursor", cursor]);
    assert!(next.status.success());
    let next: Value = serde_json::from_slice(&next.stdout).unwrap();
    assert_ne!(first["documents"], next["documents"]);
    for args in [
        vec!["--json", "docs", "read", "cli.md", "--limit", "1"],
        vec!["--json", "docs", "search", "Testrun", "--path", "cli.md"],
    ] {
        let output = run(&args);
        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    }
    let human = run(&["docs", "read", "cli.md", "--limit", "1"]);
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.starts_with("# CLI output\n"));
    assert!(human.contains("Next: testrun docs read"));
    for (args, code) in [
        (vec!["docs", "read", "missing.md"], "NOT_FOUND"),
        (vec!["docs", "list", "--limit", "0"], "INVALID_ARGUMENT"),
        (
            vec!["docs", "list", "--cursor", "invalid"],
            "invalid_cursor",
        ),
    ] {
        let mut args = args;
        args.push("--json");
        let output = run(&args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let value: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["error"]["code"], code);
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(
        std::fs::read_to_string(data).unwrap(),
        "runtime must not open this as a directory"
    );
}

#[test]
fn docs_cli_closed_pipe_exits_quietly() {
    let root = tempfile::tempdir().unwrap();
    for json in [false, true] {
        let mut command = std::process::Command::new(binary());
        command
            .current_dir(root.path())
            .args(["docs", "read", "cli.md"]);
        if json {
            command.arg("--json");
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
    }
}

#[tokio::test]
async fn bundled_docs_work_offline_with_project_replacements_and_unusable_runtime_storage() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let home = root.path().join("home");
    std::fs::create_dir_all(project.join("docs")).unwrap();
    std::fs::create_dir(&home).unwrap();
    std::fs::write(project.join("docs/mcp.md"), "project replacement").unwrap();
    let data = root.path().join("unusable-data-root");
    std::fs::write(&data, "runtime must not open this as a directory").unwrap();
    let mut child = Command::new(binary())
        .args(["mcp", "serve"])
        .current_dir(&project)
        .env("HOME", &home)
        .env("PATH", root.path().join("missing-programs"))
        .env("TESTRUN_DATA_ROOT", &data)
        .env("TESTRUN_ADB", root.path().join("missing-adb"))
        .env_remove("CODEX_THREAD_ID")
        .env_remove("TESTRUN_TASK_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for request in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme offline docs","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"docs_list","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"docs_search","arguments":{"query":"stdio","path":"mcp.md"}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"docs_read","arguments":{"path":"mcp.md","limit":1}}}),
    ] {
        let mut frame = serde_json::to_vec(&request).unwrap();
        frame.push(b'\n');
        input.write_all(&frame).await.unwrap();
    }
    drop(input);
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    for id in 2..=4 {
        let response = responses
            .iter()
            .find(|response| response["id"] == id)
            .unwrap();
        assert_eq!(
            response["result"]["structuredContent"]["ok"], true,
            "{response}"
        );
        assert_eq!(
            response["result"]["structuredContent"]["data"]["version"],
            env!("CARGO_PKG_VERSION")
        );
    }
    let read = responses
        .iter()
        .find(|response| response["id"] == 4)
        .unwrap();
    assert_eq!(
        read["result"]["structuredContent"]["data"]["content"],
        "# MCP transport\n"
    );
    assert_eq!(
        std::fs::read_to_string(&data).unwrap(),
        "runtime must not open this as a directory"
    );
    assert_eq!(std::fs::read_dir(&home).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(&project).unwrap().count(), 1);
}

#[tokio::test]
async fn mcp_setup_json_preview_and_unattended_refusal_preserve_files() {
    // Home and project are siblings: setup refuses a project containing home.
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let project = root.path().join("project");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&project).unwrap();
    for dry_run in [true, false] {
        let mut command = Command::new(binary());
        command
            .current_dir(&project)
            .env("HOME", &home)
            .args(["mcp", "setup", "--agent", "qwen", "--json"]);
        if dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().await.unwrap();
        assert_eq!(
            output.status.success(),
            dry_run,
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if dry_run {
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["scope"], "project");
            assert_eq!(
                value["registrations"][0]["entry"]["args"],
                json!(["mcp", "serve"])
            );
        } else {
            assert!(output.stdout.is_empty());
            assert_eq!(
                serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
                "AGENT_SETUP_CONFIRMATION_REQUIRED"
            );
        }
        assert!(!project.join(".qwen").exists());
    }
}

#[tokio::test]
async fn mcp_setup_rejects_a_target_selector_without_writes() {
    for selector in [vec!["--target", "device:Acme"]] {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let project = root.path().join("project");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&project).unwrap();
        let output = Command::new(binary())
            .current_dir(&project)
            .env("HOME", &home)
            .args(&selector)
            .args(["mcp", "setup", "--agent", "qwen", "--dry-run", "--json"])
            .output()
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "selector: {selector:?}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        let value: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["error"]["code"], "INVALID_ARGUMENT");
        assert!(!project.join(".qwen").exists());
        assert!(!home.join(".qwen").exists());
    }
}

#[cfg(unix)]
#[test]
fn mcp_setup_terminal_confirmation_install_repeat_and_remove() {
    assert!(
        std::process::Command::new("python3")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/agent_setup_terminal.py"
            ))
            .arg(binary())
            .arg("testrun")
            .status()
            .unwrap()
            .success()
    );
}

#[cfg(unix)]
#[test]
fn cli_output_profiles_and_json_remain_distinct_in_a_real_terminal() {
    let status = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/cli_output.py"
        ))
        .arg(binary())
        .status()
        .unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn cli_presentation_defaults_to_human_and_json_is_explicit() {
    let human = Command::new(binary())
        .args(["mcp", "serve", "--check"])
        .output()
        .await
        .unwrap();
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("MCP registration"));
    assert!(!human.stdout.contains(&0x1b));
    assert!(serde_json::from_slice::<Value>(&human.stdout).is_err());
    for args in [
        vec!["--json", "mcp", "serve", "--check"],
        vec!["mcp", "serve", "--check", "--json"],
    ] {
        let output = Command::new(binary()).args(args).output().await.unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], true);
        assert!(!value["tools"].as_array().unwrap().is_empty());
    }
    for machine in [false, true] {
        let mut command = Command::new(binary());
        if machine {
            command.arg("--json");
        }
        let output = command
            .args(["browser", "list", "--target", "android:Acme"])
            .output()
            .await
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        if machine {
            let value: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(value["error"]["code"], "INVALID_ARGUMENT");
        } else {
            assert!(String::from_utf8_lossy(&output.stderr).contains("browser: --target"));
        }
    }
}

#[tokio::test]
async fn cli_parse_errors_are_json_and_a_closed_output_pipe_is_quiet() {
    let output = Command::new(binary())
        .args(["--json", "--unknown-acme-flag"])
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "INVALID_ARGUMENT"
    );
    assert!(output.stdout.is_empty());
    let mut child = Command::new(binary())
        .args(["--json", "mcp", "serve", "--check"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn hidden_protocol_command_failures_are_json_without_the_flag() {
    let root = tempfile::tempdir().unwrap();
    let endpoint = root.path().join("acme-missing/control.sock");
    let output = Command::new(binary())
        .arg("close")
        .arg("--endpoint")
        .arg(&endpoint)
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap_or_else(|_| {
        panic!("{}", String::from_utf8_lossy(&output.stderr));
    });
    assert!(error["error"]["code"].is_string());
}

#[tokio::test]
async fn finite_stdio_drains_initialize_tools_and_ping_before_exit() {
    for _ in 0..8 {
        let mut child = Command::new(binary())
            .arg("--json")
            .args(["mcp", "serve"])
            .env_remove("CODEX_THREAD_ID")
            .env_remove("TESTRUN_TASK_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let requests = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme EOF fixture","version":"1"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"ping"}),
        ];
        let mut bytes = Vec::new();
        for request in requests {
            bytes.extend(serde_json::to_vec(&request).unwrap());
            bytes.push(b'\n');
        }
        let mut input = child.stdin.take().unwrap();
        input.write_all(&bytes).await.unwrap();
        drop(input);
        let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let responses: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(responses.len(), 3, "{responses:?}");
        for id in 1..=3 {
            assert!(
                responses
                    .iter()
                    .any(|response| response["id"] == id && response.get("result").is_some()),
                "Missing response {id}: {responses:?}"
            );
        }
        let initialized = responses
            .iter()
            .find(|response| response["id"] == 1)
            .unwrap();
        let listed = responses
            .iter()
            .find(|response| response["id"] == 2)
            .unwrap();
        let catalog = testrun::protocol::tools();
        assert_eq!(
            listed["result"],
            catalog.list_page(&json!({}), "2025-11-25").unwrap()
        );
        let definitions = catalog.listing();
        let names: std::collections::BTreeSet<_> = definitions["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        for word in initialized["result"]["instructions"]
            .as_str()
            .unwrap()
            .split_whitespace()
        {
            // Snake_case words name tools except for the documented cursor
            // error code. The full catalog can span several tools/list pages.
            let name = word
                .trim_end_matches(['.', ',', ';', ')'])
                .trim_start_matches('(');
            if name.contains('_') && name != "invalid_cursor" {
                assert!(
                    names.contains(name),
                    "Instructions advertise unregistered tool {name}"
                );
            }
        }
    }
}
#[tokio::test]
async fn serve_check_reports_the_real_catalog_without_opening_a_device() {
    let output = Command::new(binary())
        .arg("--json")
        .args(["mcp", "serve", "--check"])
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    let names: std::collections::BTreeSet<_> = value["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    for name in [
        "doctor",
        "docs_list",
        "docs_search",
        "docs_read",
        "developer_mode_reveal",
        "developer_support_install",
        "connect",
        "act",
        "observe",
        "status",
        "disconnect",
        "recording_start",
        "recording_stop",
        "recording_status",
        "preview_start",
        "network_set",
        "media_send",
    ] {
        assert!(names.contains(name), "Missing {name}");
    }
    for name in [
        "batch",
        "open_url",
        "tap",
        "screenshot",
        "run.begin",
        "session_open",
        "command_status",
        "host_info",
    ] {
        assert!(!names.contains(name), "Unexpected {name}");
    }
    assert!(names.iter().all(|name| !name.contains('.')), "{names:?}");
    let act = value["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "act")
        .unwrap();
    let actions = &act["inputSchema"]["anyOf"][0]["properties"]["actions"]["items"]["properties"]["action"]
        ["enum"];
    for action in ["tap", "delay", "open_url"] {
        assert!(
            actions.as_array().unwrap().contains(&json!(action)),
            "{actions}"
        );
    }
    assert_eq!(names.len(), value["tools"].as_array().unwrap().len());
}

#[cfg(unix)]
struct AndroidFixture {
    directory: crate::Workspace,
    adb: std::path::PathBuf,
    task: String,
}
#[cfg(unix)]
impl AndroidFixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory = crate::Workspace::new();
        image::RgbImage::new(100, 200)
            .save(directory.path().join("screen.png"))
            .unwrap();
        let adb = directory.path().join("adb");
        std::fs::write(&adb,"#!/bin/sh\nset -eu\ncd \"$(dirname \"$0\")\"\ncase \"$1\" in\ndevices) printf 'List of devices attached\\nAcme-managed device model:Acme transport_id:7\\n' ;;\n-s) case \"$3\" in\nexec-out) cat screen.png ;;\nshell) printf '%s\\n' \"$4\" >> input.log ;;\n*) exit 2 ;;\nesac ;;\n*) exit 3 ;;\nesac\n").unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            directory,
            adb,
            task: format!("testrun-android-fixture-{}", uuid::Uuid::new_v4()),
        }
    }
    async fn call(&self, args: &[&str]) -> Value {
        let output = tokio::time::timeout(
            Duration::from_secs(30),
            Command::new(binary())
                .arg("--json")
                .args(args)
                .env("TESTRUN_DATA_ROOT", self.directory.data_root())
                .env_remove("CODEX_THREAD_ID")
                .env("TESTRUN_TASK_ID", &self.task)
                .env("TESTRUN_ADB", &self.adb)
                .env("ANDROID_AVD_HOME", crate::NO_AVDS)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
#[cfg(unix)]
impl Drop for AndroidFixture {
    fn drop(&mut self) {
        let _ = std::process::Command::new(binary())
            .arg("--json")
            .arg("disconnect")
            .env("TESTRUN_DATA_ROOT", self.directory.data_root())
            .env_remove("CODEX_THREAD_ID")
            .env("TESTRUN_TASK_ID", &self.task)
            .env("TESTRUN_ADB", &self.adb)
            .env("ANDROID_AVD_HOME", crate::NO_AVDS)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
#[cfg(unix)]
#[tokio::test]
async fn android_managed_connect_act_observe_status_disconnect_uses_only_its_selected_target() {
    let fixture = AndroidFixture::new();
    let connected = fixture
        .call(&["connect", "--target", "android:Acme-managed"])
        .await;
    assert_eq!(connected["state"], "ready", "{connected}");
    assert_eq!(connected["targetId"], "android:Acme-managed");
    // Status naming a target the session does not hold is refused before any input.
    let retargeted = Command::new(binary())
        .arg("--json")
        .args(["status", "--target", "android:Acme-other"])
        .env("TESTRUN_DATA_ROOT", fixture.directory.data_root())
        .env_remove("CODEX_THREAD_ID")
        .env("TESTRUN_TASK_ID", &fixture.task)
        .env("TESTRUN_ADB", &fixture.adb)
        .env("ANDROID_AVD_HOME", crate::NO_AVDS)
        .output()
        .await
        .unwrap();
    assert!(!retargeted.status.success());
    assert!(String::from_utf8_lossy(&retargeted.stderr).contains("NOT_CONNECTED"));
    // Naming the held target reports it.
    let named = fixture
        .call(&["status", "--target", "android:Acme-managed"])
        .await;
    assert_eq!(named["targetId"], "android:Acme-managed", "{named}");
    let observed = fixture.call(&["observe"]).await;
    assert!(
        observed["observation"]["attachment"]["path"].is_string(),
        "{observed}"
    );
    let acted = fixture
        .call(&["act", r#"{"action":"tap","point":{"x":0.5,"y":0.5}}"#])
        .await;
    assert_eq!(acted["state"], "ready", "{acted}");
    let status = fixture
        .call(&["status", "--target", "android:Acme-managed"])
        .await;
    assert_eq!(status["targetId"], "android:Acme-managed");
    let log = std::fs::read_to_string(fixture.directory.path().join("input.log")).unwrap();
    assert_eq!(
        log.lines()
            .filter(|line| line.contains("'input' 'tap'"))
            .count(),
        1,
        "{log}"
    );
    let closed = fixture.call(&["disconnect"]).await;
    assert_eq!(closed["closed"], true, "{closed}");
}
