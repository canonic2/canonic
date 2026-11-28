//! Owned initializers feed ordinary HTTP without borrowing a live source page.
use super::investigation::owned_page;
use super::*;
use crate::http_fixtures::wire_inputs as wire;

#[tokio::test]
async fn http_request_input_route_identity_waits_without_mutation_and_pins_redirect_generation() {
    for network_first in [true, false] {
        let (mut worker, mut peer, _root, context, page) = owned_page().await;
        let setup = tokio::spawn(async move {
            worker
                .page_route_set(
                    Uuid::new_v4(),
                    page,
                    Some("https://example.com/Acme".into()),
                    false,
                    deadline(),
                )
                .await
                .unwrap();
            worker
        });
        let enable = command(&mut peer, "Fetch.enable").await;
        reply(&mut peer, &enable, json!({})).await;
        worker = setup.await.unwrap();
        worker.diagnostics.history.enable(context, true);
        worker
            .request_inputs
            .set(context.connection_id, context, true)
            .unwrap();
        let network = Event {
            method: "Network.requestWillBeSent".into(),
            session_id: Some("session-acme".into()),
            params: json!({"requestId":"Acme-network","timestamp":1.0,"type":"Fetch","request":{"url":"https://example.com/Acme","method":"POST","headers":{"Cookie":"browser"},"postDataEntries":[{"bytes":"AQID"}]}}),
        };
        if network_first {
            worker.diagnostic_event(&network);
        }
        worker.route_event(&Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"Acme-fetch","networkId":"Acme-network","request":{"url":"https://example.com/Acme","method":"POST","headers":{"Cookie":"browser"}},"frameId":"frame-acme","resourceType":"Fetch"})}).unwrap();
        let callback = *worker.interactions.routes.keys().next().unwrap();
        let response=contract::CallbackResponse::RouteContinueWith{overrides:serde_json::from_value(json!({"url":"https://example.com/changed","method":"PUT","headers":[{"name":"Cookie","value":"AcmeIntent=private"}],"bodyBase64":"AP8="})).unwrap()};
        if !network_first {
            assert_eq!(
                worker
                    .resolve_callback(Uuid::new_v4(), callback, response.clone(), deadline())
                    .await
                    .unwrap_err()
                    .code,
                "HTTP_REQUEST_INPUT_NOT_READY"
            );
            assert!(!worker.interactions.routes[&callback].responding);
            assert!(
                tokio::time::timeout(Duration::from_millis(25), peer.next())
                    .await
                    .is_err()
            );
            worker.diagnostic_event(&network);
        }
        let read = worker
            .diagnostics_read(context, &crate::browser::diagnostics::Query::default())
            .unwrap();
        let id = serde_json::from_value(read["events"][0]["data"]["requestId"].clone()).unwrap();
        let original = worker
            .request_inputs
            .get(context.connection_id, id)
            .unwrap();
        let continuing = tokio::spawn(async move {
            let result = worker
                .resolve_callback(Uuid::new_v4(), callback, response, deadline())
                .await;
            (worker, result)
        });
        let call = command(&mut peer, "Fetch.continueRequest").await;
        assert_eq!(call["params"]["url"], "https://example.com/changed");
        assert!(
            !call["params"]["headers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|header| header["name"]
                    .as_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case("cookie")))
        );
        reply(&mut peer, &call, json!({})).await;
        let (returned, result) = continuing.await.unwrap();
        worker = returned;
        result.unwrap();
        let copied = worker
            .request_inputs
            .get(context.connection_id, id)
            .unwrap()
            .fetch(Default::default())
            .unwrap();
        assert_eq!(copied.url, "https://example.com/changed");
        assert_eq!(copied.method, "PUT");
        assert_eq!(
            serde_json::to_value(copied.body).unwrap(),
            json!({"kind":"base64","data":"AP8="})
        );
        assert_eq!(copied.headers[0].value, "AcmeIntent=private");
        assert_eq!(original.body().unwrap(), [1, 2, 3]);
        verify_redirect_generation(&mut worker, context, id, network);
    }
}

