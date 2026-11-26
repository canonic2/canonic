//! Diagnostic persistence, bounded retention and payload privacy.
use super::*;
use clap::Parser;

fn flush(log: &Log) {
    log.flush();
}

#[test]
fn oversized_hook_input_records_size_without_payload() {
    let root = assert_fs::TempDir::new().unwrap();
    let cli = Cli::parse_from([
        "shield",
        "--debug-log-dir",
        root.path().to_str().unwrap(),
        "status",
    ]);
    let log = Log::open(&cli);
    let truncated = br#"{"tool_response":"acme-private-result"#;
    log.hook_input("codex", truncated, true, true);
    flush(&log);
    let text = std::fs::read_to_string(log.path().unwrap()).unwrap();
    let input = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|record| record["event"] == "hook_input")
        .unwrap();
    assert_eq!(input["fields"]["oversized"], true);
    assert_eq!(input["fields"]["captured_bytes"], truncated.len());
    assert!(!text.contains("acme-private-result"));
}

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
    flush(&log);
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
    flush(&first);
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
    log.writer.lock().unwrap().as_mut().unwrap().1 = LIMIT - 64;
    log.event("fixture", || json!({"bounded":"limit marker"}));
    flush(&log);
    let before = std::fs::read_to_string(log.path().unwrap()).unwrap();
    assert!(before.contains("log_limit_reached"));
    assert!(u64::try_from(before.len()).unwrap() <= LIMIT);
    log.event("after_limit", || json!({}));
    flush(&log);
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
        started: Some(Instant::now()),
        writer: Arc::new(Mutex::new(Some((File::open(&path).unwrap(), 0)))),
        traces: OnceLock::new(),
        drain: Duration::from_millis(5),
    };
    log.event("cannot_write", || json!({}));
    flush(&log);
    assert!(log.writer.lock().unwrap().is_none());
    log.event("after_failure", || json!({}));
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "preserve existing bytes\n"
    );
}

#[test]
fn disabled_event_fields_are_lazy_and_native_events_never_wait_on_the_sink() {
    let disabled = Log::disabled();
    disabled.event("disabled", || {
        panic!("disabled diagnostic fields were allocated")
    });
    assert!(disabled.traces.get().is_none());
    let root = assert_fs::TempDir::new().unwrap();
    let mut log = Log::create_with_drain(root.path(), Duration::ZERO).unwrap();
    let writer = log.writer.clone();
    let held = writer.lock().unwrap();
    let started = Instant::now();
    for _ in 0..600 {
        log.event("native_fixture", || json!({"stage":"bounded"}));
    }
    let recorder = log.traces.get().unwrap();
    let before = recorder.loss();
    assert!(before > 0);
    log.event("one_more", || json!({}));
    assert_eq!(recorder.loss(), before + 1);
    let worker = Arc::get_mut(log.traces.get_mut().unwrap())
        .unwrap()
        .take_worker();
    drop(log);
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(held);
    worker.join().unwrap();
}

