//! Diagnostic persistence, bounded retention and payload privacy.
use super::*;
use clap::Parser;

#[test]
fn hook_and_mcp_metadata_preserve_correlation_without_payloads() {
    let root = assert_fs::TempDir::new().unwrap();
    let cli = Cli::parse_from([
        "shield",
        "--debug-log-dir",
        root.path().to_str().unwrap(),
        "status",
    ]);
    let log = Log::open(&cli);
    log.hook_input("codex", &serde_json::to_vec(&json!({"hook_event_name":"PostToolUse","session_id":"acme-session","turn_id":"acme-turn","tool_use_id":"acme-call","tool_name":"mcp__shield__history_list","cwd":"/secret-path","tool_input":{"command":"secret-command"},"tool_response":"secret-result"})).unwrap(), true, false);
    log.mcp_frame("mcp_input", br#"{"id":"acme-call","method":"tools/call","params":{"name":"history_list","secret":"secret-argument"}}"#);
    log.mcp_frame(
        "mcp_output",
        br#"{"id":"acme-call","result":{"isError":false,"content":[{"text":"secret-result"}]}}"#,
    );
    log.hook_output(&json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"secret-reason"}}));
    let text = std::fs::read_to_string(log.path().unwrap()).unwrap();
    let startup: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(startup["event"], "run_start");
    assert!(startup["fields"].get("cwd").is_none());
    assert!(text.contains("acme-session"));
    assert!(text.contains("acme-turn"));
    assert!(text.contains("acme-call"));
    assert!(text.contains("history_list"));
    assert!(text.contains("permission_decision"));
    assert!(!text.contains("secret-"), "{text}");
    for line in text.lines() {
        serde_json::from_str::<Value>(line).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(log.path().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn logs_are_unique_disabled_or_unavailable_without_overwriting_files() {
    let root = assert_fs::TempDir::new().unwrap();
    let cli = Cli::parse_from([
        "shield",
        "--debug-log-dir",
        root.path().to_str().unwrap(),
        "status",
    ]);
    let first = Log::open(&cli);
    let before = std::fs::read(first.path().unwrap()).unwrap();
    let second = Log::open(&cli);
    assert_ne!(first.path(), second.path());
    assert_eq!(std::fs::read(first.path().unwrap()).unwrap(), before);
    let disabled = Cli::parse_from(["shield", "--no-debug-log", "status"]);
    assert!(Log::open(&disabled).path().is_none());
    let unavailable = Cli::parse_from([
        "shield",
        "--debug-log-dir",
        first.path().unwrap().to_str().unwrap(),
        "status",
    ]);
    assert!(Log::open(&unavailable).path().is_none());
    assert_eq!(std::fs::read(first.path().unwrap()).unwrap(), before);
}

#[test]
fn log_limit_stops_writes_without_affecting_the_caller() {
    let root = assert_fs::TempDir::new().unwrap();
    let log = Log::create(root.path()).unwrap();
    log.event(
        "fixture",
        json!({"padding":"x".repeat(usize::try_from(LIMIT).unwrap())}),
    );
    let before = std::fs::read_to_string(log.path().unwrap()).unwrap();
    assert!(before.contains("log_limit_reached"));
    assert!(u64::try_from(before.len()).unwrap() <= LIMIT);
    log.event("after_limit", json!({}));
    assert_eq!(
        std::fs::read_to_string(log.path().unwrap()).unwrap(),
        before
    );
}

#[test]
fn failed_writes_disable_logging_and_preserve_existing_bytes() {
    let root = assert_fs::TempDir::new().unwrap();
    let path = root.path().join("read-only.jsonl");
    std::fs::write(&path, "preserve existing bytes\n").unwrap();
    let log = Log {
        path: Some(path.clone()),
        started: Instant::now(),
        writer: Mutex::new(Some((File::open(&path).unwrap(), 0))),
    };
    log.event("cannot_write", json!({}));
    assert!(log.writer.lock().unwrap().is_none());
    log.event("after_failure", json!({}));
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "preserve existing bytes\n"
    );
}

#[test]
fn real_mcp_dispatch_logs_request_and_response_without_retaining_results() {
    let root = assert_fs::TempDir::new().unwrap();
    let log = std::sync::Arc::new(Log::create(root.path()).unwrap());
    let (server, _project) = crate::mcp::tests::fixture(None);
    let [initialize, initialized] = crate::mcp::tests::initialize(&json!({}), "secret-client");
    let input = format!(
        "{initialize}\n{initialized}\n{}\n",
        json!({"jsonrpc":"2.0","id":33,"method":"tools/call","params":{"name":"status","arguments":{}}})
    );
    let mut output = Vec::new();
    crate::mcp::serve(
        server,
        std::io::Cursor::new(input.into_bytes()),
        &mut output,
        &crate::mcp::options(std::time::Duration::from_secs(5), Some(log.clone())),
    )
    .unwrap();
    let response: Value = serde_json::from_slice(
        String::from_utf8_lossy(&output)
            .lines()
            .last()
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(response["id"], 33);
    assert_eq!(response["result"]["isError"], false);
    let text = std::fs::read_to_string(log.path().unwrap()).unwrap();
    let records: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(records.iter().any(|record| record["event"] == "mcp_input"
        && record["fields"]["id"] == 33
        && record["fields"]["tool"] == "status"));
    assert!(records.iter().any(|record| record["event"] == "mcp_output"
        && record["fields"]["id"] == 33
        && record["fields"]["bytes"].as_u64().unwrap() > 0));
    assert!(!text.contains("secret-client"));
    assert!(!text.contains("structuredContent"));
}
