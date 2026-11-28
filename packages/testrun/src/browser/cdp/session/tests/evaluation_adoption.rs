//! Cross-frame argument preparation owns temporary objects without replaying callers.
use super::investigation::owned_page;
use super::*;

pub(super) async fn setup() -> (
    Worker,
    Peer,
    tempfile::TempDir,
    Reference,
    Reference,
    Reference,
) {
    let (mut worker, mut peer, root, context, page) = owned_page().await;
    let task = tokio::spawn(async move {
        let result = worker
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
            serde_json::from_value(result["handle"].clone()).unwrap(),
        )
    });
    let call = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &call,
        json!({"result":{"type":"object","objectId":"Acme-source-wrapper"}}),
    )
    .await;
    let (mut worker, handle) = task.await.unwrap();
    for (method, params) in [
        (
            "Page.frameAttached",
            json!({"frameId":"frame-child","parentFrameId":"frame-acme"}),
        ),
        (
            "Runtime.executionContextCreated",
            json!({"context":{"id":2,"uniqueId":"world-child","origin":"https://example.com","name":"","auxData":{"frameId":"frame-child","isDefault":true}}}),
        ),
    ] {
        worker
            .apply(EventItem::Event(Event {
                method: method.into(),
                session_id: Some("session-acme".into()),
                params,
            }))
            .unwrap();
    }
    let child = worker
        .index
        .list(Kind::Frame)
        .into_iter()
        .find_map(|(frame, _)| {
            (worker.index.native(frame, Kind::Frame).unwrap() == "frame-child").then_some(frame)
        })
        .unwrap();
    (worker, peer, root, context, child, handle)
}

fn request(frame: Reference, handle: Reference, selector: bool) -> Request {
    let argument = json!({"kind":"array","id":1,"items":[{"kind":"handle","handle":handle},{"kind":"handle","handle":handle}]});
    serde_json::from_value(if selector {json!({"operation":"selector_evaluate","scope":frame,"options":{"locator":{"kind":"css","selector":"body"},"expression":"(element,args)=>args[0]===args[1]","isFunction":true,"argument":argument}})} else {json!({"operation":"evaluate","scope":frame,"expression":"args=>args[0]===args[1]","isFunction":true,"argument":argument})}).unwrap()
}

async fn release(peer: &mut Peer) -> String {
    let call = command(peer, "Runtime.releaseObjectGroup").await;
    let group = call["params"]["objectGroup"].as_str().unwrap().to_owned();
    reply(peer, &call, json!({})).await;
    group
}

#[tokio::test]
async fn evaluation_adoption_deduplicates_nodes_and_primitives_for_evaluation_and_selectors() {
    for selector in [false, true] {
        for node in [false, true] {
            let (mut worker, mut peer, _root, context, frame, handle) = setup().await;
            let task = tokio::spawn(async move {
                let result = worker
                    .execute_evaluation(
                        Uuid::new_v4(),
                        request(frame, handle, selector),
                        deadline(),
                    )
                    .await;
                (worker, result)
            });
            if selector {
                let init = command(&mut peer, "Runtime.evaluate").await;
                reply(
                    &mut peer,
                    &init,
                    json!({"result":{"type":"boolean","value":true}}),
                )
                .await;
            }
            let extract = command(&mut peer, "Runtime.callFunctionOn").await;
            assert_eq!(extract["params"]["objectId"], "Acme-source-wrapper");
            assert_eq!(
                extract["params"]["functionDeclaration"],
                "function(){return this.value;}"
            );
            let source_group = extract["params"]["objectGroup"].clone();
            send(&mut peer,json!({"method":"Runtime.consoleAPICalled","sessionId":"session-acme","params":{"type":"log","executionContextId":1,"timestamp":1,"args":[]}})).await;
            release_console_group(&mut peer).await;
            assert!(
                tokio::time::timeout(Duration::from_millis(25), peer.next())
                    .await
                    .is_err(),
                "Event-driven housekeeping must not release the target group during source extraction"
            );
            reply(&mut peer,&extract,if node {json!({"result":{"type":"object","subtype":"node","objectId":"Acme-source-node"}})} else {json!({"result":{"type":"number","unserializableValue":"-0"}})}).await;
            if node {
                adopt_node(&mut peer).await;
            }
            let wrap = command(&mut peer, "Runtime.callFunctionOn").await;
            assert_eq!(wrap["params"]["uniqueContextId"], "world-child");
            assert_eq!(
                wrap["params"]["arguments"][0],
                if node {
                    json!({"objectId":"Acme-adopted-node"})
                } else {
                    json!({"unserializableValue":"-0"})
                }
            );
            let target_group = wrap["params"]["objectGroup"].clone();
            assert_ne!(source_group, target_group);
            reply(
                &mut peer,
                &wrap,
                json!({"result":{"type":"object","objectId":"Acme-target-wrapper"}}),
            )
            .await;
            assert_eq!(release(&mut peer).await, source_group.as_str().unwrap());
            let caller = command(&mut peer, "Runtime.callFunctionOn").await;
            assert_eq!(caller["params"]["uniqueContextId"], "world-child");
            let args = caller["params"]["arguments"].as_array().unwrap();
            assert_eq!(args.len(), 3);
            assert_eq!(args[1], json!({"objectId":"Acme-target-wrapper"}));
            assert_eq!(
                args[2], args[1],
                "Duplicate references require one adoption, not another native allocation"
            );
            assert_ne!(
                caller["params"]["objectGroup"], target_group,
                "A retained caller result must not belong to the temporary argument group"
            );
            reply(
                &mut peer,
                &caller,
                json!({"result":{"type":"object","value":{"b":true}}}),
            )
            .await;
            assert_eq!(release(&mut peer).await, target_group.as_str().unwrap());
            let (worker, result) = task.await.unwrap();
            assert_eq!(result.unwrap()["value"]["value"], true);
            assert_eq!(
                worker.evaluation.groups.pending(
                    context,
                    super::super::execution_groups::Purpose::ArgumentAdoption
                ),
                0
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(25), peer.next())
                    .await
                    .is_err()
            );
        }
    }
}

