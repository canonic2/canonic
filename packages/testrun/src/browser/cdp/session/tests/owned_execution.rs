//! Unknown execution may continue only behind this operation's native owner.
use super::investigation::owned_page;
use super::*;

fn evaluate(page: Reference, retained: bool) -> Request {
    Request::Evaluate {
        scope: page,
        expression: "()=>true".into(),
        is_function: Some(true),
        argument: Default::default(),
        return_handle: retained,
    }
}

async fn held_actor(
    mut worker: Worker,
    mut peer: Peer,
    request: Request,
    malformed: bool,
) -> (Worker, Peer, Session, Uuid, Value) {
    let relationship = matches!(request, Request::HandleOwnerFrame { .. });
    let (session, _ordinary, mut executions, responses, _control) =
        super::execution_concurrency::client(&worker);
    let id = session.submit(Uuid::new_v4(), request, 500).unwrap();
    let queued = executions.try_recv().unwrap();
    worker.executions = Some(executions);
    worker.responses = Some(responses);
    let task = tokio::spawn(async move {
        worker.perform(queued).await;
        worker
    });
    let mut held = command(&mut peer, "Runtime.callFunctionOn").await;
    if relationship {
        reply(
            &mut peer,
            &held,
            json!({"result":{"type":"object","subtype":"node","objectId":"Acme-node"}}),
        )
        .await;
        held = command(&mut peer, "Runtime.callFunctionOn").await;
        assert_eq!(held["params"]["awaitPromise"], true);
    }
    if malformed {
        send(&mut peer,json!({"method":"Target.detachedFromTarget","params":{"sessionId":"session-acme","targetId":42}})).await;
    }
    let worker = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    let operation = session.operation(id).unwrap();
    assert_eq!(operation.outcome, Outcome::Unknown, "{operation:?}");
    assert_eq!(operation.code.as_deref(), Some("OUTCOME_UNKNOWN"));
    (worker, peer, session, id, held)
}

#[tokio::test]
async fn owned_execution_actor_preserves_exact_unknown_owners_but_fences_an_unowned_later_call() {
    for reuse in [false, true] {
        for relationship in [false, true] {
            let (mut worker, mut peer, _root, context, page) = owned_page().await;
            let request = if relationship {
                let (current, request) = relationship_request(worker, &mut peer, page).await;
                worker = current;
                request
            } else {
                evaluate(page, true)
            };
            let (mut worker, mut peer, session, original, late) =
                held_actor(worker, peer, request, false).await;
            assert_eq!(
                worker.shared.state.lock().unwrap().phase,
                Phase::Ready,
                "The actor must preserve a healthy native session behind the exact unanswered owner's bounds"
            );
            if relationship {
                assert_eq!(
                    worker.evaluation.groups.pending(
                        context,
                        super::super::execution_groups::Purpose::FrameRelationship
                    ),
                    1
                );
            } else {
                assert_eq!(worker.handle_allocation_snapshot()["uncertain"], 1);
            }
            let receipt = session.operation(original).unwrap();
            let next = session
                .submit(Uuid::new_v4(), evaluate(page, false), 5000)
                .unwrap();
            let queued = worker.executions.as_mut().unwrap().try_recv().unwrap();
            let task = tokio::spawn(async move {
                worker.perform(queued).await;
                worker
            });
            let call = command(&mut peer, "Runtime.callFunctionOn").await;
            reply(
                &mut peer,
                &call,
                json!({"result":{"type":"object","value":{"b":true}}}),
            )
            .await;
            let mut worker = task.await.unwrap();
            assert_eq!(session.operation(next).unwrap().outcome, Outcome::Succeeded);
            reply(
                &mut peer,
                &late,
                json!({"result":{"type":"object","objectId":"Acme-late-wrapper"}}),
            )
            .await;
            assert_eq!(
                serde_json::to_value(session.operation(original).unwrap()).unwrap(),
                serde_json::to_value(&receipt).unwrap()
            );
            // History can evict a settled unknown receipt while its native owner
            // remains. Reusing that UUID must not reuse the old ownership proof.
            if reuse {
                let mut state = worker.shared.state.lock().unwrap();
                for _ in 0..crate::browser::contract::MAX_OPERATIONS {
                    let id = Uuid::new_v4();
                    state.begin(id, "Acme history".into()).unwrap();
                    state.operations.get_mut(&id).unwrap().settled = true;
                }
                assert!(!state.operations.contains_key(&original));
            }
            // An older owner cannot excuse a new call with no allocation/group.
            let id = session
                .submit(
                    if reuse { original } else { Uuid::new_v4() },
                    evaluate(page, false),
                    500,
                )
                .unwrap();
            let queued = worker.executions.as_mut().unwrap().try_recv().unwrap();
            let task = tokio::spawn(async move {
                worker.perform(queued).await;
                worker
            });
            command(&mut peer, "Runtime.callFunctionOn").await;
            let worker = task.await.unwrap();
            assert_eq!(session.operation(id).unwrap().outcome, Outcome::Unknown);
            assert_eq!(worker.shared.state.lock().unwrap().phase, Phase::Lost);
            assert_eq!(
                session
                    .submit(Uuid::new_v4(), evaluate(page, false), 500)
                    .unwrap_err()
                    .code,
                "SESSION_NOT_READY"
            );
        }
    }
}

