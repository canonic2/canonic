//! Selector calls preserve typed arguments without retaining temporary handles.
use super::*;

#[tokio::test]
async fn evaluation_adoption_forwards_owned_cross_frame_handles_once_for_all_evaluator_paths() {
    for selector in [false, true] {
        for retained in [false, true] {
            let (session, mut peer, _root, _context, page) = harness().await;
            let mut child = created(
                "Acme-child",
                "Frame",
                json!({"url":"about:blank","name":"Acme","parentFrame":{"guid":"frame"},"loadStates":[]}),
            );
            child["guid"] = json!("page");
            peer.send(&child).await;
            peer.send(&json!({"guid":"page","method":"frameAttached","params":{"frame":{"guid":"Acme-child"}}})).await;
            let connection = session.0.host.connection().unwrap();
            connection.event_barrier(deadline()).await.unwrap();
            let wire = connection.resolve("Acme-child", "Frame").await.unwrap();
            let child = session
                .0
                .host
                .state
                .lock()
                .unwrap()
                .by_wire(&wire)
                .unwrap()
                .reference;
            let request=serde_json::from_value(json!({"operation":"evaluate","scope":page,"expression":"()=>document.body","isFunction":true,"returnHandle":true})).unwrap();
            let id = session.submit(Uuid::new_v4(), request, 5000).unwrap();
            let call = peer.request().await;
            assert_eq!(call["method"], "evaluateExpressionHandle");
            let mut handle = created(
                "Acme-source-node",
                "ElementHandle",
                json!({"preview":"JSHandle@node"}),
            );
            handle["guid"] = json!("frame");
            peer.send(&handle).await;
            peer.send(&json!({"id":call["id"],"result":{"handle":{"guid":"Acme-source-node"}}}))
                .await;
            let source = complete(&session, id).await.result.unwrap()["handle"].clone();
            let argument = json!({"kind":"handle","handle":source});
            let request=serde_json::from_value(if selector {json!({"operation":"selector_evaluate","scope":child,"options":{"locator":{"kind":"css","selector":"body"},"expression":"(element,node)=>node","isFunction":true,"argument":argument,"returnHandle":retained}})} else {json!({"operation":"evaluate","scope":child,"expression":"node=>node","isFunction":true,"argument":argument,"returnHandle":retained})}).unwrap();
            let id = session.submit(Uuid::new_v4(), request, 5000).unwrap();
            let call = peer.request().await;
            assert_eq!(call["guid"], "Acme-child");
            assert_eq!(
                call["params"]["arg"]["handles"],
                json!([{"guid":"Acme-source-node"}])
            );
            if retained {
                let mut result = created(
                    "Acme-independent-result",
                    "ElementHandle",
                    json!({"preview":"JSHandle@node"}),
                );
                result["guid"] = json!("Acme-child");
                peer.send(&result).await;
                peer.send(&json!({"id":call["id"],"result":{"handle":{"guid":"Acme-independent-result"}}})).await;
            } else {
                peer.send(&json!({"id":call["id"],"result":{"value":{"s":"Acme adopted"}}}))
                    .await;
            }
            let done = complete(&session, id).await;
            assert_eq!(done.outcome, Outcome::Succeeded, "{done:?}");
            if retained {
                assert_ne!(done.result.unwrap()["handle"], source);
            } else {
                assert_eq!(done.result.unwrap()["value"]["value"], "Acme adopted");
            }
            close(&session).await;
        }
    }
}

#[tokio::test]
async fn selector_evaluation_retains_results_and_maps_settled_wait_failures() {
    let (session, mut peer, _root, _, page) = harness().await;
    let request = serde_json::from_value(json!({"operation":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":".acme"},"expression":"element=>element","returnHandle":true,"waitForAttachment":true,"waitTimeoutMs":100}})).unwrap();
    let id = session.submit(Uuid::new_v4(), request, 5000).unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "evaluateExpressionHandle");
    assert_eq!(call["guid"], "frame");
    let expression = call["params"]["expression"].as_str().unwrap();
    assert!(expression.contains("\"strict\":true"));
    assert!(expression.contains("\"waitTimeoutMs\":100"));
    peer.send(&created(
        "acme-result",
        "JSHandle",
        json!({"preview":"Acme result"}),
    ))
    .await;
    peer.send(&json!({"id":call["id"],"result":{"handle":{"guid":"acme-result"}}}))
        .await;
    let done = complete(&session, id).await;
    assert_eq!(done.outcome, Outcome::Succeeded, "{done:?}");
    let handle: Reference = serde_json::from_value(done.result.unwrap()["handle"].clone()).unwrap();
    assert_eq!(
        session
            .0
            .host
            .state
            .lock()
            .unwrap()
            .resource(handle, Some(Kind::Handle))
            .unwrap()
            .wire
            .guid(),
        "acme-result"
    );
    for code in [
        "SELECTOR_WAIT_EXPIRED",
        "AMBIGUOUS_LOCATOR",
        "SELECTOR_ROOT_DETACHED",
    ] {
        let request=serde_json::from_value(json!({"operation":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":".missing"},"expression":"element=>element","waitForAttachment":true,"waitTimeoutMs":100}})).unwrap();
        let id = session.submit(Uuid::new_v4(), request, 5000).unwrap();
        let call = peer.request().await;
        assert_eq!(call["method"], "evaluateExpression");
        peer.send(&json!({"id":call["id"],"error":{"error":{"name":"Error","message":format!("Error: canonic:{code}")}},"log":[]})).await;
        let done = complete(&session, id).await;
        assert_eq!(done.code.as_deref(), Some(code));
        assert_eq!(done.result.unwrap()["dispatchCompleted"], true);
    }
    close(&session).await;
}

#[tokio::test]
async fn selector_evaluation_dispatches_first_strict_and_all_with_typed_arguments() {
    let (session, mut peer, _root, _, page) = harness().await;
    for (all, strict) in [(false, false), (false, true), (true, false)] {
        let request = serde_json::from_value(json!({"operation":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":".acme"},"expression":"(element,arg)=>arg","isFunction":true,"all":all,"strict":strict,"argument":{"kind":"number","value":"negative_zero"}}})).unwrap();
        let id = session.submit(Uuid::new_v4(), request, 5000).unwrap();
        let call = peer.request().await;
        assert_eq!(
            call["method"],
            if all {
                "evalOnSelectorAll"
            } else {
                "evalOnSelector"
            }
        );
        assert_eq!(call["params"]["selector"], "css=.acme");
        assert_eq!(
            call["params"]["arg"],
            json!({"value":{"v":"-0"},"handles":[]})
        );
        assert_eq!(
            call["params"]["strict"].as_bool(),
            if all { None } else { Some(strict) }
        );
        peer.send(&json!({"id":call["id"],"result":{"value":{"v":"-0"}}}))
            .await;
        let done = complete(&session, id).await;
        assert_eq!(done.outcome, Outcome::Succeeded, "{done:?}");
        assert_eq!(
            done.result.unwrap()["value"],
            json!({"kind":"number","value":"negative_zero"})
        );
    }
    let request = serde_json::from_value(json!({"operation":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":".acme"},"expression":"element=>element","argument":{"kind":"handle","handle":{"connectionId":page.connection_id,"id":Uuid::new_v4(),"generation":Uuid::new_v4()}}}})).unwrap();
    let id = session.submit(Uuid::new_v4(), request, 5000).unwrap();
    assert_eq!(
        complete(&session, id).await.code.as_deref(),
        Some("STALE_REFERENCE")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}
