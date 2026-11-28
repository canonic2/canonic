//! Selector calls share the main-world/document fence without temporary groups.
use super::investigation::owned_page;
use super::*;

#[tokio::test]
async fn page_scope_does_not_retry_rejected_parameters_without_document_replacement() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let running = tokio::spawn(async move {
        let result = worker
            .page_scope(Uuid::new_v4(), page, None, deadline())
            .await;
        (worker, result)
    });
    let world = command(&mut peer, "Page.createIsolatedWorld").await;
    send(&mut peer,json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":41,"origin":"://","name":page::WORLD,"uniqueId":"Acme-isolated","auxData":{"frameId":"frame-acme","isDefault":false}}}})).await;
    reply(&mut peer, &world, json!({"executionContextId":41})).await;
    let initialize = command(&mut peer, "Runtime.evaluate").await;
    send(&mut peer,json!({"id":initialize["id"],"sessionId":"session-acme","error":{"code":-32602,"message":"Invalid parameters"}})).await;
    let (_worker, result) = running.await.unwrap();
    let error = result.err().unwrap();
    assert_eq!(error.code, "BROWSER_METHOD_FAILED");
    assert_eq!(
        error.partial.unwrap()["backendFailure"]["message"],
        "Invalid parameters"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn page_scope_initialization_navigation_retries_only_an_implicit_main_frame() {
    for (explicit, rejected) in [(false, false), (true, false), (false, true), (true, true)] {
        let (mut worker, mut peer, _root, _context, page) = owned_page().await;
        let frame = worker.index.page_frame(page, None).unwrap();
        let running = tokio::spawn(async move {
            let result = worker
                .page_scope(Uuid::new_v4(), page, explicit.then_some(frame), deadline())
                .await;
            (worker, result)
        });
        let world = command(&mut peer, "Page.createIsolatedWorld").await;
        send(&mut peer,json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":41,"origin":"://","name":page::WORLD,"uniqueId":"Acme-old-isolated","auxData":{"frameId":"frame-acme","isDefault":false}}}})).await;
        reply(&mut peer, &world, json!({"executionContextId":41})).await;
        let initialize = command(&mut peer, "Runtime.evaluate").await;
        assert_eq!(initialize["params"]["uniqueContextId"], "Acme-old-isolated");
        send(&mut peer,json!({"method":"Page.frameNavigated","sessionId":"session-acme","params":{"type":"Navigation","frame":{"id":"frame-acme","loaderId":"Acme-replacement","url":"https://example.com/replacement","domainAndRegistry":"example.com","securityOrigin":"https://example.com","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await;
        if rejected {
            send(&mut peer,json!({"id":initialize["id"],"sessionId":"session-acme","error":{"code":-32602,"message":"Cannot find context with specified id"}})).await;
        } else {
            reply(
                &mut peer,
                &initialize,
                json!({"result":{"type":"boolean","value":true}}),
            )
            .await;
        }
        if !explicit {
            let world = command(&mut peer, "Page.createIsolatedWorld").await;
            send(&mut peer,json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":42,"origin":"https://example.com","name":page::WORLD,"uniqueId":"Acme-new-isolated","auxData":{"frameId":"frame-acme","isDefault":false}}}})).await;
            reply(&mut peer, &world, json!({"executionContextId":42})).await;
            let initialize = command(&mut peer, "Runtime.evaluate").await;
            assert_eq!(initialize["params"]["uniqueContextId"], "Acme-new-isolated");
            reply(
                &mut peer,
                &initialize,
                json!({"result":{"type":"boolean","value":true}}),
            )
            .await;
        }
        let (_worker, result) = running.await.unwrap();
        if explicit {
            assert_eq!(result.err().unwrap().code, "STALE_REFERENCE");
        } else {
            assert_eq!(result.unwrap().unique, "Acme-new-isolated");
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn selector_evaluation_retained_results_keep_world_ownership_and_disposal() {
    for navigate in [false, true] {
        let (mut worker, mut peer, _root, _, page) = owned_page().await;
        let request=serde_json::from_value(json!({"operation":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":".acme"},"expression":"element=>element","returnHandle":true,"waitForAttachment":true,"waitTimeoutMs":100}})).unwrap();
        let running = tokio::spawn(async move {
            let result = worker
                .execute_evaluation(Uuid::new_v4(), request, deadline())
                .await;
            (worker, result)
        });
        let init = command(&mut peer, "Runtime.evaluate").await;
        reply(
            &mut peer,
            &init,
            json!({"result":{"type":"boolean","value":true}}),
        )
        .await;
        let call = command(&mut peer, "Runtime.callFunctionOn").await;
        assert_eq!(call["params"]["returnByValue"], false);
        assert_eq!(call["params"]["awaitPromise"], true);
        assert_eq!(call["params"]["uniqueContextId"], "world-acme");
        if navigate {
            send(&mut peer,json!({"method":"Runtime.executionContextsCleared","sessionId":"session-acme","params":{}})).await;
        }
        reply(
            &mut peer,
            &call,
            json!({"result":{"type":"object","objectId":"acme-result"}}),
        )
        .await;
        let (mut worker, result) = running.await.unwrap();
        if navigate {
            assert_eq!(
                result.unwrap_err().partial.unwrap()["dispatchCompleted"],
                true
            );
        } else {
            let handle = serde_json::from_value(result.unwrap()["handle"].clone()).unwrap();
            assert!(worker.evaluation_handle_live(handle));
            let running = tokio::spawn(async move {
                let result = worker
                    .execute_evaluation(
                        Uuid::new_v4(),
                        Request::HandleDispose { handle },
                        deadline(),
                    )
                    .await
                    .unwrap();
                (worker, result)
            });
            let dispose = command(&mut peer, "Runtime.releaseObject").await;
            assert_eq!(dispose["params"]["objectId"], "acme-result");
            reply(&mut peer, &dispose, json!({})).await;
            let (_worker, result) = running.await.unwrap();
            assert_eq!(result["disposed"], true);
            assert!(
                tokio::time::timeout(Duration::from_millis(25), peer.next())
                    .await
                    .is_err()
            );
            continue;
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn selector_evaluation_uses_owned_main_world_and_does_not_replay_changed_document() {
    for navigate in [false, true] {
        let (mut worker, mut peer, _root, _, page) = owned_page().await;
        let request: Request=serde_json::from_value(json!({"operation":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":".acme"},"expression":"(element,arg)=>arg","isFunction":true,"argument":{"kind":"number","value":"negative_zero"}}})).unwrap();
        let repeated = request.clone();
        let running = tokio::spawn(async move {
            let result = worker
                .execute_evaluation(Uuid::new_v4(), request, deadline())
                .await;
            (worker, result)
        });
        let initialized = command(&mut peer, "Runtime.evaluate").await;
        assert_eq!(initialized["params"]["uniqueContextId"], "world-acme");
        reply(
            &mut peer,
            &initialized,
            json!({"result":{"type":"boolean","value":true}}),
        )
        .await;
        let call = command(&mut peer, "Runtime.callFunctionOn").await;
        assert_eq!(call["params"]["uniqueContextId"], "world-acme");
        assert_eq!(call["params"]["returnByValue"], true);
        assert_eq!(call["params"]["awaitPromise"], true);
        assert_eq!(call["params"]["arguments"], json!([{"value":{"v":"-0"}}]));
        if navigate {
            send(&mut peer,json!({"method":"Runtime.executionContextsCleared","sessionId":"session-acme","params":{}})).await;
        }
        reply(
            &mut peer,
            &call,
            json!({"result":{"type":"object","value":{"v":"-0"}}}),
        )
        .await;
        let (mut worker, result) = running.await.unwrap();
        if navigate {
            let error = result.unwrap_err();
            assert_eq!(error.partial.unwrap()["dispatchCompleted"], true);
        } else {
            assert_eq!(
                result.unwrap()["value"],
                json!({"kind":"number","value":"negative_zero"})
            );
        }
        if navigate {
            send(&mut peer,json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":2,"origin":"://","name":"","uniqueId":"world-acme-next","auxData":{"frameId":"frame-acme","isDefault":true}}}})).await;
        }
        let running = tokio::spawn(async move {
            let result = worker
                .execute_evaluation(Uuid::new_v4(), repeated, deadline())
                .await
                .unwrap();
            (worker, result)
        });
        if navigate {
            let initialized = command(&mut peer, "Runtime.evaluate").await;
            assert_eq!(initialized["params"]["uniqueContextId"], "world-acme-next");
            reply(
                &mut peer,
                &initialized,
                json!({"result":{"type":"boolean","value":true}}),
            )
            .await;
        }
        // A retained-world call must go straight to execution, not initialize again.
        let call = command(&mut peer, "Runtime.callFunctionOn").await;
        assert_eq!(
            call["params"]["uniqueContextId"],
            if navigate {
                "world-acme-next"
            } else {
                "world-acme"
            }
        );
        reply(
            &mut peer,
            &call,
            json!({"result":{"type":"object","value":{"v":"-0"}}}),
        )
        .await;
        let (_worker, result) = running.await.unwrap();
        assert_eq!(
            result["value"],
            json!({"kind":"number","value":"negative_zero"})
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn selector_evaluation_refuses_an_unknown_argument_before_helper_initialization() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let request=serde_json::from_value(json!({"operation":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":".acme"},"expression":"(element,arg)=>arg","argument":{"kind":"handle","handle":{"connectionId":page.connection_id,"id":Uuid::new_v4(),"generation":Uuid::new_v4()}}}})).unwrap();
    let error = worker
        .execute_evaluation(Uuid::new_v4(), request, deadline())
        .await
        .unwrap_err();
    assert_eq!(error.code, "STALE_REFERENCE");
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}
