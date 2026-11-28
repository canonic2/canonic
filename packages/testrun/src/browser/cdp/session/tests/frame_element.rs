//! Embedding-element allocation follows parent realms and exact native settlement.
use super::*;

async fn prepare() -> (Worker, Peer, tempfile::TempDir, Reference, Reference) {
    let (worker, peer, root, _, child, source) = super::evaluation_adoption::setup().await;
    let parent = worker.evaluation_handle_document(source).unwrap().0;
    (worker, peer, root, child, parent)
}
async fn owner(peer: &mut Peer) {
    let call = command(peer, "DOM.getFrameOwner").await;
    assert_eq!(call["sessionId"], "session-acme");
    assert_eq!(call["params"]["frameId"], "frame-child");
    reply(peer, &call, json!({"backendNodeId":7})).await;
}
async fn resolve(peer: &mut Peer) -> String {
    let call = command(peer, "DOM.resolveNode").await;
    assert_eq!(call["params"]["executionContextId"], 1);
    assert_eq!(call["params"]["backendNodeId"], 7);
    let group = call["params"]["objectGroup"].as_str().unwrap().to_owned();
    reply(
        peer,
        &call,
        json!({"object":{"type":"object","subtype":"node","objectId":"Acme-frame-element"}}),
    )
    .await;
    group
}
async fn wrap(peer: &mut Peer, group: &str) -> Value {
    let call = command(peer, "Runtime.callFunctionOn").await;
    assert_eq!(call["params"]["uniqueContextId"], "world-acme");
    assert_eq!(
        call["params"]["arguments"],
        json!([{"objectId":"Acme-frame-element"}])
    );
    assert_eq!(call["params"]["objectGroup"], group);
    assert_eq!(call["params"]["awaitPromise"], false);
    call
}