#[tokio::test]
async fn owned_execution_actor_never_masks_native_transport_loss_with_an_allocation_owner() {
    let (worker, peer, _root, _, page) = owned_page().await;
    let (worker, _peer, session, _, _) = held_actor(worker, peer, evaluate(page, true), true).await;
    assert_eq!(worker.handle_allocation_snapshot()["uncertain"], 1);
    assert_eq!(worker.shared.state.lock().unwrap().phase, Phase::Lost);
    assert_eq!(
        session
            .submit(Uuid::new_v4(), evaluate(page, false), 500)
            .unwrap_err()
            .code,
        "SESSION_NOT_READY"
    );
}

#[tokio::test]
async fn owned_execution_capacity_status_matches_the_published_schema_without_unbounded_fields() {
    let (worker, peer, _root, _, page) = owned_page().await;
    let (_worker, _peer, session, _, _) =
        held_actor(worker, peer, evaluate(page, true), false).await;
    let schema = contract::schemas()["status"].clone();
    let status = session.status();
    assert_eq!(
        status["handleCapacity"],
        json!({"limit":256,"retained":0,"pending":1,"uncertain":1})
    );
    crate::schema::validate(&schema, &status)
        .expect("A real serialized native status must satisfy its published schema");
    for (field, value) in [
        ("limit", json!(255)),
        ("retained", json!(-1)),
        ("pending", json!(257)),
        ("uncertain", json!("1")),
        ("AcmeUnknown", json!(true)),
    ] {
        let mut invalid = status.clone();
        invalid["handleCapacity"][field] = value;
        assert!(
            crate::schema::validate(&schema, &invalid).is_err(),
            "{field} must remain refused"
        );
    }
    let mut missing = status;
    missing["handleCapacity"]
        .as_object_mut()
        .unwrap()
        .remove("uncertain");
    assert!(crate::schema::validate(&schema, &missing).is_err());
}

async fn relationship_request(
    mut worker: Worker,
    peer: &mut Peer,
    page: Reference,
) -> (Worker, Request) {
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(Uuid::new_v4(), evaluate(page, true), deadline())
            .await
            .unwrap();
        (
            worker,
            serde_json::from_value(result["handle"].clone()).unwrap(),
        )
    });
    let call = command(peer, "Runtime.callFunctionOn").await;
    reply(
        peer,
        &call,
        json!({"result":{"type":"object","objectId":"Acme-wrapper"}}),
    )
    .await;
    let (current, handle) = task.await.unwrap();
    let worker = current;
    (worker, Request::HandleOwnerFrame { handle })
}
