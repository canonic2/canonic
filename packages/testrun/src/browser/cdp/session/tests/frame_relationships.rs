//! Element relationships are nullable and preserve temporary-object ownership.
use super::super::execution_groups::Purpose;
use super::investigation::owned_page;
use super::*;

async fn setup() -> (
    Worker,
    Peer,
    tempfile::TempDir,
    Reference,
    Reference,
    Reference,
) {
    let (mut worker, mut peer, root, context, page) = owned_page().await;
    let task = tokio::spawn(async move {
        let value = worker
            .execute_evaluation(
                Uuid::new_v4(),
                Request::Evaluate {
                    scope: page,
                    expression: "()=>document.body".into(),
                    is_function: Some(true),
                    argument: Default::default(),
                    return_handle: true,
                },
                deadline(),
            )
            .await
            .unwrap();
        (
            worker,
            serde_json::from_value(value["handle"].clone()).unwrap(),
        )
    });
    let call = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &call,
        json!({"result":{"type":"object","objectId":"Acme-wrapper"}}),
    )
    .await;
    let (worker, handle) = task.await.unwrap();
    (worker, peer, root, context, page, handle)
}

fn request(handle: Reference, owner: bool) -> Request {
    if owner {
        Request::HandleOwnerFrame { handle }
    } else {
        Request::HandleContentFrame { handle }
    }
}

async fn extract(peer: &mut Peer, node: bool) -> String {
    let call = command(peer, "Runtime.callFunctionOn").await;
    assert_eq!(call["params"]["objectId"], "Acme-wrapper");
    assert_eq!(
        call["params"]["functionDeclaration"],
        "function(){return this.value;}"
    );
    let group = call["params"]["objectGroup"].as_str().unwrap().to_owned();
    assert!(group.starts_with("testrun:frame-relationship:"));
    reply(
        peer,
        &call,
        if node {
            json!({"result":{"type":"object","subtype":"node","objectId":"Acme-node"}})
        } else {
            json!({"result":{"type":"number","value":7}})
        },
    )
    .await;
    group
}

async fn describe(peer: &mut Peer, name: &str, native: Option<&str>) {
    let call = command(peer, "DOM.describeNode").await;
    assert_eq!(call["params"]["depth"], 0);
    let mut node = json!({"nodeId":1,"backendNodeId":7,"nodeType":1,"nodeName":name,"localName":name.to_lowercase(),"nodeValue":""});
    if let Some(native) = native {
        node["frameId"] = json!(native);
    }
    reply(peer, &call, json!({"node":node})).await;
}

async fn release(peer: &mut Peer, group: &str) {
    let call = command(peer, "Runtime.releaseObjectGroup").await;
    assert_eq!(call["params"]["objectGroup"], group);
    reply(peer, &call, json!({})).await;
}

#[tokio::test]
async fn frame_relationships_resolve_owner_and_only_iframe_content_without_retaining_handles() {
    for (owner, name, has_frame, expected) in [
        (true, "HTML", true, true),
        (false, "IFRAME", true, true),
        (false, "FRAME", true, true),
        (false, "DIV", true, false),
        (false, "HTML", true, false),
        (false, "IFRAME", false, false),
    ] {
        let (mut worker, mut peer, _root, context, page, handle) = setup().await;
        let frame = worker.index.page_frame(page, None).unwrap();
        let task = tokio::spawn(async move {
            let result = worker
                .execute_evaluation(Uuid::new_v4(), request(handle, owner), deadline())
                .await;
            (worker, result)
        });
        let group = extract(&mut peer, true).await;
        if owner {
            let call = command(&mut peer, "Runtime.callFunctionOn").await;
            assert_eq!(call["params"]["objectId"], "Acme-node");
            assert_eq!(call["params"]["objectGroup"], group);
            reply(
                &mut peer,
                &call,
                json!({"result":{"type":"object","subtype":"node","objectId":"Acme-document"}}),
            )
            .await;
        }
        describe(&mut peer, name, has_frame.then_some("frame-acme")).await;
        release(&mut peer, &group).await;
        let (worker, result) = task.await.unwrap();
        let value = result.unwrap();
        assert_eq!(
            value["frame"],
            if expected { json!(frame) } else { Value::Null }
        );
        assert_eq!(
            value["page"],
            if expected { json!(page) } else { Value::Null }
        );
        assert_eq!(value["sourcePage"], json!(page));
        assert_eq!(value["potentiallyMutating"], owner);
        assert_eq!(value["relationshipCleanupPending"], false);
        assert!(worker.evaluation_handle_live(handle));
        assert_eq!(
            worker
                .evaluation
                .groups
                .pending(context, Purpose::FrameRelationship),
            0
        );
    }
}

#[tokio::test]
async fn frame_relationships_owner_without_document_is_null_without_describing() {
    let (mut worker, mut peer, _root, _, _, handle) = setup().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(Uuid::new_v4(), request(handle, true), deadline())
            .await;
        (worker, result)
    });
    let group = extract(&mut peer, true).await;
    let call = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &call,
        json!({"result":{"type":"object","subtype":"null","value":null}}),
    )
    .await;
    release(&mut peer, &group).await;
    assert!(task.await.unwrap().1.unwrap()["frame"].is_null());
}

