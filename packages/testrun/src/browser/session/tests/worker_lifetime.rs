//! Worker channels, rather than page-frame documents, own evaluation handles.
use super::*;

fn owned_create(parent: &str, guid: &str, kind: &str, initializer: Value) -> Value {
    let mut message = created(guid, kind, initializer);
    message["guid"] = json!(parent);
    message
}
async fn worker(session: &Session, peer: &mut Fixture, guid: &str) -> Reference {
    peer.send(&owned_create(
        "page",
        guid,
        "Worker",
        json!({"url":"https://example.com/Acme-worker.js"}),
    ))
    .await;
    peer.send(&json!({"guid":"page","method":"worker","params":{"worker":{"guid":guid}}}))
        .await;
    tokio::time::timeout_at(deadline(), async {
        loop {
            let changed = session.0.host.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(resource) = session
                .0
                .host
                .state
                .lock()
                .unwrap()
                .resources
                .values()
                .find(|r| r.kind == Kind::Worker && r.wire.guid() == guid)
                .cloned()
            {
                return resource.reference;
            }
            changed.await;
        }
    })
    .await
    .unwrap()
}
async fn wire(session: &Session, guid: &str, kind: &str) -> crate::browser::objects::ObjectRef {
    tokio::time::timeout_at(deadline(), async {
        loop {
            let changed = session.0.host.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Ok(wire) = session
                .0
                .host
                .connection()
                .unwrap()
                .resolve(guid, kind)
                .await
            {
                return wire;
            }
            changed.await;
        }
    })
    .await
    .unwrap()
}
fn evaluate(scope: Reference, retain: bool) -> Request {
    Request::Evaluate {
        scope,
        expression: "() => self.AcmeIdentity".into(),
        is_function: Some(true),
        argument: Default::default(),
        return_handle: retain,
    }
}
async fn handle(session: &Session, peer: &mut Fixture, worker: Reference, guid: &str) -> Reference {
    let id = session
        .submit(Uuid::new_v4(), evaluate(worker, true), 5000)
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "evaluateExpressionHandle");
    let parent = call["guid"].as_str().unwrap();
    peer.send(&owned_create(
        parent,
        guid,
        "JSHandle",
        json!({"preview":"Acme worker value"}),
    ))
    .await;
    peer.send(&json!({"id":call["id"],"result":{"handle":{"guid":guid}}}))
        .await;
    let done = complete(session, id).await;
    assert_eq!(done.outcome, Outcome::Succeeded, "{done:?}");
    let result = done.result.unwrap();
    assert_eq!(result["worker"], json!(worker));
    assert!(result["frame"].is_null());
    serde_json::from_value(result["handle"].clone()).unwrap()
}

#[tokio::test]
async fn worker_handle_survives_frame_replacement_during_an_inflight_read_without_worker_disposal()
{
    let (session, mut peer, _root, _, page) = harness().await;
    let worker = worker(&session, &mut peer, "Acme-worker").await;
    let handle = handle(&session, &mut peer, worker, "Acme-worker-value").await;
    peer.send(&owned_create("page","Acme-child","Frame",json!({"parentFrame":{"guid":"frame"},"url":"https://example.com/child","name":"","loadStates":[]}))).await;
    peer.send(&owned_create(
        "Acme-child",
        "Acme-frame-value",
        "JSHandle",
        json!({"preview":"Acme document value"}),
    ))
    .await;
    let frame_wire = wire(&session, "Acme-child", "Frame").await;
    let frame_handle_wire = wire(&session, "Acme-frame-value", "JSHandle").await;
    let (frame, document_handle) = {
        let mut state = session.0.host.state.lock().unwrap();
        let frame = state
            .issue(Kind::Frame, frame_wire.clone(), Some(page))
            .unwrap();
        let handle = state
            .issue(Kind::Handle, frame_handle_wire, Some(frame))
            .unwrap();
        (frame, handle)
    };
    let id = session
        .submit(Uuid::new_v4(), Request::HandleRead { handle }, 5000)
        .unwrap();
    let read = peer.request().await;
    assert_eq!(read["method"], "jsonValue");
    assert_eq!(read["guid"], "Acme-worker-value");
    peer.send(&json!({"guid":"Acme-child","method":"navigated","params":{"url":"https://example.com/next","name":"","newDocument":{}}})).await;
    let dispose = peer.request().await;
    assert_eq!(dispose["method"], "dispose");
    assert_eq!(dispose["guid"], "Acme-frame-value");
    peer.send(&json!({"id":dispose["id"],"result":{}})).await;
    session
        .0
        .host
        .connection()
        .unwrap()
        .event_barrier(deadline())
        .await
        .unwrap();
    {
        let state = session.0.host.state.lock().unwrap();
        assert!(state.resource(frame, None).is_err());
        assert!(state.resource(document_handle, None).is_err());
        assert!(state.resource(worker, Some(Kind::Worker)).is_ok());
        assert!(state.resource(handle, Some(Kind::Handle)).is_ok());
    }
    peer.send(&json!({"id":read["id"],"result":{"value":{"s":"Acme retained worker"}}}))
        .await;
    let done = complete(&session, id).await;
    assert_eq!(done.outcome, Outcome::Succeeded, "{done:?}");
    let result = done.result.unwrap();
    assert_eq!(result["worker"], json!(worker));
    assert_eq!(result["value"]["value"], "Acme retained worker");
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err(),
        "Unrelated frame change must not dispose worker values or probe the worker"
    );
    close(&session).await;
}

