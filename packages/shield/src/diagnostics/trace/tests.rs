use super::*;
use clap::Parser as _;

fn make_log(root: &assert_fs::TempDir) -> Arc<crate::diagnostics::Log> {
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        root.path().to_str().unwrap(),
        "status",
    ]);
    Arc::new(crate::diagnostics::Log::open(&cli))
}

/// Record assertions require writer completion, not the best-effort exit drain.
fn drain(log: Arc<crate::diagnostics::Log>) {
    let mut log = Arc::into_inner(log).expect("test owns the log");
    let recorder = Arc::get_mut(log.traces.get_mut().expect("trace was started"))
        .expect("all request links were released");
    recorder.sender.take();
    recorder.worker.take().unwrap().join().unwrap();
}

#[test]
fn request_links_cross_threads_and_keep_nested_work_and_other_requests_separate() {
    let root = assert_fs::TempDir::new().unwrap();
    let log = make_log(&root);
    let path = log.path().unwrap().to_owned();
    {
        let first = log
            .request("mcp", "policy_apply", Some(&json!(7)), Some("connection-a"))
            .unwrap();
        let second = log
            .request("mcp", "status", Some(&json!(7)), Some("connection-b"))
            .unwrap();
        let _scope = first.enter();
        {
            let _span = span("outer");
            let _nested = span("inner");
            count("source.hydrated", 2);
        }
        let link = first.clone();
        thread::spawn(move || {
            let _scope = link.enter();
            let _span = span("worker");
            count("source.hydrated", 1);
        })
        .join()
        .unwrap();
        {
            let _scope = second.enter();
            let _span = span("other");
            count("source.hydrated", 5);
        }
        first.finish("success");
        second.finish("refused");
    }
    drain(log);
    let records: Vec<Value> = std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let ended: Vec<_> = records
        .iter()
        .filter(|v| v["event"] == "request_end")
        .collect();
    assert_eq!(ended.len(), 2);
    assert_eq!(ended[0]["fields"]["counts"]["source.hydrated"], 3);
    assert_eq!(ended[1]["fields"]["counts"]["source.hydrated"], 5);
    assert_ne!(
        ended[0]["fields"]["trace_id"],
        ended[1]["fields"]["trace_id"]
    );
    let inner = records
        .iter()
        .find(|v| v["fields"]["stage"] == "inner")
        .unwrap();
    let outer = records
        .iter()
        .find(|v| v["fields"]["stage"] == "outer")
        .unwrap();
    assert_eq!(
        inner["fields"]["parent_span_id"],
        outer["fields"]["span_id"]
    );
    assert!(records.iter().any(|v| v["fields"]["stage"] == "worker"));
}

#[test]
fn disabled_tracing_has_no_request_or_attached_work_and_unfinished_requests_are_explicit() {
    let disabled = Arc::new(crate::diagnostics::Log::disabled());
    assert!(disabled.request("command", "command", None, None).is_none());
    assert!(span("disabled").inner.is_none());
    let root = assert_fs::TempDir::new().unwrap();
    let log = make_log(&root);
    let path = log.path().unwrap().to_owned();
    drop(log.request("mcp", "status", None, None));
    drain(log);
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("request_incomplete"));
}

#[test]
fn bounded_spans_report_loss_and_a_stalled_sink_cannot_block_the_request() {
    let root = assert_fs::TempDir::new().unwrap();
    let log = make_log(&root);
    let path = log.path().unwrap().to_owned();
    {
        let request = log.request("mcp", "status", None, None).unwrap();
        let _scope = request.enter();
        for _ in 0..SPANS + 2 {
            drop(span("bounded"));
        }
        request.finish("success");
    }
    drain(log);
    let text = std::fs::read_to_string(path).unwrap();
    let records: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let end = records
        .iter()
        .find(|v| v["event"] == "request_end")
        .unwrap();
    assert_eq!(end["fields"]["span_count"], SPANS);
    assert_eq!(end["fields"]["dropped_spans"], 2);
    assert_eq!(
        records
            .iter()
            .filter(|v| v["event"] == "request_span")
            .count(),
        usize::try_from(SPANS).unwrap()
    );
    let root = assert_fs::TempDir::new().unwrap();
    let log = make_log(&root);
    let request = log.request("mcp", "status", None, None).unwrap();
    let writer = log.writer.clone();
    let held = writer.lock().unwrap();
    let started = Instant::now();
    for _ in 0..QUEUE + 10 {
        request.0.recorder.emit("overflow", json!({}));
    }
    assert!(request.0.recorder.dropped.load(Ordering::Relaxed) > 0);
    request.finish("success");
    drop(request);
    // Keep ownership for cleanup without preventing Recorder's real Drop.
    let mut log = Arc::into_inner(log).expect("test owns the log");
    let worker = Arc::get_mut(log.traces.get_mut().unwrap())
        .unwrap()
        .worker
        .take()
        .unwrap();
    drop(log);
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(held);
    worker.join().unwrap();
}

#[test]
fn approval_wait_keeps_its_interval_across_a_worker_continuation() {
    let root = assert_fs::TempDir::new().unwrap();
    let log = make_log(&root);
    let path = log.path().unwrap().to_owned();
    let request = log.request("mcp", "policy_apply", None, None).unwrap();
    let waiting = request.wait("approval.wait", "approval.wait_us");
    thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(2));
        drop(waiting);
        request.finish("refused");
    })
    .join()
    .unwrap();
    drain(log);
    let records: Vec<Value> = std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let span = records
        .iter()
        .find(|v| v["fields"]["stage"] == "approval.wait")
        .unwrap();
    let end = records
        .iter()
        .find(|v| v["event"] == "request_end")
        .unwrap();
    assert!(span["fields"]["inclusive_us"].as_u64().unwrap() >= 2000);
    assert_eq!(
        end["fields"]["counts"]["approval.wait_us"],
        span["fields"]["inclusive_us"]
    );
    assert_eq!(end["fields"]["outcome"], "refused");
}
