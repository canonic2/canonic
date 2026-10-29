//! Debug logs stay separate from real native-hook protocol output and never retain payloads.
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value, json};

#[test]
fn native_hook_logs_oversized_input_and_response_without_changing_protocol() {
    let root = assert_fs::TempDir::new().unwrap();
    let logs = root.path().join("logs");
    let payload = json!({"hook_event_name":"PostToolUse","tool_name":"mcp__shield__history_list","tool_response":"private-result".repeat(600_000)}).to_string();
    let output = cargo_bin_cmd!("shield")
        .args([
            "--debug-log-dir",
            logs.to_str().unwrap(),
            "__internal",
            "native-hook",
            "--project",
            root.path().to_str().unwrap(),
            "--agent",
            "codex",
            "--event",
            "PostToolUse",
        ])
        .write_stdin(payload.clone())
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    // An oversized completion that never names PreToolUse records nothing.
    assert_eq!(response, json!({}));
    let paths: Vec<_> = std::fs::read_dir(&logs)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(paths.len(), 1);
    let text = std::fs::read_to_string(&paths[0]).unwrap();
    let records: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let input = records
        .iter()
        .find(|record| record["event"] == "hook_input")
        .unwrap();
    assert_eq!(input["fields"]["oversized"], true);
    assert_eq!(input["fields"]["captured_bytes"], 8 * 1024 * 1024);
    let result = records
        .iter()
        .find(|record| record["event"] == "hook_output")
        .unwrap();
    assert_eq!(
        result["fields"]["host_event"],
        response["hookSpecificOutput"]["hookEventName"]
    );
    assert_eq!(records.last().unwrap()["event"], "run_end");
    assert!(!text.contains("private-result"));
    let disabled_logs = root.path().join("disabled");
    let disabled = cargo_bin_cmd!("shield")
        .env("SHIELD_DEBUG_LOG_DIR", &disabled_logs)
        .args([
            "--no-debug-log",
            "__internal",
            "native-hook",
            "--project",
            root.path().to_str().unwrap(),
            "--agent",
            "codex",
            "--event",
            "PostToolUse",
        ])
        .write_stdin(payload)
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(disabled.stdout, output.stdout);
    assert!(!disabled_logs.exists());
}
