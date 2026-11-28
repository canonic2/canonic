//! Exact SDK frameElement replies are retained under the parent document.
use super::*;

async fn child(session: &Session, peer: &mut Fixture) -> Reference {
    let mut value = created(
        "Acme-child",
        "Frame",
        json!({"url":"about:blank","name":"Acme","parentFrame":{"guid":"frame"},"loadStates":[]}),
    );
    value["guid"] = json!("page");
    peer.send(&value).await;
    peer.send(
        &json!({"guid":"page","method":"frameAttached","params":{"frame":{"guid":"Acme-child"}}}),
    )
    .await;
    let connection = session.0.host.connection().unwrap();
    connection.event_barrier(deadline()).await.unwrap();
    let wire = connection.resolve("Acme-child", "Frame").await.unwrap();
    session
        .0
        .host
        .state
        .lock()
        .unwrap()
        .by_wire(&wire)
        .unwrap()
        .reference
}

#[tokio::test]
async fn frame_owner_embedding_sdk_uses_stable_child_and_exact_parent_document_after_navigation() {
    let (session, mut peer, _root, _, _page) = harness().await;
    let child = child(&session, &mut peer).await;
    let retained = session
        .submit(Uuid::new_v4(), Request::FrameRetain { frame: child }, 5000)
        .unwrap();
    let retained = complete(&session, retained).await;
    assert_eq!(retained.outcome, Outcome::Succeeded);
    let frame = serde_json::from_value(retained.result.unwrap()["frame"].clone()).unwrap();
    let id = session
        .submit(Uuid::new_v4(), Request::FrameElement { frame }, 5000)
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["guid"], "Acme-child");
    assert_eq!(call["method"], "frameElement");
    peer.send(&json!({"guid":"Acme-child","method":"navigated","params":{"url":"https://example.com/next","name":"","newDocument":{}}})).await;
    let mut element = created(
        "Acme-stable-embedding",
        "ElementHandle",
        json!({"preview":"JSHandle@iframe"}),
    );
    element["guid"] = json!("frame");
    peer.send(&element).await;
    peer.send(&json!({"id":call["id"],"result":{"element":{"guid":"Acme-stable-embedding"}}}))
        .await;
    let done = complete(&session, id).await;
    assert_eq!(done.outcome, Outcome::Succeeded, "{done:?}");
    let value = done.result.unwrap();
    assert_eq!(value["childFrame"], json!(frame));
    let handle = serde_json::from_value(value["handle"].clone()).unwrap();
    // Read status before taking the state lock that status itself needs.
    let status = session.status();
    let resource = status["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["reference"] == json!(frame))
        .unwrap()
        .clone();
    crate::schema::validate(
        &crate::browser::contract::schemas()["status"]["oneOf"][0]["properties"]["resources"]["items"],
        &resource,
    )
    .unwrap();
    {
        let state = session.0.host.state.lock().unwrap();
        assert_eq!(
            state.resource(child, Some(Kind::Frame)).err(),
            Some("STALE_REFERENCE")
        );
        assert_ne!(state.owner_frame(frame).unwrap().reference, child);
        assert!(state.resource(handle, Some(Kind::Handle)).is_ok());
    }
    let id = session
        .submit(Uuid::new_v4(), Request::FrameOwnerRelease { frame }, 5000)
        .unwrap();
    let done = complete(&session, id).await;
    assert_eq!(done.outcome, Outcome::Succeeded);
    assert_eq!(done.result.unwrap()["nativeScopeClosed"], false);
    assert!(
        session
            .0
            .host
            .state
            .lock()
            .unwrap()
            .resource(handle, Some(Kind::Handle))
            .is_ok()
    );
    close(&session).await;
}

#[tokio::test]
async fn frame_element_sdk_uses_exact_rpc_and_parent_document_ownership() {
    let (session, mut peer, _root, _, _page) = harness().await;
    let child = child(&session, &mut peer).await;
    let id = session
        .submit(Uuid::new_v4(), Request::FrameElement { frame: child }, 5000)
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["guid"], "Acme-child");
    assert_eq!(call["method"], "frameElement");
    assert_eq!(call["params"], json!({}));
    let mut element = created(
        "Acme-embedding",
        "ElementHandle",
        json!({"preview":"JSHandle@iframe"}),
    );
    element["guid"] = json!("frame");
    peer.send(&element).await;
    peer.send(&json!({"id":call["id"],"result":{"element":{"guid":"Acme-embedding"}}}))
        .await;
    let done = complete(&session, id).await;
    assert_eq!(done.outcome, Outcome::Succeeded, "{done:?}");
    let value = done.result.unwrap();
    let handle = serde_json::from_value(value["handle"].clone()).unwrap();
    {
        let state = session.0.host.state.lock().unwrap();
        assert_eq!(
            state.resource(handle, Some(Kind::Handle)).unwrap().parent,
            Some(serde_json::from_value(value["frame"].clone()).unwrap())
        );
    }
    close(&session).await;
}

