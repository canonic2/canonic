//! Exact SDK element relationships retain source-document and returned-frame authority.
use super::*;

async fn retain(
    session: &Session,
    peer: &mut Fixture,
    page: Reference,
    element: bool,
) -> Reference {
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::Evaluate {
                scope: page,
                expression: "()=>document.body".into(),
                is_function: Some(true),
                argument: Default::default(),
                return_handle: true,
            },
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "evaluateExpressionHandle");
    let mut create = created(
        "Acme-element",
        if element { "ElementHandle" } else { "JSHandle" },
        json!({"preview":"Acme"}),
    );
    create["guid"] = call["guid"].clone();
    peer.send(&create).await;
    peer.send(&json!({"id":call["id"],"result":{"handle":{"guid":"Acme-element"}}}))
        .await;
    serde_json::from_value(complete(session, id).await.result.unwrap()["handle"].clone()).unwrap()
}
fn request(handle: Reference, owner: bool) -> Request {
    if owner {
        Request::HandleOwnerFrame { handle }
    } else {
        Request::HandleContentFrame { handle }
    }
}

#[tokio::test]
async fn frame_relationships_sdk_preserve_exact_methods_nullable_frames_and_source_page() {
    for owner in [false, true] {
        for found in [false, true] {
            let (session, mut peer, _root, _, page) = harness().await;
            let handle = retain(&session, &mut peer, page, true).await;
            let id = session
                .submit(Uuid::new_v4(), request(handle, owner), 5000)
                .unwrap();
            let call = peer.request().await;
            assert_eq!(call["guid"], "Acme-element");
            assert_eq!(
                call["method"],
                if owner { "ownerFrame" } else { "contentFrame" }
            );
            assert_eq!(call["params"], json!({}));
            peer.send(&json!({"id":call["id"],"result":if found {json!({"frame":{"guid":"frame"}})} else {json!({})}})).await;
            let done = complete(&session, id).await;
            assert_eq!(done.outcome, Outcome::Succeeded, "{done:?}");
            let value = done.result.unwrap();
            assert_eq!(value["sourcePage"], json!(page));
            assert_eq!(value["page"], if found { json!(page) } else { Value::Null });
            assert_eq!(value["frame"].is_null(), !found);
            assert_eq!(value["potentiallyMutating"], owner);
            close(&session).await;
        }
    }
}

#[tokio::test]
async fn frame_relationships_sdk_non_element_handles_are_refused_before_dispatch() {
    let (session, mut peer, _root, _, page) = harness().await;
    let handle = retain(&session, &mut peer, page, false).await;
    for owner in [false, true] {
        let id = session
            .submit(Uuid::new_v4(), request(handle, owner), 5000)
            .unwrap();
        assert_eq!(
            complete(&session, id).await.code.as_deref(),
            Some("INVALID_SCOPE")
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn frame_relationships_sdk_document_replacement_fences_late_results() {
    let (session, mut peer, _root, _, page) = harness().await;
    let handle = retain(&session, &mut peer, page, true).await;
    let id = session
        .submit(Uuid::new_v4(), request(handle, true), 5000)
        .unwrap();
    let call = peer.request().await;
    peer.send(&json!({"guid":"frame","method":"navigated","params":{"url":"https://example.com/replacement","name":"","newDocument":{}}})).await;
    let dispose = peer.request().await;
    assert_eq!(dispose["method"], "dispose");
    peer.send(&json!({"id":dispose["id"],"result":{}})).await;
    peer.send(&json!({"id":call["id"],"result":{"frame":{"guid":"frame"}}}))
        .await;
    assert_eq!(
        complete(&session, id).await.code.as_deref(),
        Some("DOCUMENT_CHANGED")
    );
    close(&session).await;
}
