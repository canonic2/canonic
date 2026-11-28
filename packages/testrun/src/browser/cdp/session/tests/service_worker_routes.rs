//! Context-owned service requests cannot acquire a creating page's authority.
use super::investigation::owned_page;
use super::*;
use crate::browser::{contract::CallbackResponse, routing::HandlerAction};
mod capture;
mod defaults;
mod har;

fn service_worker(worker: &mut Worker) -> Reference {
    let info = json!({"targetId":"Acme-service","browserContextId":"context-acme","type":"service_worker","title":"Acme","url":"https://example.com/service.js","attached":false,"canAccessOpener":false});
    worker
        .apply(EventItem::Event(Event {
            method: "Target.targetCreated".into(),
            session_id: None,
            params: json!({"targetInfo":info}),
        }))
        .unwrap();
    worker.apply(EventItem::Event(Event{method:"Target.attachedToTarget".into(),session_id:None,params:json!({"sessionId":"Acme-service-session","targetInfo":info,"waitingForDebugger":false})})).unwrap();
    worker.index.target_reference("Acme-service").unwrap()
}

#[tokio::test]
async fn native_service_worker_routes_keep_context_authority_after_creating_page_closure() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let opening = tokio::spawn(async move {
        let result = worker
            .network_route(
                Uuid::new_v4(),
                context,
                HandlerAction::Add {
                    url: "https://example.com/Acme-request".into(),
                    times: None,
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    assert_eq!(enable["sessionId"], "session-acme");
    reply(&mut peer, &enable, json!({})).await;
    let (mut worker, added) = opening.await.unwrap();
    let handler = added.unwrap()["handlerId"].clone();
    // An equivalent page policy must never select a service-worker request.
    worker
        .network_route(
            Uuid::new_v4(),
            page,
            HandlerAction::Add {
                url: "https://example.com/Acme-request".into(),
                times: None,
            },
            deadline(),
        )
        .await
        .unwrap();
    let service = service_worker(&mut worker);
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("Acme-service-session".into()),params:json!({"requestId":"Acme-service-request","request":{"url":"https://example.com/Acme-request","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"Acme-worker-frame","resourceType":"Fetch"})})).unwrap();
    let callbacks = worker.interactions.snapshot();
    let routes = callbacks["callbacks"].as_array().unwrap();
    assert_eq!(
        routes.len(),
        1,
        "A live context-owned worker request must acquire its own callback: {callbacks}"
    );
    let callback: Reference = serde_json::from_value(routes[0]["reference"].clone()).unwrap();
    assert_eq!(routes[0]["page"], Value::Null);
    assert_eq!(routes[0]["worker"], json!(service));
    assert_eq!(routes[0]["policyScope"], json!(context));
    assert_eq!(routes[0]["handlerId"], handler);
    let edit = worker
        .configure_context_credentials(Uuid::new_v4(), context, None, deadline())
        .await
        .unwrap_err();
    assert_eq!(
        edit.code, "ROUTE_CALLBACKS_PENDING",
        "Worker callbacks must fence context defaults too"
    );
    worker.index.retire(page).unwrap();
    worker.expire_routes().await;
    assert_eq!(
        worker.interactions.routes.len(),
        1,
        "Closing the creating page cannot expire worker authority"
    );
    let continuing = tokio::spawn(async move {
        let result = worker
            .resolve_route(callback, CallbackResponse::RouteContinue {}, deadline())
            .await;
        (worker, result)
    });
    let request = command(&mut peer, "Fetch.continueRequest").await;
    assert_eq!(request["sessionId"], "Acme-service-session");
    assert_eq!(
        request["params"],
        json!({"requestId":"Acme-service-request"})
    );
    reply(&mut peer, &request, json!({})).await;
    let (worker, result) = continuing.await.unwrap();
    assert_eq!(result.unwrap()["resolved"], true);
    assert!(worker.interactions.routes.is_empty());
}

#[tokio::test]
async fn native_worker_routes_leave_dedicated_workers_to_page_interception_and_shared_workers_bare()
{
    let (mut worker, _peer, _root, context, _page) = owned_page().await;
    for kind in ["worker", "shared_worker"] {
        let info = json!({"targetId":format!("Acme-{kind}"),"browserContextId":"context-acme","type":kind,"title":"Acme","url":"https://example.com/worker.js","attached":false,"canAccessOpener":false});
        let session = format!("Acme-{kind}-session");
        worker
            .apply(EventItem::Event(Event {
                method: "Target.attachedToTarget".into(),
                session_id: Some("session-acme".into()),
                params: json!({"sessionId":session,"targetInfo":info,"waitingForDebugger":false}),
            }))
            .unwrap();
        let reference = worker
            .index
            .target_reference(&format!("Acme-{kind}"))
            .unwrap();
        assert_eq!(worker.index.worker_scope(reference).unwrap().0, context);
        // Dedicated-worker requests pause in the page's own interception, which
        // enabling Fetch on the worker session would break.
        assert!(
            worker.route_scope(&session).is_none(),
            "{kind} sessions cannot own routes"
        );
        assert!(worker.headers_for_session(&session).is_none());
    }
    worker
        .context_layout
        .entry(context)
        .or_default()
        .extra_http_headers = Some(vec![crate::browser::http::Header {
        name: "X-Acme".into(),
        value: "context".into(),
    }]);
    assert_eq!(
        worker.headers_for_session("Acme-worker-session"),
        Some(std::collections::BTreeMap::from([(
            "x-acme".to_string(),
            "context".to_string()
        )]))
    );
    assert!(
        worker
            .headers_for_session("Acme-shared_worker-session")
            .is_none(),
        "Pinned Chromium never attaches shared workers"
    );
    let service = service_worker(&mut worker);
    let (owner, route_owner) = worker.route_scope("Acme-service-session").unwrap();
    assert_eq!(owner, context);
    assert_eq!(route_owner.reference(), service);
    assert!(route_owner.page().is_none());
}

#[tokio::test]
async fn native_service_worker_routes_configure_future_workers_and_identify_failed_setup_owner() {
    for fail in [false, true] {
        let (mut worker, mut peer, _root, context, _page) = owned_page().await;
        let setup = tokio::spawn(async move {
            let result = worker
                .network_route(
                    Uuid::new_v4(),
                    context,
                    HandlerAction::Add {
                        url: "https://example.com/Acme-request".into(),
                        times: None,
                    },
                    deadline(),
                )
                .await;
            (worker, result)
        });
        let page = command(&mut peer, "Fetch.enable").await;
        reply(&mut peer, &page, json!({})).await;
        let (mut worker, result) = setup.await.unwrap();
        result.unwrap();
        let service = service_worker(&mut worker);
        let configuring = tokio::spawn(async move {
            let result = worker
                .configure_fetch(Uuid::new_v4(), "Acme-service-session", deadline())
                .await;
            (worker, result)
        });
        let enable = command(&mut peer, "Fetch.enable").await;
        assert_eq!(enable["sessionId"], "Acme-service-session");
        assert_eq!(
            enable["params"],
            json!({"patterns":[{"urlPattern":"https://example.com/Acme-request","requestStage":"Request"}],"handleAuthRequests":false})
        );
        if fail {
            send(
                &mut peer,
                json!({"id":enable["id"],"error":{"code":-32000,"message":"Acme setup refusal"}}),
            )
            .await;
        } else {
            reply(&mut peer, &enable, json!({})).await;
        }
        let (mut worker, result) = configuring.await.unwrap();
        if fail {
            let error = result.unwrap_err();
            let partial = error.partial.unwrap();
            assert!(partial["page"].is_null());
            assert_eq!(partial["worker"], json!(service));
            assert_eq!(partial["context"], json!(context));
            assert_eq!(partial["nextAction"], "context_close");
        } else {
            result.unwrap();
            worker
                .configure_fetch(Uuid::new_v4(), "Acme-service-session", deadline())
                .await
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(25), peer.next())
                    .await
                    .is_err(),
                "Acknowledged setup must not replay Fetch.enable"
            );
        }
    }
}

#[tokio::test]
async fn native_service_worker_routes_update_existing_workers_without_page_policy_leakage() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    service_worker(&mut worker);
    let setup = tokio::spawn(async move {
        let result = worker
            .network_route(
                Uuid::new_v4(),
                context,
                HandlerAction::Add {
                    url: "https://example.com/Acme-request".into(),
                    times: None,
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    for session in ["Acme-service-session", "session-acme"] {
        let enable = command(&mut peer, "Fetch.enable").await;
        assert_eq!(enable["sessionId"], session);
        reply(&mut peer, &enable, json!({})).await;
    }
    let (mut worker, result) = setup.await.unwrap();
    result.unwrap();
    let adding = tokio::spawn(async move {
        let result = worker
            .network_route(
                Uuid::new_v4(),
                page,
                HandlerAction::Add {
                    url: "https://example.com/Acme-page-only".into(),
                    times: None,
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    assert_eq!(enable["sessionId"], "session-acme");
    assert_eq!(enable["params"]["patterns"].as_array().unwrap().len(), 2);
    reply(&mut peer, &enable, json!({})).await;
    let (_worker, result) = adding.await.unwrap();
    result.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "A page policy must not reconfigure context-owned service workers"
    );
}

#[tokio::test]
async fn native_service_worker_routes_fallback_keeps_owner_and_old_attachments_cannot_reply() {
    for close_context in [false, true] {
        let (mut worker, mut peer, _root, context, _page) = owned_page().await;
        let setup = tokio::spawn(async move {
            let result = worker
                .network_route(
                    Uuid::new_v4(),
                    context,
                    HandlerAction::Add {
                        url: "https://example.com/Acme-request".into(),
                        times: None,
                    },
                    deadline(),
                )
                .await;
            (worker, result)
        });
        let enable = command(&mut peer, "Fetch.enable").await;
        reply(&mut peer, &enable, json!({})).await;
        let (mut worker, first) = setup.await.unwrap();
        let first = first.unwrap()["handlerId"].clone();
        let second = worker
            .network_route(
                Uuid::new_v4(),
                context,
                HandlerAction::Add {
                    url: "https://example.com/Acme-request".into(),
                    times: None,
                },
                deadline(),
            )
            .await
            .unwrap()["handlerId"]
            .clone();
        let service = service_worker(&mut worker);
        worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("Acme-service-session".into()),params:json!({"requestId":"Acme-fallback-request","request":{"url":"https://example.com/Acme-request","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"Acme-worker-frame","resourceType":"Fetch"})})).unwrap();
        let original = *worker.interactions.routes.keys().next().unwrap();
        assert_eq!(
            worker.interactions.snapshot()["callbacks"][0]["handlerId"],
            second
        );
        let forwarded = worker
            .resolve_route(original, CallbackResponse::RouteFallback {}, deadline())
            .await
            .unwrap();
        assert_eq!(forwarded["fallback"], true);
        let current = *worker.interactions.routes.keys().next().unwrap();
        assert_ne!(original, current, "Fallback creates a new arrival owner");
        let snapshot = worker.interactions.snapshot();
        assert_eq!(snapshot["callbacks"][0]["worker"], json!(service));
        assert!(snapshot["callbacks"][0]["page"].is_null());
        assert_eq!(snapshot["callbacks"][0]["handlerId"], first);
        if close_context {
            let closing = tokio::spawn(async move {
                let result = worker
                    .execute(
                        Uuid::new_v4(),
                        Request::ContextClose { context },
                        deadline(),
                    )
                    .await;
                (worker, result)
            });
            let dispose = command(&mut peer, "Target.disposeBrowserContext").await;
            reply(&mut peer, &dispose, json!({})).await;
            let (closed, result) = closing.await.unwrap();
            worker = closed;
            assert_eq!(result.unwrap()["closed"], true);
            assert!(
                worker.interactions.routes.is_empty(),
                "Acknowledged context close must retire worker callbacks before publishing its receipt"
            );
        } else {
            worker
                .apply(EventItem::Event(Event {
                    method: "Target.detachedFromTarget".into(),
                    session_id: None,
                    params: json!({"sessionId":"Acme-service-session","targetId":"Acme-service"}),
                }))
                .unwrap();
        }
        assert_eq!(
            worker
                .resolve_route(current, CallbackResponse::RouteContinue {}, deadline())
                .await
                .unwrap_err()
                .code,
            "STALE_REFERENCE"
        );
        worker.expire_routes().await;
        assert!(worker.interactions.routes.is_empty());
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err(),
            "Expired worker callback authority cannot dispatch to a retained or replacement target"
        );
    }
}

#[test]
fn native_service_worker_routes_discovery_preserves_engine_and_context_scope_limits() {
    for host in [false, true] {
        let chrome = contract::capabilities(Engine::Chrome, host);
        assert_eq!(chrome["routing"]["serviceWorkers"], host);
        assert_eq!(
            chrome["routing"]["serviceWorkerScope"],
            "context-manual-and-har-routes"
        );
        assert_eq!(chrome["routing"]["serviceWorkerHar"], host);
        assert_eq!(chrome["routing"]["serviceWorkerAuthentication"], host);
        assert_eq!(chrome["settings"]["authentication"]["serviceWorkers"], host);
        for engine in [Engine::Firefox, Engine::Webkit] {
            assert_eq!(
                contract::capabilities(engine, host)["routing"]["serviceWorkers"],
                false
            );
        }
    }
}
