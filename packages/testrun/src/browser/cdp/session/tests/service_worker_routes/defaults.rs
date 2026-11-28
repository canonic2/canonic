//! Context credentials must retain exact worker attachment and request ownership.
use super::*;

fn event(method: &str) -> EventItem {
    let mut params = json!({"requestId":"Acme-worker-auth","request":{"url":"https://example.com/wire/auth","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"Acme-worker-frame","resourceType":"Fetch"});
    if method == "Fetch.requestPaused" {
        params["networkId"] = json!("Acme-worker-network");
    } else {
        params["authChallenge"] = json!({"origin":"https://foreign.invalid","scheme":"Basic","realm":"Acme","source":"Server"});
    }
    EventItem::Event(Event {
        method: method.into(),
        session_id: Some("Acme-service-session".into()),
        params,
    })
}

#[tokio::test]
async fn native_service_worker_defaults_headers_setup_update_and_reset_never_borrow_page_headers() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let header = |value: &str| crate::browser::http::Header {
        name: "X-Acme-Context".into(),
        value: value.into(),
    };
    worker.context_layout.insert(
        context,
        ContextOptions {
            extra_http_headers: Some(vec![header("initial")]),
            ..Default::default()
        },
    );
    worker.page_headers.insert(
        page,
        vec![crate::browser::http::Header {
            name: "X-Acme-Page".into(),
            value: "page-only".into(),
        }],
    );
    let service = service_worker(&mut worker);
    let setup = tokio::spawn(async move {
        worker
            .setup(
                Uuid::new_v4(),
                service,
                Kind::Worker,
                "Acme-service-session",
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    for method in [
        "Runtime.enable",
        "Network.enable",
        "Network.setExtraHTTPHeaders",
        "Target.setAutoAttach",
    ] {
        let request = command(&mut peer, method).await;
        assert_eq!(request["sessionId"], "Acme-service-session");
        if method == "Network.setExtraHTTPHeaders" {
            assert_eq!(
                request["params"]["headers"],
                json!({"x-acme-context":"initial"})
            );
        }
        reply(&mut peer, &request, json!({})).await;
    }
    let mut worker = setup.await.unwrap();
    for reset in [false, true] {
        if reset {
            worker.index.retire(page).unwrap();
        }
        let task = tokio::spawn(async move {
            let request=serde_json::from_value(json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":if reset {json!([])}else{json!([{"name":"X-Acme-Context","value":"changed"}])}}})).unwrap();
            let result = worker
                .execute_one(Uuid::new_v4(), request, deadline())
                .await;
            (worker, result)
        });
        let mut sessions = BTreeSet::new();
        for _ in 0..if reset { 1 } else { 2 } {
            let enable = command(&mut peer, "Network.enable").await;
            let session = enable["sessionId"].as_str().unwrap().to_owned();
            assert!(sessions.insert(session.clone()));
            reply(&mut peer, &enable, json!({})).await;
            let setter = command(&mut peer, "Network.setExtraHTTPHeaders").await;
            assert_eq!(setter["sessionId"], session);
            let expected = if reset {
                json!({})
            } else if session == "Acme-service-session" {
                json!({"x-acme-context":"changed"})
            } else {
                json!({"x-acme-context":"changed","x-acme-page":"page-only"})
            };
            assert_eq!(setter["params"]["headers"], expected);
            reply(&mut peer, &setter, json!({})).await;
        }
        assert!(sessions.contains("Acme-service-session"));
        let (next, result) = task.await.unwrap();
        worker = next;
        assert_eq!(
            result.unwrap()["configuredAttachments"],
            if reset { 1 } else { 2 }
        );
    }
}

#[tokio::test]
async fn native_service_worker_defaults_credentials_configure_and_respond_on_original_attachment() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let credentials = serde_json::from_value(
        json!({"username":"Acme","password":"Acme-private","origin":"https://example.com"}),
    )
    .unwrap();
    worker.context_layout.insert(
        context,
        ContextOptions {
            http_credentials: Some(credentials),
            ..Default::default()
        },
    );
    let service = service_worker(&mut worker);
    let configuring = tokio::spawn(async move {
        worker
            .configure_fetch(Uuid::new_v4(), "Acme-service-session", deadline())
            .await
            .unwrap();
        worker
    });
    let network = command(&mut peer, "Network.enable").await;
    assert_eq!(network["sessionId"], "Acme-service-session");
    reply(&mut peer, &network, json!({})).await;
    let enable = command(&mut peer, "Fetch.enable").await;
    assert_eq!(enable["sessionId"], "Acme-service-session");
    assert_eq!(enable["params"]["handleAuthRequests"], true);
    reply(&mut peer, &enable, json!({})).await;
    let mut worker = configuring.await.unwrap();
    worker.index.retire(page).unwrap();
    for (method, wire, response) in [
        ("Fetch.requestPaused", "Fetch.continueRequest", None),
        (
            "Fetch.authRequired",
            "Fetch.continueWithAuth",
            Some("ProvideCredentials"),
        ),
        (
            "Fetch.authRequired",
            "Fetch.continueWithAuth",
            Some("CancelAuth"),
        ),
    ] {
        worker.apply(event(method)).unwrap();
        let route = worker.interactions.routes.values().next().unwrap();
        assert_eq!(route.owner.worker(), Some(service));
        assert_eq!(route.owner.page(), None);
        assert!(
            route.auto_pass,
            "Automatic worker credentials cannot become an application callback"
        );
        let servicing = tokio::spawn(async move {
            worker.poll_har_routes().await;
            worker
        });
        let request = command(&mut peer, wire).await;
        assert_eq!(request["sessionId"], "Acme-service-session");
        if let Some(response) = response {
            assert_eq!(
                request["params"]["authChallengeResponse"]["response"],
                response
            );
        }
        if response == Some("ProvideCredentials") {
            assert_eq!(
                request["params"]["authChallengeResponse"]["username"],
                "Acme"
            );
            assert_eq!(
                request["params"]["authChallengeResponse"]["password"],
                "Acme-private"
            );
        }
        reply(&mut peer, &request, json!({})).await;
        worker = servicing.await.unwrap();
        assert!(worker.interactions.routes.is_empty());
    }
}

#[tokio::test]
async fn native_service_worker_defaults_manual_auth_keeps_precedence_and_worker_authority() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    worker.context_layout.insert(
        context,
        ContextOptions {
            http_credentials: Some(
                serde_json::from_value(json!({"username":"Acme","password":"Acme-private"}))
                    .unwrap(),
            ),
            ..Default::default()
        },
    );
    let setup = tokio::spawn(async move {
        worker
            .scoped_route_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                Some("https://example.com/wire/auth".into()),
                true,
                deadline(),
            )
            .await
            .unwrap();
        let service = service_worker(&mut worker);
        worker
            .configure_fetch(Uuid::new_v4(), "Acme-service-session", deadline())
            .await
            .unwrap();
        (worker, service)
    });
    for session in ["session-acme", "Acme-service-session"] {
        for method in ["Network.enable", "Fetch.enable"] {
            let request = command(&mut peer, method).await;
            assert_eq!(request["sessionId"], session);
            if method == "Fetch.enable" {
                assert_eq!(request["params"]["handleAuthRequests"], true);
            }
            reply(&mut peer, &request, json!({})).await;
        }
    }
    let (mut worker, service) = setup.await.unwrap();
    worker.index.retire(page).unwrap();
    worker.apply(event("Fetch.authRequired")).unwrap();
    let callback = *worker.interactions.routes.keys().next().unwrap();
    let route = &worker.interactions.routes[&callback];
    assert_eq!(route.owner.worker(), Some(service));
    assert_eq!(route.policy_scope, context);
    assert!(
        !route.auto_pass,
        "An explicit manual authentication policy keeps ownership over automatic defaults"
    );
    let snapshot = worker.interactions.snapshot();
    assert!(!snapshot.to_string().contains("Acme-private"));
    let servicing = tokio::spawn(async move {
        worker
            .resolve_route(callback, CallbackResponse::AuthCancel {}, deadline())
            .await
            .unwrap();
        worker
    });
    let reply_request = command(&mut peer, "Fetch.continueWithAuth").await;
    assert_eq!(reply_request["sessionId"], "Acme-service-session");
    assert_eq!(
        reply_request["params"]["authChallengeResponse"],
        json!({"response":"CancelAuth"})
    );
    reply(&mut peer, &reply_request, json!({})).await;
    assert!(servicing.await.unwrap().interactions.routes.is_empty());
}
