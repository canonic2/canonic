//! Lanes, keys, capacity, deadlines, cancellation and progress.

use crate::fixture::{Events, Host, Latch, Script, arguments, result_text, text, tool_name};
use canonic_mcp::stdio::{Lane, Options};
use serde_json::json;
use std::time::Duration;

/// Records `start <message>` and `end <message>`, waiting on `latch` for `acme_probe`
/// and `acme_change`.
fn recording(events: &Events, latch: &Latch) -> Script {
    let events = events.clone();
    let latch = latch.clone();
    Script::new(move |request, context| {
        let message = arguments(&request)["message"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        events.record(format!("start {message}"));
        let finished = tool_name(&request) == "acme_echo" || latch.wait(context);
        events.record(format!(
            "{} {message}",
            if finished { "end" } else { "stopped" }
        ));
        text(&message)
    })
}

fn lanes(workers: usize, capacity: usize) -> Options {
    Options {
        lanes: vec![Lane::new("tools", workers, capacity)],
        ..Options::default()
    }
}

#[test]
fn requests_with_one_key_run_in_order_while_other_keys_proceed() {
    let events = Events::new();
    let latch = Latch::default();
    let host = Host::ready(recording(&events, &latch), lanes(2, 8), json!({}));
    host.call(1, "acme_change", json!({"message":"a1","key":"a"}));
    assert_eq!(events.next(), "start a1");
    host.call(2, "acme_change", json!({"message":"a2","key":"a"}));
    host.call(3, "acme_echo", json!({"message":"b1","key":"b"}));
    assert_eq!(events.next(), "start b1");
    assert_eq!(events.next(), "end b1");
    assert_eq!(host.recv()["id"], 3);
    assert_eq!(events.none_within(Duration::from_millis(50)), None);
    latch.release();
    assert_eq!(events.next(), "end a1");
    assert_eq!(events.next(), "start a2");
    assert_eq!(events.next(), "end a2");
    let mut ids = [host.recv()["id"].clone(), host.recv()["id"].clone()];
    ids.sort_by_key(serde_json::Value::as_u64);
    assert_eq!(ids, [json!(1), json!(2)]);
}

#[test]
fn a_full_lane_refuses_new_requests() {
    let events = Events::new();
    let latch = Latch::default();
    let host = Host::ready(recording(&events, &latch), lanes(1, 1), json!({}));
    host.call(1, "acme_change", json!({"message":"first"}));
    assert_eq!(events.next(), "start first");
    host.call(2, "acme_change", json!({"message":"second"}));
    let refused = host.recv();
    assert_eq!(refused["id"], 2);
    assert_eq!(refused["error"]["code"], -32000);
    latch.release();
    assert_eq!(result_text(&host.recv()), "first");
}

#[test]
fn a_queued_request_expires_without_running() {
    let events = Events::new();
    let latch = Latch::default();
    let host = Host::ready(recording(&events, &latch), lanes(1, 8), json!({}));
    host.call(1, "acme_change", json!({"message":"slow"}));
    assert_eq!(events.next(), "start slow");
    host.call(2, "acme_change", json!({"message":"late","timeoutMs":30}));
    let expired = host.recv();
    assert_eq!(expired["id"], 2);
    assert_eq!(expired["error"]["code"], -32001);
    assert_eq!(
        expired["error"]["message"],
        "Request deadline exceeded before dispatch"
    );
    latch.release();
    assert_eq!(events.next(), "end slow");
    assert_eq!(host.recv()["id"], 1);
    assert_eq!(events.none_within(Duration::from_millis(50)), None);
}

#[test]
fn a_running_request_past_its_deadline_is_answered_and_stopped() {
    let events = Events::new();
    let latch = Latch::default();
    let host = Host::ready(recording(&events, &latch), lanes(1, 8), json!({}));
    host.call(1, "acme_change", json!({"message":"stuck","timeoutMs":30}));
    let expired = host.recv();
    assert_eq!(expired["id"], 1);
    assert_eq!(expired["error"]["message"], "Request deadline exceeded");
    assert_eq!(events.next(), "start stuck");
    assert_eq!(events.next(), "stopped stuck");
    host.assert_quiet();
}

#[test]
fn cancellation_stops_work_and_sends_no_reply() {
    let events = Events::new();
    let latch = Latch::default();
    let host = Host::ready(recording(&events, &latch), lanes(1, 8), json!({}));
    host.call(1, "acme_change", json!({"message":"running"}));
    assert_eq!(events.next(), "start running");
    host.call(2, "acme_change", json!({"message":"queued"}));
    host.cancel(2);
    host.cancel(1);
    assert_eq!(events.next(), "stopped running");
    host.assert_quiet();
    assert_eq!(events.none_within(Duration::from_millis(50)), None);
}

#[test]
fn oversized_progress_notifications_are_dropped_without_error_replies() {
    for oversized_token in [true, false] {
        let events = Events::new();
        let recorded = events.clone();
        let latch = Latch::default();
        let blocked = latch.clone();
        let script = Script::new(move |_, context| {
            let message = (!oversized_token).then(|| "x".repeat(2048));
            assert!(context.progress(1.0, None, message.as_deref()));
            recorded.record("progress queued");
            blocked.wait(context);
            text("done")
        });
        let options = Options {
            max_output_bytes: 1024,
            ..Options::default()
        };
        let host = Host::ready(script, options, json!({}));
        let token = if oversized_token {
            "x".repeat(2048)
        } else {
            "acme".into()
        };
        host.send(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"acme_change","_meta":{"progressToken":token}}}),
        );
        assert_eq!(events.next(), "progress queued");
        host.assert_quiet();
        latch.release();
        assert_eq!(result_text(&host.recv()), "done");
    }
}

#[test]
fn progress_reaches_the_caller_before_the_reply() {
    let script = Script::new(|_, context| {
        assert!(context.progress(1.0, Some(2.0), Some("Acme step one")));
        text("done")
    });
    let host = Host::ready(script, Options::default(), json!({}));
    host.send(
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
        "name":"acme_change","arguments":{},"_meta":{"progressToken":"acme-1"}}}),
    );
    let progress = host.recv();
    assert_eq!(progress["method"], "notifications/progress");
    assert_eq!(
        progress["params"],
        json!({"progressToken":"acme-1","progress":1.0,"total":2.0,"message":"Acme step one"})
    );
    assert_eq!(result_text(&host.recv()), "done");

    let silent = Script::new(|_, context| {
        assert!(!context.progress(1.0, None, None));
        text("done")
    });
    let host = Host::ready(silent, Options::default(), json!({}));
    host.call(1, "acme_change", json!({}));
    assert_eq!(result_text(&host.recv()), "done");
}