#[test]
fn oversized_diagnostic_records_are_counted_without_entering_the_queue() {
    let root = assert_fs::TempDir::new().unwrap();
    let log = Log::create(root.path()).unwrap();
    log.event("oversized", || json!({"padding":"x".repeat(32 * 1024)}));
    flush(&log);
    assert_eq!(log.traces.get().unwrap().loss(), 1);
    assert_eq!(std::fs::read(log.path().unwrap()).unwrap(), [] as [u8; 0]);
    log.event("bounded", || json!({}));
    flush(&log);
    let row: Value = serde_json::from_slice(&std::fs::read(log.path().unwrap()).unwrap()).unwrap();
    assert_eq!(row["event"], "bounded");
    assert_eq!(row["dropped_records"], 1);
    assert_eq!(row["dropped_records_total"], 1);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one transport lifetime checks trace ordering, publication and redaction together"
)]
fn real_mcp_dispatch_logs_request_and_response_without_retaining_results() {
    use crate::mcp::{self, tests::Transport};

    let root = assert_fs::TempDir::new().unwrap();
    let log = std::sync::Arc::new(Log::create(root.path()).unwrap());
    let (server, project) = mcp::tests::fixture(None);
    let disabled = std::sync::Arc::new(Log::disabled());
    assert!(mcp::lifecycle::Log::start(&disabled).is_none());
    let baseline = mcp::Server::for_review((*project.review).clone()).unwrap();
    let lifecycle = mcp::lifecycle::Log::start(&log).unwrap();
    let mut options = mcp::options(Some(log.clone()));
    options.observations = Some(lifecycle.sender());
    let transports = [
        Transport::with_options(baseline.with_log(disabled), mcp::options(None)),
        Transport::with_options(server.with_log(log.clone()), options),
    ];
    for transport in &transports {
        for message in mcp::tests::initialize(&json!({}), "secret-client") {
            transport.send(&message);
        }
        assert!(transport.receive()["result"].is_object());
    }
    let mut timings = [Vec::new(), Vec::new()];
    for id in 32..42 {
        let first = id % 2;
        let mut responses = [Value::Null, Value::Null];
        for index in [first, 1 - first] {
            let started = Instant::now();
            responses[index] = transports[index].call(json!(id), "pack_list", &json!({}));
            if id > 32 {
                timings[index].push(started.elapsed().as_micros());
            }
        }
        assert_eq!(responses[0]["id"], id);
        assert_eq!(responses[0]["result"]["isError"], false);
        assert_eq!(responses[0], responses[1]);
    }
    for samples in &mut timings {
        samples.sort_unstable();
    }
    eprintln!(
        "warm in-memory stdio median: disabled {} us, enabled {} us",
        timings[0][4], timings[1][4]
    );
    let deadline = Instant::now() + std::time::Duration::from_secs(1);
    let (text, records) = loop {
        let text = std::fs::read_to_string(log.path().unwrap()).unwrap();
        let records: Vec<Value> = text
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        if ["mcp_input", "mcp_output"].iter().all(|event| {
            records
                .iter()
                .any(|record| record["event"] == *event && record["fields"]["id"] == 33)
        }) && records.iter().any(|record| {
            record["event"] == "mcp_transport" && record["fields"]["stage"] == "Publication"
        }) {
            break (text, records);
        }
        assert!(
            Instant::now() < deadline,
            "frame metadata was not delivered while the process remained alive"
        );
        std::thread::yield_now();
    };
    assert!(records.iter().any(|record| record["event"] == "mcp_input"
        && record["fields"]["id"] == 33
        && record["fields"]["tool"] == "pack_list"));
    assert!(records.iter().any(|record| record["event"] == "mcp_output"
        && record["fields"]["id"] == 33
        && record["fields"]["bytes"].as_u64().unwrap() > 0));
    let start = records
        .iter()
        .find(|record| record["event"] == "request_start" && record["fields"]["request_id"] == 33)
        .unwrap();
    let link = &records
        .iter()
        .find(|record| {
            record["event"] == "request_transport"
                && record["fields"]["trace_id"] == start["fields"]["trace_id"]
        })
        .unwrap()["fields"];
    for (stage, parent) in [
        ("Decode", None),
        ("Validation", Some("Decode")),
        ("Queue", None),
        ("Encode", None),
        ("Publication", None),
    ] {
        for transition in ["start", "end"] {
            assert!(
                records.iter().any(|record| {
                    let fields = &record["fields"];
                    record["event"] == "mcp_transport"
                        && fields["connection"] == link["connection"]
                        && fields["request"] == link["request"]
                        && fields["stage"] == stage
                        && fields["transition"] == transition
                        && parent.is_none_or(|parent| fields["parent_stage"] == parent)
                }),
                "missing {stage} {transition} for {link}"
            );
        }
    }
    assert!(!text.contains("secret-client"));
    assert!(!text.contains("structuredContent"));
    for transport in transports {
        assert!(transport.close().1.is_ok());
    }
    drop(lifecycle);
}