#[tokio::test]
async fn evaluation_adoption_refuses_non_nodes_without_running_the_caller() {
    let (mut worker, mut peer, _root, context, frame, handle) = setup().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(Uuid::new_v4(), request(frame, handle, false), deadline())
            .await;
        (worker, result)
    });
    let extract = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &extract,
        json!({"result":{"type":"object","objectId":"Acme-ordinary-object"}}),
    )
    .await;
    release(&mut peer).await;
    release(&mut peer).await;
    let (worker, result) = task.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "FOREIGN_HANDLE");
    assert_eq!(error.partial.unwrap()["callerDispatched"], false);
    assert_eq!(
        worker.evaluation.groups.pending(
            context,
            super::super::execution_groups::Purpose::ArgumentAdoption
        ),
        0
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn evaluation_adoption_fences_source_loss_before_caller_dispatch() {
    let (mut worker, mut peer, _root, _context, frame, handle) = setup().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(Uuid::new_v4(), request(frame, handle, false), deadline())
            .await;
        (worker, result)
    });
    let extract = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &extract,
        json!({"result":{"type":"object","subtype":"node","objectId":"Acme-source-node"}}),
    )
    .await;
    let describe = command(&mut peer, "DOM.describeNode").await;
    send(&mut peer,json!({"method":"Runtime.executionContextDestroyed","sessionId":"session-acme","params":{"executionContextId":1,"executionContextUniqueId":"world-acme"}})).await;
    reply(&mut peer,&describe,json!({"node":{"nodeId":1,"backendNodeId":7,"nodeType":1,"nodeName":"BODY","localName":"body","nodeValue":""}})).await;
    release(&mut peer).await;
    release(&mut peer).await;
    let (_worker, result) = task.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "STALE_REFERENCE");
    let partial = error.partial.unwrap();
    assert_eq!(partial["callerDispatched"], false);
    assert_eq!(partial["cause"]["adoptionStage"], "describe-node");
    assert_eq!(partial["cause"]["cause"]["sourceWorldLive"], false);
    assert_eq!(partial["cause"]["cause"]["targetWorldLive"], true);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn evaluation_adoption_retains_failed_cleanup_without_repeating_preparation() {
    let (mut worker, mut peer, _root, context, frame, handle) = setup().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(Uuid::new_v4(), request(frame, handle, false), deadline())
            .await;
        (worker, result)
    });
    let extract = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &extract,
        json!({"result":{"type":"object","objectId":"Acme-ordinary-object"}}),
    )
    .await;
    let failed = command(&mut peer, "Runtime.releaseObjectGroup").await;
    send(&mut peer,json!({"id":failed["id"],"sessionId":failed["sessionId"],"error":{"code":-32000,"message":"Acme cleanup refused"}})).await;
    release(&mut peer).await;
    let (mut worker, result) = task.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "FOREIGN_HANDLE");
    assert_eq!(error.partial.unwrap()["argumentCleanupPending"], true);
    assert_eq!(
        worker.evaluation.groups.pending(
            context,
            super::super::execution_groups::Purpose::ArgumentAdoption
        ),
        1
    );
    let retry = tokio::spawn(async move {
        worker.release_execution_groups(deadline()).await;
        worker
    });
    let group = release(&mut peer).await;
    assert_eq!(group, failed["params"]["objectGroup"].as_str().unwrap());
    let worker = retry.await.unwrap();
    assert_eq!(
        worker.evaluation.groups.pending(
            context,
            super::super::execution_groups::Purpose::ArgumentAdoption
        ),
        0
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn evaluation_adoption_unknown_caller_retains_only_its_temporary_target_owner() {
    let (mut worker, mut peer, _root, context, frame, handle) = setup().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(Uuid::new_v4(), request(frame, handle, false), deadline())
            .await;
        (worker, result)
    });
    let extract = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &extract,
        json!({"result":{"type":"string","value":"Acme"}}),
    )
    .await;
    let wrap = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &wrap,
        json!({"result":{"type":"object","objectId":"Acme-target-wrapper"}}),
    )
    .await;
    release(&mut peer).await;
    let caller = command(&mut peer, "Runtime.callFunctionOn").await;
    assert_eq!(caller["params"]["awaitPromise"], true);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    let (worker, result) = task.await.unwrap();
    tokio::time::resume();
    let error = result.unwrap_err();
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    assert_eq!(error.partial.unwrap()["argumentCleanupPending"], true);
    assert_eq!(
        worker.evaluation.groups.pending(
            context,
            super::super::execution_groups::Purpose::ArgumentAdoption
        ),
        1
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Unknown callers cannot release their arguments or be replayed"
    );
}

