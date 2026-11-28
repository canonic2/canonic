//! Native request identity, private text and independent context subscriptions.
use super::*;
use crate::browser::diagnostics::{DiagnosticKind, EventCondition, Query};

async fn page_fixture() -> (Worker, Peer, tempfile::TempDir, Reference, Reference) {
    let (mut worker, mut peer, root) = fixture().await;
    let task = tokio::spawn(async move {
        let id = Uuid::new_v4();
        let context = worker.context_create(id, deadline()).await.unwrap();
        let page = worker.page_create(id, context, deadline()).await.unwrap();
        (worker, context, page)
    });
    serve_page_allocation(&mut peer).await;
    let (worker, context, page) = task.await.unwrap();
    (worker, peer, root, context, page)
}

fn event(worker: &mut Worker, method: &str, params: Value) {
    worker.diagnostic_event(&Event {
        method: method.into(),
        session_id: Some("session-acme".into()),
        params,
    });
}

fn request(id: &str) -> Value {
    json!({"requestId":id,"timestamp":1.0,"type":"Fetch","request":{"url":"https://user:Acme-secret@example.com/api?token=Acme-secret#Acme-secret","method":"POST"}})
}

#[tokio::test]
async fn request_bodies_sent_during_trusted_entry_are_never_captured_or_read() {
    let (mut worker, _peer, _root, context, _page) = page_fixture().await;
    worker.diagnostics.history.enable(context, true);
    worker
        .request_inputs
        .set(context.connection_id, context, true)
        .unwrap();
    let mut entry = request("Acme-sign-in");
    entry["request"]["postDataEntries"] = json!([{"bytes":"QWNtZS1wYXNzd29yZA=="}]);
    worker.diagnostics.history.withhold_bodies(context, true);
    event(&mut worker, "Network.requestWillBeSent", entry);
    worker.diagnostics.history.withhold_bodies(context, false);
    event(
        &mut worker,
        "Network.requestWillBeSent",
        request("Acme-after"),
    );
    let read = worker.diagnostics_read(context, &Query::default()).unwrap();
    let events = read["events"].as_array().unwrap();
    let sign_in = &events[0]["data"];
    assert_eq!(sign_in["requestBodyWithheld"], true);
    assert!(sign_in.get("requestInputAvailable").is_none());
    // Capture is attempted again once entry ends.
    assert!(events[1]["data"].get("requestInputAvailable").is_some());
    assert!(events[1]["data"].get("requestBodyWithheld").is_none());
    let id = serde_json::from_value(sign_in["requestId"].clone()).unwrap();
    let error = worker
        .network_read(
            Uuid::new_v4(),
            context,
            id,
            serde_json::from_value(json!({"body":"request"})).unwrap(),
            deadline(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "BODY_WITHHELD");
    assert!(!format!("{error:?}").contains("QWNtZS1wYXNzd29yZA"));
}

#[tokio::test]
async fn http_request_input_default_native_captures_before_subscription_ack_and_reads_exact_bytes()
{
    let (mut worker, mut peer, _root, context, _page) = page_fixture().await;
    assert!(!worker.request_inputs.enabled(context));
    let task = tokio::spawn(async move {
        worker
            .diagnostics_subscribe(Uuid::new_v4(), context, true, deadline())
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Network.enable").await;
    assert_eq!(enable["params"]["maxPostDataSize"], 0);
    let mut source = request("Acme-default-body");
    source["loaderId"] = json!("Acme-loader");
    source["documentURL"] = json!("https://example.com/");
    source["wallTime"] = json!(1.0);
    source["initiator"] = json!({"type":"other"});
    source["redirectHasExtraInfo"] = json!(false);
    source["request"]["headers"] = json!({"Authorization":"Acme-private"});
    source["request"]["initialPriority"] = json!("High");
    source["request"]["referrerPolicy"] = json!("no-referrer");
    source["request"]["hasPostData"] = json!(true);
    source["request"]["postDataEntries"] = json!([{"bytes":"AP+A"}]);
    source["request"]["postData"] = json!("Acme lossy text");
    crate::browser::cdp::schema::event("Network.requestWillBeSent", &source).unwrap();
    send(
        &mut peer,
        json!({"method":"Network.requestWillBeSent","sessionId":"session-acme","params":source}),
    )
    .await;
    send(&mut peer,json!({"method":"Network.loadingFinished","sessionId":"session-acme","params":{"requestId":"Acme-default-body","timestamp":2.0,"encodedDataLength":3}})).await;
    reply(&mut peer, &enable, json!({})).await;
    let log = command(&mut peer, "Log.enable").await;
    reply(&mut peer, &log, json!({})).await;
    worker = task.await.unwrap();
    assert!(worker.request_inputs.enabled(context));
    let read = worker.diagnostics_read(context, &Query::default()).unwrap();
    let first = read["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["kind"] == "request")
        .unwrap();
    assert_eq!(first["data"]["requestInputAvailable"], true);
    assert!(!read.to_string().contains("Acme-private"));
    assert!(!read.to_string().contains("AP+A"));
    let id = serde_json::from_value(first["data"]["requestId"].clone()).unwrap();
    let body = worker
        .network_read(
            Uuid::new_v4(),
            context,
            id,
            serde_json::from_value(json!({"body":"request"})).unwrap(),
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(body["requestBodyFidelity"], "binary-request-initializer");
    let evidence = serde_json::from_value(body["body"]["evidenceId"].clone()).unwrap();
    assert_eq!(
        worker
            .network_evidence
            .get(evidence)
            .unwrap()
            .read()
            .unwrap(),
        [0, 255, 128]
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Default exact body reads do not query lossy late CDP text"
    );
}

#[tokio::test]
async fn http_request_input_native_event_body_read_uses_exact_retained_bytes_without_late_text_query()
 {
    let (mut worker, mut peer, _root, context, _page) = page_fixture().await;
    worker.diagnostics.history.enable(context, true);
    worker
        .request_inputs
        .set(context.connection_id, context, true)
        .unwrap();
    assert_eq!(worker.capture_post_data_budget("session-acme"), 0);
    let mut input = request("Acme-body");
    input["request"]["headers"] = json!({"Authorization":"Acme-private"});
    input["request"]["postDataEntries"] = json!([{"bytes":"AP+A"}]);
    input["request"]["postData"] = json!("Acme lossy text");
    event(&mut worker, "Network.requestWillBeSent", input);
    event(
        &mut worker,
        "Network.loadingFinished",
        json!({"requestId":"Acme-body","timestamp":2.0,"encodedDataLength":3}),
    );
    let diagnostics = worker.diagnostics_read(context, &Query::default()).unwrap();
    let id = serde_json::from_value(diagnostics["events"][0]["data"]["requestId"].clone()).unwrap();
    assert!(!diagnostics.to_string().contains("Acme-private"));
    assert!(!diagnostics.to_string().contains("AP+A"));
    let options = serde_json::from_value(json!({"body":"request"})).unwrap();
    let result = worker
        .network_read(Uuid::new_v4(), context, id, options, deadline())
        .await
        .unwrap();
    assert_eq!(result["requestBodyFidelity"], "binary-request-initializer");
    assert_eq!(result["body"]["attachment"]["bytes"], 3);
    let evidence = serde_json::from_value(result["body"]["evidenceId"].clone()).unwrap();
    assert_eq!(
        worker
            .network_evidence
            .get(evidence)
            .unwrap()
            .read()
            .unwrap(),
        [0, 255, 128]
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Exact bytes require no lossy late body call"
    );
    worker
        .request_inputs
        .remove(context.connection_id, id)
        .unwrap();
    let options = serde_json::from_value(json!({"body":"request"})).unwrap();
    assert_eq!(
        worker
            .network_read(Uuid::new_v4(), context, id, options, deadline())
            .await
            .unwrap_err()
            .code,
        "BODY_UNAVAILABLE"
    );
}

#[tokio::test]
async fn native_page_request_gc_collects_garbage_in_the_page_session() {
    let (mut worker, mut peer, _root, _context, page) = page_fixture().await;
    // Native admission accepts the request before it reaches the page session.
    supported(&Request::PageRequestGc { page }).unwrap();
    let task = tokio::spawn(async move {
        let result = worker
            .execute_one(Uuid::new_v4(), Request::PageRequestGc { page }, deadline())
            .await;
        (worker, result)
    });
    let call = command(&mut peer, "HeapProfiler.collectGarbage").await;
    assert_eq!(call["sessionId"], "session-acme");
    reply(&mut peer, &call, json!({})).await;
    let (_worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap(), json!({"page":page,"requested":true}));
}

#[tokio::test]
async fn native_network_read_reports_sdk_relations_raw_headers_and_sizes_across_a_redirect() {
    let (mut worker, _peer, _root, context, _page) = page_fixture().await;
    worker.diagnostics.history.enable(context, true);
    event(
        &mut worker,
        "Network.requestWillBeSent",
        json!({"requestId":"Acme-1","timestamp":1.0,"wallTime":1.0,"type":"Fetch","request":{"url":"https://example.com/redirect","method":"GET","headers":{"X-Acme":"one"}}}),
    );
    // ExtraInfo carries the headers the browser actually sent, with cookies.
    event(
        &mut worker,
        "Network.requestWillBeSentExtraInfo",
        json!({"requestId":"Acme-1","headers":{"X-Acme":"one","Cookie":"acme=private"}}),
    );
    let text = "HTTP/1.1 302 Found\r\nLocation: /final\r\n\r\n";
    event(
        &mut worker,
        "Network.responseReceivedExtraInfo",
        json!({"requestId":"Acme-1","headers":{"Location":"/final"},"headersText":text}),
    );
    event(
        &mut worker,
        "Network.requestWillBeSent",
        json!({"requestId":"Acme-1","timestamp":2.0,"wallTime":2.0,"type":"Fetch","request":{"url":"https://example.com/final","method":"GET","headers":{"X-Acme":"one"}},"redirectResponse":{"url":"https://example.com/redirect","status":302,"statusText":"Found","headers":{"Location":"/final"},"protocol":"http/1.1"}}),
    );
    event(
        &mut worker,
        "Network.responseReceived",
        json!({"requestId":"Acme-1","timestamp":3.0,"type":"Fetch","response":{"url":"https://example.com/final","status":200,"statusText":"OK","headers":{"Content-Length":"10"},"protocol":"h2"}}),
    );
    event(
        &mut worker,
        "Network.loadingFinished",
        json!({"requestId":"Acme-1","timestamp":4.0,"encodedDataLength":60}),
    );
    let history = worker.diagnostics_read(context, &Query::default()).unwrap();
    let requests = history["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "request")
        .map(|event| serde_json::from_value::<Uuid>(event["data"]["requestId"].clone()).unwrap())
        .collect::<Vec<_>>();
    let [first, last] = requests[..] else {
        panic!("{history}")
    };
    let options =
        || serde_json::from_value(json!({"rawHeaders":true,"responseDetails":true})).unwrap();
    let predecessor = worker
        .network_read(Uuid::new_v4(), context, first, options(), deadline())
        .await
        .unwrap();
    assert_eq!(predecessor["redirectedTo"], json!(last));
    assert_eq!(predecessor["statusText"], "Found");
    assert_eq!(predecessor["status"], 302);
    assert_eq!(predecessor["method"], "GET");
    assert_eq!(predecessor["resourceType"], "fetch");
    assert_eq!(predecessor["responseDetails"]["httpVersion"], "HTTP/1.1");
    assert_eq!(
        predecessor["rawRequestHeaders"]["entries"],
        json!([{"name":"Cookie","value":"acme=private"},{"name":"X-Acme","value":"one"}])
    );
    // The reported header text length wins; a redirect has no transfer size.
    assert_eq!(
        predecessor["responseDetails"]["sizes"]["responseHeadersSize"],
        text.len()
    );
    assert_eq!(
        predecessor["responseDetails"]["sizes"]["responseBodySize"],
        0
    );
    let last_read = worker
        .network_read(Uuid::new_v4(), context, last, options(), deadline())
        .await
        .unwrap();
    assert_eq!(last_read["redirectedFrom"], json!(first.to_string()));
    assert_eq!(last_read["redirectedTo"], Value::Null);
    assert_eq!(last_read["status"], 200);
    assert_eq!(last_read["resourceType"], "fetch");
    assert_eq!(last_read["responseDetails"]["httpVersion"], "HTTP/2.0");
    assert_eq!(
        last_read["rawResponseHeaders"]["entries"],
        json!([{"name":"Content-Length","value":"10"}])
    );
    // Computed status line and headers, with the encoded length's body share.
    let response_headers = 4 + 8 + 3 + 2 + (14 + 2 + 4) + 2;
    assert_eq!(
        last_read["responseDetails"]["sizes"],
        json!({"requestBodySize":0,"requestHeadersSize":4+3+6+8+(6+3+4),"responseBodySize":60-response_headers,"responseHeadersSize":response_headers,"transferSize":60})
    );
    // Raw values stay out of masked reads and history.
    assert!(!history.to_string().contains("acme=private"));
}

#[tokio::test]
async fn native_event_wait_progress_survives_old_cursor_eviction_and_cancel() {
    use crate::browser::activity::{Condition, Event as PageEvent};
    let (mut worker, _peer, _root, context, page) = page_fixture().await;
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "events_wait".into())
        .unwrap();
    let after = worker
        .interactions
        .history
        .read(context, None, 100)
        .unwrap()["latestCursor"]
        .as_u64()
        .unwrap();
    let conditions = vec![Condition {
        event: PageEvent::Dialog,
        page: Some(page),
    }];
    let request = Request::EventsWait {
        context,
        after,
        conditions: conditions.clone(),
    };
    for _ in 0..200 {
        worker
            .interactions
            .history
            .push(context, page, PageEvent::FrameNavigated, page);
    }
    assert!(
        worker
            .probe_events(id, context, after, &conditions)
            .unwrap()
            .is_none()
    );
    let cursor = worker.shared.state.lock().unwrap().operations[&id]
        .result
        .as_ref()
        .unwrap()["nextCursor"]
        .as_u64()
        .unwrap();
    assert_eq!(cursor, after + 200);
    for _ in 0..400 {
        worker
            .interactions
            .history
            .push(context, page, PageEvent::FrameNavigated, page);
    }
    assert!(
        worker
            .probe_events(id, context, after, &conditions)
            .unwrap()
            .is_none()
    );
    let cancelled = worker.event_wait_failure(id, &request, "CANCELLED".into());
    assert_eq!(cancelled.partial.unwrap()["nextCursor"], after + 600);
    let expired = worker.event_wait_failure(id, &request, "CONDITION_TIMEOUT".into());
    assert_eq!(expired.code, "WAIT_EXPIRED");
    assert_eq!(expired.partial.unwrap()["nextCursor"], after + 600);
    let inline = worker
        .events_wait(
            id,
            context,
            after + 200,
            &[Condition {
                event: PageEvent::FrameNavigated,
                page: Some(page),
            }],
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(inline["nextCursor"], after + 201);
    worker
        .interactions
        .history
        .push(context, page, PageEvent::PageClosed, page);
    let failure = worker
        .probe_events(id, context, after, &conditions)
        .unwrap_err();
    assert_eq!(failure.code, "PAGE_CLOSED");
    assert_eq!(failure.partial.unwrap()["nextCursor"], after + 601);
    // This observation path never needs a browser command.
    worker.connection.take();
}

#[tokio::test]
async fn native_event_wait_close_witness_survives_either_destroy_reply_ordering() {
    use crate::browser::activity::{Condition, Event as PageEvent};
    for destroyed_first in [false, true] {
        let (mut worker, mut peer, _root, context, page) = page_fixture().await;
        let after = worker
            .interactions
            .history
            .read(context, None, 100)
            .unwrap()["latestCursor"]
            .as_u64()
            .unwrap();
        let id = Uuid::new_v4();
        worker
            .shared
            .state
            .lock()
            .unwrap()
            .begin(id, "page_close".into())
            .unwrap();
        let closing = tokio::spawn(async move {
            let result = worker
                .execute_one(id, Request::PageClose { page }, deadline())
                .await;
            (worker, result)
        });
        let close = command(&mut peer, "Target.closeTarget").await;
        if destroyed_first {
            send(
                &mut peer,
                json!({"method":"Target.targetDestroyed","params":{"targetId":"page-acme"}}),
            )
            .await;
        }
        reply(&mut peer, &close, json!({"success":true})).await;
        let (mut worker, result) = closing.await.unwrap();
        assert_eq!(result.unwrap()["closed"], true);
        let matched = worker
            .interactions
            .history
            .wait(
                context,
                after,
                after,
                &[Condition {
                    event: PageEvent::PageClosed,
                    page: Some(page),
                }],
            )
            .unwrap();
        assert_eq!(
            matched["event"]["event"], "page_closed",
            "destroyed_first={destroyed_first}: {matched}"
        );
        if !destroyed_first {
            worker
                .apply(EventItem::Event(Event {
                    session_id: None,
                    method: "Target.targetDestroyed".into(),
                    params: json!({"targetId":"page-acme"}),
                }))
                .unwrap();
        }
        let read = worker
            .interactions
            .history
            .read(context, Some(after), 100)
            .unwrap();
        assert_eq!(
            read["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["event"] == "page_closed")
                .count(),
            1
        );
        worker.connection.take();
    }
}

#[tokio::test]
async fn network_console_logs_exclude_enable_replay_and_require_exact_raw_url_for_identity() {
    let (mut worker, mut peer, _root, context, _page) = page_fixture().await;
    let starting = tokio::spawn(async move {
        worker
            .diagnostics_subscribe(Uuid::new_v4(), context, true, deadline())
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let enable = command(&mut peer, "Log.enable").await;
    send(&mut peer,json!({"method":"Log.entryAdded","sessionId":"session-acme","params":{"entry":{"source":"network","level":"error","text":"Acme old network log","url":"https://example.com/old","timestamp":1}}})).await;
    let cleanup = command(&mut peer, "Runtime.releaseObjectGroup").await;
    reply(&mut peer, &cleanup, json!({})).await;
    reply(&mut peer, &enable, json!({})).await;
    worker = starting.await.unwrap();
    assert!(
        worker
            .diagnostics_read(
                context,
                &Query {
                    kind: Some(DiagnosticKind::Console),
                    include_text: true,
                    ..Default::default()
                }
            )
            .unwrap()["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let source = request("Acme-private-native-request");
    event(&mut worker, "Network.requestWillBeSent", source.clone());
    let log = json!({"entry":{"source":"network","level":"error","text":"Acme private network log","url":source["request"]["url"],"networkRequestId":"Acme-private-native-request","timestamp":2}});
    event(&mut worker, "Log.entryAdded", log.clone());
    let read = worker
        .diagnostics_read(
            context,
            &Query {
                kind: Some(DiagnosticKind::Console),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(read["events"][0]["data"]["requestId"].is_string());
    assert_eq!(
        read["events"][0]["data"]["requestIdentity"],
        "native-network-id-and-exact-url"
    );
    assert!(!read.to_string().contains("Acme-secret"));
    assert!(!read.to_string().contains("Acme-private-native-request"));
    assert!(!read.to_string().contains("Acme private network log"));
    let mut changed = source;
    changed["request"]["url"] = json!("https://user:Acme-secret@example.com/api?token=changed");
    event(&mut worker, "Network.requestWillBeSent", changed);
    event(&mut worker, "Log.entryAdded", log.clone());
    let read = worker
        .diagnostics_read(
            context,
            &Query {
                kind: Some(DiagnosticKind::Console),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(read["events"][1]["data"]["requestId"].is_null());
    worker.apply(EventItem::Gap { dropped: 1 }).unwrap();
    event(&mut worker, "Log.entryAdded", log);
    let read = worker
        .diagnostics_read(
            context,
            &Query {
                kind: Some(DiagnosticKind::Console),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(read["events"][0]["data"]["requestId"].is_null());
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_diagnostic_arguments_unknown_read_remains_owned_until_attachment_teardown() {
    let (mut worker, mut peer, _root, context, _) = page_fixture().await;
    worker.diagnostics.history.enable(context, true);
    worker
        .diagnostic_arguments_operation(
            Uuid::new_v4(),
            Request::DiagnosticsArgumentsSet {
                context,
                enabled: true,
            },
            deadline(),
        )
        .await
        .unwrap();
    event(
        &mut worker,
        "Runtime.consoleAPICalled",
        json!({"type":"log","executionContextId":1,"timestamp":1,"args":[{"type":"object","objectId":"Acme-private-object"}]}),
    );
    let history = worker.diagnostics_read(context, &Query::default()).unwrap();
    let group_id =
        serde_json::from_value(history["events"][0]["data"]["arguments"]["groupId"].clone())
            .unwrap();
    let operation = Uuid::new_v4();
    let shared = worker.shared.clone();
    shared
        .state
        .lock()
        .unwrap()
        .begin(operation, "argument-read-fixture".into())
        .unwrap();
    let reading = tokio::spawn(async move {
        let result = worker
            .diagnostic_arguments_operation(
                operation,
                Request::DiagnosticsArgumentsRead {
                    context,
                    group_id,
                    query: Default::default(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let _call = command(&mut peer, "Runtime.callFunctionOn").await;
    shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&operation)
        .unwrap()
        .cancel_requested = true;
    shared.changed.notify_waiters();
    let (mut worker, result) = reading.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    worker
        .apply(EventItem::Event(Event {
            method: "Runtime.executionContextDestroyed".into(),
            session_id: Some("session-acme".into()),
            params: json!({"executionContextId":1,"executionContextUniqueId":"world-acme"}),
        }))
        .unwrap();
    worker.fence_diagnostic_arguments();
    let listed = worker
        .diagnostic_arguments_operation(
            Uuid::new_v4(),
            Request::DiagnosticsArgumentsList { context },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(listed["pendingReadCleanup"], 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    worker
        .apply(EventItem::Event(Event {
            method: "Target.targetDestroyed".into(),
            session_id: None,
            params: json!({"targetId":"page-acme"}),
        }))
        .unwrap();
    worker.release_unretained_arguments(deadline()).await;
    let listed = worker
        .diagnostic_arguments_operation(
            Uuid::new_v4(),
            Request::DiagnosticsArgumentsList { context },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(listed["pendingReadCleanup"], 0);
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One retained console group verifies world identity, getter results and cleanup acknowledgements on the same transport"
)]
async fn native_diagnostic_arguments_retain_exact_world_objects_and_fence_cleanup_admission() {
    let (mut worker, mut peer, _root, context, _page) = page_fixture().await;
    worker.diagnostics.history.enable(context, true);
    worker
        .diagnostic_arguments_operation(
            Uuid::new_v4(),
            Request::DiagnosticsArgumentsSet {
                context,
                enabled: true,
            },
            deadline(),
        )
        .await
        .unwrap();
    event(
        &mut worker,
        "Runtime.consoleAPICalled",
        json!({"type":"log","executionContextId":1,"args":[{"type":"object","objectId":"Acme-private-object","className":"Object"}]}),
    );
    let history = worker.diagnostics_read(context, &Query::default()).unwrap();
    assert!(!history.to_string().contains("Acme-private-object"));
    let group_id =
        serde_json::from_value(history["events"][0]["data"]["arguments"]["groupId"].clone())
            .unwrap();
    worker.discard_diagnostic_objects().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    // An event without a proved world must release its objects independently.
    event(
        &mut worker,
        "Runtime.consoleAPICalled",
        json!({"type":"log","args":[{"type":"object","objectId":"unretained-object"}]}),
    );
    let cleanup = tokio::spawn(async move {
        worker.discard_diagnostic_objects().await;
        worker
    });
    let release = command(&mut peer, "Runtime.releaseObject").await;
    assert_eq!(release["params"]["objectId"], "unretained-object");
    reply(&mut peer, &release, json!({})).await;
    let mut worker = cleanup.await.unwrap();
    let read = tokio::spawn(async move {
        let result = worker
            .diagnostic_arguments_operation(
                Uuid::new_v4(),
                Request::DiagnosticsArgumentsRead {
                    context,
                    group_id,
                    query: Default::default(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let call = command(&mut peer, "Runtime.callFunctionOn").await;
    assert_eq!(call["params"]["uniqueContextId"], "world-acme");
    assert_eq!(
        call["params"]["arguments"][1]["objectId"],
        "Acme-private-object"
    );
    reply(
        &mut peer,
        &call,
        json!({"result":{"type":"object","value":{"n":17}}}),
    )
    .await;
    let cleanup = command(&mut peer, "Runtime.releaseObjectGroup").await;
    assert!(
        cleanup["params"]["objectGroup"]
            .as_str()
            .unwrap()
            .starts_with("testrun:diagnostic-argument-read:")
    );
    reply(&mut peer, &cleanup, json!({})).await;
    let (mut worker, result) = read.await.unwrap();
    assert_eq!(result.unwrap()["value"]["value"], 17);
    let remove = tokio::spawn(async move {
        let result = worker
            .diagnostic_arguments_operation(
                Uuid::new_v4(),
                Request::DiagnosticsArgumentsRemove { context, group_id },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
    assert_eq!(release["params"]["objectGroup"], "console");
    send(&mut peer,json!({"sessionId":"session-acme","method":"Runtime.consoleAPICalled","params":{"type":"log","executionContextId":1,"timestamp":1,"args":[{"type":"object","objectId":"during-cleanup"}]}})).await;
    reply(&mut peer, &release, json!({})).await;
    let (mut worker, result) = remove.await.unwrap();
    result.unwrap();
    let read = worker.diagnostics_read(context, &Query::default()).unwrap();
    assert_eq!(
        read["events"][2]["data"]["arguments"]["unavailable"],
        "CONSOLE_CLEANUP_PENDING"
    );
    assert!(
        worker
            .diagnostic_arguments_operation(
                Uuid::new_v4(),
                Request::DiagnosticsArgumentsRead {
                    context,
                    group_id,
                    query: Default::default()
                },
                deadline()
            )
            .await
            .is_err()
    );
    let cleanup = tokio::spawn(async move {
        worker.discard_diagnostic_objects().await;
        worker
    });
    let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
    reply(&mut peer, &release, json!({})).await;
    let mut worker = cleanup.await.unwrap();
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_diagnostics_redirects_keep_exact_identity_and_omit_private_text() {
    let (mut worker, _peer, _root, context, page) = page_fixture().await;
    worker.diagnostics.history.enable(context, true);
    event(
        &mut worker,
        "Network.requestWillBeSent",
        request("request-1"),
    );
    event(
        &mut worker,
        "Network.requestWillBeSent",
        request("request-2"),
    );
    event(
        &mut worker,
        "Runtime.consoleAPICalled",
        json!({"type":"error","timestamp":1.0,"args":[{"type":"string","value":"Acme-secret 409"}],"stackTrace":{"callFrames":[{"functionName":"Acme-private-stack","url":"https://user:Acme-secret@example.com/app.js?token=Acme-secret","lineNumber":4,"columnNumber":2}]}}),
    );
    let mut redirect = request("request-1");
    redirect["redirectResponse"] = json!({"status":307,"fromServiceWorker":false});
    event(&mut worker, "Network.requestWillBeSent", redirect);
    event(
        &mut worker,
        "Network.responseReceived",
        json!({"requestId":"request-1","timestamp":2.0,"response":{"status":409,"fromServiceWorker":true}}),
    );
    event(
        &mut worker,
        "Network.loadingFinished",
        json!({"requestId":"request-1","timestamp":3.0,"encodedDataLength":12}),
    );
    let read = worker.diagnostics_read(context, &Query::default()).unwrap();
    let events = read["events"].as_array().unwrap();
    assert_eq!(events.len(), 8);
    assert!(!read.to_string().contains("Acme-secret"));
    assert!(!read.to_string().contains("Acme-private-stack"));
    assert!(events.iter().all(|event| event["page"] == json!(page)));
    let first = &events[0]["data"]["requestId"];
    let second = &events[1]["data"]["requestId"];
    assert_ne!(first, second);
    assert!(events[2]["data"].get("requestId").is_none());
    assert_eq!(&events[3]["data"]["requestId"], first);
    assert_eq!(events[3]["data"]["status"], 307);
    assert_eq!(&events[5]["data"]["redirectedFrom"], first);
    let redirected = &events[5]["data"]["requestId"];
    assert_ne!(redirected, first);
    assert_ne!(redirected, second);
    assert_eq!(&events[6]["data"]["requestId"], redirected);
    assert_eq!(&events[7]["data"]["requestId"], redirected);
    assert_eq!(events[6]["data"]["fromServiceWorker"], true);
    let private = worker
        .diagnostics_read(
            context,
            &Query {
                include_text: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(private["events"][2]["data"]["text"], "Acme-secret 409");
    assert!(!private.to_string().contains("Acme-private-stack"));
    let stacks = worker
        .diagnostics_read(
            context,
            &Query {
                include_stack: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!stacks.to_string().contains("Acme-secret"));
    assert_eq!(
        stacks["events"][2]["data"]["stack"]["frames"][0]["functionName"],
        "Acme-private-stack"
    );
    assert_eq!(stacks["events"][2]["data"]["stackIncluded"], true);
    let condition = EventCondition {
        web_socket_id: None,
        event: DiagnosticKind::Response,
        page: Some(page),
        request_id: Some(serde_json::from_value(redirected.clone()).unwrap()),
        method: Some("POST".into()),
        status: Some(409),
        url: Some("https://example.com/api".into()),
    };
    assert_eq!(
        worker
            .probe_diagnostics(context, 0, &[condition])
            .unwrap()
            .unwrap()["event"]["sequence"],
        7
    );
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_diagnostics_gap_fences_waits_and_does_not_guess_missing_requests() {
    let (mut worker, _peer, _root, context, _) = page_fixture().await;
    worker.diagnostics.history.enable(context, true);
    event(
        &mut worker,
        "Network.requestWillBeSent",
        request("request-1"),
    );
    let before = worker.diagnostics_read(context, &Query::default()).unwrap();
    worker.apply(EventItem::Gap { dropped: 3 }).unwrap();
    assert_eq!(
        worker
            .diagnostics_read(
                context,
                &Query {
                    after: Some(0),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
        "invalid_cursor"
    );
    event(
        &mut worker,
        "Network.responseReceived",
        json!({"requestId":"request-1","timestamp":2.0,"response":{"url":"https://example.com/api","status":409}}),
    );
    let after = worker.diagnostics_read(context, &Query::default()).unwrap();
    assert_eq!(after["dropped"], 3);
    assert_eq!(after["forgottenRequestIdentities"], 1);
    assert_eq!(after["events"][0]["data"]["initialEventMissing"], true);
    assert_ne!(
        before["events"][0]["data"]["requestId"],
        after["events"][0]["data"]["requestId"]
    );
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_diagnostics_partial_setup_stop_and_neighbor_scope_remain_owned() {
    let (mut worker, mut peer, _root, context, _) = page_fixture().await;
    let neighbor = worker.index.register_context("neighbor-context").unwrap();
    worker.diagnostics.history.enable(neighbor, true);
    let task = tokio::spawn(async move {
        let result = worker
            .diagnostics_subscribe(Uuid::new_v4(), context, true, deadline())
            .await;
        (worker, result)
    });
    let enable = command(&mut peer, "Network.enable").await;
    assert_eq!(enable["params"]["maxTotalBufferSize"], 1048576);
    send(&mut peer, json!({"id":enable["id"],"sessionId":enable["sessionId"],"error":{"code":-32000,"message":"Acme-private failure"}})).await;
    let (mut worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap_err().code, "BROWSER_METHOD_FAILED");
    assert_eq!(
        worker.diagnostics_read(context, &Query::default()).unwrap()["subscriptionPending"],
        true
    );
    let stopped = worker
        .diagnostics_subscribe(Uuid::new_v4(), context, false, deadline())
        .await
        .unwrap();
    assert_eq!(stopped["active"], false);
    assert_eq!(stopped["subscriptionPending"], false);
    assert!(worker.diagnostics.history.active(neighbor));
    let read = worker
        .diagnostics_read(neighbor, &Query::default())
        .unwrap();
    assert_eq!(read["events"], json!([]));
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn source_loss_releases_console_objects_without_receiving_their_argument_ids() {
    let (mut worker, mut peer, _root, _, _) = page_fixture().await;
    worker.apply(EventItem::Gap { dropped: 1 }).unwrap();
    let task = tokio::spawn(async move {
        worker.discard_diagnostic_objects().await;
        worker
    });
    let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
    assert_eq!(release["sessionId"], "session-acme");
    assert_eq!(release["params"]["objectGroup"], "console");
    reply(&mut peer, &release, json!({})).await;
    let mut worker = task.await.unwrap();
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn diagnostic_batch_observation_keeps_the_callback_lane_responsive() {
    let (mut worker, _peer, _root, context, _) = page_fixture().await;
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "callback-fixture".into())
        .unwrap();
    let (sender, receiver) = mpsc::channel(1);
    worker.responses = Some(receiver);
    sender
        .send(Command {
            queue_slot: None,
            id,
            request: Some(Request::CallbackResolve {
                callback: Reference {
                    id: Uuid::new_v4(),
                    ..context
                },
                response: contract::CallbackResponse::Dialog {
                    accept: false,
                    prompt_text: None,
                },
            }),
            deadline: deadline(),
        })
        .await
        .unwrap();
    worker.observation_tick(deadline()).await.unwrap();
    let operation = worker.shared.state.lock().unwrap().operations[&id].clone();
    assert!(
        operation.settled,
        "A diagnostic batch wait must service callback replies"
    );
    assert_eq!(operation.code.as_deref(), Some("STALE_REFERENCE"));
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_console_replay_is_excluded_and_cleanup_retries_on_the_control_lane() {
    let (mut worker, mut peer, _root, context, _) = page_fixture().await;
    worker.diagnostics.history.enable(context, true);
    worker.configured.clear();
    event(
        &mut worker,
        "Runtime.consoleAPICalled",
        json!({"type":"log","timestamp":1.0,"args":[{"type":"string","value":"Acme old message"}]}),
    );
    assert_eq!(
        worker.diagnostics_read(context, &Query::default()).unwrap()["events"],
        json!([])
    );
    worker.configured.insert("session-acme".into());
    for succeeds in [false, true] {
        let task = tokio::spawn(async move {
            worker.discard_diagnostic_objects().await;
            worker
        });
        let discard = command(&mut peer, "Runtime.releaseObjectGroup").await;
        assert_eq!(discard["sessionId"], "session-acme");
        assert_eq!(discard["params"]["objectGroup"], "console");
        if succeeds {
            reply(&mut peer, &discard, json!({})).await;
        } else {
            send(&mut peer, json!({"id":discard["id"],"sessionId":discard["sessionId"],"error":{"code":-32000,"message":"Acme cleanup failed"}})).await;
        }
        worker = task.await.unwrap();
        assert_eq!(worker.shared.state.lock().unwrap().phase, Phase::Ready);
    }
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_diagnostics_identity_retention_is_bounded_under_redirects_and_reuse() {
    let (mut worker, _peer, _root, context, _) = page_fixture().await;
    worker.diagnostics.history.enable(context, true);
    let capacity = crate::browser::diagnostics::MAX_REQUESTS;
    for i in 0..capacity + 88 {
        event(
            &mut worker,
            "Network.requestWillBeSent",
            request(&format!("request-{i}")),
        );
    }
    for _ in 0..100 {
        event(
            &mut worker,
            "Network.requestWillBeSent",
            request(&format!("request-{}", capacity + 87)),
        );
    }
    let summary = worker.diagnostics.history.summary();
    assert_eq!(summary["records"], capacity + 188);
    assert_eq!(summary["forgottenRequestIdentities"], 88);
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_diagnostics_dev_page_burst_retains_document_modules_and_console_until_read() {
    use crate::browser::cdp::events::Buffer;
    let (mut worker, _peer, _root, context, _) = page_fixture().await;
    worker.diagnostics.history.enable(context, true);
    let buffer = Buffer::default();
    let mut events = Events(buffer.clone());
    // Queue an entire load before allowing the actor to consume it, as can
    // happen while a navigation command awaits its reply. Interleave requests
    // so all 1,000 identities must survive until their terminal events arrive.
    for method in [
        "Network.requestWillBeSent",
        "Network.requestWillBeSentExtraInfo",
        "Network.responseReceived",
        "Network.responseReceivedExtraInfo",
        "Network.dataReceived",
        "Network.loadingFinished",
    ] {
        for i in 0..1000 {
            let id = format!("request-{i}");
            let params = match method {
                "Network.requestWillBeSent" => {
                    json!({"requestId":id,"timestamp":1.0,"type":if i == 0 {"Document"} else {"Script"},"request":{"url":if i == 0 {"https://example.com/".to_owned()} else {format!("https://example.com/module-{i}.js")},"method":"GET"}})
                }
                "Network.responseReceived" => {
                    json!({"requestId":id,"timestamp":2.0,"response":{"url":"https://example.com/module.js","status":200}})
                }
                "Network.requestWillBeSentExtraInfo" | "Network.responseReceivedExtraInfo" => {
                    json!({"requestId":id,"headers":{"Content-Type":"application/javascript"}})
                }
                "Network.dataReceived" => {
                    json!({"requestId":id,"dataLength":1024,"encodedDataLength":1024})
                }
                _ => json!({"requestId":id,"timestamp":3.0,"encodedDataLength":1024}),
            };
            let bytes = serde_json::to_vec(&params).unwrap().len() + 128;
            buffer
                .push(
                    Event {
                        method: method.into(),
                        session_id: Some("session-acme".into()),
                        params,
                    },
                    bytes,
                )
                .unwrap();
        }
    }
    buffer
        .push(
            Event {
                method: "Runtime.consoleAPICalled".into(),
                session_id: Some("session-acme".into()),
                params: json!({"type":"log","args":[{"type":"string","value":"Acme loaded"}]}),
            },
            256,
        )
        .unwrap();
    while let Some(item) = events.try_next() {
        match item {
            EventItem::Event(event) => worker.diagnostic_event(&event),
            EventItem::Gap { dropped } => panic!("Page load lost {dropped} source events"),
        }
    }
    let summary = worker.diagnostics.history.summary();
    assert_eq!(summary["records"], 3001);
    assert_eq!(summary["dropped"], 0);
    assert_eq!(summary["forgottenRequestIdentities"], 0);
    let mut query = Query {
        limit: 100,
        include_text: true,
        ..Default::default()
    };
    let mut retained = Vec::new();
    loop {
        let read = worker.diagnostics_read(context, &query).unwrap();
        retained.extend(read["events"].as_array().unwrap().iter().cloned());
        query.cursor = read["nextCursor"].as_str().map(str::to_owned);
        if query.cursor.is_none() {
            break;
        }
    }
    assert_eq!(retained.len(), 3001);
    assert_eq!(retained[0]["sequence"], 1);
    assert_eq!(retained[0]["data"]["url"], "https://example.com/");
    let document_id = retained[0]["data"]["requestId"].clone();
    assert_eq!(retained[1000]["data"]["requestId"], document_id);
    assert_eq!(retained[2000]["data"]["requestId"], document_id);
    assert_eq!(retained[3000]["data"]["text"], "Acme loaded");
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_diagnostics_inactive_source_loss_preserves_stopped_history_and_drop_count() {
    let (mut worker, _peer, _root, context, _) = page_fixture().await;
    worker.apply(EventItem::Gap { dropped: 1576 }).unwrap();
    assert_eq!(worker.diagnostics.history.summary()["dropped"], 0);
    worker.diagnostics.history.enable(context, true);
    event(
        &mut worker,
        "Network.requestWillBeSent",
        request("document"),
    );
    worker.diagnostics.stop(context);
    let before = worker.diagnostics_read(context, &Query::default()).unwrap();
    worker.apply(EventItem::Gap { dropped: 100 }).unwrap();
    let after = worker.diagnostics_read(context, &Query::default()).unwrap();
    assert_eq!(before, after);
    worker.close(deadline()).await.unwrap();
}
