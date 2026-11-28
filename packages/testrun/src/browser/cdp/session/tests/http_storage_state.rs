//! File admission stays on the responsive native lane and allocates only after preparation.
use super::*;

async fn native_state_source(worker: &mut Worker) -> (Reference, Reference) {
    let context = worker.index.register_context("context-acme").unwrap();
    worker
        .context_layout
        .insert(context, ContextOptions::default());
    let info = json!({"targetId":"page-acme","browserContextId":"context-acme","type":"page","title":"Acme","url":"http://127.0.0.1:12345","attached":true,"canAccessOpener":false});
    worker.apply(EventItem::Event(Event { method:"Target.attachedToTarget".into(),session_id:None,params:json!({"sessionId":"session-acme","targetInfo":info,"waitingForDebugger":false}) })).unwrap();
    worker.apply(EventItem::Event(Event { method:"Page.frameNavigated".into(),session_id:Some("session-acme".into()),params:json!({"type":"Navigation","frame":{"id":"frame-acme","loaderId":"loader-acme","url":"http://127.0.0.1:12345","domainAndRegistry":"","securityOrigin":"http://127.0.0.1:12345","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}) })).unwrap();
    worker
        .apply(EventItem::Event(Event {
            method: "Target.targetDestroyed".into(),
            session_id: None,
            params: json!({"targetId":"page-acme"}),
        }))
        .unwrap();
    let created = worker.http_operation(Uuid::new_v4(),serde_json::from_value(json!({"operation":"http_context_create","options":{"browserContext":context,"userAgent":"Acme"}})).unwrap(),deadline()).await.unwrap();
    (
        context,
        serde_json::from_value(created["httpContext"].clone()).unwrap(),
    )
}
async fn serve_native_state_collection(peer: &mut Peer) {
    let create = command(peer, "Target.createTarget").await;
    assert_eq!(
        create["params"],
        json!({"url":"about:blank","browserContextId":"context-acme","hidden":true,"background":true})
    );
    let mut info = json!({"targetId":"storage-acme","browserContextId":"context-acme","type":"other","title":"","url":"","attached":false,"canAccessOpener":false});
    send(
        peer,
        json!({"method":"Target.targetCreated","params":{"targetInfo":info}}),
    )
    .await;
    reply(peer, &create, json!({"targetId":"storage-acme"})).await;
    let get = command(peer, "Target.getTargetInfo").await;
    info["url"] = json!("about:blank");
    reply(peer, &get, json!({"targetInfo":info})).await;
    let attach = command(peer, "Target.attachToTarget").await;
    send(peer,json!({"method":"Target.attachedToTarget","params":{"sessionId":"storage-session","targetInfo":info,"waitingForDebugger":false}})).await;
    reply(peer, &attach, json!({"sessionId":"storage-session"})).await;
    for method in [
        "Page.enable",
        "Runtime.enable",
        "Network.setBypassServiceWorker",
        "Fetch.enable",
    ] {
        let call = command(peer, method).await;
        assert_eq!(call["sessionId"], "storage-session");
        if method == "Network.setBypassServiceWorker" {
            assert_eq!(call["params"]["bypass"], true);
        }
        reply(peer, &call, json!({})).await;
    }
    let navigate = command(peer, "Page.navigate").await;
    assert_eq!(navigate["params"]["url"], "http://127.0.0.1:12345");
    send(peer,json!({"sessionId":"storage-session","method":"Fetch.requestPaused","params":{"requestId":"storage-request","request":{"url":"http://127.0.0.1:12345/","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"strict-origin-when-cross-origin"},"frameId":"storage-frame","resourceType":"Document"}})).await;
    let fulfill = command(peer, "Fetch.fulfillRequest").await;
    assert_eq!(fulfill["sessionId"], "storage-session");
    assert_eq!(fulfill["params"]["requestId"], "storage-request");
    assert_eq!(fulfill["params"]["body"], "PGh0bWw+PC9odG1sPg==");
    reply(peer, &fulfill, json!({})).await;
    reply(
        peer,
        &navigate,
        json!({"frameId":"storage-frame","loaderId":"storage-loader","isDownload":false}),
    )
    .await;
    let tree = command(peer, "Page.getFrameTree").await;
    reply(peer,&tree,json!({"frameTree":{"frame":{"id":"storage-frame","loaderId":"storage-loader","url":"http://127.0.0.1:12345/","domainAndRegistry":"","securityOrigin":"http://127.0.0.1:12345","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await;
    let world = command(peer, "Page.createIsolatedWorld").await;
    assert_eq!(world["params"]["frameId"], "storage-frame");
    reply(peer, &world, json!({"executionContextId":41})).await;
    let evaluate = command(peer, "Runtime.evaluate").await;
    assert_eq!(evaluate["params"]["contextId"], 41);
    assert!(
        evaluate["params"]["expression"]
            .as_str()
            .unwrap()
            .contains("storageScript_exports")
    );
    reply(peer,&evaluate,json!({"result":{"type":"object","value":{"localStorage":[{"name":"Acme-account","value":"Acme-private-before"}]}}})).await;
}

#[tokio::test]
async fn native_http_state_hidden_collection_stages_before_cleanup_and_recovers_without_source() {
    let (mut worker, mut peer, _root) = fixture().await;
    let (context, client) = native_state_source(&mut worker).await;
    assert!(worker.index.list(Kind::Page).is_empty());
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "http_context_storage_state".into())
        .unwrap();
    let task = tokio::spawn(async move {
        let result = worker
            .native_http_storage_state(id, client, Default::default(), deadline())
            .await;
        (worker, result)
    });
    let read = command(&mut peer, "Storage.getCookies").await;
    reply(&mut peer, &read, json!({"cookies":[]})).await;
    serve_native_state_collection(&mut peer).await;
    let close = command(&mut peer, "Target.closeTarget").await;
    send(
        &mut peer,
        json!({"id":close["id"],"error":{"code":-32000,"message":"Acme close refused"}}),
    )
    .await;
    let targets = command(&mut peer, "Target.getTargets").await;
    reply(&mut peer,&targets,json!({"targetInfos":[{"targetId":"storage-acme","type":"other","title":"","url":"http://127.0.0.1:12345/","attached":true,"canAccessOpener":false,"browserContextId":"context-acme"}]})).await;
    let (mut worker, result) = task.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "CLEANUP_PENDING");
    let partial = error.partial.unwrap();
    assert_eq!(partial["stateRetained"], true);
    let capture = serde_json::from_value(partial["captureId"].clone()).unwrap();
    assert_eq!(
        worker.http_contexts.evidence_list()["pendingArtifacts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(worker.index.list(Kind::Page).is_empty());
    assert!(
        worker
            .shared
            .resources
            .lock()
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .all(|resource| resource["kind"] == "context")
    );
    worker.http_contexts.remove(client).unwrap();
    worker.index.retire(context).unwrap();
    let task = tokio::spawn(async move {
        let result = worker.native_http_state_finalize(capture, deadline()).await;
        (worker, result)
    });
    let close = command(&mut peer, "Target.closeTarget").await;
    assert_eq!(close["params"]["targetId"], "storage-acme");
    send(
        &mut peer,
        json!({"method":"Target.targetDestroyed","params":{"targetId":"storage-acme"}}),
    )
    .await;
    reply(&mut peer, &close, json!({"success":true})).await;
    let (worker, result) = task.await.unwrap();
    let receipt = result.unwrap();
    let artifact = worker
        .http_contexts
        .evidence_get(serde_json::from_value(receipt["reports"][0]["evidenceId"].clone()).unwrap())
        .unwrap();
    let state: Value = serde_json::from_slice(&artifact.read().unwrap()).unwrap();
    assert_eq!(
        state,
        json!({"cookies":[],"origins":[{"origin":"http://127.0.0.1:12345","localStorage":[{"name":"Acme-account","value":"Acme-private-before"}]}]})
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_http_state_unknown_hidden_allocation_requires_browser_cleanup_without_replay() {
    let (mut worker, mut peer, _root) = fixture().await;
    let (_, client) = native_state_source(&mut worker).await;
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "http_context_storage_state".into())
        .unwrap();
    let task = tokio::spawn(async move {
        let result = worker
            .native_http_storage_state(
                id,
                client,
                Default::default(),
                Instant::now() + Duration::from_millis(100),
            )
            .await;
        (worker, result)
    });
    let read = command(&mut peer, "Storage.getCookies").await;
    reply(&mut peer, &read, json!({"cookies":[]})).await;
    let create = command(&mut peer, "Target.createTarget").await;
    let (mut worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap_err().code, "CLEANUP_PENDING");
    assert_eq!(worker.shared.state.lock().unwrap().phase, Phase::Lost);
    reply(&mut peer, &create, json!({"targetId":"late-storage-acme"})).await;
    send(&mut peer,json!({"method":"Target.targetCreated","params":{"targetInfo":{"targetId":"late-storage-acme","type":"other","title":"","url":"","attached":false,"canAccessOpener":false,"browserContextId":"context-acme"}}})).await;
    tokio::task::yield_now().await;
    worker.drain().unwrap();
    assert!(worker.index.list(Kind::Page).is_empty());
    assert_eq!(
        worker.http_contexts.evidence_list()["pendingArtifacts"],
        json!([])
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_http_state_snapshots_visited_origins_after_the_cookie_read_settles() {
    let (mut worker, mut peer, _root) = fixture().await;
    let context = worker.index.register_context("context-acme").unwrap();
    worker
        .context_layout
        .insert(context, ContextOptions::default());
    let created = worker.http_operation(Uuid::new_v4(),serde_json::from_value(json!({"operation":"http_context_create","options":{"browserContext":context,"userAgent":"Acme"}})).unwrap(),deadline()).await.unwrap();
    let client = serde_json::from_value(created["httpContext"].clone()).unwrap();
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "http_context_storage_state".into())
        .unwrap();
    let task = tokio::spawn(async move {
        let result = worker
            .native_http_storage_state(id, client, Default::default(), deadline())
            .await;
        (worker, result)
    });
    let read = command(&mut peer, "Storage.getCookies").await;
    send(&mut peer,json!({"method":"Target.attachedToTarget","params":{"sessionId":"session-acme","targetInfo":{"targetId":"page-acme","browserContextId":"context-acme","type":"page","title":"Acme","url":"http://127.0.0.1:12345","attached":true,"canAccessOpener":false},"waitingForDebugger":false}})).await;
    send(&mut peer,json!({"sessionId":"session-acme","method":"Page.frameNavigated","params":{"type":"Navigation","frame":{"id":"frame-acme","loaderId":"loader-acme","url":"http://127.0.0.1:12345","domainAndRegistry":"","securityOrigin":"http://127.0.0.1:12345","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await;
    send(
        &mut peer,
        json!({"method":"Target.targetDestroyed","params":{"targetId":"page-acme"}}),
    )
    .await;
    reply(&mut peer, &read, json!({"cookies":[]})).await;
    serve_native_state_collection(&mut peer).await;
    let close = command(&mut peer, "Target.closeTarget").await;
    send(
        &mut peer,
        json!({"method":"Target.targetDestroyed","params":{"targetId":"storage-acme"}}),
    )
    .await;
    reply(&mut peer, &close, json!({"success":true})).await;
    let (worker, result) = task.await.unwrap();
    let receipt = result.unwrap();
    assert_eq!(receipt["reports"][0]["originCount"], 1);
    assert!(worker.index.list(Kind::Page).is_empty());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_http_state_cleanup_waits_for_destruction_after_close_acknowledgement() {
    for during_snapshot in [true, false] {
        let (mut worker, mut peer, _root) = fixture().await;
        let (_, client) = native_state_source(&mut worker).await;
        let id = Uuid::new_v4();
        worker
            .shared
            .state
            .lock()
            .unwrap()
            .begin(id, "http_context_storage_state".into())
            .unwrap();
        let mut task = tokio::spawn(async move {
            let result = worker
                .native_http_storage_state(id, client, Default::default(), deadline())
                .await;
            (worker, result)
        });
        let read = command(&mut peer, "Storage.getCookies").await;
        reply(&mut peer, &read, json!({"cookies":[]})).await;
        serve_native_state_collection(&mut peer).await;
        let close = command(&mut peer, "Target.closeTarget").await;
        reply(&mut peer, &close, json!({"success":true})).await;
        let targets = command(&mut peer, "Target.getTargets").await;
        if during_snapshot {
            send(
                &mut peer,
                json!({"method":"Target.targetDestroyed","params":{"targetId":"storage-acme"}}),
            )
            .await;
        }
        reply(&mut peer,&targets,json!({"targetInfos":[{"targetId":"storage-acme","type":"other","title":"","url":"http://127.0.0.1:12345/","attached":true,"canAccessOpener":false,"browserContextId":"context-acme"}]})).await;
        if !during_snapshot {
            tokio::select! {
                result=&mut task=>panic!("Close acknowledgement settled before target destruction: {:?}",result.map(|(_,result)|result)),
                ()=tokio::time::sleep(Duration::from_millis(25))=>{},
            }
            send(
                &mut peer,
                json!({"method":"Target.targetDestroyed","params":{"targetId":"storage-acme"}}),
            )
            .await;
        }
        let (worker, result) = task.await.unwrap();
        assert_eq!(result.unwrap()["reports"][0]["originCount"], 1);
        assert_eq!(
            worker.http_contexts.evidence_list()["pendingArtifacts"],
            json!([])
        );
        assert!(worker.index.list(Kind::Page).is_empty());
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn http_storage_state_native_cancelled_file_admission_allocates_no_http_client_or_native_call()
 {
    let (mut worker, mut peer, _root) = fixture().await;
    let readers = crate::browser::http::hold_readers().await;
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "http_context_create".into())
        .unwrap();
    let shared = worker.shared.clone();
    let task = tokio::spawn(async move {
        let result = worker.http_operation(id, serde_json::from_value(json!({"operation":"http_context_create","options":{"storageState":"/private/tmp/Acme-unread-state.json"}})).unwrap(), deadline()).await;
        (worker, result)
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.next())
            .await
            .is_err()
    );
    shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&id)
        .unwrap()
        .cancel_requested = true;
    shared.changed.notify_waiters();
    let (worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap_err().code, "CANCELLED");
    assert_eq!(worker.http_contexts.list()["contexts"], json!([]));
    assert_eq!(worker.connection.as_ref().unwrap().status().pending, 0);
    drop(readers);
}

#[tokio::test]
async fn http_storage_state_native_dispatch_exports_isolated_state_without_protocol_or_browser_cookie_reads()
 {
    let (mut worker, mut peer, _root) = fixture().await;
    let created = worker.http_operation(Uuid::new_v4(), serde_json::from_value(json!({"operation":"http_context_create","options":{"storageState":{"cookies":[{"name":"Acme","value":"Acme-private-state","domain":"example.com","path":"/","expires":-1,"httpOnly":true,"secure":true,"sameSite":"Strict"}]}}})).unwrap(), deadline()).await.unwrap();
    let captured = worker.http_operation(Uuid::new_v4(), serde_json::from_value(json!({"operation":"http_context_storage_state","httpContext":created["httpContext"]})).unwrap(), deadline()).await.unwrap();
    assert!(!captured.to_string().contains("Acme-private-state"));
    let state: Value = serde_json::from_slice(
        &worker
            .http_contexts
            .evidence_get(
                serde_json::from_value(captured["reports"][0]["evidenceId"].clone()).unwrap(),
            )
            .unwrap()
            .read()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(state["cookies"][0]["value"], "Acme-private-state");
    let context = worker.index.register_context("context-acme").unwrap();
    worker
        .context_layout
        .insert(context, ContextOptions::default());
    let associated = worker.http_operation(Uuid::new_v4(), serde_json::from_value(json!({"operation":"http_context_create","options":{"browserContext":context,"userAgent":"Acme"}})).unwrap(), deadline()).await.unwrap();
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "http_context_storage_state".into())
        .unwrap();
    let task = tokio::spawn(async move {
        let result = worker.http_operation(id,serde_json::from_value(json!({"operation":"http_context_storage_state","httpContext":associated["httpContext"]})).unwrap(),deadline()).await;
        (worker, result)
    });
    let read = command(&mut peer, "Storage.getCookies").await;
    assert_eq!(read["params"]["browserContextId"], "context-acme");
    reply(&mut peer, &read, json!({"cookies":[]})).await;
    let (worker, result) = task.await.unwrap();
    let captured = result.unwrap();
    assert_eq!(captured["reports"][0]["browserAssociated"], true);
    assert_eq!(worker.index.list(Kind::Context).len(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}
