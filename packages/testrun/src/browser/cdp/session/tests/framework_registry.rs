//! Public handle cleanup cannot bypass a live framework registry's owner.
use super::*;

async fn registry() -> (Worker, Peer, tempfile::TempDir, Reference) {
    let (mut worker, mut peer, root, _context, page) = investigation::owned_page().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_evaluation(
                Uuid::new_v4(),
                Request::Evaluate {
                    scope: page,
                    expression: "()=>({})".into(),
                    is_function: Some(true),
                    argument: Default::default(),
                    return_handle: true,
                },
                deadline(),
            )
            .await
            .unwrap();
        let registry = serde_json::from_value(result["handle"].clone()).unwrap();
        worker.frameworks.reserve().unwrap();
        worker.frameworks.insert(registry);
        (worker, registry)
    });
    let call = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &call,
        json!({"result":{"type":"object","objectId":"Acme-framework-registry"}}),
    )
    .await;
    let (worker, registry) = task.await.unwrap();
    (worker, peer, root, registry)
}

#[tokio::test]
async fn native_framework_registry_public_disposal_respects_owner_on_response_lane() {
    let (mut worker, mut peer, _root, registry) = registry().await;
    let refused = worker
        .execute_one(
            Uuid::new_v4(),
            Request::HandleDispose { handle: registry },
            deadline(),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, "FRAMEWORK_RELEASE_REQUIRED");
    let (session, _ordinary, _executions, mut responses, _control) =
        execution_concurrency::client(&worker);
    let capacity = session
        .0
        .capacity
        .clone()
        .try_acquire_many_owned(32)
        .unwrap();
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::HandleDispose { handle: registry },
            5000,
        )
        .unwrap();
    let command = responses.try_recv().unwrap();
    let task = tokio::spawn(async move {
        worker.respond(command).await;
        worker
    });
    let worker = tokio::time::timeout(Duration::from_millis(250), task)
        .await
        .expect("Public registry disposal must refuse without a native release command")
        .unwrap();
    assert_eq!(
        session.operation(id).unwrap().code.as_deref(),
        Some("FRAMEWORK_RELEASE_REQUIRED")
    );
    assert!(worker.evaluation_handle_live(registry));
    assert!(worker.frameworks.require(registry).is_ok());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    drop(capacity);
}

#[tokio::test]
async fn native_framework_registry_owned_release_preserves_cleanup_and_repeated_receipt() {
    let (mut worker, mut peer, _root, registry) = registry().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_one(
                Uuid::new_v4(),
                Request::FrameworkRelease { registry },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let close = command(&mut peer, "Runtime.callFunctionOn").await;
    assert!(
        close["params"]["functionDeclaration"]
            .as_str()
            .unwrap()
            .contains("registry.close()")
    );
    reply(
        &mut peer,
        &close,
        json!({"result":{"type":"object","value":{"v":"undefined"}}}),
    )
    .await;
    let dispose = command(&mut peer, "Runtime.releaseObject").await;
    assert_eq!(dispose["params"]["objectId"], "Acme-framework-registry");
    reply(&mut peer, &dispose, json!({})).await;
    let (mut worker, result) = task.await.unwrap();
    let result = result.unwrap();
    assert_eq!(result["released"], true);
    assert_eq!(result["hookInstallationPreserved"], true);
    assert!(!worker.evaluation_handle_live(registry));
    assert!(worker.frameworks.closed(registry));
    assert_eq!(
        worker
            .execute_one(
                Uuid::new_v4(),
                Request::FrameworkRelease { registry },
                deadline()
            )
            .await
            .unwrap()["released"],
        true
    );
    assert_eq!(
        worker
            .execute_one(
                Uuid::new_v4(),
                Request::HandleDispose { handle: registry },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "STALE_REFERENCE"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_framework_registry_failed_close_retains_owner_for_retry() {
    let (mut worker, mut peer, _root, registry) = registry().await;
    let task = tokio::spawn(async move {
        let result = worker
            .execute_one(
                Uuid::new_v4(),
                Request::FrameworkRelease { registry },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let close = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &close,
        json!({"result":{"type":"undefined"},"exceptionDetails":{"exceptionId":1,"lineNumber":0,"columnNumber":0,"text":"Uncaught","exception":{"type":"object","subtype":"error","description":"Error: Acme framework close refused"}}}),
    )
    .await;
    let (mut worker, result) = task.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "JAVASCRIPT_EXCEPTION");
    assert!(worker.evaluation_handle_live(registry));
    assert!(worker.frameworks.require(registry).is_ok());
    assert!(!worker.frameworks.closed(registry));
    assert_eq!(
        worker
            .execute_one(
                Uuid::new_v4(),
                Request::HandleDispose { handle: registry },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "FRAMEWORK_RELEASE_REQUIRED"
    );
    let task = tokio::spawn(async move {
        let result = worker
            .execute_one(
                Uuid::new_v4(),
                Request::FrameworkRelease { registry },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let close = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &close,
        json!({"result":{"type":"object","value":{"v":"undefined"}}}),
    )
    .await;
    let dispose = command(&mut peer, "Runtime.releaseObject").await;
    reply(&mut peer, &dispose, json!({})).await;
    let (worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap()["released"], true);
    assert!(worker.frameworks.closed(registry));
    assert!(!worker.evaluation_handle_live(registry));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}
