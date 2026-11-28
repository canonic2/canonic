//! Pending native allocations consume the same bound as published handles.
use super::investigation::owned_page;
use super::*;

fn retain(scope: Reference) -> Request {
    Request::Evaluate {
        scope,
        expression: "()=>({Acme:true})".into(),
        is_function: Some(true),
        argument: Default::default(),
        return_handle: true,
    }
}

async fn seeded(count: usize) -> (Worker, Peer, tempfile::TempDir, Reference) {
    let (mut worker, mut peer, root, _, page) = owned_page().await;
    let task = tokio::spawn(async move {
        for _ in 0..count {
            worker
                .execute_evaluation(Uuid::new_v4(), retain(page), deadline())
                .await
                .unwrap();
        }
        worker
    });
    for ordinal in 0..count {
        let call = command(&mut peer, "Runtime.callFunctionOn").await;
        reply(
            &mut peer,
            &call,
            json!({"result":{"type":"object","objectId":format!("Acme-wrapper-{ordinal}")}}),
        )
        .await;
    }
    (task.await.unwrap(), peer, root, page)
}

async fn settled(shared: &Arc<Shared>, id: Uuid) -> Operation {
    loop {
        let changed = shared.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if let Some(operation) = shared
            .state
            .lock()
            .unwrap()
            .operations
            .get(&id)
            .filter(|operation| operation.settled)
            .cloned()
        {
            return operation;
        }
        changed.await;
    }
}

#[tokio::test]
async fn retained_allocation_pending_caller_reserves_the_last_handle_before_another_dispatch() {
    let (mut worker, mut peer, _root, page) = seeded(255).await;
    let (session, _ordinary, mut executions, responses, _control) =
        super::execution_concurrency::client(&worker);
    let id = session.submit(Uuid::new_v4(), retain(page), 5000).unwrap();
    let mut original = executions.try_recv().unwrap();
    worker.executions = Some(executions);
    worker.responses = Some(responses);
    let running = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(id, original.request.take().unwrap(), original.deadline)
            .await;
        let succeeded = result.is_ok();
        worker.finish(id, result).await;
        drop(original);
        (worker, succeeded)
    });
    let held = command(&mut peer, "Runtime.callFunctionOn").await;
    let second = session.submit(Uuid::new_v4(), retain(page), 5000).unwrap();
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        settled(&session.0.shared, second),
    )
    .await
    .expect("A pending allocation must reserve capacity before another native caller can dispatch");
    assert_eq!(result.code.as_deref(), Some("HANDLE_CAPACITY"));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "The refused caller must never reach native execution"
    );
    reply(
        &mut peer,
        &held,
        json!({"result":{"type":"object","objectId":"Acme-final-wrapper"}}),
    )
    .await;
    let (mut worker, result) = running.await.unwrap();
    assert!(result);
    assert_eq!(
        worker
            .execute_evaluation(Uuid::new_v4(), retain(page), deadline())
            .await
            .unwrap_err()
            .code,
        "HANDLE_CAPACITY"
    );
}