#[tokio::test]
async fn frame_owner_embedding_native_accepts_child_document_rotation_at_each_parent_lookup_stage()
{
    for stage in ["owner", "resolve", "wrap"] {
        let (mut worker, mut peer, _root, child, parent) = prepare().await;
        let retained = worker
            .execute_evaluation(
                Uuid::new_v4(),
                Request::FrameRetain { frame: child },
                deadline(),
            )
            .await
            .unwrap();
        let frame = serde_json::from_value(retained["frame"].clone()).unwrap();
        let task = tokio::spawn(async move {
            let result = worker
                .execute_evaluation(Uuid::new_v4(), Request::FrameElement { frame }, deadline())
                .await;
            (worker, result)
        });
        let rotate = json!({"method":"Page.frameNavigated","sessionId":"session-acme","params":{"type":"Navigation","frame":{"id":"frame-child","parentId":"frame-acme","loaderId":"Acme-next-loader","url":"https://example.com/next","domainAndRegistry":"example.com","securityOrigin":"https://example.com","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}});
        super::super::super::schema::event("Page.frameNavigated", &rotate["params"]).unwrap();
        if stage == "owner" {
            send(&mut peer, rotate.clone()).await;
        }
        owner(&mut peer).await;
        if stage == "resolve" {
            send(&mut peer, rotate.clone()).await;
        }
        let group = resolve(&mut peer).await;
        let call = wrap(&mut peer, &group).await;
        if stage == "wrap" {
            send(&mut peer, rotate).await;
        }
        reply(
            &mut peer,
            &call,
            json!({"result":{"type":"object","objectId":"Acme-stable-frame-wrapper"}}),
        )
        .await;
        let (mut worker, result) = task.await.unwrap();
        let result = result.unwrap();
        assert_eq!(result["childFrame"], json!(frame));
        assert_eq!(result["frame"], json!(parent));
        let handle = serde_json::from_value(result["handle"].clone()).unwrap();
        assert!(worker.evaluation_handle_live(handle));
        assert_eq!(
            worker.index.native(child, Kind::Frame),
            Err(super::super::super::resources::ResourceError::Stale)
        );
        let current = worker.index.owner_frame(frame).unwrap();
        assert_eq!(current.id, child.id);
        assert_ne!(current, child);
        let released = worker
            .execute_evaluation(
                Uuid::new_v4(),
                Request::FrameOwnerRelease { frame },
                deadline(),
            )
            .await
            .unwrap();
        assert_eq!(released["nativeScopeClosed"], false);
        assert!(worker.evaluation_handle_live(handle));
        assert!(worker.index.native(current, Kind::Frame).is_ok());
    }
}

#[tokio::test]
async fn frame_element_native_transfers_one_group_and_retains_parent_authority_after_child_detach()
{
    let (mut worker, mut peer, _root, child, parent) = prepare().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(
                Uuid::new_v4(),
                Request::FrameElement { frame: child },
                deadline(),
            )
            .await;
        (worker, result)
    });
    owner(&mut peer).await;
    let group = resolve(&mut peer).await;
    let call = wrap(&mut peer, &group).await;
    reply(
        &mut peer,
        &call,
        json!({"result":{"type":"object","objectId":"Acme-frame-wrapper"}}),
    )
    .await;
    let (mut worker, result) = task.await.unwrap();
    let value = result.unwrap();
    assert_eq!(value["frame"], json!(parent));
    assert_eq!(value["childFrame"], json!(child));
    let handle: Reference = serde_json::from_value(value["handle"].clone()).unwrap();
    assert_eq!(
        worker.handle_allocation_snapshot(),
        json!({"limit":256,"retained":2,"pending":0,"uncertain":0})
    );
    worker
        .apply(EventItem::Event(Event {
            method: "Page.frameDetached".into(),
            session_id: Some("session-acme".into()),
            params: json!({"frameId":"frame-child","reason":"remove"}),
        }))
        .unwrap();
    assert!(worker.evaluation_handle_live(handle));
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(
                Uuid::new_v4(),
                Request::HandleDispose { handle },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
    assert_eq!(release["params"]["objectGroup"], group);
    reply(&mut peer, &release, json!({})).await;
    let (worker, result) = task.await.unwrap();
    result.unwrap();
    assert!(!worker.evaluation_handle_live(handle));
}

#[tokio::test]
async fn frame_element_native_race_releases_only_its_group_and_never_publishes_a_handle() {
    for parent_loss in [false, true] {
        let (mut worker, mut peer, _root, child, _) = prepare().await;
        let task = tokio::spawn(async move {
            let result = worker
                .execute_evaluation(
                    Uuid::new_v4(),
                    Request::FrameElement { frame: child },
                    deadline(),
                )
                .await;
            (worker, result)
        });
        owner(&mut peer).await;
        let group = resolve(&mut peer).await;
        let call = wrap(&mut peer, &group).await;
        let event = if parent_loss {
            json!({"method":"Runtime.executionContextDestroyed","sessionId":"session-acme","params":{"executionContextId":1,"executionContextUniqueId":"world-acme"}})
        } else {
            json!({"method":"Page.frameDetached","sessionId":"session-acme","params":{"frameId":"frame-child","reason":"remove"}})
        };
        send(&mut peer, event).await;
        reply(
            &mut peer,
            &call,
            json!({"result":{"type":"object","objectId":"Acme-stale-wrapper"}}),
        )
        .await;
        let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
        assert_eq!(release["params"]["objectGroup"], group);
        reply(&mut peer, &release, json!({})).await;
        let (worker, result) = task.await.unwrap();
        assert_eq!(result.unwrap_err().code, "DOCUMENT_CHANGED");
        assert_eq!(worker.handle_allocation_snapshot()["pending"], 0);
        assert!(worker.evaluation_resources().len() <= 1);
    }
}

#[tokio::test]
async fn frame_element_native_unknown_creation_keeps_capacity_and_refuses_main_frames_before_dispatch()
 {
    for stage in ["resolve", "wrap"] {
        let (mut worker, mut peer, _root, child, parent) = prepare().await;
        assert_eq!(
            worker
                .execute_evaluation(
                    Uuid::new_v4(),
                    Request::FrameElement { frame: parent },
                    deadline()
                )
                .await
                .unwrap_err()
                .code,
            "INVALID_SCOPE"
        );
        let task = tokio::spawn(async move {
            let result = worker
                .execute_evaluation(
                    Uuid::new_v4(),
                    Request::FrameElement { frame: child },
                    Instant::now() + Duration::from_millis(150),
                )
                .await;
            (worker, result)
        });
        owner(&mut peer).await;
        if stage == "wrap" {
            let group = resolve(&mut peer).await;
            wrap(&mut peer, &group).await;
        } else {
            command(&mut peer, "DOM.resolveNode").await;
        }
        let (worker, result) = task.await.unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.partial.unwrap()["handleAllocationPending"], true);
        assert_eq!(
            worker.handle_allocation_snapshot(),
            json!({"limit":256,"retained":1,"pending":1,"uncertain":1})
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn frame_element_native_capacity_refuses_before_lookup_and_failed_cleanup_retries_only_release()
 {
    let (mut worker, mut peer, _root, child, parent) = prepare().await;
    let task = tokio::spawn(async move {
        for _ in 0..255 {
            worker
                .execute_evaluation(
                    Uuid::new_v4(),
                    Request::Evaluate {
                        scope: parent,
                        expression: "()=>true".into(),
                        is_function: Some(true),
                        argument: Default::default(),
                        return_handle: true,
                    },
                    deadline(),
                )
                .await
                .unwrap();
        }
        worker
    });
    for ordinal in 0..255 {
        let call = command(&mut peer, "Runtime.callFunctionOn").await;
        reply(
            &mut peer,
            &call,
            json!({"result":{"type":"object","objectId":format!("Acme-capacity-{ordinal}")}}),
        )
        .await;
    }
    let mut worker = task.await.unwrap();
    assert_eq!(
        worker
            .execute_evaluation(
                Uuid::new_v4(),
                Request::FrameElement { frame: child },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "HANDLE_CAPACITY"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    drop(worker);
    drop(peer);

    let (mut worker, mut peer, _root, child, _) = prepare().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(
                Uuid::new_v4(),
                Request::FrameElement { frame: child },
                deadline(),
            )
            .await;
        (worker, result)
    });
    owner(&mut peer).await;
    let call = command(&mut peer, "DOM.resolveNode").await;
    let group = call["params"]["objectGroup"].clone();
    reply(&mut peer, &call, json!({"object":{"type":"undefined"}})).await;
    let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
    assert_eq!(release["params"]["objectGroup"], group);
    send(&mut peer,json!({"id":release["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme release refused"}})).await;
    let (mut worker, result) = task.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "BACKEND_PROTOCOL");
    assert_eq!(error.partial.unwrap()["relationshipCleanupPending"], true);
    assert_eq!(worker.handle_allocation_snapshot()["pending"], 0);
    let task = tokio::spawn(async move {
        worker.release_execution_groups(deadline()).await;
        worker
    });
    let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
    assert_eq!(release["params"]["objectGroup"], group);
    reply(&mut peer, &release, json!({})).await;
    let _worker = task.await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn frame_element_native_context_close_preserves_unconfirmed_creation_receipt_flags() {
    for stage in ["owner", "resolve", "wrap"] {
        let (mut worker, mut peer, _root, child, _) = prepare().await;
        let context = worker.index.diagnostic_scope("session-acme").unwrap().0;
        let (session, _ordinary, mut executions, responses, _control) =
            super::execution_concurrency::client(&worker);
        let original = session
            .submit(Uuid::new_v4(), Request::FrameElement { frame: child }, 5000)
            .unwrap();
        let queued = executions.try_recv().unwrap();
        worker.executions = Some(executions);
        worker.responses = Some(responses);
        let task = tokio::spawn(async move {
            worker.perform(queued).await;
            worker
        });
        if stage == "owner" {
            command(&mut peer, "DOM.getFrameOwner").await;
        } else {
            owner(&mut peer).await;
            if stage == "resolve" {
                command(&mut peer, "DOM.resolveNode").await;
            } else {
                let group = resolve(&mut peer).await;
                wrap(&mut peer, &group).await;
            }
        }
        let closing = session
            .submit(Uuid::new_v4(), Request::ContextClose { context }, 5000)
            .unwrap();
        let dispose = command(&mut peer, "Target.disposeBrowserContext").await;
        reply(&mut peer, &dispose, json!({})).await;
        // The creation reply never arrives, so completing at all proves the
        // wait ended without it; the deadline only bounds a hang.
        let worker = tokio::time::timeout_at(deadline(), task)
            .await
            .expect(
                "Context disposal ends the local wait without confirming the native creation reply",
            )
            .unwrap();
        assert_eq!(
            session.operation(closing).unwrap().outcome,
            Outcome::Succeeded
        );
        let receipt = session.operation(original).unwrap();
        assert_eq!(receipt.outcome, Outcome::Unknown);
        assert_eq!(receipt.code.as_deref(), Some("STALE_REFERENCE"));
        let partial = receipt.result.unwrap();
        assert_eq!(partial["scopeClosed"], true);
        assert_eq!(partial["callerEffectsUnknown"], true);
        assert_eq!(
            worker.shared.state.lock().unwrap().phase,
            Phase::Ready,
            "Closing the exact owned context must not lose the sibling browser session"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err(),
            "Closure cannot replay creation or release its retired group through another attachment"
        );
    }
}

#[tokio::test]
async fn frame_element_cleanup_backpressure_refuses_all_new_retained_results_before_dispatch() {
    for kind in ["embedding", "evaluate", "property", "selector"] {
        let (mut worker, mut peer, _root, child, parent) = prepare().await;
        let source: Reference =
            serde_json::from_value(worker.evaluation_resources()[0]["reference"].clone()).unwrap();
        for ordinal in 0..256 {
            worker.queue_execution_group(
                "session-acme".into(),
                format!("testrun:Acme-pending-cleanup-{ordinal}"),
            );
        }
        let request=match kind {
            "embedding"=>Request::FrameElement {frame:child},
            "evaluate"=>Request::Evaluate {scope:parent,expression:"()=>true".into(),is_function:Some(true),argument:Default::default(),return_handle:true},
            "property"=>Request::HandleProperty {handle:source,name:"Acme".into()},
            _=>serde_json::from_value(json!({"operation":"selector_evaluate","scope":parent,"options":{"locator":{"kind":"css","selector":"body"},"expression":"element=>element","isFunction":true,"returnHandle":true}})).unwrap(),
        };
        let error = worker
            .execute_evaluation(
                Uuid::new_v4(),
                request,
                Instant::now() + Duration::from_millis(150),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "EXECUTION_CLEANUP_PENDING", "{kind}: {error:?}");
        assert_eq!(
            worker.handle_allocation_snapshot(),
            json!({"limit":256,"retained":1,"pending":0,"uncertain":0})
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err(),
            "Backpressure must refuse before selector preparation, relationship lookup or caller creation"
        );
    }
}

#[tokio::test]
async fn frame_element_cleanup_backpressure_preserves_existing_handle_reads_and_disposal() {
    for dispose in [false, true] {
        let (mut worker, mut peer, _root, _, _) = prepare().await;
        let handle: Reference =
            serde_json::from_value(worker.evaluation_resources()[0]["reference"].clone()).unwrap();
        for ordinal in 0..256 {
            worker.queue_execution_group(
                "session-acme".into(),
                format!("testrun:Acme-safe-release-{ordinal}"),
            );
        }
        let task = tokio::spawn(async move {
            let request = if dispose {
                Request::HandleDispose { handle }
            } else {
                Request::HandleRead { handle }
            };
            let result = worker
                .execute_evaluation(Uuid::new_v4(), request, deadline())
                .await;
            (worker, result)
        });
        let call = command(
            &mut peer,
            if dispose {
                "Runtime.releaseObject"
            } else {
                "Runtime.callFunctionOn"
            },
        )
        .await;
        reply(
            &mut peer,
            &call,
            if dispose {
                json!({})
            } else {
                json!({"result":{"type":"object","value":{"b":true}}})
            },
        )
        .await;
        let (worker, result) = task.await.unwrap();
        assert!(
            result.is_ok(),
            "Existing resources must remain usable for recovery: {result:?}"
        );
        assert_eq!(
            worker.execution_cleanup_capacity().unwrap_err().code,
            "EXECUTION_CLEANUP_PENDING"
        );
        assert_eq!(worker.evaluation_handle_live(handle), !dispose);
    }
}

#[tokio::test]
async fn frame_element_cleanup_backpressure_recovers_by_releasing_without_replaying_creation() {
    let (mut worker, mut peer, _root, child, _) = prepare().await;
    for ordinal in 0..256 {
        worker.queue_execution_group(
            "session-acme".into(),
            format!("testrun:Acme-safe-release-{ordinal}"),
        );
    }
    let task = tokio::spawn(async move {
        worker.release_evaluation().await;
        worker
    });
    let mut released = std::collections::BTreeSet::new();
    for _ in 0..256 {
        let call = command(&mut peer, "Runtime.releaseObjectGroup").await;
        let group = call["params"]["objectGroup"].as_str().unwrap();
        assert!(group.starts_with("testrun:Acme-safe-release-"));
        assert!(released.insert(group.to_owned()));
        reply(&mut peer, &call, json!({})).await;
    }
    let mut worker = task.await.unwrap();
    worker.execution_cleanup_capacity().unwrap();
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(
                Uuid::new_v4(),
                Request::FrameElement { frame: child },
                deadline(),
            )
            .await;
        (worker, result)
    });
    owner(&mut peer).await;
    let group = resolve(&mut peer).await;
    let call = wrap(&mut peer, &group).await;
    reply(
        &mut peer,
        &call,
        json!({"result":{"type":"object","objectId":"Acme-after-safe-release"}}),
    )
    .await;
    let (_worker, result) = task.await.unwrap();
    assert!(result.is_ok());
}

#[tokio::test]
async fn native_worker_events_follow_page_ownership_after_browser_discovery() {
    let (mut worker, _peer, _root, context, page) = super::investigation::owned_page().await;
    let info = |id: &str, kind: &str| json!({"targetId":id,"type":kind,"title":"Acme","url":"https://example.com/acme.js","attached":false,"canAccessOpener":false,"browserContextId":"context-acme"});
    // Browser-level discovery indexes the dedicated worker before the page
    // attachment names its page; service workers stay context-owned.
    for (method, session, params) in [
        (
            "Target.targetCreated",
            None,
            json!({"targetInfo":info("worker-acme","worker")}),
        ),
        (
            "Target.attachedToTarget",
            Some("session-acme"),
            json!({"sessionId":"session-worker","targetInfo":info("worker-acme","worker"),"waitingForDebugger":false}),
        ),
        (
            "Target.targetCreated",
            None,
            json!({"targetInfo":info("service-acme","service_worker")}),
        ),
    ] {
        worker
            .apply(EventItem::Event(Event {
                method: method.into(),
                session_id: session.map(str::to_owned),
                params,
            }))
            .unwrap();
    }
    let read = worker
        .interactions
        .history
        .read(context, None, 100)
        .unwrap();
    let events = read["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["event"] != "page_created")
        .map(|event| (event["event"].clone(), event["page"].clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        events,
        vec![
            (json!("worker_created"), json!(page)),
            (json!("service_worker_created"), Value::Null),
        ]
    );
}

#[tokio::test]
async fn native_page_events_record_child_frames_and_root_session_load_states() {
    let (mut worker, _peer, _root, context, page) = super::investigation::owned_page().await;
    for method in [
        "Page.frameAttached",
        "Page.domContentEventFired",
        "Page.loadEventFired",
        "Page.frameDetached",
    ] {
        let params = match method {
            "Page.frameAttached" => json!({"frameId":"frame-child","parentFrameId":"frame-acme"}),
            "Page.frameDetached" => json!({"frameId":"frame-child","reason":"remove"}),
            _ => json!({"timestamp":1.0}),
        };
        worker
            .apply(EventItem::Event(Event {
                method: method.into(),
                session_id: Some("session-acme".into()),
                params,
            }))
            .unwrap();
    }
    let read = worker
        .interactions
        .history
        .read(context, None, 100)
        .unwrap();
    let events = read["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| (event["event"].clone(), event["page"].clone()))
        .filter(|(event, _)| event != "page_created")
        .collect::<Vec<_>>();
    assert_eq!(
        events,
        vec![
            (json!("frame_attached"), json!(page)),
            (json!("dom_content_loaded"), json!(page)),
            (json!("load"), json!(page)),
            (json!("frame_detached"), json!(page)),
        ]
    );
    let attached = &read["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["event"] == "frame_attached")
        .unwrap()["resource"];
    let detached = &read["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["event"] == "frame_detached")
        .unwrap()["resource"];
    assert_eq!(attached, detached);
    assert_ne!(attached, &json!(page));
}
