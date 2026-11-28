//! Response facts stay scoped and completion timing is independent of body reads.
use super::*;

#[tokio::test]
async fn network_response_details_preserve_live_and_finished_timing_without_body_dispatch() {
    for engine in [Engine::Firefox, Engine::Webkit] {
        exercise(engine).await;
    }
}
async fn exercise(engine: Engine) {
    let (session, mut peer, _root, context, _page) = harness().await;
    session.0.host.state.lock().unwrap().engine = engine;
    peer.send(&created("net-request","Request",json!({"frame":{"guid":"frame"},"url":"https://example.com/private?token=Acme","method":"GET","resourceType":"fetch","headers":[],"isNavigationRequest":false}))).await;
    peer.send(&created("net-response","Response",json!({"request":{"guid":"net-request"},"url":"https://example.com/private?token=Acme","status":200,"statusText":"OK","headers":[],"timing":{"startTime":1000.0,"domainLookupStart":-1,"domainLookupEnd":-1,"connectStart":1.0,"secureConnectionStart":-1,"connectEnd":2.0,"requestStart":2.0,"responseStart":3.0},"fromServiceWorker":false}))).await;
    let connection = session.0.host.connection().unwrap();
    let until = deadline();
    let wire = loop {
        if connection.resolve("net-response", "Response").await.is_ok()
            && let Ok(wire) = connection.resolve("net-request", "Request").await
        {
            break wire;
        }
        assert!(Instant::now() < until);
        tokio::task::yield_now().await;
    };
    let request_id = {
        let mut history = session.0.host.diagnostics.lock().unwrap();
        history.enable(context, true);
        let (id, _) = history.request(wire.clone(), true);
        history.scope_request(&wire, context, true, false);
        id
    };
    for complete_request in [false, true] {
        if complete_request {
            peer.send(&json!({"guid":"context","method":"requestFinished","params":{"request":{"guid":"net-request"},"response":{"guid":"net-response"},"responseEndTiming":9.0,"page":{"guid":"page"}}})).await;
            while session
                .0
                .host
                .diagnostics
                .lock()
                .unwrap()
                .request_end(&wire)
                != Some(9.0)
            {
                assert!(Instant::now() < until);
                tokio::task::yield_now().await;
            }
        }
        let reading = session
            .submit(
                Uuid::new_v4(),
                Request::NetworkRead {
                    context,
                    request_id,
                    options: crate::browser::network::Read {
                        response_details: true,
                        ..Default::default()
                    },
                },
                5000,
            )
            .unwrap();
        let response = peer.request().await;
        assert_eq!(response["method"], "response");
        peer.send(&json!({"id":response["id"],"result":{"response":{"guid":"net-response"}}}))
            .await;
        let pending = engine == Engine::Webkit && !complete_request;
        // Connection details, then httpVersion and sizes once finished: the
        // pinned driver resolves both only at completion.
        let calls = if pending { 0 } else { 2 } + 2 * usize::from(complete_request);
        for _ in 0..calls {
            let call = peer.request().await;
            let result = match call["method"].as_str().unwrap() {
                "serverAddr" => json!({"value":{"ipAddress":"192.0.2.1","port":443}}),
                "securityDetails" => {
                    json!({"value":{"protocol":"TLS 1.3","subjectName":"example.com","issuer":"Acme CA","validFrom":1,"validTo":2}})
                }
                "httpVersion" => json!({"value":"HTTP/1.1"}),
                "sizes" => {
                    json!({"sizes":{"requestBodySize":0,"requestHeadersSize":40,"responseBodySize":4,"responseHeadersSize":30}})
                }
                method => panic!("Unexpected body/header call: {method}"),
            };
            peer.send(&json!({"id":call["id"],"result":result})).await;
        }
        let operation = complete(&session, reading).await;
        assert_eq!(operation.outcome, Outcome::Succeeded);
        let result = operation.result.unwrap();
        assert_response_details(&result, complete_request, pending);
    }
    session
        .0
        .host
        .diagnostics
        .lock()
        .unwrap()
        .forget_object(&wire);
    assert!(
        session
            .0
            .host
            .diagnostics
            .lock()
            .unwrap()
            .request_end(&wire)
            .is_none()
    );
    close(&session).await;
}

fn assert_response_details(result: &Value, complete_request: bool, pending: bool) {
    assert_eq!(result["complete"], complete_request);
    assert_eq!(
        result["responseDetails"]["timing"]["responseEnd"],
        if complete_request { 9.0 } else { -1.0 }
    );
    assert_eq!(
        result["responseDetails"]["connectionDetailsPending"],
        pending
    );
    assert_eq!(
        result["responseDetails"]["httpVersion"],
        if complete_request {
            json!("HTTP/1.1")
        } else {
            Value::Null
        }
    );
    assert_eq!(result["responseDetails"]["statusText"], "OK");
    assert_eq!(result["responseDetails"]["sizesPending"], !complete_request);
    assert_eq!(
        result["responseDetails"]["sizes"]["responseBodySize"],
        if complete_request {
            json!(4)
        } else {
            Value::Null
        }
    );
    if pending {
        assert!(result["responseDetails"]["serverAddr"].is_null());
        assert!(result["responseDetails"]["securityDetails"].is_null());
    } else {
        assert_eq!(result["responseDetails"]["serverAddr"]["port"], 443);
        assert_eq!(
            result["responseDetails"]["securityDetails"]["issuer"],
            "Acme CA"
        );
    }
    assert!(!result.to_string().contains("token=Acme"));
}