#[tokio::test]
async fn retained_allocation_unanswered_creation_keeps_capacity_until_native_world_destruction() {
    let (mut worker, mut peer, _root, page) = seeded(255).await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(
                Uuid::new_v4(),
                retain(page),
                Instant::now() + Duration::from_millis(150),
            )
            .await;
        (worker, result)
    });
    let _unanswered = command(&mut peer, "Runtime.callFunctionOn").await;
    let (mut worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(
                Uuid::new_v4(),
                retain(page),
                Instant::now() + Duration::from_millis(150),
            )
            .await;
        (worker, result)
    });
    let (mut worker, result) = task.await.unwrap();
    assert_eq!(
        result.unwrap_err().code,
        "HANDLE_CAPACITY",
        "An unanswered creation may already own a native wrapper"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    worker
        .apply(EventItem::Event(Event {
            method: "Runtime.executionContextDestroyed".into(),
            session_id: Some("session-acme".into()),
            params: json!({"executionContextId":1,"executionContextUniqueId":"world-acme"}),
        }))
        .unwrap();
    worker.fence_evaluation();
    // A fresh world can allocate after the prior epoch's unknown ownership ends.
    worker.apply(EventItem::Event(Event {method:"Runtime.executionContextCreated".into(),session_id:Some("session-acme".into()),params:json!({"context":{"id":2,"uniqueId":"Acme-new-world","origin":"://","name":"","auxData":{"frameId":"frame-acme","isDefault":true}}})})).unwrap();
    let task = tokio::spawn(async move {
        worker
            .execute_evaluation(Uuid::new_v4(), retain(page), deadline())
            .await
    });
    let call = command(&mut peer, "Runtime.callFunctionOn").await;
    assert_eq!(call["params"]["uniqueContextId"], "Acme-new-world");
    reply(
        &mut peer,
        &call,
        json!({"result":{"type":"object","objectId":"Acme-new-wrapper"}}),
    )
    .await;
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn retained_allocation_uncertainty_survives_reference_rotation_and_context_clear_until_exact_end()
 {
    for detach in [false, true] {
        let (mut worker, mut peer, _root, page) = seeded(0).await;
        let task = tokio::spawn(async move {
            let result = worker
                .execute_evaluation(
                    Uuid::new_v4(),
                    retain(page),
                    Instant::now() + Duration::from_millis(150),
                )
                .await;
            (worker, result)
        });
        command(&mut peer, "Runtime.callFunctionOn").await;
        let (mut worker, result) = task.await.unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.partial.unwrap()["handleAllocationPending"], true);
        let frame = worker.index.execution_world(page).unwrap().0;
        worker.index.content_replaced(frame).unwrap();
        worker.fence_evaluation();
        assert_eq!(worker.handle_allocation_snapshot()["pending"], 1);
        worker
            .apply(EventItem::Event(Event {
                method: "Runtime.executionContextDestroyed".into(),
                session_id: Some("unowned-session".into()),
                params: json!({"executionContextId":1,"executionContextUniqueId":"world-acme"}),
            }))
            .unwrap();
        worker
            .apply(EventItem::Event(Event {
                method: "Runtime.executionContextsCleared".into(),
                session_id: Some("session-acme".into()),
                params: json!({}),
            }))
            .unwrap();
        worker.apply(EventItem::Event(Event {method:"Runtime.executionContextDestroyed".into(),session_id:Some("session-acme".into()),params:json!({"executionContextId":99,"executionContextUniqueId":"Acme-other-world"})})).unwrap();
        assert_eq!(worker.handle_allocation_snapshot()["uncertain"], 1);
        worker
            .apply(EventItem::Event(if detach {
                Event {
                    method: "Target.detachedFromTarget".into(),
                    session_id: None,
                    params: json!({"sessionId":"session-acme"}),
                }
            } else {
                Event {
                    method: "Runtime.executionContextDestroyed".into(),
                    session_id: Some("session-acme".into()),
                    params: json!({"executionContextId":1,"executionContextUniqueId":"world-acme"}),
                }
            }))
            .unwrap();
        assert_eq!(worker.handle_allocation_snapshot()["pending"], 0);
    }
}

#[tokio::test]
async fn retained_allocation_malformed_lifetime_evidence_cannot_release_uncertainty() {
    let (mut worker, mut peer, _root, page) = seeded(0).await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(
                Uuid::new_v4(),
                retain(page),
                Instant::now() + Duration::from_millis(150),
            )
            .await;
        (worker, result)
    });
    command(&mut peer, "Runtime.callFunctionOn").await;
    let (mut worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    assert!(
        worker
            .apply(EventItem::Event(Event {
                method: "Runtime.executionContextDestroyed".into(),
                session_id: Some("session-acme".into()),
                params: json!({"executionContextUniqueId":"world-acme"})
            }))
            .is_err()
    );
    assert_eq!(worker.handle_allocation_snapshot()["uncertain"], 1);
}

#[tokio::test]
async fn retained_allocation_confirmed_failure_releases_capacity_for_one_successful_creation() {
    let (mut worker, mut peer, _root, page) = seeded(255).await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(Uuid::new_v4(), retain(page), deadline())
            .await;
        (worker, result)
    });
    let call = command(&mut peer, "Runtime.callFunctionOn").await;
    send(&mut peer,json!({"id":call["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme confirmed refusal"}})).await;
    let (mut worker, result) = task.await.unwrap();
    assert!(result.is_err());
    assert_eq!(
        worker.handle_allocation_snapshot(),
        json!({"limit":256,"retained":255,"pending":0,"uncertain":0})
    );
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(Uuid::new_v4(), retain(page), deadline())
            .await;
        (worker, result)
    });
    let call = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &call,
        json!({"result":{"type":"object","objectId":"Acme-successful-wrapper"}}),
    )
    .await;
    let (worker, result) = task.await.unwrap();
    result.unwrap();
    assert_eq!(
        worker.handle_allocation_snapshot(),
        json!({"limit":256,"retained":256,"pending":0,"uncertain":0})
    );
}