#[tokio::test]
async fn worker_close_and_disposal_fence_late_results_without_reviving_same_url_workers() {
    for disposal in [false, true] {
        let (session, mut peer, _root, _, page) = harness().await;
        let first = worker(&session, &mut peer, "Acme-first").await;
        let second = worker(&session, &mut peer, "Acme-second").await;
        let handle = handle(&session, &mut peer, first, "Acme-first-value").await;
        let id = session
            .submit(Uuid::new_v4(), evaluate(first, false), 5000)
            .unwrap();
        let call = peer.request().await;
        assert_eq!(call["guid"], "Acme-first");
        peer.send(&if disposal {json!({"guid":"Acme-first","method":"__dispose__","params":{"reason":"Acme worker closed"}})}else{json!({"guid":"Acme-first","method":"close","params":{}})}).await;
        tokio::time::timeout_at(deadline(), async {
            loop {
                let changed = session.0.host.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if session
                    .0
                    .host
                    .state
                    .lock()
                    .unwrap()
                    .resource(first, None)
                    .is_err()
                {
                    break;
                }
                changed.await;
            }
        })
        .await
        .unwrap();
        peer.send(&json!({"id":call["id"],"result":{"value":{"n":42}}}))
            .await;
        let done = complete(&session, id).await;
        assert_eq!(done.code.as_deref(), Some("STALE_REFERENCE"), "{done:?}");
        assert_eq!(done.result.unwrap()["dispatchCompleted"], true);
        let list = session
            .submit(Uuid::new_v4(), Request::WorkerList { page }, 5000)
            .unwrap();
        let done = complete(&session, list).await;
        assert_eq!(done.outcome, Outcome::Succeeded, "{done:?}");
        let workers = done.result.unwrap()["workers"].as_array().unwrap().clone();
        assert_eq!(workers.len(), 1, "{workers:?}");
        assert_eq!(workers[0]["worker"], json!(second));
        let stale = session
            .submit(Uuid::new_v4(), Request::HandleRead { handle }, 5000)
            .unwrap();
        assert_eq!(
            complete(&session, stale).await.code.as_deref(),
            Some("STALE_REFERENCE")
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.request())
                .await
                .is_err()
        );
        let survivor = session
            .submit(Uuid::new_v4(), evaluate(second, false), 5000)
            .unwrap();
        let survivor_call = peer.request().await;
        assert_eq!(survivor_call["guid"], "Acme-second");
        peer.send(&json!({"id":survivor_call["id"],"result":{"value":{"n":84}}}))
            .await;
        assert_eq!(
            complete(&session, survivor).await.outcome,
            Outcome::Succeeded
        );
        if !disposal {
            peer.send(&json!({"guid":"Acme-first","method":"__dispose__","params":{"reason":"Acme disposal witness"}})).await;
        }
        close(&session).await;
    }
}
