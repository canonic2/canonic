//! Resource discovery, reads and lifecycle through the shared stdio driver.

use crate::fixture::{Events, Host, Latch, Script, arguments};
use canonic_mcp::{
    ReadResourceResult, Resource, ResourceCatalog, ResourceContents, ResourceTemplate, rpc,
    stdio::{Options, Outcome},
};
use serde_json::json;
use std::{sync::Arc, time::Duration};

fn options() -> Options {
    let mut options = Options::default();
    options.session.resources = Some(Arc::new(
        ResourceCatalog::new(
            vec![Resource::new("acme://guide", "Guide")],
            vec![ResourceTemplate::new("acme://reports/{id}", "Reports")],
        )
        .expect("valid resource catalog"),
    ));
    options
}

fn read(host: &Host, id: u64, uri: &str) {
    host.send(&rpc::request(
        json!(id),
        "resources/read",
        json!({"uri":uri,"_meta":{"progressToken":"read-progress"}}),
    ));
}

fn contents(uri: &str) -> Outcome {
    Outcome::Resource(
        ReadResourceResult::new(vec![
            ResourceContents::text(uri, Some("text/plain".into()), "Acme report")
                .expect("valid text"),
            ResourceContents::blob("acme://capture", Some("image/png".into()), &[0, 255, 42])
                .expect("valid binary"),
        ])
        .expect("valid read result"),
    )
}

#[test]
fn resources_discovery_text_binary_unknown_and_validation() {
    let script = Script::new(|request, context| {
        let uri = arguments(&request)["uri"].as_str().unwrap();
        if uri == "acme://missing" {
            return Outcome::Method(Err(canonic_mcp::resource::not_found(uri)));
        }
        context.progress(1.0, Some(1.0), Some("Read report"));
        contents(uri)
    });
    let mut host = Host::start(script, options());
    assert_eq!(
        host.initialize(json!({}))["result"]["capabilities"]["resources"],
        json!({})
    );
    host.send(&rpc::request(json!(1), "resources/list", json!({})));
    assert_eq!(host.recv()["result"]["resources"][0]["uri"], "acme://guide");
    host.send(&rpc::request(
        json!(2),
        "resources/templates/list",
        json!({}),
    ));
    assert_eq!(
        host.recv()["result"]["resourceTemplates"][0]["uriTemplate"],
        "acme://reports/{id}"
    );
    read(&host, 3, "acme://reports/a%2Fb");
    let mut reply = host.recv();
    if reply.get("method").is_some() {
        assert_eq!(reply["params"]["progressToken"], "read-progress");
        reply = host.recv();
    }
    assert_eq!(
        reply["result"]["contents"][0]["uri"],
        "acme://reports/a%2Fb"
    );
    assert_eq!(reply["result"]["contents"][0]["text"], "Acme report");
    assert_eq!(reply["result"]["contents"][1]["blob"], "AP8q");
    read(&host, 4, "acme://missing");
    let reply = host.recv();
    assert_eq!(reply["error"]["code"], -32002);
    assert_eq!(reply["error"]["data"]["uri"], "acme://missing");
    read(&host, 5, "relative");
    assert_eq!(host.recv()["error"]["code"], rpc::INVALID_PARAMS);
    host.assert_quiet();
    host.finish().0.unwrap();
}

fn blocking(events: &Events, latch: &Latch) -> Script {
    let events = events.clone();
    let latch = latch.clone();
    Script::new(move |request, context| {
        let uri = arguments(&request)["uri"].as_str().unwrap_or("tool");
        events.record(format!("start {uri}"));
        let finished = latch.wait(context);
        if !finished {
            assert!(!context.progress(2.0, None, Some("late")));
        }
        events.record(format!("end {uri}"));
        contents("acme://report")
    })
}

#[test]
fn resources_cancel_queued_and_running_suppress_late_replies() {
    let events = Events::new();
    let latch = Latch::default();
    let host = Host::ready(blocking(&events, &latch), options(), json!({}));
    read(&host, 1, "acme://running");
    assert_eq!(events.next(), "start acme://running");
    read(&host, 2, "acme://queued");
    host.cancel(2);
    host.cancel(1);
    assert_eq!(events.next(), "end acme://running");
    latch.release();
    read(&host, 3, "acme://after");
    assert_eq!(host.recv()["id"], 3);
    assert_eq!(events.next(), "start acme://after");
    assert_eq!(events.next(), "end acme://after");
    host.assert_quiet();
    host.finish().0.unwrap();
}

#[test]
fn resources_queued_and_running_deadlines_release_ids() {
    let events = Events::new();
    let latch = Latch::default();
    let mut opts = options();
    opts.timeout = Duration::from_millis(50);
    let host = Host::ready(blocking(&events, &latch), opts, json!({}));
    read(&host, 1, "acme://running");
    assert_eq!(events.next(), "start acme://running");
    read(&host, 2, "acme://queued");
    for _ in 0..2 {
        assert_eq!(host.recv()["error"]["code"], rpc::DEADLINE_EXCEEDED);
    }
    latch.release();
    read(&host, 1, "acme://after");
    let reply = host.recv();
    assert_eq!(reply["id"], 1);
    assert!(reply["result"]["contents"].is_array());
    host.finish().0.unwrap();
}