#[tokio::test]
async fn retained_allocation_unknown_selector_preparation_releases_the_undispatched_creation_slot()
{
    let (mut worker, mut peer, _root, page) = seeded(255).await;
    let request=serde_json::from_value(json!({"operation":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":"body"},"expression":"element=>element","isFunction":true,"returnHandle":true}})).unwrap();
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(
                Uuid::new_v4(),
                request,
                Instant::now() + Duration::from_millis(150),
            )
            .await;
        (worker, result)
    });
    command(&mut peer, "Runtime.evaluate").await;
    let (worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    assert_eq!(
        worker.handle_allocation_snapshot(),
        json!({"limit":256,"retained":255,"pending":0,"uncertain":0})
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn retained_allocation_missing_adoption_group_is_not_a_native_settlement_witness() {
    for closed in [false, true] {
        let (mut worker, mut peer, _root, context, frame, handle) =
            super::evaluation_adoption::setup().await;
        let native = worker
            .index
            .native(worker.index.owning_page(frame).unwrap(), Kind::Page)
            .unwrap();
        let request=serde_json::from_value(json!({"operation":"evaluate","scope":frame,"expression":"value=>({Acme:value})","isFunction":true,"argument":{"kind":"handle","handle":handle},"returnHandle":true})).unwrap();
        let mut task = tokio::spawn(async move {
            let result = worker
                .execute_evaluation(Uuid::new_v4(), request, deadline())
                .await;
            (worker, result)
        });
        let call = command(&mut peer, "Runtime.callFunctionOn").await;
        reply(
            &mut peer,
            &call,
            json!({"result":{"type":"object","subtype":"node","objectId":"Acme-source-node"}}),
        )
        .await;
        let call = command(&mut peer, "DOM.describeNode").await;
        reply(&mut peer,&call,json!({"node":{"nodeId":1,"backendNodeId":7,"nodeType":1,"nodeName":"BODY","localName":"body","nodeValue":""}})).await;
        let call = command(&mut peer, "DOM.resolveNode").await;
        reply(
            &mut peer,
            &call,
            json!({"object":{"type":"object","subtype":"node","objectId":"Acme-adopted-node"}}),
        )
        .await;
        let call = command(&mut peer, "Runtime.callFunctionOn").await;
        reply(
            &mut peer,
            &call,
            json!({"result":{"type":"object","objectId":"Acme-target-wrapper"}}),
        )
        .await;
        let call = command(&mut peer, "Runtime.releaseObjectGroup").await;
        reply(&mut peer, &call, json!({})).await;
        let _caller = command(&mut peer, "Runtime.callFunctionOn").await;
        send(&mut peer,if closed {
        json!({"method":"Target.targetDestroyed","params":{"targetId":native}})
    } else {json!({"method":"Target.detachedFromTarget","params":{"sessionId":"session-acme","targetId":42}})}).await;
        let mut peer_open = true;
        let (worker,result)=tokio::time::timeout_at(deadline(),async {
        loop {
            tokio::select! {
                joined=&mut task=>break joined.unwrap(),
                message=peer.next(),if peer_open=>{
                    let Some(Ok(message))=message else {peer_open=false;continue;};
                    if message.is_text() {
                        let call:Value=serde_json::from_str(message.to_text().unwrap()).unwrap();
                        assert_eq!(call["method"],"Runtime.releaseObjectGroup");
                        reply(&mut peer,&call,json!({})).await;
                    }
                }
            }
        }
    }).await.unwrap();
        let error = result.unwrap_err();
        if closed {
            assert_eq!(error.code, "STALE_REFERENCE");
            assert_eq!(error.partial.as_ref().unwrap()["scopeClosed"], true);
            assert_eq!(
                error.partial.as_ref().unwrap()["callerEffectsUnknown"],
                true
            );
            assert_eq!(
                worker.evaluation.groups.pending(
                    context,
                    super::super::execution_groups::Purpose::ArgumentAdoption
                ),
                0,
                "The scope has retired private bookkeeping without a native caller reply"
            );
        } else {
            assert_eq!(error.code, "OUTCOME_UNKNOWN");
        }
        assert_eq!(worker.handle_allocation_snapshot()["pending"], 1);
        assert_eq!(worker.handle_allocation_snapshot()["uncertain"], 1);
    }
}
