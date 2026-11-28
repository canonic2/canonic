//! A retained initializer is separate from the SDK's live source object graph.
use super::*;
use crate::http_fixtures::wire_inputs as wire;

#[test]
fn http_request_input_private_release_and_capture_disable_remain_responsive_cleanup() {
    let context = Reference {
        connection_id: Uuid::new_v4(),
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    for input in [
        json!({"operation":"http_request_input_remove","sourceRequestId":Uuid::new_v4()}),
        json!({"operation":"http_request_inputs_set","context":context,"enabled":false}),
    ] {
        let request: Request = serde_json::from_value(input).unwrap();
        assert!(super::super::control::responsive(&request));
        assert!(super::super::control::independent(&request, true));
        assert!(!super::super::control::fenced(&request));
    }
    let request: Request = serde_json::from_value(
        json!({"operation":"http_request_inputs_set","context":context,"enabled":true}),
    )
    .unwrap();
    assert!(!super::super::control::responsive(&request));
    assert!(super::super::control::fenced(&request));
}
async fn run(session: &Session, input: Value) -> Value {
    let id = session
        .submit(Uuid::new_v4(), serde_json::from_value(input).unwrap(), 5000)
        .unwrap();
    let operation = complete(session, id).await;
    assert_eq!(operation.outcome, Outcome::Succeeded, "{operation:?}");
    operation.result.unwrap()
}
#[tokio::test]
async fn request_bodies_sent_during_trusted_entry_are_never_captured_or_read() {
    let (session, mut peer, _root, context, _page) = harness().await;
    diagnostic_subscription(&session, &mut peer, context, true).await;
    let diagnostics = || session.0.host.diagnostics.lock().unwrap();
    diagnostics().withhold_bodies(context, true);
    peer.send(&created("Acme-sign-in", "Request", json!({"url":"https://example.com/Acme-sign-in","method":"POST","headers":[],"postData":"QWNtZS1wYXNzd29yZA==","frame":{"guid":"frame"},"isNavigationRequest":false,"resourceType":"fetch"}))).await;
    peer.send(&json!({"guid":"context","method":"request","params":{"request":{"guid":"Acme-sign-in"},"page":{"guid":"page"}}})).await;
    let read = run(
        &session,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    diagnostics().withhold_bodies(context, false);
    let event = read["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["kind"] == "request")
        .unwrap();
    assert_eq!(event["data"]["requestBodyWithheld"], true);
    assert!(event["data"].get("requestInputAvailable").is_none());
    let id = session
        .submit(
            Uuid::new_v4(),
            serde_json::from_value(json!({"operation":"network_read","context":context,"requestId":event["data"]["requestId"],"options":{"body":"request"}})).unwrap(),
            5000,
        )
        .unwrap();
    let operation = complete(&session, id).await;
    assert_eq!(operation.code.as_deref(), Some("BODY_WITHHELD"));
    assert!(!format!("{operation:?}").contains("QWNtZS1wYXNzd29yZA"));
    close(&session).await;
}
#[tokio::test]
async fn http_request_input_default_sdk_captures_during_subscription_reply_and_keeps_opt_out() {
    let (session, mut peer, _root, context, _page) = harness().await;
    let id = session
        .submit(Uuid::new_v4(), Request::DiagnosticsStart { context }, 5000)
        .unwrap();
    for subscription in crate::browser::diagnostics::SUBSCRIPTIONS {
        let call = peer.request().await;
        assert_eq!(call["method"], "updateSubscription");
        assert_eq!(call["params"], json!({"event":subscription,"enabled":true}));
        if *subscription == "request" {
            peer.send(&created("Acme-default-request", "Request", json!({"url":"https://example.com/Acme-default","method":"POST","headers":[{"name":"Authorization","value":"Acme-private"}],"postData":"AP+A","frame":{"guid":"frame"},"isNavigationRequest":false,"resourceType":"fetch"}))).await;
            peer.send(&json!({"guid":"context","method":"request","params":{"request":{"guid":"Acme-default-request"},"page":{"guid":"page"}}})).await;
        }
        peer.send(&json!({"id":call["id"],"result":{}})).await;
    }
    let started = complete(&session, id).await;
    assert_eq!(started.outcome, Outcome::Succeeded, "{started:?}");
    let read = run(
        &session,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    let event = read["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["kind"] == "request")
        .unwrap();
    assert_eq!(event["data"]["requestInputAvailable"], true);
    assert!(!read.to_string().contains("Acme-private"));
    assert!(!read.to_string().contains("AP+A"));
    let source = event["data"]["requestId"].clone();
    let body = run(&session, json!({"operation":"network_read","context":context,"requestId":source,"options":{"body":"request"}})).await;
    assert_eq!(body["requestBodyFidelity"], "binary-request-initializer");
    assert_eq!(
        std::fs::read(body["body"]["attachment"]["path"].as_str().unwrap()).unwrap(),
        [0, 255, 128]
    );
    diagnostic_subscription(&session, &mut peer, context, false).await;
    run(
        &session,
        json!({"operation":"http_request_inputs_set","context":context,"enabled":false}),
    )
    .await;
    diagnostic_subscription(&session, &mut peer, context, true).await;
    assert!(
        !session
            .0
            .host
            .request_inputs
            .lock()
            .unwrap()
            .enabled(context)
    );
    assert!(
        session
            .0
            .host
            .request_inputs
            .lock()
            .unwrap()
            .get(
                context.connection_id,
                serde_json::from_value(source).unwrap()
            )
            .is_ok()
    );
    peer.send(&created("Acme-opted-out-request", "Request", json!({"url":"https://example.com/Acme-opted-out","method":"POST","headers":[],"postData":"AP+A","frame":{"guid":"frame"},"isNavigationRequest":false,"resourceType":"fetch"}))).await;
    peer.send(&json!({"guid":"context","method":"request","params":{"request":{"guid":"Acme-opted-out-request"},"page":{"guid":"page"}}})).await;
    let read = run(
        &session,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    let event = read["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["data"]["url"] == "https://example.com/Acme-opted-out")
        .unwrap();
    assert!(event["data"].get("requestInputAvailable").is_none());
    let unavailable: Uuid = serde_json::from_value(event["data"]["requestId"].clone()).unwrap();
    assert_eq!(
        session
            .0
            .host
            .request_inputs
            .lock()
            .unwrap()
            .get(context.connection_id, unavailable)
            .unwrap_err()
            .code,
        "HTTP_REQUEST_INPUT_UNKNOWN"
    );
    close(&session).await;
}
#[tokio::test]
async fn http_request_input_sdk_fetch_survives_context_close_and_preserves_empty_overrides() {
    let site = wire::Site::start().await;
    let (session, mut peer, _root, context, _page) = harness().await;
    let client = run(
        &session,
        json!({"operation":"http_context_create","options":{}}),
    )
    .await["httpContext"]
        .clone();
    let enabled = run(
        &session,
        json!({"operation":"http_request_inputs_set","context":context,"enabled":true}),
    )
    .await;
    assert_eq!(enabled["enabled"], true);
    let connection = context.connection_id;
    let source_id = Uuid::new_v4();
    let snapshot = crate::browser::http::request_inputs::Snapshot::pinned(&json!({"url":format!("{}/Acme",site.url),"method":"POST","headers":[{"name":"X-Acme","value":"source"}],"postData":"AP+A"})).unwrap();
    session
        .0
        .host
        .request_inputs
        .lock()
        .unwrap()
        .insert(connection, context, source_id, snapshot)
        .unwrap();
    peer.send(&json!({"guid":"context","method":"close","params":{}}))
        .await;
    let result = run(&session, json!({"operation":"http_fetch_request","httpContext":client,"sourceRequestId":source_id,"options":{"method":"","headers":[],"body":{"kind":"text","text":""}}})).await;
    assert_eq!(result["status"], 200);
    let records = site.records.lock().unwrap().clone();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["method"], "POST");
    assert_eq!(records[0]["bodyBase64"], "");
    assert!(records[0]["headers"].get("x-acme").is_none());
    assert!(
        !session
            .0
            .host
            .request_inputs
            .lock()
            .unwrap()
            .enabled(context)
    );
    let result = run(
        &session,
        json!({"operation":"http_fetch_request","httpContext":client,"sourceRequestId":source_id}),
    )
    .await;
    assert_eq!(result["status"], 200);
    assert_eq!(site.records.lock().unwrap()[1]["bodyBase64"], "AP+A");
    assert_eq!(
        run(
            &session,
            json!({"operation":"http_request_input_remove","sourceRequestId":source_id})
        )
        .await["removed"],
        true
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err(),
        "No live source object query"
    );
    close(&session).await;
}