#[tokio::test]
async fn frame_element_sdk_detached_child_disposes_late_result_instead_of_publishing() {
    let (session, mut peer, _root, _, _page) = harness().await;
    let child = child(&session, &mut peer).await;
    let id = session
        .submit(Uuid::new_v4(), Request::FrameElement { frame: child }, 5000)
        .unwrap();
    let call = peer.request().await;
    peer.send(
        &json!({"guid":"page","method":"frameDetached","params":{"frame":{"guid":"Acme-child"}}}),
    )
    .await;
    let mut element = created(
        "Acme-late-embedding",
        "ElementHandle",
        json!({"preview":"JSHandle@iframe"}),
    );
    element["guid"] = json!("frame");
    peer.send(&element).await;
    peer.send(&json!({"id":call["id"],"result":{"element":{"guid":"Acme-late-embedding"}}}))
        .await;
    let dispose = peer.request().await;
    assert_eq!(dispose["method"], "dispose");
    assert_eq!(dispose["guid"], "Acme-late-embedding");
    peer.send(&json!({"id":dispose["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, id).await.code.as_deref(),
        Some("DOCUMENT_CHANGED")
    );
    close(&session).await;
}

#[tokio::test]
async fn sdk_page_request_gc_sends_the_pinned_driver_request() {
    let (session, mut peer, _root, _, page) = harness().await;
    let id = session
        .submit(Uuid::new_v4(), Request::PageRequestGc { page }, 5000)
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["guid"], "page");
    assert_eq!(call["method"], "requestGC");
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    let done = complete(&session, id).await;
    assert_eq!(done.outcome, Outcome::Succeeded, "{done:?}");
    assert_eq!(done.result.unwrap(), json!({"page":page,"requested":true}));
    close(&session).await;
}

#[tokio::test]
async fn sdk_page_events_record_frames_main_frame_load_states_and_workers() {
    let (session, mut peer, _root, context, page) = harness().await;
    let child = child(&session, &mut peer).await;
    // A child frame's load states are not page load events.
    for (guid, state) in [
        ("Acme-child", "load"),
        ("frame", "domcontentloaded"),
        ("frame", "load"),
        ("frame", "networkidle"),
    ] {
        peer.send(&json!({"guid":guid,"method":"loadstate","params":{"add":state}}))
            .await;
    }
    let mut worker = created(
        "Acme-worker",
        "Worker",
        json!({"url":"https://example.com/Acme-worker.js"}),
    );
    worker["guid"] = json!("page");
    peer.send(&worker).await;
    peer.send(&json!({"guid":"page","method":"worker","params":{"worker":{"guid":"Acme-worker"}}}))
        .await;
    peer.send(
        &json!({"guid":"page","method":"frameDetached","params":{"frame":{"guid":"Acme-child"}}}),
    )
    .await;
    session
        .0
        .host
        .connection()
        .unwrap()
        .event_barrier(deadline())
        .await
        .unwrap();
    let read = session
        .submit(
            Uuid::new_v4(),
            Request::EventsRead {
                context,
                cursor: None,
                limit: 100,
            },
            5000,
        )
        .unwrap();
    let read = complete(&session, read).await.result.unwrap();
    let events = read["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| (event["event"].clone(), event["resource"].clone()))
        .filter(|(event, _)| event != "page_created")
        .collect::<Vec<_>>();
    let worker = events
        .iter()
        .find(|(event, _)| event == "worker_created")
        .map(|(_, resource)| resource.clone())
        .unwrap();
    assert_eq!(
        events,
        vec![
            (json!("frame_attached"), json!(child)),
            (json!("dom_content_loaded"), json!(page)),
            (json!("load"), json!(page)),
            (json!("worker_created"), worker),
            (json!("frame_detached"), json!(child)),
        ]
    );
    close(&session).await;
}