#[tokio::test]
async fn frame_relationships_reject_primitives_and_foreign_or_wrong_kind_handles() {
    for owner in [false, true] {
        let (mut worker, mut peer, _root, _, page, handle) = setup().await;
        let task = tokio::spawn(async move {
            let result = worker
                .execute_evaluation(Uuid::new_v4(), request(handle, owner), deadline())
                .await;
            (worker, result)
        });
        let group = extract(&mut peer, false).await;
        release(&mut peer, &group).await;
        let (mut worker, result) = task.await.unwrap();
        assert_eq!(result.unwrap_err().code, "INVALID_SCOPE");
        let foreign = Reference {
            connection_id: Uuid::new_v4(),
            ..handle
        };
        assert_eq!(
            worker
                .execute_evaluation(Uuid::new_v4(), request(foreign, owner), deadline())
                .await
                .unwrap_err()
                .code,
            "FOREIGN_REFERENCE"
        );
        assert!(
            worker
                .execute_evaluation(Uuid::new_v4(), request(page, owner), deadline())
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn frame_relationships_failed_cleanup_retries_release_without_repeating_lookup() {
    let (mut worker, mut peer, _root, context, _, handle) = setup().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(Uuid::new_v4(), request(handle, false), deadline())
            .await;
        (worker, result)
    });
    let group = extract(&mut peer, true).await;
    describe(&mut peer, "IFRAME", Some("frame-acme")).await;
    let call = command(&mut peer, "Runtime.releaseObjectGroup").await;
    send(&mut peer,json!({"id":call["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme release refused"}})).await;
    let (mut worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap()["relationshipCleanupPending"], true);
    assert_eq!(
        worker
            .evaluation
            .groups
            .pending(context, Purpose::FrameRelationship),
        1
    );
    let task = tokio::spawn(async move {
        worker.release_execution_groups(deadline()).await;
        worker
    });
    release(&mut peer, &group).await;
    assert_eq!(
        task.await
            .unwrap()
            .evaluation
            .groups
            .pending(context, Purpose::FrameRelationship),
        0
    );
}

#[tokio::test]
async fn frame_relationships_capacity_refuses_before_native_dispatch() {
    let (mut worker, mut peer, _root, context, _, handle) = setup().await;
    for _ in 0..32 {
        worker
            .evaluation
            .groups
            .begin(context, "session-acme".into(), Purpose::FrameRelationship)
            .unwrap();
    }
    for owner in [false, true] {
        assert_eq!(
            worker
                .execute_evaluation(Uuid::new_v4(), request(handle, owner), deadline())
                .await
                .unwrap_err()
                .code,
            "FRAME_RELATIONSHIP_CAPACITY"
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn frame_relationships_unanswered_lookup_retains_group_without_native_release() {
    let (mut worker, mut peer, _root, context, _, handle) = setup().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(
                Uuid::new_v4(),
                request(handle, false),
                Instant::now() + Duration::from_millis(150),
            )
            .await;
        (worker, result)
    });
    let call = command(&mut peer, "Runtime.callFunctionOn").await;
    assert!(
        call["params"]["objectGroup"]
            .as_str()
            .unwrap()
            .starts_with("testrun:frame-relationship:")
    );
    let (mut worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    worker.release_execution_groups(deadline()).await;
    assert_eq!(
        worker
            .evaluation
            .groups
            .pending(context, Purpose::FrameRelationship),
        1
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn frame_relationships_document_loss_during_cleanup_prevents_publication() {
    let (mut worker, mut peer, _root, _, _, handle) = setup().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(Uuid::new_v4(), request(handle, false), deadline())
            .await;
        (worker, result)
    });
    extract(&mut peer, true).await;
    describe(&mut peer, "IFRAME", Some("frame-acme")).await;
    let call = command(&mut peer, "Runtime.releaseObjectGroup").await;
    send(&mut peer,json!({"method":"Runtime.executionContextDestroyed","sessionId":"session-acme","params":{"executionContextId":1,"executionContextUniqueId":"world-acme"}})).await;
    reply(&mut peer, &call, json!({})).await;
    assert_eq!(task.await.unwrap().1.unwrap_err().code, "DOCUMENT_CHANGED");
}

#[tokio::test]
async fn owner_getter_awaits_promises_and_preserves_the_pinned_user_gesture() {
    let (mut worker, mut peer, _root, _, _, handle) = setup().await;
    let task = tokio::spawn(async move {
        worker
            .execute_evaluation(Uuid::new_v4(), request(handle, true), deadline())
            .await
    });
    let group = extract(&mut peer, true).await;
    let call = command(&mut peer, "Runtime.callFunctionOn").await;
    assert_eq!(
        call["params"]["awaitPromise"], true,
        "The SDK owner-document helper awaits application-defined Promise property values"
    );
    assert_eq!(
        call["params"]["userGesture"], true,
        "The SDK evaluator supplies a user gesture to property access"
    );
    reply(
        &mut peer,
        &call,
        json!({"result":{"type":"object","subtype":"node","objectId":"Acme-awaited-document"}}),
    )
    .await;
    describe(&mut peer, "HTML", Some("frame-acme")).await;
    release(&mut peer, &group).await;
    task.await.unwrap().unwrap();
}