#[test]
fn resources_queued_and_running_finish_after_eof() {
    let events = Events::new();
    let latch = Latch::default();
    let mut host = Host::ready(blocking(&events, &latch), options(), json!({}));
    read(&host, 1, "acme://running");
    assert_eq!(events.next(), "start acme://running");
    read(&host, 2, "acme://queued");
    host.call(3, "acme_change", json!({}));
    host.close();
    let reply = host.recv();
    assert_eq!(reply["id"], 3);
    assert_eq!(reply["error"]["code"], rpc::REQUEST_CANCELLED);
    latch.release();
    let (result, replies) = host.finish();
    result.unwrap();
    assert_eq!(
        replies
            .iter()
            .map(|reply| reply["id"].clone())
            .collect::<Vec<_>>(),
        vec![json!(1), json!(2)]
    );
    assert!(
        replies
            .iter()
            .all(|reply| reply["result"]["contents"].is_array())
    );
}

#[test]
fn resources_output_overflow_becomes_internal_error() {
    let mut opts = options();
    opts.max_output_bytes = 1024;
    let host = Host::ready(
        Script::new(|_, _| {
            Outcome::Resource(
                ReadResourceResult::new(vec![
                    ResourceContents::text("acme://report", None, "x".repeat(2000)).unwrap(),
                ])
                .unwrap(),
            )
        }),
        opts,
        json!({}),
    );
    read(&host, 1, "acme://report");
    assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
    host.finish().0.unwrap();
}

#[test]
fn resources_listings_remain_responsive_while_reads_are_busy() {
    let events = Events::new();
    let latch = Latch::default();
    let host = Host::ready(blocking(&events, &latch), options(), json!({}));
    read(&host, 1, "acme://running");
    assert_eq!(events.next(), "start acme://running");
    host.send(&rpc::request(json!(2), "resources/list", json!({})));
    assert_eq!(host.recv()["id"], 2);
    latch.release();
    assert_eq!(host.recv()["id"], 1);
    host.finish().0.unwrap();
}

#[test]
fn resources_duplicate_id_and_panics_keep_protocol_behavior() {
    let events = Events::new();
    let latch = Latch::default();
    let host = Host::ready(blocking(&events, &latch), options(), json!({}));
    read(&host, 1, "acme://running");
    assert_eq!(events.next(), "start acme://running");
    read(&host, 1, "acme://duplicate");
    assert_eq!(host.recv()["error"]["code"], rpc::INVALID_REQUEST);
    assert!(host.finish().0.is_err());
    let host = Host::ready(
        Script::new(|_, _| panic!("resource fixture")),
        options(),
        json!({}),
    );
    read(&host, 1, "acme://report");
    assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
    host.finish().0.unwrap();
}

#[test]
fn resources_progress_uses_read_token_before_completion() {
    let latch = Latch::default();
    let blocked = latch.clone();
    let host = Host::ready(
        Script::new(move |_, context| {
            assert!(context.progress(1.0, Some(2.0), Some("Reading")));
            blocked.wait(context);
            contents("acme://report")
        }),
        options(),
        json!({}),
    );
    read(&host, 1, "acme://report");
    let progress = host.recv();
    assert_eq!(progress["method"], "notifications/progress");
    assert_eq!(progress["params"]["progressToken"], "read-progress");
    assert_eq!(progress["params"]["progress"], 1.0);
    latch.release();
    assert_eq!(host.recv()["id"], 1);
    host.assert_quiet();
    host.finish().0.unwrap();
}

#[test]
fn resources_capacity_refusal_does_not_dispatch() {
    let events = Events::new();
    let latch = Latch::default();
    let mut opts = options();
    opts.lanes[0].capacity = 1;
    let host = Host::ready(blocking(&events, &latch), opts, json!({}));
    read(&host, 1, "acme://running");
    assert_eq!(events.next(), "start acme://running");
    read(&host, 2, "acme://refused");
    assert_eq!(host.recv()["error"]["code"], rpc::SERVER_ERROR);
    latch.release();
    assert_eq!(host.recv()["id"], 1);
    assert_eq!(events.next(), "end acme://running");
    assert_eq!(events.none_within(Duration::ZERO), None);
    host.finish().0.unwrap();
}

#[test]
fn resources_queued_deadline_never_dispatches_behind_a_longer_tool() {
    let events = Events::new();
    let latch = Latch::default();
    let mut opts = options();
    opts.timeout = Duration::from_millis(20);
    let host = Host::ready(blocking(&events, &latch), opts, json!({}));
    host.call(1, "acme_probe", json!({"timeoutMs":3000}));
    assert_eq!(events.next(), "start tool");
    read(&host, 2, "acme://queued");
    let reply = host.recv();
    assert_eq!(reply["id"], 2);
    assert_eq!(reply["error"]["code"], rpc::DEADLINE_EXCEEDED);
    assert_eq!(
        reply["error"]["message"],
        "Request deadline exceeded before dispatch"
    );
    host.cancel(1);
    assert_eq!(events.next(), "end tool");
    host.assert_quiet();
    host.finish().0.unwrap();
}