#[tokio::test]
async fn evaluation_adoption_preserves_unknown_effects_after_acknowledged_context_close() {
    let (mut worker, mut peer, _root, context, frame, handle) = setup().await;
    let (session, _ordinary, executions, responses, _control) =
        super::execution_concurrency::client(&worker);
    worker.executions = Some(executions);
    worker.responses = Some(responses);
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(Uuid::new_v4(), request(frame, handle, false), deadline())
            .await;
        (worker, result)
    });
    let extract = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &extract,
        json!({"result":{"type":"string","value":"Acme"}}),
    )
    .await;
    let wrap = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &wrap,
        json!({"result":{"type":"object","objectId":"Acme-target-wrapper"}}),
    )
    .await;
    release(&mut peer).await;
    let caller = command(&mut peer, "Runtime.callFunctionOn").await;
    assert_eq!(caller["params"]["awaitPromise"], true);
    let closing = session
        .submit(Uuid::new_v4(), Request::ContextClose { context }, 5000)
        .unwrap();
    let dispose = command(&mut peer, "Target.disposeBrowserContext").await;
    reply(&mut peer, &dispose, json!({})).await;
    let (worker, result) = tokio::time::timeout(Duration::from_millis(250), task)
        .await
        .expect("Context closure must end adopted caller waits without their original deadline")
        .unwrap();
    assert_eq!(
        session.operation(closing).unwrap().outcome,
        Outcome::Succeeded
    );
    let error = result.unwrap_err();
    assert_eq!(error.code, "STALE_REFERENCE");
    let partial = error.partial.unwrap();
    assert_eq!(partial["scopeClosed"], true);
    assert_eq!(partial["callerEffectsUnknown"], true);
    assert_eq!(partial["potentiallyMutating"], true);
    assert_eq!(partial["cause"]["callerEffectsUnknown"], true);
    assert_eq!(partial["argumentCleanupPending"], false);
    assert_eq!(
        worker.evaluation.groups.pending(
            context,
            super::super::execution_groups::Purpose::ArgumentAdoption
        ),
        0
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Closed-context arguments cannot be released through a replacement attachment or caller replay"
    );
}

async fn adopt_node(peer: &mut Peer) {
    let describe = command(peer, "DOM.describeNode").await;
    assert_eq!(describe["params"]["objectId"], "Acme-source-node");
    reply(peer,&describe,json!({"node":{"nodeId":1,"backendNodeId":7,"nodeType":1,"nodeName":"BODY","localName":"body","nodeValue":""}})).await;
    let resolve = command(peer, "DOM.resolveNode").await;
    assert_eq!(resolve["params"]["backendNodeId"], 7);
    assert_eq!(resolve["params"]["executionContextId"], 2);
    reply(
        peer,
        &resolve,
        json!({"object":{"type":"object","subtype":"node","objectId":"Acme-adopted-node"}}),
    )
    .await;
}

async fn release_console_group(peer: &mut Peer) {
    let housekeeping = command(peer, "Runtime.releaseObjectGroup").await;
    assert_eq!(
        housekeeping["params"]["objectGroup"], "console",
        "Event-driven housekeeping may release console objects, but must not release a borrowed adoption group"
    );
    reply(peer, &housekeeping, json!({})).await;
}