#[test]
fn http_request_input_native_admission_accepts_all_registered_operations_and_batch_wrappers() {
    let context = Reference {
        connection_id: Uuid::new_v4(),
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let source = Uuid::new_v4();
    for input in [
        json!({"operation":"http_request_inputs_set","context":context,"enabled":true}),
        json!({"operation":"http_request_inputs_set","context":context,"enabled":false}),
        json!({"operation":"http_fetch_request","httpContext":context,"sourceRequestId":source}),
        json!({"operation":"http_request_input_remove","sourceRequestId":source}),
    ] {
        let request: Request = serde_json::from_value(input.clone()).unwrap();
        supported(&request).unwrap();
        crate::schema::validate(&contract::schemas()["request"], &input).unwrap();
        supported(&Request::Batch {
            requests: vec![request],
        })
        .unwrap();
    }
}

#[tokio::test]
async fn http_request_input_native_release_during_http_response_keeps_the_dispatched_copy_alive() {
    let site = wire::Site::start().await;
    site.hold("/Acme-held");
    let (mut worker, _peer, _root, context, _page) = owned_page().await;
    let created = worker
        .http_operation(
            Uuid::new_v4(),
            serde_json::from_value(json!({"operation":"http_context_create","options":{}}))
                .unwrap(),
            deadline(),
        )
        .await
        .unwrap();
    let source_id = Uuid::new_v4();
    let source=crate::browser::http::request_inputs::Snapshot::native(&json!({"url":format!("{}/Acme-held",site.url),"method":"POST","headers":{"Content-Length":"6"},"postDataEntries":[{"bytes":"AP+A"}]})).unwrap();
    worker
        .request_inputs
        .insert(context.connection_id, context, source_id, source)
        .unwrap();
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "fixture".into())
        .unwrap();
    let shared = worker.shared.clone();
    let (tx, rx) = mpsc::channel(8);
    worker.responses = Some(rx);
    let fetching = tokio::spawn(async move {
        let result=worker.http_operation(id,serde_json::from_value(json!({"operation":"http_fetch_request","httpContext":created["httpContext"],"sourceRequestId":source_id})).unwrap(),deadline()).await;
        (worker, result)
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while site.records.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let release_id = Uuid::new_v4();
    shared
        .state
        .lock()
        .unwrap()
        .begin(release_id, "fixture".into())
        .unwrap();
    tx.send(Command {
        queue_slot: None,
        id: release_id,
        request: Some(Request::HttpRequestInputRemove {
            source_request_id: source_id,
        }),
        deadline: deadline(),
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !shared.state.lock().unwrap().operations[&release_id].settled {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let released = shared.state.lock().unwrap().operations[&release_id].clone();
    assert_eq!(released.outcome, Outcome::Succeeded, "{released:?}");
    assert_eq!(released.result.unwrap()["removed"], true);
    assert!(!fetching.is_finished());
    site.release();
    let (worker, result) = fetching.await.unwrap();
    assert_eq!(result.unwrap()["status"], 200);
    assert!(
        worker
            .request_inputs
            .get(context.connection_id, source_id)
            .is_err()
    );
    assert_eq!(site.records.lock().unwrap()[0]["bodyBase64"], "AP+A");
    assert_eq!(
        site.records.lock().unwrap()[0]["headers"]["content-length"],
        "3"
    );
}

#[tokio::test]
async fn http_request_input_native_responsive_dispatch_disables_capture_and_releases_retained_sources()
 {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let connection = context.connection_id;
    let source_id = Uuid::new_v4();
    let snapshot = crate::browser::http::request_inputs::Snapshot::native(&json!({"url":"https://example.com/Acme","method":"POST","headers":{},"postDataEntries":[{"bytes":"AP+A"}]})).unwrap();
    worker
        .request_inputs
        .set(connection, context, true)
        .unwrap();
    worker
        .request_inputs
        .insert(connection, context, source_id, snapshot)
        .unwrap();
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "fixture".into())
        .unwrap();
    let disable = tokio::spawn(async move {
        worker
            .respond(Command {
                queue_slot: None,
                id,
                request: Some(Request::HttpRequestInputsSet {
                    context,
                    enabled: false,
                }),
                deadline: deadline(),
            })
            .await;
        worker
    });
    let budget = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &budget, json!({})).await;
    worker = disable.await.unwrap();
    let result = worker.shared.state.lock().unwrap().operations[&id].clone();
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    assert!(!worker.request_inputs.enabled(context));
    let retained = worker.request_inputs.get(connection, source_id).unwrap();
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "fixture".into())
        .unwrap();
    worker
        .respond(Command {
            queue_slot: None,
            id,
            request: Some(Request::HttpRequestInputRemove {
                source_request_id: source_id,
            }),
            deadline: deadline(),
        })
        .await;
    let result = worker.shared.state.lock().unwrap().operations[&id].clone();
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    assert_eq!(result.result.unwrap()["removed"], true);
    assert!(worker.request_inputs.get(connection, source_id).is_err());
    assert_eq!(retained.body().unwrap(), [0, 255, 128]);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn http_request_input_native_fetch_retains_binary_source_after_capture_disable_and_release() {
    let site = wire::Site::start().await;
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let created = worker
        .http_operation(
            Uuid::new_v4(),
            serde_json::from_value(json!({"operation":"http_context_create","options":{}}))
                .unwrap(),
            deadline(),
        )
        .await
        .unwrap();
    let connection = worker.shared.state.lock().unwrap().connection;
    let source_id = Uuid::new_v4();
    let snapshot = crate::browser::http::request_inputs::Snapshot::native(&json!({
        "url":format!("{}/Acme?old=one",site.url),"method":"POST",
        "headers":{"X-Acme":"source"},"postDataEntries":[{"bytes":"AP+A"}]
    }))
    .unwrap();
    worker
        .request_inputs
        .set(connection, context, true)
        .unwrap();
    worker
        .request_inputs
        .insert(connection, context, source_id, snapshot)
        .unwrap();
    worker.request_inputs.close_context(context);
    let request: Request = serde_json::from_value(json!({"operation":"http_fetch_request","httpContext":created["httpContext"],"sourceRequestId":source_id,"options":{"method":"","headers":[],"params":"Acme=one&Acme=two"}})).unwrap();
    assert!(super::super::execution::independent(&request));
    let response = worker
        .http_operation(Uuid::new_v4(), request, deadline())
        .await
        .unwrap();
    assert_eq!(response["status"], 200);
    let records = site.records.lock().unwrap().clone();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["method"], "POST");
    assert_eq!(records[0]["url"], "/Acme?Acme=one&Acme=two");
    assert_eq!(records[0]["bodyBase64"], "AP+A");
    assert!(records[0]["headers"].get("x-acme").is_none());
    let source = worker.request_inputs.get(connection, source_id).unwrap();
    let removal: Request = serde_json::from_value(
        json!({"operation":"http_request_input_remove","sourceRequestId":source_id}),
    )
    .unwrap();
    assert_eq!(
        worker
            .http_operation(Uuid::new_v4(), removal.clone(), deadline())
            .await
            .unwrap()["removed"],
        true
    );
    assert_eq!(
        worker
            .http_operation(Uuid::new_v4(), removal, deadline())
            .await
            .unwrap()["removed"],
        false
    );
    assert_eq!(source.fetch(Default::default()).unwrap().method, "POST");
    let missing = serde_json::from_value(json!({"operation":"http_fetch_request","httpContext":created["httpContext"],"sourceRequestId":source_id})).unwrap();
    assert_eq!(
        worker
            .http_operation(Uuid::new_v4(), missing, deadline())
            .await
            .unwrap_err()
            .code,
        "HTTP_REQUEST_INPUT_UNKNOWN"
    );
    assert_eq!(site.records.lock().unwrap().len(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Source reuse makes no page/CDP body query"
    );
}

fn verify_redirect_generation(worker: &mut Worker, context: Reference, id: Uuid, network: Event) {
    let mut redirect = network;
    redirect.params["request"] =
        json!({"url":"https://example.com/redirected","method":"GET","headers":{}});
    redirect.params["redirectResponse"] = json!({"status":302});
    worker.diagnostic_event(&redirect);
    let foreign = Reference {
        id: Uuid::new_v4(),
        ..context
    };
    assert!(
        worker
            .diagnostics
            .take_request_input_id(foreign, "session-acme", "Acme-network")
            .is_none()
    );
    let next = worker
        .diagnostics
        .take_request_input_id(context, "Acme-child-inspector", "Acme-network")
        .unwrap();
    assert_ne!(next, id);
    assert_eq!(
        worker
            .request_inputs
            .get(context.connection_id, id)
            .unwrap()
            .fetch(Default::default())
            .unwrap()
            .url,
        "https://example.com/changed"
    );
}
