//! New-document events arriving before navigation replies retain their authority.
use super::*;
use crate::browser::page::{Load, Navigation};

pub(super) async fn isolated_scope(peer: &mut Peer, fresh: bool) {
    isolated_frame_scope(peer, "frame-acme", fresh).await;
}

pub(super) async fn isolated_frame_scope(peer: &mut Peer, frame: &str, fresh: bool) {
    let create = command(peer, "Page.createIsolatedWorld").await;
    assert_eq!(create["params"]["frameId"], frame);
    if fresh {
        send(peer,json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":2,"uniqueId":"isolated-acme","origin":"://","name":page::WORLD,"auxData":{"frameId":frame,"isDefault":false}}}})).await;
    }
    reply(peer, &create, json!({"executionContextId":2})).await;
    runtime_value(peer, Value::Null).await;
}

async fn runtime_value(peer: &mut Peer, value: Value) {
    let evaluate = command(peer, "Runtime.evaluate").await;
    assert_eq!(evaluate["params"]["uniqueContextId"], "isolated-acme");
    reply(
        peer,
        &evaluate,
        json!({"result":{"type":"object","value":value}}),
    )
    .await;
}

#[tokio::test]
async fn load_navigation_follows_client_redirect_without_replaying_navigation() {
    let (mut worker, mut peer, _root, _, page) = super::investigation::owned_page().await;
    let task = tokio::spawn(async move {
        let result = worker
            .navigate(
                Uuid::new_v4(),
                page,
                Navigation::Goto {
                    url: "https://app.example.com/".into(),
                    wait_until: Load::Load,
                },
                deadline(),
            )
            .await
            .unwrap();
        assert_eq!(result["url"], "https://auth.example.com/");
        assert_eq!(result["response"]["status"], 200);
        worker
    });
    isolated_scope(&mut peer, true).await;
    let lifecycle = command(&mut peer, "Page.setLifecycleEventsEnabled").await;
    reply(&mut peer, &lifecycle, json!({})).await;
    let network = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &network, json!({})).await;
    let navigate = command(&mut peer, "Page.navigate").await;
    for (loader, url) in [
        ("loader-app", "https://app.example.com/"),
        ("loader-auth", "https://auth.example.com/"),
    ] {
        send(&mut peer,json!({"method":"Runtime.executionContextsCleared","sessionId":"session-acme","params":{}})).await;
        send(&mut peer,json!({"method":"Page.frameNavigated","sessionId":"session-acme","params":{"type":"Navigation","frame":{"id":"frame-acme","loaderId":loader,"url":url,"domainAndRegistry":"example.com","securityOrigin":url.trim_end_matches('/'),"mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await;
        send(&mut peer,json!({"method":"Page.lifecycleEvent","sessionId":"session-acme","params":{"frameId":"frame-acme","loaderId":loader,"name":"init","timestamp":1.0}})).await;
    }
    reply(
        &mut peer,
        &navigate,
        json!({"frameId":"frame-acme","loaderId":"loader-app"}),
    )
    .await;
    send(&mut peer,json!({"method":"Network.responseReceived","sessionId":"session-acme","params":{"requestId":"request-auth","frameId":"frame-acme","loaderId":"loader-auth","timestamp":1.5,"type":"Document","hasExtraInfo":false,"response":{"url":"https://auth.example.com/","status":200,"statusText":"OK","headers":{},"mimeType":"text/html","charset":"utf-8","connectionReused":false,"connectionId":1,"encodedDataLength":64,"securityState":"secure"}}})).await;
    send(&mut peer,json!({"method":"Page.lifecycleEvent","sessionId":"session-acme","params":{"frameId":"frame-acme","loaderId":"loader-auth","name":"load","timestamp":2.0}})).await;
    let world = command(&mut peer, "Page.createIsolatedWorld").await;
    send(&mut peer,json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":3,"uniqueId":"isolated-auth","origin":"https://auth.example.com","name":page::WORLD,"auxData":{"frameId":"frame-acme","isDefault":false}}}})).await;
    reply(&mut peer, &world, json!({"executionContextId":3})).await;
    for value in [Value::Null, json!({"url":"https://auth.example.com/"})] {
        let evaluate = command(&mut peer, "Runtime.evaluate").await;
        assert_eq!(evaluate["params"]["uniqueContextId"], "isolated-auth");
        reply(
            &mut peer,
            &evaluate,
            json!({"result":{"type":"object","value":value}}),
        )
        .await;
    }
    let mut worker = task.await.unwrap();
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn navigation_method_failure_keeps_cdp_code_and_message_in_the_batch_receipt() {
    let (mut worker, mut peer, _root, _, page) = super::investigation::owned_page().await;
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "navigate".into())
        .unwrap();
    let task = tokio::spawn(async move {
        let result = worker
            .execute(
                id,
                Request::Batch {
                    requests: vec![Request::PageNavigate {
                        page,
                        navigation: Navigation::Goto {
                            url: "https://example.com/".into(),
                            wait_until: Load::Commit,
                        },
                    }],
                },
                deadline(),
            )
            .await;
        assert_eq!(result.as_ref().unwrap_err().code, "BROWSER_METHOD_FAILED");
        worker.finish(id, result).await;
        let receipt = worker.shared.state.lock().unwrap().operations[&id]
            .result
            .clone()
            .unwrap();
        assert_eq!(receipt["failedIndex"], 0);
        assert_eq!(receipt["completedActions"], 0);
        assert_eq!(
            receipt["partial"]["backendFailure"],
            json!({"source":"cdp-reply","kind":"method-error","dispatch":"replied","code":-32602,"message":"Invalid parameters"})
        );
        assert!(!receipt.to_string().contains("Acme private"));
        assert_eq!(worker.shared.state.lock().unwrap().phase, Phase::Ready);
        worker
    });
    isolated_scope(&mut peer, true).await;
    let lifecycle = command(&mut peer, "Page.setLifecycleEventsEnabled").await;
    reply(&mut peer, &lifecycle, json!({})).await;
    let network = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &network, json!({})).await;
    let navigate = command(&mut peer, "Page.navigate").await;
    send(&mut peer, json!({"id":navigate["id"],"sessionId":"session-acme","error":{"code":-32602,"message":"Invalid parameters","data":"Acme private data"}})).await;
    let mut worker = task.await.unwrap();
    worker.close(deadline()).await.unwrap();
}

#[test]
fn method_failure_message_is_bounded_without_splitting_unicode_or_exposing_data() {
    let error = protocol_error(super::super::super::Error::Remote(
        super::super::super::RemoteError::Command {
            code: -32602,
            message: "é".repeat(2050),
            data: Some(json!("Acme private data")),
        },
    ));
    let partial = error.partial.unwrap();
    assert_eq!(
        partial["backendFailure"]["message"].as_str().unwrap(),
        "é".repeat(2048)
    );
    assert!(partial["backendFailure"].get("data").is_none());
}

#[tokio::test]
async fn content_setup_keeps_new_child_events_delivered_before_the_reply() {
    let (mut worker, mut peer, _root) = fixture().await;
    let id = Uuid::new_v4();
    let task = tokio::spawn(async move {
        let context = worker.context_create(id, deadline()).await.unwrap();
        let page = worker.page_create(id, context, deadline()).await.unwrap();
        let old = worker.index.page_frame(page, None).unwrap();
        let result = worker
            .navigate(
                id,
                page,
                Navigation::SetContent {
                    html: "<iframe></iframe>".into(),
                    wait_until: Load::Load,
                },
                deadline(),
            )
            .await
            .unwrap();
        assert_eq!(result["documentReplaced"], true);
        assert_eq!(
            worker.index.page_frame(page, Some(old)).unwrap_err(),
            super::super::super::ResourceError::Stale
        );
        let child = worker
            .index
            .event_frame("session-acme", "new-child")
            .unwrap();
        assert!(worker.index.page_frame(page, Some(child)).is_ok());
        worker
    });
    serve_page_allocation(&mut peer).await;
    isolated_scope(&mut peer, true).await;
    let lifecycle = command(&mut peer, "Page.setLifecycleEventsEnabled").await;
    reply(&mut peer, &lifecycle, json!({})).await;
    let network = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &network, json!({})).await;
    runtime_value(&mut peer, Value::Null).await;
    let content = command(&mut peer, "Page.setDocumentContent").await;
    send(&mut peer,json!({"method":"Page.frameAttached","sessionId":"session-acme","params":{"frameId":"new-child","parentFrameId":"frame-acme"}})).await;
    send(&mut peer,json!({"method":"Page.lifecycleEvent","sessionId":"session-acme","params":{"frameId":"frame-acme","loaderId":"loader-acme","name":"load","timestamp":1.0}})).await;
    reply(&mut peer, &content, json!({})).await;
    isolated_scope(&mut peer, false).await;
    runtime_value(&mut peer, json!({"url":"about:blank"})).await;
    let mut worker = task.await.unwrap();
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn cancelled_private_read_does_not_lose_the_connection_or_replay_work() {
    let (mut worker, mut peer, _root) = fixture().await;
    let id = Uuid::new_v4();
    let shared = worker.shared.clone();
    shared
        .state
        .lock()
        .unwrap()
        .begin(id, "read".into())
        .unwrap();
    let task = tokio::spawn(async move {
        let error = worker
            .call_read(
                id,
                None,
                "Runtime.evaluate",
                json!({"expression":"1"}),
                deadline(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "CANCELLED");
        worker.finish(id, Err(error)).await;
        assert_eq!(worker.shared.state.lock().unwrap().phase, Phase::Ready);
        assert_eq!(
            worker.shared.state.lock().unwrap().operations[&id].outcome,
            Outcome::Cancelled
        );
        worker
            .call_read(
                Uuid::new_v4(),
                None,
                "Runtime.evaluate",
                json!({"expression":"2"}),
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let first = command(&mut peer, "Runtime.evaluate").await;
    shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&id)
        .unwrap()
        .cancel_requested = true;
    shared.changed.notify_waiters();
    let second = command(&mut peer, "Runtime.evaluate").await;
    assert_ne!(first["id"], second["id"]);
    assert_eq!(second["params"]["expression"], "2");
    reply(
        &mut peer,
        &first,
        json!({"result":{"type":"number","value":1}}),
    )
    .await;
    reply(
        &mut peer,
        &second,
        json!({"result":{"type":"number","value":2}}),
    )
    .await;
    let mut worker = task.await.unwrap();
    worker.close(deadline()).await.unwrap();
}
