//! Finalization faults retain their original native collector and evidence.
use super::*;
use crate::browser::contract::{HarCaptureContent, HarCaptureMode};

#[tokio::test]
async fn native_file_preparation_keeps_the_callback_lane_responsive_and_cancels_before_http_dispatch()
 {
    let permits = crate::browser::http::hold_readers().await;
    let (mut worker, _peer, root, _context, _page) = owned_page().await;
    let selected = root.path().join("Acme-upload.json");
    std::fs::write(&selected, b"Acme private file").unwrap();
    let request=serde_json::from_value(json!({"url":"https://example.com/upload","method":"POST","body":{"kind":"multipart","fields":[{"kind":"file_path","name":"upload","path":selected}]}})).unwrap();
    let shared = worker.shared.clone();
    let (tx, rx) = mpsc::channel(8);
    worker.responses = Some(rx);
    let id = Uuid::new_v4();
    shared
        .state
        .lock()
        .unwrap()
        .begin(id, "fixture".into())
        .unwrap();
    let preparing = tokio::spawn(async move {
        let result = worker.prepare_http(id, request, deadline()).await;
        (worker, result)
    });
    let read = response_operation(&shared, &tx, Request::HttpContextList {}).await;
    let response = wait_response(&shared, read).await;
    assert_eq!(response.outcome, Outcome::Succeeded, "{response:?}");
    shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&id)
        .unwrap()
        .cancel_requested = true;
    let (_worker, result) = preparing.await.unwrap();
    assert_eq!(result.unwrap_err().code, "CANCELLED");
    drop(permits);
}

#[tokio::test]
async fn native_css_query_failure_is_fenced_when_its_document_changes_during_cleanup() {
    let (mut worker, mut peer, _root, _context, page) = owned_page().await;
    let opening = tokio::spawn(async move {
        let value = worker
            .developer_open(Uuid::new_v4(), page, Some("elements".into()), deadline())
            .await
            .unwrap();
        (
            worker,
            serde_json::from_value::<Reference>(value["developer"].clone()).unwrap(),
        )
    });
    let c = command(&mut peer, "Target.getDevToolsTarget").await;
    reply(&mut peer, &c, json!({})).await;
    let c = command(&mut peer, "Target.openDevTools").await;
    reply(&mut peer, &c, json!({"targetId":"Acme-css-frontend"})).await;
    let c = command(&mut peer, "Target.attachToTarget").await;
    reply(
        &mut peer,
        &c,
        json!({"sessionId":"Acme-css-frontend-session"}),
    )
    .await;
    let c = command(&mut peer, "Runtime.enable").await;
    reply(&mut peer, &c, json!({})).await;
    let c = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &c,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let c = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &c,
        json!({"result":{"type":"object","value":{"nativeFrontend":true,"modules":{}}}}),
    )
    .await;
    let (mut worker, developer) = opening.await.unwrap();
    let query = tokio::spawn(async move {
        let result = worker
            .page_css(
                Uuid::new_v4(),
                Request::PageCss {
                    developer,
                    page,
                    frame: None,
                    locator: crate::browser::page::Locator::Css {
                        selector: "#Acme-probe".into(),
                    },
                    offset: 0,
                    limit: 10,
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let world = command(&mut peer, "Page.createIsolatedWorld").await;
    send(&mut peer,json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":2,"uniqueId":"Acme-css-world","origin":"https://example.com","name":super::super::page::WORLD,"auxData":{"frameId":"frame-acme","isDefault":false,"type":"isolated"}}}})).await;
    reply(&mut peer, &world, json!({"executionContextId":2})).await;
    let initialized = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &initialized,
        json!({"result":{"type":"undefined"}}),
    )
    .await;
    let node = command(&mut peer, "Runtime.evaluate").await;
    reply(&mut peer,&node,json!({"result":{"type":"object","subtype":"error"},"exceptionDetails":{"exceptionId":1,"lineNumber":0,"columnNumber":0,"executionContextId":2,"text":"Uncaught","exception":{"type":"object","subtype":"error","description":"Error: canonic:ELEMENT_NOT_FOUND"}}})).await;
    let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
    send(&mut peer,json!({"method":"Runtime.executionContextDestroyed","sessionId":"session-acme","params":{"executionContextId":2,"executionContextUniqueId":"Acme-css-world"}})).await;
    send(&mut peer,json!({"method":"Page.frameNavigated","sessionId":"session-acme","params":{"frame":{"id":"frame-acme","loaderId":"Acme-css-replacement","url":"https://example.com/replacement","domainAndRegistry":"example.com","securityOrigin":"https://example.com","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await;
    reply(&mut peer, &release, json!({})).await;
    let (worker, result) = query.await.unwrap();
    assert_eq!(result.unwrap_err().code, "STALE_REFERENCE");
    assert!(worker.index.route(page, Some(Kind::Page)).is_ok());
    if let Ok(Some(Ok(message))) =
        tokio::time::timeout(Duration::from_millis(25), peer.next()).await
    {
        assert!(
            !message.is_text(),
            "Unexpected command after document loss: {message:?}"
        );
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One filter lifecycle verifies URL normalization, independent authentication authority and late challenge fallback"
)]
async fn native_glob_filters_preserve_scope_auth_authority_and_default_after_release() {
    use crate::browser::routing::{HandlerAction, Matcher};
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    for glob in [false, true] {
        let setup = tokio::spawn(async move {
            if glob {
                worker
                    .network_route(
                        Uuid::new_v4(),
                        page,
                        HandlerAction::AddMatcher {
                            times: None,
                            matcher: Matcher::Glob {
                                pattern: "https://example.com/acme/*".into(),
                            },
                        },
                        deadline(),
                    )
                    .await
                    .unwrap();
            } else {
                worker
                    .scoped_route_set(
                        Uuid::new_v4(),
                        context,
                        Kind::Context,
                        Some("https://example.com/login".into()),
                        true,
                        deadline(),
                    )
                    .await
                    .unwrap();
            }
            worker
        });
        let enable = command(&mut peer, "Fetch.enable").await;
        assert_eq!(enable["params"]["handleAuthRequests"], true);
        reply(&mut peer, &enable, json!({})).await;
        worker = setup.await.unwrap();
    }
    let challenge = |id: &str, url: &str| {
        EventItem::Event(Event {
            method: "Fetch.authRequired".into(),
            session_id: Some("session-acme".into()),
            params: json!({"requestId":id,"request":{"url":url,"method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch","authChallenge":{"origin":"https://example.com","scheme":"Basic","realm":"Acme","source":"Server"}}),
        })
    };
    worker
        .apply(challenge("Acme-owned-auth", "https://example.com/acme/one"))
        .unwrap();
    let callbacks = worker.interactions.snapshot();
    assert_eq!(callbacks["callbacks"].as_array().unwrap().len(), 1);
    assert_eq!(callbacks["callbacks"][0]["policyScope"], json!(context));
    let callback = serde_json::from_value(callbacks["callbacks"][0]["reference"].clone()).unwrap();
    let cancel = tokio::spawn(async move {
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::AuthCancel {},
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let auth_cancel = command(&mut peer, "Fetch.continueWithAuth").await;
    assert_eq!(
        auth_cancel["params"]["authChallengeResponse"],
        json!({"response":"CancelAuth"})
    );
    reply(&mut peer, &auth_cancel, json!({})).await;
    let mut worker = cancel.await.unwrap();
    assert!(worker.interactions.routes.is_empty());
    let releasing = tokio::spawn(async move {
        worker
            .scoped_route_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                None,
                false,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    assert_eq!(enable["params"]["handleAuthRequests"], false);
    reply(&mut peer, &enable, json!({})).await;
    worker = releasing.await.unwrap();
    assert_eq!(worker.routing_rules.handlers.len(page), 1);
    worker
        .apply(challenge(
            "Acme-unmatched-auth",
            "https://example.com/acme/one",
        ))
        .unwrap();
    assert_eq!(worker.interactions.snapshot()["callbacks"], json!([]));
    assert_eq!(
        worker.interactions.snapshot()["routeContinuationsPending"],
        1
    );
    let passing = tokio::spawn(async move {
        worker.poll_har_routes().await;
        worker
    });
    let continuation = command(&mut peer, "Fetch.continueWithAuth").await;
    assert_eq!(
        continuation["params"],
        json!({"requestId":"Acme-unmatched-auth","authChallengeResponse":{"response":"Default"}})
    );
    reply(&mut peer, &continuation, json!({})).await;
    assert!(passing.await.unwrap().interactions.routes.is_empty());
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "The same Page and Context chain must retain ownership across nonmatches, fallback and final acknowledgement"
)]
async fn native_glob_candidates_keep_logical_precedence_and_continue_nonmatches_privately() {
    use crate::browser::routing::{HandlerAction, Matcher};
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let mut context_handler = None;
    for (scope, pattern) in [
        (context, "https://example.com/{acme,shared}/*"),
        (page, "https://example.com/acme/*"),
    ] {
        let setup = tokio::spawn(async move {
            let result = worker
                .network_route(
                    Uuid::new_v4(),
                    scope,
                    HandlerAction::AddMatcher {
                        times: None,
                        matcher: Matcher::Glob {
                            pattern: pattern.into(),
                        },
                    },
                    deadline(),
                )
                .await;
            (worker, result)
        });
        if scope == context {
            let enable = command(&mut peer, "Fetch.enable").await;
            assert!(
                enable["params"]["patterns"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|pattern| pattern["urlPattern"] == "https://*")
            );
            reply(&mut peer, &enable, json!({})).await;
        }
        let (next, result) = setup.await.unwrap();
        worker = next;
        if scope == context {
            context_handler = Some(result.unwrap()["handlerId"].clone());
        } else {
            result.unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(25), peer.next())
                    .await
                    .is_err(),
                "A logically distinct glob reuses the acknowledged transport filter"
            );
        }
    }
    let paused = |id: &str, url: &str| {
        EventItem::Event(Event {
            method: "Fetch.requestPaused".into(),
            session_id: Some("session-acme".into()),
            params: json!({"requestId":id,"request":{"url":url,"method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"}),
        })
    };
    worker
        .apply(paused("Acme-context", "https://example.com/shared/one"))
        .unwrap();
    let callback = *worker.interactions.routes.keys().next().unwrap();
    assert_eq!(
        worker.interactions.snapshot()["callbacks"][0]["policyScope"],
        json!(context)
    );
    assert_eq!(
        worker.interactions.snapshot()["callbacks"][0]["handlerId"],
        context_handler.unwrap()
    );
    let abort = tokio::spawn(async move {
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::RouteAbort { error_code: None },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let fail = command(&mut peer, "Fetch.failRequest").await;
    reply(&mut peer, &fail, json!({})).await;
    worker = abort.await.unwrap();
    worker
        .apply(paused("Acme-nonmatch", "https://example.com/acme/one/two"))
        .unwrap();
    assert_eq!(worker.interactions.snapshot()["callbacks"], json!([]));
    assert_eq!(
        worker.interactions.snapshot()["routeContinuationsPending"],
        1
    );
    assert_eq!(worker.interactions.routes.len(), 1);
    let passing = tokio::spawn(async move {
        worker.poll_har_routes().await;
        worker
    });
    let continuing = command(&mut peer, "Fetch.continueRequest").await;
    assert_eq!(continuing["params"]["requestId"], "Acme-nonmatch");
    reply(&mut peer, &continuing, json!({})).await;
    let mut worker = passing.await.unwrap();
    assert!(worker.interactions.routes.is_empty());
    assert!(worker.index.route(page, Some(Kind::Page)).is_ok());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    worker
        .apply(paused("Acme-unknown", "https://example.com/no-match"))
        .unwrap();
    let pending = tokio::spawn(async move {
        worker.poll_har_routes().await;
        worker
    });
    let original = command(&mut peer, "Fetch.continueRequest").await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    tokio::time::resume();
    let mut worker = pending.await.unwrap();
    assert_eq!(
        worker.interactions.snapshot()["routeContinuationsPending"],
        1
    );
    assert_eq!(worker.interactions.snapshot()["callbacks"], json!([]));
    assert!(
        worker
            .interactions
            .routes
            .values()
            .all(|route| route.responding)
    );
    worker.poll_har_routes().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "An unknown continuation must not be repeated"
    );
    let target = worker.index.native(page, Kind::Page).unwrap().to_owned();
    worker
        .apply(EventItem::Event(Event {
            method: "Target.targetDestroyed".into(),
            session_id: None,
            params: json!({"targetId":target}),
        }))
        .unwrap();
    worker.expire_routes().await;
    assert_eq!(
        worker.interactions.snapshot()["routeContinuationsPending"],
        0
    );
    reply(&mut peer, &original, json!({})).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    worker.drain().unwrap();
    assert!(worker.interactions.routes.is_empty());
}

async fn clear_routes_and_release_late_pause(mut worker: Worker, mut peer: Peer, page: Reference) {
    use crate::browser::routing::HandlerAction;
    let clearing = tokio::spawn(async move {
        worker
            .network_route(
                Uuid::new_v4(),
                page,
                HandlerAction::Clear { behavior: None },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let disable = command(&mut peer, "Fetch.disable").await;
    reply(&mut peer, &disable, json!({})).await;
    let mut worker = clearing.await.unwrap();
    assert_eq!(worker.routing_rules.handlers.len(page), 0);
    // A request paused as interception turned off has no handler left: it is
    // continued privately, never offered as a callback that blocks later edits.
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"late-acme","request":{"url":"https://example.com/favicon.ico","method":"GET","headers":{},"initialPriority":"Low","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Other"})})).unwrap();
    assert!(
        worker.interactions.snapshot()["callbacks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        worker.interactions.snapshot()["routeContinuationsPending"],
        1
    );
    let continuing = tokio::spawn(async move {
        worker.poll_har_routes().await;
        worker
    });
    let resumed = command(&mut peer, "Fetch.continueRequest").await;
    assert_eq!(resumed["params"]["requestId"], "late-acme");
    // Chrome has already released it; the failure drops the route, not retries it.
    send(&mut peer, json!({"id":resumed["id"],"sessionId":"session-acme","error":{"code":-32602,"message":"Invalid InterceptionId."}})).await;
    let worker = continuing.await.unwrap();
    assert!(worker.interactions.routes.is_empty());
}

fn typed_settlement_cases() -> [(u64, contract::CallbackResponse, Value, Value); 2] {
    [
        (
            1,
            contract::CallbackResponse::BindingResolveValue {
                value: serde_json::from_value(json!({"kind":"bigint","value":"9007199254740993"}))
                    .unwrap(),
            },
            json!({"bi":"9007199254740993"}),
            json!(false),
        ),
        (
            2,
            contract::CallbackResponse::BindingRejectValue {
                value: serde_json::from_value(json!({"kind":"undefined"})).unwrap(),
            },
            json!({"v":"undefined"}),
            json!("value"),
        ),
    ]
}

#[tokio::test]
async fn native_distinct_route_urls_install_a_union_and_remove_only_the_selected_pattern() {
    use crate::browser::routing::HandlerAction;
    let (mut worker, mut peer, _root, _context, page) = owned_page().await;
    let mut ids = Vec::new();
    for (index, url) in ["https://example.com/first", "https://example.com/second"]
        .into_iter()
        .enumerate()
    {
        let setup = tokio::spawn(async move {
            let result = worker
                .network_route(
                    Uuid::new_v4(),
                    page,
                    HandlerAction::Add {
                        url: url.into(),
                        times: None,
                    },
                    deadline(),
                )
                .await;
            (worker, result)
        });
        let enable = command(&mut peer, "Fetch.enable").await;
        assert_eq!(
            enable["params"]["patterns"].as_array().unwrap().len(),
            index + 1
        );
        reply(&mut peer, &enable, json!({})).await;
        let (next, result) = setup.await.unwrap();
        worker = next;
        ids.push(serde_json::from_value::<Uuid>(result.unwrap()["handlerId"].clone()).unwrap());
    }
    for (index, url) in ["https://example.com/first", "https://example.com/second"]
        .into_iter()
        .enumerate()
    {
        worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":format!("Acme-{index}"),"request":{"url":url,"method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
        let callback = *worker.interactions.routes.keys().next().unwrap();
        assert_eq!(
            worker.routing_rules.handlers.current(callback),
            Some(ids[index])
        );
        let abort = tokio::spawn(async move {
            worker
                .resolve_route(
                    callback,
                    contract::CallbackResponse::RouteAbort { error_code: None },
                    deadline(),
                )
                .await
                .unwrap();
            worker
        });
        let command = command(&mut peer, "Fetch.failRequest").await;
        reply(&mut peer, &command, json!({})).await;
        worker = abort.await.unwrap();
    }
    let first = ids[0];
    let removing = tokio::spawn(async move {
        worker
            .network_route(
                Uuid::new_v4(),
                page,
                HandlerAction::RemoveMatcher {
                    matcher: crate::browser::routing::Matcher::Exact {
                        url: "https://example.com/first".into(),
                    },
                },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    assert_eq!(
        enable["params"]["patterns"],
        json!([{"urlPattern":"https://example.com/second","requestStage":"Request"}])
    );
    reply(&mut peer, &enable, json!({})).await;
    worker = removing.await.unwrap();
    assert!(!worker.routing_rules.handlers.contains(page, first));
    assert!(worker.routing_rules.handlers.contains(page, ids[1]));
    clear_routes_and_release_late_pause(worker, peer, page).await;
}

#[tokio::test]
async fn native_route_handler_rejected_setup_retains_identity_until_explicit_clear() {
    use crate::browser::routing::HandlerAction;
    let (mut worker, mut peer, _root, _context, page) = owned_page().await;
    let setup = tokio::spawn(async move {
        let result = worker
            .network_route(
                Uuid::new_v4(),
                page,
                HandlerAction::Add {
                    times: None,
                    url: "https://example.com/acme".into(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    send(&mut peer, json!({"id":enable["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme rejected"}})).await;
    let (mut worker, result) = setup.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "BROWSER_METHOD_FAILED");
    let handler = serde_json::from_value(error.partial.unwrap()["handlerId"].clone()).unwrap();
    assert!(worker.routing_rules.handlers.contains(page, handler));
    assert!(worker.routing_rules.handlers.pending(page));
    assert_eq!(
        worker
            .network_route(
                Uuid::new_v4(),
                page,
                HandlerAction::Add {
                    times: None,
                    url: "https://example.com/acme".into()
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "ROUTE_POLICY_PENDING"
    );
    worker
        .network_route(
            Uuid::new_v4(),
            page,
            HandlerAction::Clear { behavior: None },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(worker.routing_rules.handlers.len(page), 0);
    assert!(!worker.routing_rules.handlers.pending(page));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One paused request exercises admission before enable acknowledgement, ordered handoff and terminal cleanup"
)]
async fn native_route_handlers_fall_back_locally_and_continue_the_paused_request_once() {
    use crate::browser::routing::HandlerAction;
    let (mut worker, mut peer, _root, _context, page) = owned_page().await;
    let url = "https://example.com/acme?item=1";
    let shared = worker.shared.clone();
    let setup = tokio::spawn(async move {
        let result = worker
            .network_route(
                Uuid::new_v4(),
                page,
                HandlerAction::Add {
                    url: url.into(),
                    times: None,
                },
                deadline(),
            )
            .await
            .unwrap();
        (worker, result)
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    assert_eq!(
        enable["params"]["patterns"][0]["urlPattern"],
        "https://example.com/acme\\?item=1"
    );
    send(&mut peer, json!({"method":"Fetch.requestPaused","sessionId":"session-acme","params":{"requestId":"Acme early chain","request":{"url":url,"method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"}})).await;
    let until = deadline();
    let early = loop {
        if let Some(callback) = shared.details.lock().unwrap()["callbacks"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["kind"] == "route"))
            .cloned()
        {
            break callback;
        }
        assert!(Instant::now() < until, "Early handler callback unavailable");
        tokio::task::yield_now().await;
    };
    assert!(early["handlerId"].is_string());
    reply(&mut peer, &enable, json!({})).await;
    let (mut worker, first) = setup.await.unwrap();
    assert_eq!(early["handlerId"], first["handlerId"]);
    let callback = serde_json::from_value(early["reference"].clone()).unwrap();
    let aborting = tokio::spawn(async move {
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::RouteAbort { error_code: None },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let abort = command(&mut peer, "Fetch.failRequest").await;
    assert_eq!(abort["params"]["requestId"], "Acme early chain");
    reply(&mut peer, &abort, json!({})).await;
    let mut worker = aborting.await.unwrap();
    let mut handlers = vec![first["handlerId"].clone()];
    for _ in 0..2 {
        let added = worker
            .network_route(
                Uuid::new_v4(),
                page,
                HandlerAction::Add {
                    url: url.into(),
                    times: None,
                },
                deadline(),
            )
            .await
            .unwrap();
        handlers.push(added["handlerId"].clone());
    }
    worker.apply(EventItem::Event(Event { method: "Fetch.requestPaused".into(), session_id: Some("session-acme".into()), params: json!({"requestId":"Acme paused chain","request":{"url":url,"method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"}) })).unwrap();
    let mut current = *worker.interactions.routes.keys().next().unwrap();
    let expiry = worker.interactions.routes[&current].expires;
    for expected in handlers.iter().rev() {
        assert_eq!(
            worker
                .routing_rules
                .handlers
                .current(current)
                .map(|id| json!(id))
                .unwrap(),
            *expected
        );
        assert_eq!(worker.interactions.routes[&current].expires, expiry);
        if *expected != handlers[0] {
            let reply = worker
                .resolve_route(
                    current,
                    contract::CallbackResponse::RouteFallback {},
                    deadline(),
                )
                .await
                .unwrap();
            current = serde_json::from_value(reply["nextCallback"].clone()).unwrap();
        }
    }
    assert_eq!(
        worker
            .network_route(
                Uuid::new_v4(),
                page,
                HandlerAction::Clear { behavior: None },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "ROUTE_CALLBACKS_PENDING"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    let completing = tokio::spawn(async move {
        worker
            .resolve_route(
                current,
                contract::CallbackResponse::RouteContinue {},
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let request = command(&mut peer, "Fetch.continueRequest").await;
    assert_eq!(request["params"]["requestId"], "Acme paused chain");
    reply(&mut peer, &request, json!({})).await;
    let mut worker = completing.await.unwrap();
    assert!(worker.interactions.routes.is_empty());
    let clearing = tokio::spawn(async move {
        worker
            .network_route(
                Uuid::new_v4(),
                page,
                HandlerAction::Clear { behavior: None },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let request = command(&mut peer, "Fetch.disable").await;
    reply(&mut peer, &request, json!({})).await;
    let worker = clearing.await.unwrap();
    assert_eq!(worker.routing_rules.handlers.len(page), 0);
}

#[tokio::test]
async fn native_heap_exception_group_cleanup_retries_on_the_private_frontend() {
    use super::super::execution_groups::Purpose;
    let (mut worker, mut peer, _root, context, _) = owned_page().await;
    let key = worker
        .evaluation
        .groups
        .begin_private(
            context,
            "Acme frontend".into(),
            "Acme inspector".into(),
            Purpose::HeapInspection,
        )
        .unwrap();
    let owned = key.clone();
    let call = tokio::spawn(async move {
        let result = worker.execution_group_call(
            Uuid::new_v4(), &owned, "Runtime.evaluate",
            json!({"expression":"throw Error('Acme')","objectGroup":owned.1,"returnByValue":true}), deadline(),
        ).await;
        (worker, result)
    });
    let request = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &request,
        json!({"result":{"type":"object","subtype":"error","objectId":"Acme error"},"exceptionDetails":{"exceptionId":1,"text":"Uncaught","lineNumber":0,"columnNumber":0,"exception":{"type":"object","subtype":"error","objectId":"Acme error"}}}),
    )
    .await;
    let (mut worker, result) = call.await.unwrap();
    assert!(result.unwrap().get("exceptionDetails").is_some());
    worker.fence_execution_groups();
    assert!(worker.evaluation.groups.contains(&key));
    for rejected in [true, false] {
        let cleanup = tokio::spawn(async move {
            worker.release_execution_groups(deadline()).await;
            worker
        });
        let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
        assert_eq!(release["sessionId"], "Acme inspector");
        assert_eq!(release["params"]["objectGroup"], key.1);
        if rejected {
            send(&mut peer, json!({"id":release["id"],"sessionId":"Acme inspector","error":{"code":-32000,"message":"Acme retry"}})).await;
        } else {
            reply(&mut peer, &release, json!({})).await;
        }
        worker = cleanup.await.unwrap();
        assert_eq!(worker.evaluation.groups.contains(&key), rejected);
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One controlled result crosses both execution worlds and proves temporary object release before delivery"
)]
async fn native_developer_result_annotations_adopt_nodes_into_the_inspection_world() {
    for lose_inspection in [false, true] {
        let (mut worker, mut peer, _root, _, page) = owned_page().await;
        let frame = worker.index.execution_world(page).unwrap().0;
        let allocated = tokio::spawn(async move {
            let result = worker
                .execute_evaluation(
                    Uuid::new_v4(),
                    Request::Evaluate {
                        scope: page,
                        expression: "document.documentElement".into(),
                        is_function: Some(false),
                        argument: Default::default(),
                        return_handle: true,
                    },
                    deadline(),
                )
                .await
                .unwrap();
            (worker, result)
        });
        let call = command(&mut peer, "Runtime.callFunctionOn").await;
        reply(
            &mut peer,
            &call,
            json!({"result":{"type":"object","objectId":"owned-result"}}),
        )
        .await;
        let (mut worker, result) = allocated.await.unwrap();
        let handle = serde_json::from_value(result["handle"].clone()).unwrap();
        let main_locator = json!({"kind":"element","frame":frame,"id":Uuid::new_v4()});
        let isolated_locator = json!({"kind":"element","frame":frame,"id":Uuid::new_v4()});
        let projected = json!({"value":{"n":17},"annotations":[{"index":0,"path":[],"locator":main_locator}],"complete":true,"projection":"own-enumerable-data-properties"});
        let reading = tokio::spawn(async move {
            let result = worker
                .developer_result_read(Uuid::new_v4(), handle, deadline())
                .await;
            (worker, result)
        });
        let read = command(&mut peer, "Runtime.callFunctionOn").await;
        reply(
            &mut peer,
            &read,
            json!({"result":{"type":"object","value":{"s":projected.to_string()}}}),
        )
        .await;
        let create = command(&mut peer, "Page.createIsolatedWorld").await;
        send(&mut peer,json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":2,"origin":"://","name":super::super::page::WORLD,"uniqueId":"inspection-acme","auxData":{"frameId":"frame-acme","isDefault":false}}}})).await;
        reply(&mut peer, &create, json!({"executionContextId":2})).await;
        let initialize = command(&mut peer, "Runtime.evaluate").await;
        reply(
            &mut peer,
            &initialize,
            json!({"result":{"type":"boolean","value":true}}),
        )
        .await;
        let source = command(&mut peer, "Runtime.callFunctionOn").await;
        assert_eq!(source["params"]["uniqueContextId"], "world-acme");
        let group = source["params"]["objectGroup"].clone();
        assert!(
            group
                .as_str()
                .unwrap()
                .starts_with("testrun:developer-result-transfer:")
        );
        reply(
            &mut peer,
            &source,
            json!({"result":{"type":"object","subtype":"node","objectId":"source-node"}}),
        )
        .await;
        let describe = command(&mut peer, "DOM.describeNode").await;
        assert_eq!(describe["params"]["objectId"], "source-node");
        reply(&mut peer,&describe,json!({"node":{"nodeId":7,"backendNodeId":7,"nodeType":1,"nodeName":"BUTTON","localName":"button","nodeValue":""}})).await;
        let resolve = command(&mut peer, "DOM.resolveNode").await;
        assert_eq!(resolve["params"]["backendNodeId"], 7);
        assert_eq!(resolve["params"]["executionContextId"], 2);
        assert_eq!(resolve["params"]["objectGroup"], group);
        reply(
            &mut peer,
            &resolve,
            json!({"object":{"type":"object","subtype":"node","objectId":"inspection-node"}}),
        )
        .await;
        let adopt = command(&mut peer, "Runtime.callFunctionOn").await;
        assert_eq!(adopt["params"]["objectId"], "inspection-node");
        assert_eq!(adopt["params"]["arguments"][0]["value"], json!(frame));
        reply(
            &mut peer,
            &adopt,
            json!({"result":{"type":"object","value":isolated_locator}}),
        )
        .await;
        let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
        assert_eq!(release["params"]["objectGroup"], group);
        if lose_inspection {
            send(&mut peer,json!({"method":"Runtime.executionContextDestroyed","sessionId":"session-acme","params":{"executionContextId":2,"executionContextUniqueId":"inspection-acme"}})).await;
        }
        reply(&mut peer, &release, json!({})).await;
        let (mut worker, result) = reading.await.unwrap();
        if lose_inspection {
            let error = result.unwrap_err();
            assert_eq!(error.code, "STALE_REFERENCE");
            assert_eq!(error.partial.unwrap()["projectionCompleted"], false);
        } else {
            let result = result.unwrap();
            assert_eq!(result["annotations"][0]["locator"], isolated_locator);
            assert_ne!(result["annotations"][0]["locator"], main_locator);
            assert_eq!(result["projectionCleanupPending"], false);
        }
        worker.close(deadline()).await.unwrap();
    }
}

#[tokio::test]
async fn native_temporary_group_settlement_requires_its_own_rpc_witness() {
    use super::super::execution_groups::Purpose;
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    for attempt in 0..3 {
        let key = worker
            .evaluation
            .groups
            .begin(context, "session-acme".into(), Purpose::DeveloperTransfer)
            .unwrap();
        let owned = key.clone();
        let operation = Uuid::new_v4();
        let shared = worker.shared.clone();
        if attempt == 2 {
            shared
                .state
                .lock()
                .unwrap()
                .begin(operation, "temporary-group-fixture".into())
                .unwrap();
        }
        let call = tokio::spawn(async move {
            let until = if attempt == 0 {
                Instant::now()
            } else {
                deadline()
            };
            let result=worker.execution_group_call(operation,&owned,"Runtime.callFunctionOn",json!({"functionDeclaration":"function(){return document.body}","uniqueContextId":"world-acme","objectGroup":owned.1}),until).await;
            (worker, result)
        });
        if attempt != 0 {
            let request = command(&mut peer, "Runtime.callFunctionOn").await;
            if attempt == 1 {
                send(&mut peer,json!({"id":request["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme rejected"}})).await;
            } else {
                shared
                    .state
                    .lock()
                    .unwrap()
                    .operations
                    .get_mut(&operation)
                    .unwrap()
                    .cancel_requested = true;
                shared.changed.notify_waiters();
            }
        }
        let (returned, result) = call.await.unwrap();
        worker = returned;
        assert_eq!(
            result.unwrap_err().code,
            match attempt {
                0 => "NOT_DISPATCHED",
                1 => "BROWSER_METHOD_FAILED",
                _ => "OUTCOME_UNKNOWN",
            }
        );
        if attempt == 2 {
            worker.release_execution_groups(deadline()).await;
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
            worker.fence_execution_groups();
            assert!(worker.evaluation.groups.contains(&key));
            worker
                .apply(EventItem::Event(Event {
                    method: "Target.targetDestroyed".into(),
                    session_id: None,
                    params: json!({"targetId":"page-acme"}),
                }))
                .unwrap();
            worker.fence_execution_groups();
            assert!(!worker.evaluation.groups.contains(&key));
        } else {
            let cleanup = tokio::spawn(async move {
                worker.release_execution_groups(deadline()).await;
                worker
            });
            let release = command(&mut peer, "Runtime.releaseObjectGroup").await;
            assert_eq!(release["params"]["objectGroup"], json!(key.1));
            reply(&mut peer, &release, json!({})).await;
            worker = cleanup.await.unwrap();
            assert!(!worker.evaluation.groups.contains(&key));
        }
    }
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_route_fallback_overrides_reach_one_terminal_continue() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let setting = tokio::spawn(async move {
        worker
            .scoped_route_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                Some("https://example.com/acme".into()),
                false,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let mut worker = setting.await.unwrap();
    worker
        .page_route_set(
            Uuid::new_v4(),
            page,
            Some("https://example.com/acme".into()),
            false,
            deadline(),
        )
        .await
        .unwrap();
    worker.apply(EventItem::Event(Event {method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"overridden-acme","request":{"url":"https://example.com/acme","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
    let original = *worker.interactions.routes.keys().next().unwrap();
    let expiry = worker.interactions.routes[&original].expires;
    let result = worker
        .resolve_callback(
            Uuid::new_v4(),
            original,
            contract::CallbackResponse::RouteFallbackWith {
                overrides: crate::browser::routing::Overrides {
                    url: Some("https://example.com/changed".into()),
                    method: Some("POST".into()),
                    body_base64: Some("AP8=".into()),
                    ..Default::default()
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    let next = serde_json::from_value(result["nextCallback"].clone()).unwrap();
    assert_eq!(worker.interactions.routes[&next].expires, expiry);
    assert_eq!(worker.interactions.routes[&next].policy_scope, context);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    let continuing = tokio::spawn(async move {
        let result = worker
            .resolve_callback(
                Uuid::new_v4(),
                next,
                contract::CallbackResponse::RouteContinueWith {
                    overrides: crate::browser::routing::Overrides {
                        method: Some("PUT".into()),
                        ..Default::default()
                    },
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let request = command(&mut peer, "Fetch.continueRequest").await;
    assert_eq!(request["params"]["requestId"], "overridden-acme");
    assert_eq!(request["params"]["url"], "https://example.com/changed");
    assert_eq!(request["params"]["method"], "PUT");
    assert_eq!(request["params"]["postData"], "AP8=");
    reply(&mut peer, &request, json!({})).await;
    let (worker, result) = continuing.await.unwrap();
    result.unwrap();
    assert!(worker.interactions.routes.is_empty());
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One owned transport exercises parent and child discovery, replacement and invocation correlation"
)]
async fn native_webmcp_page_refs_fence_frames_and_replaced_registrations() {
    use crate::browser::webmcp_debug::PageAction;

    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let frame = worker.index.execution_world(page).unwrap().0;
    let main = worker.index.native(frame, Kind::Frame).unwrap();
    let child = "frame-acme-child";
    for (method, params) in [
        (
            "Page.frameAttached",
            json!({"frameId":child,"parentFrameId":main}),
        ),
        (
            "Runtime.executionContextCreated",
            json!({"context":{"id":2,"origin":"https://example.com","name":"","uniqueId":"world-child","auxData":{"frameId":child,"isDefault":true}}}),
        ),
    ] {
        worker
            .index
            .apply(Event {
                method: method.into(),
                session_id: Some("session-acme".into()),
                params,
            })
            .unwrap();
    }
    let child_scope = worker.index.context_frame(context, child).unwrap();
    let page_enable = tokio::spawn(async move {
        let result = worker
            .webmcp_page_operation(Uuid::new_v4(), page, PageAction::Enable {}, deadline())
            .await;
        (worker, result.unwrap())
    });
    let enable = command(&mut peer, "WebMCP.enable").await;
    send(&mut peer, json!({"sessionId":"session-acme","method":"WebMCP.toolsAdded","params":{"tools":[
        {"frameId":main,"name":"acme","description":"Main page registration"},
        {"frameId":child,"name":"acme","description":"Child frame registration"},
        {"frameId":main,"name":"debug","description":"Debug registration","annotations":{"debugging":true}}
    ]}})).await;
    reply(&mut peer, &enable, json!({})).await;
    let (mut worker, status) = page_enable.await.unwrap();
    assert_eq!(status["tools"].as_array().unwrap().len(), 1);
    assert_eq!(status["tools"][0]["description"], "Main page registration");
    let tool_ref = serde_json::from_value(status["tools"][0]["toolRef"].clone()).unwrap();
    let status = worker
        .webmcp_page_operation(
            Uuid::new_v4(),
            child_scope,
            PageAction::Enable {},
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(
        status["tools"][0]["description"],
        "Child frame registration"
    );
    let child_ref = serde_json::from_value(status["tools"][0]["toolRef"].clone()).unwrap();
    let invoke = |tool_ref| PageAction::Invoke {
        tool_ref,
        parameters: json!({}),
    };
    assert_eq!(
        worker
            .webmcp_page_operation(Uuid::new_v4(), child_scope, invoke(tool_ref), deadline())
            .await
            .unwrap_err()
            .code,
        "WEB_MCP_TOOL_STALE"
    );

    worker.webmcp_debug_event(&Event {
        method: "WebMCP.toolsAdded".into(),
        session_id: Some("session-acme".into()),
        params: json!({"tools":[{"frameId":main,"name":"acme","description":"Replacement"}]}),
    });
    assert_eq!(
        worker
            .webmcp_page_operation(Uuid::new_v4(), page, invoke(tool_ref), deadline())
            .await
            .unwrap_err()
            .code,
        "WEB_MCP_TOOL_STALE"
    );

    let invoking = tokio::spawn(async move {
        let result = worker
            .webmcp_page_operation(Uuid::new_v4(), child_scope, invoke(child_ref), deadline())
            .await;
        (worker, result.unwrap())
    });
    let invoke = command(&mut peer, "WebMCP.invokeTool").await;
    assert_eq!(invoke["params"]["frameId"], child);
    assert_eq!(invoke["params"]["toolName"], "acme");
    send(&mut peer,json!({"sessionId":"session-acme","method":"WebMCP.toolResponded","params":{"invocationId":"acme-child-call","status":"Completed","output":{"isError":true}}})).await;
    reply(
        &mut peer,
        &invoke,
        json!({"invocationId":"acme-child-call"}),
    )
    .await;
    let (mut worker, result) = invoking.await.unwrap();
    let invocation_id = serde_json::from_value(result["invocationId"].clone()).unwrap();
    let result = worker
        .webmcp_page_operation(
            Uuid::new_v4(),
            child_scope,
            PageAction::Read { invocation_id },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(result["state"], "completed");
    assert_eq!(result["result"]["output"]["isError"], true);
}

#[tokio::test]
async fn native_webmcp_cancelled_dispatch_keeps_receipt_and_late_acknowledgement() {
    use crate::browser::webmcp_debug::Action;
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let frame = worker.index.execution_world(page).unwrap().0;
    let frame = worker.index.native(frame, Kind::Frame).unwrap();
    let enable = tokio::spawn(async move {
        worker
            .webmcp_debug_operation(Uuid::new_v4(), page, Action::Enable {}, deadline())
            .await
            .unwrap();
        worker
    });
    let request = command(&mut peer, "WebMCP.enable").await;
    send(&mut peer, json!({"sessionId":"session-acme","method":"WebMCP.toolsAdded","params":{"tools":[
        {"frameId":frame,"name":"acme","description":"Acme test","annotations":{"debugging":true}}
    ]}})).await;
    reply(&mut peer, &request, json!({})).await;
    let mut worker = enable.await.unwrap();
    let id = Uuid::new_v4();
    let shared = worker.shared.clone();
    shared
        .state
        .lock()
        .unwrap()
        .begin(id, "web_mcp_debug".into())
        .unwrap();
    let invoke = tokio::spawn(async move {
        worker
            .perform(Command {
                id,
                request: Some(Request::WebMcpDebug {
                    scope: page,
                    action: Action::Invoke {
                        name: "acme".into(),
                        parameters: json!({}),
                    },
                }),
                deadline: deadline(),
                queue_slot: None,
            })
            .await;
        worker
    });
    let request = command(&mut peer, "WebMCP.invokeTool").await;
    shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&id)
        .unwrap()
        .cancel_requested = true;
    shared.changed.notify_waiters();
    let mut worker = invoke.await.unwrap();
    let operation = shared.state.lock().unwrap().operations[&id].clone();
    assert_eq!(operation.code.as_deref(), Some("OUTCOME_UNKNOWN"));
    assert_eq!(shared.state.lock().unwrap().phase, Phase::Ready);
    let invocation_id =
        serde_json::from_value(operation.result.unwrap()["invocationId"].clone()).unwrap();
    let read = |invocation_id| Action::Read { invocation_id };
    let receipt = worker
        .webmcp_debug_operation(Uuid::new_v4(), page, read(invocation_id), deadline())
        .await
        .unwrap();
    assert_eq!(receipt["state"], "awaiting_ack");
    send(&mut peer, json!({"sessionId":"session-acme","method":"WebMCP.toolResponded","params":{"invocationId":"acme-late","status":"Completed","output":"Acme result"}})).await;
    reply(&mut peer, &request, json!({"invocationId":"acme-late"})).await;
    let expires = deadline();
    loop {
        let receipt = worker
            .webmcp_debug_operation(Uuid::new_v4(), page, read(invocation_id), expires)
            .await
            .unwrap();
        if receipt["state"] == "completed" {
            assert_eq!(receipt["result"]["output"], "Acme result");
            break;
        }
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "The same owned transport exercises correlation, cancellation, disable and source-loss teardown without replacing resources"
)]
async fn native_webmcp_debug_keeps_ack_completion_and_private_results_separate() {
    use crate::browser::webmcp_debug::Action;
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let shared = worker.shared.clone();
    let (responses, receiver) = mpsc::channel(8);
    worker.responses = Some(receiver);
    let frame = worker.index.execution_world(page).unwrap().0;
    let native_frame = worker.index.native(frame, Kind::Frame).unwrap();
    let enabling = tokio::spawn(async move {
        let value = worker
            .webmcp_debug_operation(Uuid::new_v4(), page, Action::Enable {}, deadline())
            .await;
        (worker, value)
    });
    let enable = command(&mut peer, "WebMCP.enable").await;
    let closing = response_operation(
        &shared,
        &responses,
        Request::WebMcpDebug {
            scope: page,
            action: Action::Disable {},
        },
    )
    .await;
    assert_eq!(
        wait_response(&shared, closing).await.code.as_deref(),
        Some("WEB_MCP_SUBSCRIPTION_PENDING")
    );
    send(&mut peer, json!({"sessionId":"session-acme","method":"WebMCP.toolsAdded","params":{"tools":[
        {"frameId":native_frame,"name":"Acme debug","description":"Inspect Acme","inputSchema":{"type":"object"},"annotations":{"debugging":true}},
        {"frameId":native_frame,"name":"Acme checkout","description":"End user action","annotations":{"debugging":false}}
    ]}})).await;
    reply(&mut peer, &enable, json!({})).await;
    let (mut worker, result) = enabling.await.unwrap();
    assert_eq!(result.unwrap()["tools"].as_array().unwrap().len(), 1);
    let invoking = tokio::spawn(async move {
        let result = worker
            .webmcp_debug_operation(
                Uuid::new_v4(),
                page,
                Action::Invoke {
                    name: "Acme debug".into(),
                    parameters: json!({}),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let invoke = command(&mut peer, "WebMCP.invokeTool").await;
    let status = response_operation(
        &shared,
        &responses,
        Request::WebMcpDebug {
            scope: page,
            action: Action::Status {},
        },
    )
    .await;
    assert_eq!(
        wait_response(&shared, status).await.outcome,
        Outcome::Succeeded
    );
    send(&mut peer,json!({"sessionId":"session-acme","method":"WebMCP.toolInvoked","params":{"frameId":native_frame,"toolName":"Acme debug","invocationId":"foreign-invocation","input":"{}"}})).await;
    send(&mut peer,json!({"sessionId":"session-acme","method":"WebMCP.toolResponded","params":{"invocationId":"foreign-invocation","status":"Completed","output":"Acme foreign result"}})).await;
    // Exercise events drained before the caller observes its acknowledgement.
    send(&mut peer,json!({"sessionId":"session-acme","method":"WebMCP.toolInvoked","params":{"frameId":native_frame,"toolName":"Acme debug","invocationId":"acme-invocation","input":"{}"}})).await;
    send(&mut peer,json!({"sessionId":"session-acme","method":"WebMCP.toolResponded","params":{"invocationId":"acme-invocation","status":"Completed","output":{"private":"Acme result"}}})).await;
    reply(
        &mut peer,
        &invoke,
        json!({"invocationId":"acme-invocation"}),
    )
    .await;
    let (mut worker, result) = invoking.await.unwrap();
    let result = result.unwrap();
    assert!(result.get("result").is_none());
    let invocation_id = serde_json::from_value(result["invocationId"].clone()).unwrap();
    let status = worker
        .webmcp_debug_operation(Uuid::new_v4(), page, Action::Status {}, deadline())
        .await
        .unwrap();
    assert!(!status.to_string().contains("Acme result"));
    let value = worker
        .webmcp_debug_operation(
            Uuid::new_v4(),
            page,
            Action::Read { invocation_id },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(value["state"], "completed");
    assert_eq!(value["result"]["output"]["private"], "Acme result");
    let pending = tokio::spawn(async move {
        let result = worker
            .webmcp_debug_operation(
                Uuid::new_v4(),
                page,
                Action::Invoke {
                    name: "Acme debug".into(),
                    parameters: json!({}),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let invoke = command(&mut peer, "WebMCP.invokeTool").await;
    reply(&mut peer, &invoke, json!({"invocationId":"acme-pending"})).await;
    let (mut worker, result) = pending.await.unwrap();
    let invocation_id = serde_json::from_value(result.unwrap()["invocationId"].clone()).unwrap();
    assert_eq!(
        worker
            .webmcp_debug_operation(Uuid::new_v4(), page, Action::Disable {}, deadline())
            .await
            .unwrap_err()
            .code,
        "WEB_MCP_INVOCATIONS_PENDING"
    );
    let waiting = tokio::spawn(async move {
        let result = worker
            .call_read(
                Uuid::new_v4(),
                Some("session-acme"),
                "Runtime.evaluate",
                json!({"expression":"acmePendingPageCall"}),
                deadline(),
            )
            .await;
        (worker, result)
    });
    let held = command(&mut peer, "Runtime.evaluate").await;
    let cancel = response_operation(
        &shared,
        &responses,
        Request::WebMcpDebug {
            scope: page,
            action: Action::Cancel { invocation_id },
        },
    )
    .await;
    let cancel_command = command(&mut peer, "WebMCP.cancelInvocation").await;
    assert_eq!(cancel_command["params"]["invocationId"], "acme-pending");
    send(&mut peer,json!({"sessionId":"session-acme","method":"WebMCP.toolResponded","params":{"invocationId":"acme-pending","status":"Canceled"}})).await;
    reply(&mut peer, &cancel_command, json!({})).await;
    assert_eq!(
        wait_response(&shared, cancel).await.outcome,
        Outcome::Succeeded
    );
    reply(
        &mut peer,
        &held,
        json!({"result":{"type":"number","value":17}}),
    )
    .await;
    let (mut worker, result) = waiting.await.unwrap();
    result.unwrap();
    assert_eq!(
        worker
            .webmcp_debug_operation(
                Uuid::new_v4(),
                page,
                Action::Read { invocation_id },
                deadline()
            )
            .await
            .unwrap()["state"],
        "canceled"
    );
    assert!(worker.index.diagnostic_scope("session-acme").is_some());
    let mut disabling = tokio::spawn(async move {
        let result = worker
            .webmcp_debug_operation(Uuid::new_v4(), page, Action::Disable {}, deadline())
            .await;
        (worker, result)
    });
    let disable = tokio::select! {
        command=command(&mut peer,"WebMCP.disable")=>command,
        result=&mut disabling=>panic!("Disable settled before dispatch: {:?}",result.map(|(_,result)|result)),
    };
    reply(&mut peer, &disable, json!({})).await;
    let (mut worker, result) = disabling.await.unwrap();
    assert_eq!(result.unwrap()["disabled"], true);
    assert_eq!(
        worker
            .webmcp_debug_operation(Uuid::new_v4(), page, Action::Disable {}, deadline())
            .await
            .unwrap()["disabled"],
        true
    );
    let enabling = tokio::spawn(async move {
        let result = worker
            .webmcp_debug_operation(Uuid::new_v4(), page, Action::Enable {}, deadline())
            .await;
        (worker, result)
    });
    let enable = command(&mut peer, "WebMCP.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let (mut worker, result) = enabling.await.unwrap();
    result.unwrap();
    worker.apply(EventItem::Gap { dropped: 1 }).unwrap();
    assert_eq!(
        worker
            .webmcp_debug_operation(
                Uuid::new_v4(),
                page,
                Action::Invoke {
                    name: "Acme debug".into(),
                    parameters: json!({})
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "WEB_MCP_OBSERVATION_UNAVAILABLE"
    );
    assert_eq!(
        worker
            .webmcp_debug_operation(Uuid::new_v4(), page, Action::Disable {}, deadline())
            .await
            .unwrap_err()
            .code,
        "WEB_MCP_SESSION_TEARDOWN_REQUIRED"
    );
    let native = worker.index.native(page, Kind::Page).unwrap();
    worker
        .apply(EventItem::Event(Event {
            method: "Target.targetDestroyed".into(),
            session_id: None,
            params: json!({"targetId":native}),
        }))
        .unwrap();
    assert_eq!(
        worker
            .webmcp_debug_operation(Uuid::new_v4(), page, Action::Disable {}, deadline())
            .await
            .unwrap()["disabled"],
        true
    );
}

pub(super) async fn owned_page() -> (Worker, Peer, tempfile::TempDir, Reference, Reference) {
    let (mut worker, mut peer, root) = fixture().await;
    let setup = tokio::spawn(async move {
        let id = Uuid::new_v4();
        let context = worker.context_create(id, deadline()).await.unwrap();
        let page = worker.page_create(id, context, deadline()).await.unwrap();
        (worker, context, page)
    });
    serve_page_allocation(&mut peer).await;
    let (worker, context, page) = setup.await.unwrap();
    (worker, peer, root, context, page)
}

async fn registered_binding_fixture(
    mut worker: Worker,
    peer: &mut Peer,
    scope: Reference,
    name: &str,
) -> (Worker, Uuid, String) {
    let name = name.to_owned();
    let adding = tokio::spawn(async move {
        let result = worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingAdd { scope, name },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let probe = command(peer, "Runtime.callFunctionOn").await;
    reply(
        peer,
        &probe,
        json!({"result":{"type":"boolean","value":false}}),
    )
    .await;
    let add = command(peer, "Runtime.addBinding").await;
    let raw = add["params"]["name"].as_str().unwrap().to_owned();
    reply(peer, &add, json!({})).await;
    let script = command(peer, "Page.addScriptToEvaluateOnNewDocument").await;
    reply(peer, &script, json!({"identifier":format!("script-{raw}")})).await;
    let (worker, result) = adding.await.unwrap();
    let binding = serde_json::from_value(result.unwrap()["bindingId"].clone()).unwrap();
    (worker, binding, raw)
}

#[tokio::test]
async fn native_binding_typed_settlements_use_response_lane_and_keep_failed_reply_owned() {
    let (worker, mut peer, _root, _, page) = owned_page().await;
    let (mut worker, _, raw) =
        registered_binding_fixture(worker, &mut peer, page, "AcmeTypedBridge").await;
    let (session, _ordinary, _executions, mut responses, _control) =
        super::execution_concurrency::client(&worker);
    let capacity = session
        .0
        .capacity
        .clone()
        .try_acquire_many_owned(32)
        .unwrap();
    for (ticket, response, encoded, reject) in typed_settlement_cases() {
        worker.apply(EventItem::Event(Event{method:"Runtime.bindingCalled".into(),session_id:Some("session-acme".into()),params:json!({"name":raw,"executionContextId":1,"payload":json!({"ticket":ticket,"args":[{"v":"undefined"}]}).to_string()})})).unwrap();
        let callback: Reference =
            serde_json::from_value(worker.binding_snapshot()[0]["reference"].clone()).unwrap();
        let invalid = contract::CallbackResponse::BindingResolveValue {
            value: crate::browser::evaluation::EvaluationValue::Handle { handle: page },
        };
        assert_eq!(
            session
                .submit(
                    Uuid::new_v4(),
                    Request::CallbackResolve {
                        callback,
                        response: invalid
                    },
                    5000
                )
                .unwrap_err()
                .code,
            "BINDING_HANDLE_UNSUPPORTED"
        );
        assert_eq!(worker.binding_count(), 1);
        assert!(responses.try_recv().is_err());
        let id = session
            .submit(
                Uuid::new_v4(),
                Request::CallbackResolve {
                    callback,
                    response: response.clone(),
                },
                5000,
            )
            .unwrap();
        let command = responses.try_recv().unwrap();
        let task = tokio::spawn(async move {
            worker.respond(command).await;
            worker
        });
        let wire = super::command(&mut peer, "Runtime.callFunctionOn").await;
        assert_eq!(
            wire["params"]["functionDeclaration"],
            crate::browser::bindings::SETTLE
        );
        assert_eq!(wire["params"]["arguments"][2]["value"], encoded);
        assert_eq!(wire["params"]["arguments"][3]["value"], reject);
        reply(
            &mut peer,
            &wire,
            json!({"result":{"type":"boolean","value":true}}),
        )
        .await;
        worker = task.await.unwrap();
        assert_eq!(session.operation(id).unwrap().outcome, Outcome::Succeeded);
        assert_eq!(worker.binding_count(), 0);
        let id = session
            .submit(
                Uuid::new_v4(),
                Request::CallbackResolve { callback, response },
                5000,
            )
            .unwrap();
        worker.respond(responses.try_recv().unwrap()).await;
        assert_eq!(
            session.operation(id).unwrap().code.as_deref(),
            Some("STALE_REFERENCE")
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
    drop(capacity);
}

#[tokio::test]
async fn native_context_binding_names_overlap_page_claims_until_witnessed_page_closure() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let other = worker.index.register_context("context-other").unwrap();
    worker
        .binding_operation(
            Uuid::new_v4(),
            Request::BindingAdd {
                scope: other,
                name: "AcmeContext".into(),
            },
            deadline(),
        )
        .await
        .unwrap();
    let (mut worker, binding, _) =
        registered_binding_fixture(worker, &mut peer, context, "AcmeContext").await;
    assert_eq!(
        worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingAdd {
                    scope: page,
                    name: "AcmeContext".into()
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "BINDING_NAME_CONFLICT"
    );
    assert_eq!(
        worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingRemove {
                    scope: other,
                    binding_id: binding
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "UNKNOWN_BINDING"
    );
    let (mut worker, _, _) = registered_binding_fixture(worker, &mut peer, page, "AcmePage").await;
    worker.index.mark_closing(page).unwrap();
    assert_eq!(
        worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingAdd {
                    scope: context,
                    name: "AcmePage".into()
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "BINDING_NAME_CONFLICT"
    );
    worker.index.retire(page).unwrap();
    worker
        .binding_operation(
            Uuid::new_v4(),
            Request::BindingAdd {
                scope: context,
                name: "AcmePage".into(),
            },
            deadline(),
        )
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_context_binding_future_frame_callbacks_keep_the_page_and_world_identity() {
    let (worker, mut peer, _root, context, page) = owned_page().await;
    let (mut worker, binding, raw) =
        registered_binding_fixture(worker, &mut peer, context, "AcmeFrame").await;
    let info = json!({"targetId":"frame-oop","browserContextId":"context-acme","type":"iframe","title":"Acme","url":"https://example.com/frame","attached":true,"canAccessOpener":false});
    worker.apply(EventItem::Event(Event { method: "Target.attachedToTarget".into(), session_id: Some("session-acme".into()), params: json!({"sessionId":"session-frame","targetInfo":info,"waitingForDebugger":true}) })).unwrap();
    worker.apply(EventItem::Event(Event { method: "Runtime.executionContextCreated".into(), session_id: Some("session-frame".into()), params: json!({"context":{"id":7,"uniqueId":"Acme-frame-world","origin":"https://example.com","name":"","auxData":{"frameId":"frame-oop","isDefault":true,"type":"default"}}}) })).unwrap();
    let installing = tokio::spawn(async move {
        let result = worker
            .configure_bindings(Uuid::new_v4(), Kind::Frame, "session-frame", deadline())
            .await;
        (worker, result)
    });
    let probe = command(&mut peer, "Runtime.callFunctionOn").await;
    assert_eq!(probe["sessionId"], "session-frame");
    reply(
        &mut peer,
        &probe,
        json!({"result":{"type":"boolean","value":false}}),
    )
    .await;
    let add = command(&mut peer, "Runtime.addBinding").await;
    assert_eq!(add["params"]["name"], raw);
    reply(&mut peer, &add, json!({})).await;
    let script = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    assert_eq!(script["params"]["runImmediately"], true);
    reply(
        &mut peer,
        &script,
        json!({"identifier":"Acme-frame-binding"}),
    )
    .await;
    let (mut worker, result) = installing.await.unwrap();
    result.unwrap();
    worker.apply(EventItem::Event(Event { method: "Runtime.bindingCalled".into(), session_id: Some("session-frame".into()), params: json!({"name":raw,"executionContextId":7,"payload":json!({"ticket":1,"args":[{"s":"Acme private frame"}]}).to_string()}) })).unwrap();
    let callbacks = worker.binding_snapshot();
    assert_eq!(callbacks.len(), 1);
    assert_eq!(callbacks[0]["page"], json!(page));
    assert_eq!(callbacks[0]["bindingId"], json!(binding));
    assert!(!json!(callbacks).to_string().contains("Acme private frame"));
    let callback = serde_json::from_value(callbacks[0]["reference"].clone()).unwrap();
    let read = worker
        .binding_operation(
            Uuid::new_v4(),
            Request::BindingRead { callback },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(read["arguments"][0]["value"], "Acme private frame");
    assert_eq!(
        read["frame"],
        json!(
            worker
                .index
                .event_frame("session-frame", "frame-oop")
                .unwrap()
        )
    );
    worker
        .apply(EventItem::Event(Event {
            method: "Target.targetDestroyed".into(),
            session_id: None,
            params: json!({"targetId":"frame-oop"}),
        }))
        .unwrap();
    assert!(worker.binding_snapshot().is_empty());
    worker
        .configure_bindings(Uuid::new_v4(), Kind::Frame, "session-frame", deadline())
        .await
        .unwrap();
    assert_eq!(
        worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingList { scope: context },
                deadline()
            )
            .await
            .unwrap()["bindings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_binding_registration_orders_preexisting_user_scripts_without_running_them_again() {
    let (mut worker, mut peer, _root, context, _) = owned_page().await;
    let adding = tokio::spawn(async move {
        let result = worker
            .init_script(
                Uuid::new_v4(),
                Request::ContextInitScriptAdd {
                    context,
                    source: "globalThis.AcmeInit=AcmeLateBinding()".into(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let user = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    reply(
        &mut peer,
        &user,
        json!({"identifier":"Acme-before-binding"}),
    )
    .await;
    let (mut worker, added) = adding.await.unwrap();
    let script_id = serde_json::from_value(added.unwrap()["scriptId"].clone()).unwrap();
    let binding = tokio::spawn(async move {
        let result = worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingAdd {
                    scope: context,
                    name: "AcmeLateBinding".into(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let probe = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &probe,
        json!({"result":{"type":"boolean","value":false}}),
    )
    .await;
    let native = command(&mut peer, "Runtime.addBinding").await;
    reply(&mut peer, &native, json!({})).await;
    let bridge = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    reply(
        &mut peer,
        &bridge,
        json!({"identifier":"Acme-binding-bridge"}),
    )
    .await;
    let old = command(&mut peer, "Page.removeScriptToEvaluateOnNewDocument").await;
    assert_eq!(old["params"]["identifier"], "Acme-before-binding");
    reply(&mut peer, &old, json!({})).await;
    let ordered = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    assert_eq!(ordered["params"]["source"], user["params"]["source"]);
    assert_eq!(ordered["params"]["runImmediately"], false);
    reply(
        &mut peer,
        &ordered,
        json!({"identifier":"Acme-after-binding"}),
    )
    .await;
    let (mut worker, result) = binding.await.unwrap();
    assert_eq!(result.unwrap()["registered"], true);
    let removing = tokio::spawn(async move {
        let result = worker
            .init_script(
                Uuid::new_v4(),
                Request::ContextInitScriptRemove { context, script_id },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let remove = command(&mut peer, "Page.removeScriptToEvaluateOnNewDocument").await;
    assert_eq!(remove["params"]["identifier"], "Acme-after-binding");
    reply(&mut peer, &remove, json!({})).await;
    let (_worker, result) = removing.await.unwrap();
    assert_eq!(result.unwrap()["removed"], true);
}

#[tokio::test]
async fn native_binding_does_not_reorder_user_scripts_after_context_close_begins() {
    let (mut worker, mut peer, _root, context, _) = owned_page().await;
    let adding = tokio::spawn(async move {
        let result = worker
            .init_script(
                Uuid::new_v4(),
                Request::ContextInitScriptAdd {
                    context,
                    source: "globalThis.Acme=42".into(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let source = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    reply(
        &mut peer,
        &source,
        json!({"identifier":"Acme-owned-script"}),
    )
    .await;
    let (mut worker, result) = adding.await.unwrap();
    result.unwrap();
    worker.index.mark_closing(context).unwrap();
    assert_eq!(
        worker
            .order_init_scripts_last(Uuid::new_v4(), "session-acme", deadline())
            .await
            .unwrap_err()
            .code,
        "STALE_REFERENCE"
    );
    assert!(worker.index.retained(context, Kind::Context));
    assert!(worker.index.native(context, Kind::Context).is_err());
    assert_eq!(
        worker.evaluation.scripts.list(context)["scripts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    worker.index.retire(context).unwrap();
    worker.fence_init_scripts();
    assert!(
        worker.evaluation.scripts.list(context)["scripts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_unknown_context_binding_cleanup_stops_future_installation_and_waits_for_original_target_loss()
 {
    let (worker, mut peer, _root, context, page) = owned_page().await;
    let (mut worker, binding, _) =
        registered_binding_fixture(worker, &mut peer, context, "AcmeStopping").await;
    let shared = worker.shared.clone();
    let operation = Uuid::new_v4();
    shared
        .state
        .lock()
        .unwrap()
        .begin(operation, "fixture".into())
        .unwrap();
    let removing = tokio::spawn(async move {
        let result = worker
            .binding_operation(
                operation,
                Request::BindingRemove {
                    scope: context,
                    binding_id: binding,
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let remove = command(&mut peer, "Page.removeScriptToEvaluateOnNewDocument").await;
    shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&operation)
        .unwrap()
        .cancel_requested = true;
    shared.changed.notify_waiters();
    let (mut worker, result) = removing.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    assert_eq!(
        worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingRemove {
                    scope: context,
                    binding_id: binding
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "BINDING_CLEANUP_PENDING"
    );
    let info = json!({"targetId":"frame-stopping","browserContextId":"context-acme","type":"iframe","title":"Acme","url":"https://example.com/frame","attached":true,"canAccessOpener":false});
    worker.apply(EventItem::Event(Event { method: "Target.attachedToTarget".into(), session_id: Some("session-acme".into()), params: json!({"sessionId":"session-stopping","targetInfo":info,"waitingForDebugger":true}) })).unwrap();
    worker
        .configure_bindings(Uuid::new_v4(), Kind::Frame, "session-stopping", deadline())
        .await
        .unwrap();
    let list = worker
        .binding_operation(
            Uuid::new_v4(),
            Request::BindingList { scope: context },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(list["bindings"][0]["state"], "removing");
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    let target = worker.index.native(page, Kind::Page).unwrap().to_owned();
    worker
        .apply(EventItem::Event(Event {
            method: "Target.targetDestroyed".into(),
            session_id: None,
            params: json!({"targetId":target}),
        }))
        .unwrap();
    reply(&mut peer, &remove, json!({})).await;
    worker.drain().unwrap();
    assert_eq!(
        worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingRemove {
                    scope: context,
                    binding_id: binding
                },
                deadline()
            )
            .await
            .unwrap()["removed"],
        true
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_framework_preparation_cannot_report_ready_after_its_page_closes() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let prepare = tokio::spawn(async move {
        let result = worker
            .framework_operation(
                Uuid::new_v4(),
                Request::FrameworkPrepare { page },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let add = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    send(
        &mut peer,
        json!({"method":"Target.targetDestroyed","params":{"targetId":"page-acme"}}),
    )
    .await;
    reply(
        &mut peer,
        &add,
        json!({"identifier":"framework-script-acme"}),
    )
    .await;
    let (worker, result) = prepare.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "STALE_REFERENCE");
    assert_eq!(error.partial.unwrap()["scopeClosed"], true);
    assert!(
        worker.evaluation.scripts.list(page)["scripts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn native_trace_reuses_verified_tab_attachment_for_start_end_and_stream_reads() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let native = worker.index.native(page, Kind::Page).unwrap();
    worker.apply(EventItem::Event(Event{method:"Target.targetInfoChanged".into(),session_id:None,params:json!({"targetInfo":{"targetId":native,"parentId":"acme-tab","browserContextId":"context-acme","type":"page","title":"Acme","url":"https://example.com/","attached":true,"canAccessOpener":false}})})).unwrap();
    for round in 0..2 {
        let start = tokio::spawn(async move {
            let result = worker
                .trace_start(Uuid::new_v4(), context, Default::default(), deadline())
                .await;
            (worker, result)
        });
        if round == 0 {
            let c = command(&mut peer, "Target.getTargetInfo").await;
            assert_eq!(c["params"]["targetId"], "acme-tab");
            let info = json!({"targetId":"acme-tab","browserContextId":"context-acme","type":"tab","title":"Acme","url":"https://example.com/","attached":false,"canAccessOpener":false});
            reply(&mut peer, &c, json!({"targetInfo":info})).await;
            let c = command(&mut peer, "Target.attachToTarget").await;
            send(&mut peer,json!({"method":"Target.attachedToTarget","params":{"sessionId":"tab-session-acme","targetInfo":info,"waitingForDebugger":false}})).await;
            reply(&mut peer, &c, json!({"sessionId":"tab-session-acme"})).await;
        }
        let c = command(&mut peer, "Tracing.start").await;
        assert_eq!(c["sessionId"], "tab-session-acme");
        reply(&mut peer, &c, json!({})).await;
        let (mut current, result) = start.await.unwrap();
        assert_eq!(result.unwrap()["metadataPage"], json!(page));
        let stop = tokio::spawn(async move {
            let result = current.trace_stop(deadline()).await;
            (current, result)
        });
        let c = command(&mut peer, "Tracing.end").await;
        assert_eq!(c["sessionId"], "tab-session-acme");
        reply(&mut peer, &c, json!({})).await;
        send(&mut peer,json!({"method":"Tracing.tracingComplete","sessionId":"tab-session-acme","params":{"dataLossOccurred":false,"stream":"stream-acme"}})).await;
        let c = command(&mut peer, "IO.read").await;
        assert_eq!(c["sessionId"], "tab-session-acme");
        reply(
            &mut peer,
            &c,
            json!({"data":"{\"traceEvents\":[]}","eof":true}),
        )
        .await;
        let c = command(&mut peer, "IO.close").await;
        reply(&mut peer, &c, json!({})).await;
        let (current, result) = stop.await.unwrap();
        assert_eq!(result.unwrap()["report"]["complete"], true);
        worker = current;
    }
}

#[tokio::test]
async fn native_local_evidence_result_does_not_depend_on_unrelated_target_setup() {
    let (mut worker, mut peer, root, _, _) = owned_page().await;
    worker.apply(EventItem::Event(Event{method:"Target.targetCreated".into(),session_id:None,params:json!({"targetInfo":{"targetId":"acme-unconfigured-worker","browserContextId":"context-acme","type":"service_worker","title":"Acme","url":"https://example.com/worker.js","attached":false,"canAccessOpener":false}})})).unwrap();
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "local-evidence".into())
        .unwrap();
    worker
        .perform(Command {
            queue_slot: None,
            id,
            request: Some(Request::DeveloperEvidenceList {}),
            deadline: deadline(),
        })
        .await;
    assert_eq!(
        worker.shared.state.lock().unwrap().operations[&id].outcome,
        Outcome::Succeeded
    );
    let input = root.path().join("har-import");
    crate::artifacts::directory(&input).unwrap();
    let path = input.join("source.har");
    crate::artifacts::write(&path, br#"{"log":{"version":"1.2","entries":[]}}"#).unwrap();
    let id = Uuid::new_v4();
    worker
        .shared
        .state
        .lock()
        .unwrap()
        .begin(id, "har-register".into())
        .unwrap();
    worker
        .perform(Command {
            queue_slot: None,
            id,
            request: Some(Request::HarRegister {
                path: path.to_string_lossy().into_owned(),
            }),
            deadline: deadline(),
        })
        .await;
    let registered = worker.shared.state.lock().unwrap().operations[&id].clone();
    assert_eq!(registered.outcome, Outcome::Succeeded, "{registered:?}");
    let archive_id =
        serde_json::from_value(registered.result.unwrap()["archiveId"].clone()).unwrap();
    for request in [
        Request::HarList {},
        Request::HarFinalize { archive_id },
        Request::HarRemove { archive_id },
    ] {
        let id = Uuid::new_v4();
        worker
            .shared
            .state
            .lock()
            .unwrap()
            .begin(id, "har-response".into())
            .unwrap();
        worker
            .respond(Command {
                queue_slot: None,
                id,
                request: Some(request),
                deadline: deadline(),
            })
            .await;
        assert_eq!(
            worker.shared.state.lock().unwrap().operations[&id].outcome,
            Outcome::Succeeded
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_heap_chunks_are_private_and_source_loss_preserves_an_explicit_prefix() {
    for lost in [false, true] {
        let (mut worker, mut peer, _root, _, page) = owned_page().await;
        let shared = worker.shared.clone();
        let (tx, rx) = mpsc::channel(8);
        worker.responses = Some(rx);
        let task = tokio::spawn(async move {
            let result = worker
                .heap_capture(Uuid::new_v4(), page, 4096, deadline())
                .await;
            (worker, result)
        });
        let c = command(&mut peer, "HeapProfiler.enable").await;
        let stop = response_operation(&shared, &tx, Request::HeapStop { page }).await;
        assert_eq!(
            wait_response(&shared, stop).await.code.as_deref(),
            Some("HEAP_CAPTURE_BUSY")
        );
        reply(&mut peer, &c, json!({})).await;
        let c = command(&mut peer, "HeapProfiler.takeHeapSnapshot").await;
        let bytes = "{\"snapshot\":{},\"strings\":[\"Acme heap private\"]}";
        send(&mut peer,json!({"method":"HeapProfiler.addHeapSnapshotChunk","sessionId":"session-acme","params":{"chunk":bytes}})).await;
        if lost {
            peer.close(None).await.unwrap();
            drop(peer);
            let (mut worker, result) = task.await.unwrap();
            assert!(result.is_err());
            let reports = worker.seal_heap().unwrap();
            assert_eq!(
                reports["reports"][0]["format"],
                "v8-heap-snapshot-json-prefix"
            );
            assert_eq!(reports["reports"][0]["complete"], false);
            continue;
        }
        reply(&mut peer, &c, json!({})).await;
        let (mut worker, result) = task.await.unwrap();
        let reports = result.unwrap();
        let epoch = worker.heap_status()["profilers"][0]["identityEpoch"].clone();
        worker.heap_gap();
        assert_ne!(worker.heap_status()["profilers"][0]["identityEpoch"], epoch);
        assert_eq!(reports["reports"][0]["identityEpoch"], epoch);
        let stopping = tokio::spawn(async move {
            worker.heap_stop(page, deadline()).await.unwrap();
            worker
        });
        let c = command(&mut peer, "HeapProfiler.disable").await;
        reply(&mut peer, &c, json!({})).await;
        let worker = stopping.await.unwrap();
        assert!(!reports.to_string().contains("Acme heap private"));
        assert_eq!(reports["reports"][0]["format"], "v8-heap-snapshot-json");
        let id = serde_json::from_value(reports["reports"][0]["evidenceId"].clone()).unwrap();
        assert_eq!(
            worker.developers.evidence.get(id).unwrap().read().unwrap(),
            bytes.as_bytes()
        );
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One binding lifecycle tests private callback identity across frames, disposal and refusal to replay"
)]
async fn native_binding_arguments_are_private_and_replies_are_document_bound_and_not_replayed() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let setup_shared = worker.shared.clone();
    let (setup_sender, setup_receiver) = mpsc::channel(8);
    worker.responses = Some(setup_receiver);
    let add = tokio::spawn(async move {
        let result = worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingAdd {
                    scope: page,
                    name: "acme".into(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let c = command(&mut peer, "Runtime.callFunctionOn").await;
    reply(
        &mut peer,
        &c,
        json!({"result":{"type":"boolean","value":false}}),
    )
    .await;
    let c = command(&mut peer, "Runtime.addBinding").await;
    let raw = c["params"]["name"].clone();
    reply(&mut peer, &c, json!({})).await;
    let c = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    assert_eq!(c["params"]["runImmediately"], true);
    let pending_binding = Uuid::parse_str(
        raw.as_str()
            .unwrap()
            .strip_prefix("__testrun_binding_")
            .unwrap(),
    )
    .unwrap();
    let removing = response_operation(
        &setup_shared,
        &setup_sender,
        Request::BindingRemove {
            scope: page,
            binding_id: pending_binding,
        },
    )
    .await;
    assert_eq!(
        wait_response(&setup_shared, removing).await.code.as_deref(),
        Some("BINDING_PENDING")
    );
    reply(&mut peer, &c, json!({"identifier":"acme-binding-script"})).await;
    let (mut worker, result) = add.await.unwrap();
    let binding_id = serde_json::from_value(result.unwrap()["bindingId"].clone()).unwrap();
    let event = |ticket| Event {
        method: "Runtime.bindingCalled".into(),
        session_id: Some("session-acme".into()),
        params: json!({"name":raw,"executionContextId":1,"payload":json!({"ticket":ticket,"args":[{"s":"Acme private"},{"n":41}]}).to_string()}),
    };
    let mut malformed = event(1);
    malformed.params["payload"] = json!("not JSON");
    worker.apply(EventItem::Event(malformed)).unwrap();
    assert!(worker.binding_snapshot().is_empty());
    worker.apply(EventItem::Event(event(1))).unwrap();
    let snapshot = worker.binding_snapshot();
    assert!(!json!(snapshot).to_string().contains("Acme private"));
    let callback = serde_json::from_value(snapshot[0]["reference"].clone()).unwrap();
    let args = worker
        .binding_operation(
            Uuid::new_v4(),
            Request::BindingRead { callback },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(args["arguments"][0]["value"], "Acme private");
    assert_eq!(
        worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingRemove {
                    scope: page,
                    binding_id
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "BINDING_CALLS_PENDING"
    );
    let resolve = tokio::spawn(async move {
        let result = worker
            .resolve_binding(
                callback,
                contract::CallbackResponse::BindingResolve { value: json!(42) },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let c = command(&mut peer, "Runtime.callFunctionOn").await;
    assert_eq!(
        c["params"]["functionDeclaration"],
        crate::browser::bindings::SETTLE
    );
    assert_eq!(c["params"]["arguments"][2]["value"], json!({"n":42}));
    reply(
        &mut peer,
        &c,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let (mut worker, result) = resolve.await.unwrap();
    assert_eq!(result.unwrap()["resolved"], true);
    assert_eq!(
        worker
            .resolve_binding(
                callback,
                contract::CallbackResponse::BindingReject {},
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "STALE_REFERENCE"
    );
    worker.apply(EventItem::Event(event(2))).unwrap();
    worker
        .apply(EventItem::Event(Event {
            method: "Runtime.executionContextsCleared".into(),
            session_id: Some("session-acme".into()),
            params: json!({}),
        }))
        .unwrap();
    assert!(worker.binding_snapshot().is_empty());
    worker.apply(EventItem::Event(event(3))).unwrap();
    assert!(worker.binding_snapshot().is_empty());
    let shared = worker.shared.clone();
    let (sender, receiver) = mpsc::channel(8);
    worker.responses = Some(receiver);
    let removing = tokio::spawn(async move {
        let result = worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingRemove {
                    scope: page,
                    binding_id,
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let remove = command(&mut peer, "Page.removeScriptToEvaluateOnNewDocument").await;
    let duplicate = response_operation(
        &shared,
        &sender,
        Request::BindingRemove {
            scope: page,
            binding_id,
        },
    )
    .await;
    assert_eq!(
        wait_response(&shared, duplicate).await.code.as_deref(),
        Some("BINDING_REMOVAL_PENDING")
    );
    reply(&mut peer, &remove, json!({})).await;
    let remove = command(&mut peer, "Runtime.removeBinding").await;
    assert_eq!(remove["params"]["name"], raw);
    reply(&mut peer, &remove, json!({})).await;
    let (mut worker, result) = removing.await.unwrap();
    assert_eq!(result.unwrap()["removed"], true);
    assert!(
        worker
            .binding_operation(
                Uuid::new_v4(),
                Request::BindingList { scope: page },
                deadline()
            )
            .await
            .unwrap()["bindings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn native_init_script_late_reply_keeps_uncertainty_and_never_reports_a_closed_page_registered()
 {
    for closed in [false, true] {
        let (mut worker, mut peer, _root, _, page) = owned_page().await;
        let add = tokio::spawn(async move {
            let result = worker
                .init_script(
                    Uuid::new_v4(),
                    Request::PageInitScriptAdd {
                        page,
                        source: "globalThis.Acme=42".into(),
                    },
                    deadline(),
                )
                .await;
            (worker, result)
        });
        let c = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
        if closed {
            send(
                &mut peer,
                json!({"method":"Target.targetDestroyed","params":{"targetId":"page-acme"}}),
            )
            .await;
        }
        reply(
            &mut peer,
            &c,
            if closed {
                json!({"identifier":"late-acme"})
            } else {
                json!({})
            },
        )
        .await;
        let (_worker, result) = add.await.unwrap();
        let error = result.unwrap_err();
        let partial = error.partial.unwrap();
        assert!(partial["scriptId"].is_string());
        // Transport schema validation rejects a malformed allocation reply
        // before the owner sees it; dispatched registration remains uncertain.
        if closed {
            assert_eq!(partial["scopeClosed"], true);
        } else {
            assert_eq!(error.code, "OUTCOME_UNKNOWN");
            assert_eq!(partial["registrationUncertain"], true);
        }
    }
}

#[tokio::test]
async fn native_route_expiry_aborts_and_backs_off_a_rejected_cleanup_without_reopening_callback() {
    let (mut worker, mut peer, _root, _, _) = owned_page().await;
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"expired-acme","request":{"url":"https://example.com/acme","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
    let callback =
        serde_json::from_value(worker.interactions.snapshot()["callbacks"][0]["reference"].clone())
            .unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    tokio::time::resume();
    assert_eq!(
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::RouteContinue {},
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "CALLBACK_EXPIRED"
    );
    let expire = tokio::spawn(async move {
        worker.expire_routes().await;
        worker
    });
    let c = command(&mut peer, "Fetch.failRequest").await;
    assert_eq!(c["params"]["requestId"], "expired-acme");
    send(&mut peer,json!({"id":c["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme cleanup retry"}})).await;
    let mut worker = expire.await.unwrap();
    worker.expire_routes().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Cleanup retried without its backoff"
    );
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(3)).await;
    tokio::time::resume();
    assert_eq!(
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::RouteContinue {},
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "CALLBACK_EXPIRED"
    );
    let expire = tokio::spawn(async move {
        worker.expire_routes().await;
        worker
    });
    let c = command(&mut peer, "Fetch.failRequest").await;
    reply(&mut peer, &c, json!({})).await;
    let worker = expire.await.unwrap();
    assert!(
        worker.interactions.snapshot()["callbacks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One paused authentication challenge verifies rejection, credential dispatch, duplicate replies and expiry"
)]
async fn native_auth_challenges_keep_credentials_private_and_cancel_on_expiry() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let setup = tokio::spawn(async move {
        worker
            .page_route_set(
                Uuid::new_v4(),
                page,
                Some("https://example.com/acme".into()),
                true,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let c = command(&mut peer, "Fetch.enable").await;
    assert_eq!(c["params"]["handleAuthRequests"], true);
    reply(&mut peer, &c, json!({})).await;
    let mut worker = setup.await.unwrap();
    let event = |id: &str| Event {
        method: "Fetch.authRequired".into(),
        session_id: Some("session-acme".into()),
        params: json!({"requestId":id,"request":{"url":"https://example.com/acme?token=Acme-secret","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Document","authChallenge":{"source":"Server","origin":"https://example.com","scheme":"basic","realm":"Acme-private-realm"}}),
    };
    worker.apply(EventItem::Event(event("acme-auth"))).unwrap();
    let snapshot = worker.interactions.snapshot();
    assert!(!snapshot.to_string().contains("Acme-secret"));
    assert!(!snapshot.to_string().contains("Acme-private-realm"));
    let callback = serde_json::from_value(snapshot["callbacks"][0]["reference"].clone()).unwrap();
    assert_eq!(
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::RouteContinue {},
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "CALLBACK_RESPONSE_MISMATCH"
    );
    let resolve = tokio::spawn(async move {
        let result = worker
            .resolve_route(
                callback,
                contract::CallbackResponse::AuthCredentials {
                    username: "Acme".into(),
                    password: "Acme-password".into(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let c = command(&mut peer, "Fetch.continueWithAuth").await;
    assert_eq!(
        c["params"]["authChallengeResponse"],
        json!({"response":"ProvideCredentials","username":"Acme","password":"Acme-password"})
    );
    reply(&mut peer, &c, json!({})).await;
    let (mut worker, result) = resolve.await.unwrap();
    result.unwrap();
    assert_eq!(
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::AuthCancel {},
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "STALE_REFERENCE"
    );
    worker
        .apply(EventItem::Event(event("acme-default-auth")))
        .unwrap();
    let callback =
        serde_json::from_value(worker.interactions.snapshot()["callbacks"][0]["reference"].clone())
            .unwrap();
    let resolve = tokio::spawn(async move {
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::AuthDefault {},
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let c = command(&mut peer, "Fetch.continueWithAuth").await;
    assert_eq!(
        c["params"]["authChallengeResponse"],
        json!({"response":"Default"})
    );
    reply(&mut peer, &c, json!({})).await;
    let mut worker = resolve.await.unwrap();
    worker
        .apply(EventItem::Event(event("acme-expired-auth")))
        .unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    tokio::time::resume();
    let expire = tokio::spawn(async move {
        worker.expire_routes().await;
        worker
    });
    let c = command(&mut peer, "Fetch.continueWithAuth").await;
    assert_eq!(
        c["params"]["authChallengeResponse"],
        json!({"response":"CancelAuth"})
    );
    reply(&mut peer, &c, json!({})).await;
    assert!(
        expire.await.unwrap().interactions.snapshot()["callbacks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn native_paused_worker_setup_runs_once_and_resumes_during_an_outstanding_call() {
    let (mut worker, mut peer, _root, _, _) = owned_page().await;
    worker.startup.enabled = true;
    worker.startup.operation = Uuid::new_v4();
    let task = tokio::spawn(async move {
        let result = worker
            .call(
                Uuid::new_v4(),
                Some("session-acme"),
                "Runtime.evaluate",
                json!({"expression":"42","returnByValue":true}),
                deadline(),
            )
            .await;
        (worker, result)
    });
    let outer = command(&mut peer, "Runtime.evaluate").await;
    let info = json!({"targetId":"paused-worker","browserContextId":"context-acme","type":"worker","title":"Acme","url":"https://example.com/worker.js","attached":true,"canAccessOpener":false});
    send(&mut peer,json!({"method":"Target.attachedToTarget","sessionId":"session-acme","params":{"sessionId":"paused-worker-session","targetInfo":info,"waitingForDebugger":true}})).await;
    let enable = command(&mut peer, "Runtime.enable").await;
    assert_eq!(enable["sessionId"], "paused-worker-session");
    send(&mut peer,json!({"method":"Runtime.executionContextCreated","sessionId":"paused-worker-session","params":{"context":{"id":1,"uniqueId":"Acme-paused-world","origin":"https://example.com","name":""}}})).await;
    reply(&mut peer, &enable, json!({})).await;
    let attach = command(&mut peer, "Target.setAutoAttach").await;
    assert_eq!(attach["params"]["waitForDebuggerOnStart"], true);
    reply(&mut peer, &attach, json!({})).await;
    let resume = command(&mut peer, "Runtime.runIfWaitingForDebugger").await;
    assert_eq!(resume["sessionId"], "paused-worker-session");
    reply(&mut peer, &resume, json!({})).await;
    reply(
        &mut peer,
        &outer,
        json!({"result":{"type":"number","value":42}}),
    )
    .await;
    let (worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap()["result"]["value"], 42);
    assert_eq!(worker.startup_summary()["pausedTargets"], 0);
    assert!(worker.configured.contains("paused-worker-session"));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_paused_target_setup_failure_retains_ownership_without_repeating_setup() {
    let (mut worker, mut peer, _root, _, _) = owned_page().await;
    worker.startup.enabled = true;
    let info = json!({"targetId":"paused-worker","browserContextId":"context-acme","type":"worker","title":"Acme","url":"https://example.com/worker.js","attached":true,"canAccessOpener":false});
    worker.apply(EventItem::Event(Event{method:"Target.attachedToTarget".into(),session_id:Some("session-acme".into()),params:json!({"sessionId":"paused-worker-session","targetInfo":info,"waitingForDebugger":true})})).unwrap();
    let task = tokio::spawn(async move {
        let result = worker.reconcile(Uuid::new_v4(), deadline()).await;
        (worker, result)
    });
    let enable = command(&mut peer, "Runtime.enable").await;
    send(&mut peer,json!({"id":enable["id"],"sessionId":"paused-worker-session","error":{"code":-32000,"message":"Acme setup failure"}})).await;
    let (mut worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap_err().code, "BROWSER_METHOD_FAILED");
    assert_eq!(worker.startup_summary()["pausedTargets"], 1);
    assert_eq!(
        worker
            .reconcile(Uuid::new_v4(), deadline())
            .await
            .unwrap_err()
            .code,
        "TARGET_STARTUP_PENDING"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_edge_paused_page_setup_classifies_browser_ui_without_resuming_application_pages() {
    for classification in ["browser_ui", "page", "foreign-ui"] {
        let (mut worker, mut peer, _root, context, original) = owned_page().await;
        worker
            .connection
            .as_ref()
            .unwrap()
            .accept_edge_target_metadata();
        worker.startup.enabled = true;
        worker.startup.busy = true;
        let info = json!({"targetId":"edge-candidate","browserContextId":"context-acme","type":"page","title":"","url":"","attached":true,"canAccessOpener":false});
        worker.apply(EventItem::Event(Event{method:"Target.attachedToTarget".into(),session_id:None,params:json!({"sessionId":"edge-candidate-session","targetInfo":info,"waitingForDebugger":true})})).unwrap();
        let candidate = worker.index.target_reference("edge-candidate").unwrap();
        let task = tokio::spawn(async move {
            let result = worker
                .call(
                    Uuid::new_v4(),
                    Some("edge-candidate-session"),
                    "Page.enable",
                    json!({}),
                    deadline(),
                )
                .await;
            (worker, result)
        });
        let enable = command(&mut peer, "Page.enable").await;
        let metadata = command(&mut peer, "Target.getTargetInfo").await;
        assert_eq!(metadata["params"]["targetId"], "edge-candidate");
        reply(&mut peer, &metadata, json!({"targetInfo":{"targetId":"edge-candidate","browserContextId":if classification=="foreign-ui"{"foreign-context"}else{"context-acme"},"type":if classification=="page"{"page"}else{"browser_ui"},"title":"","url":"edge://downloads-hub/","attached":true,"canAccessOpener":false}})).await;
        if classification == "browser_ui" {
            let resume = command(&mut peer, "Runtime.runIfWaitingForDebugger").await;
            assert_eq!(resume["sessionId"], "edge-candidate-session");
            reply(&mut peer, &resume, json!({})).await;
            reply(&mut peer, &enable, json!({})).await;
            let detach = command(&mut peer, "Target.detachFromTarget").await;
            assert_eq!(detach["params"]["sessionId"], "edge-candidate-session");
            send(&mut peer,json!({"method":"Target.detachedFromTarget","params":{"sessionId":"edge-candidate-session","targetId":"edge-candidate"}})).await;
            reply(&mut peer, &detach, json!({})).await;
        } else if classification == "page" {
            // A real application page remains paused until the normal setup
            // sequence completes; metadata alone grants no early resume.
            reply(&mut peer, &enable, json!({})).await;
        }
        let (worker, result) = task.await.unwrap();
        assert!(worker.index.retained(original, Kind::Page));
        assert!(worker.index.retained(context, Kind::Context));
        if classification == "browser_ui" {
            assert_eq!(result.unwrap_err().code, "STALE_REFERENCE");
            assert!(!worker.index.retained(candidate, Kind::Page));
            assert_eq!(worker.startup_summary()["pausedTargets"], 0);
            let history = worker
                .interactions
                .history
                .page(context, None, 100)
                .unwrap();
            assert!(
                history["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|event| event["event"] == "page_closed"
                        && event["page"] == json!(candidate))
            );
        } else {
            if classification == "page" {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err().code, "INVALID_SCOPE");
            }
            assert!(worker.index.retained(candidate, Kind::Page));
            assert_eq!(worker.startup_summary()["pausedTargets"], 1);
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(10), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_unowned_target_startup_pause_is_resumed_without_adopting_the_target() {
    for detached in [false, true] {
        let (mut worker, mut peer, _root, _, _) = owned_page().await;
        worker.startup.enabled = true;
        let info = json!({"targetId":"unowned-page","type":"page","title":"Acme","url":"about:blank","attached":true,"canAccessOpener":false});
        worker.apply(EventItem::Event(Event{method:"Target.attachedToTarget".into(),session_id:None,params:json!({"sessionId":"unowned-page-session","targetInfo":info,"waitingForDebugger":true})})).unwrap();
        let task = tokio::spawn(async move {
            worker.reconcile(Uuid::new_v4(), deadline()).await.unwrap();
            worker
        });
        let resume = command(&mut peer, "Runtime.runIfWaitingForDebugger").await;
        assert_eq!(resume["sessionId"], "unowned-page-session");
        if detached {
            send(&mut peer,json!({"method":"Target.detachedFromTarget","params":{"sessionId":"unowned-page-session","targetId":"unowned-page"}})).await;
            send(&mut peer,json!({"id":resume["id"],"sessionId":"unowned-page-session","error":{"code":-32000,"message":"Acme session closed"}})).await;
        } else {
            reply(&mut peer, &resume, json!({})).await;
        }
        let worker = task.await.unwrap();
        assert!(worker.index.target_reference("unowned-page").is_none());
        assert_eq!(worker.startup_summary()["pausedTargets"], 0);
    }
}

#[tokio::test]
async fn native_owned_teardown_does_not_resume_a_pending_startup_pause() {
    let (mut worker, mut peer, _root, _, _) = owned_page().await;
    worker.startup.enabled = true;
    let info = json!({"targetId":"unowned-page","type":"page","title":"Acme","url":"about:blank","attached":true,"canAccessOpener":false});
    worker.apply(EventItem::Event(Event{method:"Target.attachedToTarget".into(),session_id:None,params:json!({"sessionId":"unowned-page-session","targetInfo":info,"waitingForDebugger":true})})).unwrap();
    worker.shared.state.lock().unwrap().phase = Phase::Closing;
    worker.reconcile(Uuid::new_v4(), deadline()).await.unwrap();
    assert_eq!(worker.startup_summary()["pausedTargets"], 1);
    assert!(!worker.startup_pending());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_startup_diagnostic_overflow_preserves_target_resume_and_later_calls() {
    let (mut worker, mut peer, _root, _, _) = owned_page().await;
    worker.startup.enabled = true;
    let buffer = worker.events.as_ref().unwrap().0.clone();
    // A debugger-start attachment is critical and must survive the burst.
    buffer.push(Event {
        method: "Target.attachedToTarget".into(),
        session_id: Some("session-acme".into()),
        params: json!({"sessionId":"paused-worker-session","targetInfo":{"targetId":"paused-worker","browserContextId":"context-acme","type":"worker","title":"Acme","url":"https://example.com/worker.js","attached":true,"canAccessOpener":false},"waitingForDebugger":true}),
    }, 256).unwrap();
    for _ in 0..crate::browser::cdp::MAX_EVENTS {
        buffer.push(Event {
            method: "Network.dataReceived".into(),
            session_id: Some("session-acme".into()),
            params: json!({"requestId":"Acme-module","timestamp":1.0,"dataLength":128,"encodedDataLength":128}),
        }, 128).unwrap();
    }
    worker.drain().unwrap();
    assert!(worker.navigation.gaps > 0);
    assert_eq!(worker.startup_summary()["failure"], Value::Null);
    assert_eq!(worker.startup_summary()["pausedTargets"], 1);
    let task = tokio::spawn(async move {
        worker.reconcile(Uuid::new_v4(), deadline()).await.unwrap();
        let result = worker
            .call_read(
                Uuid::new_v4(),
                Some("session-acme"),
                "Runtime.evaluate",
                json!({"expression":"42","returnByValue":true}),
                deadline(),
            )
            .await
            .unwrap();
        assert_eq!(result["result"]["value"], 42);
        assert_eq!(worker.startup_summary()["pausedTargets"], 0);
        assert_eq!(worker.shared.state.lock().unwrap().phase, Phase::Ready);
        worker
    });
    let enable = command(&mut peer, "Runtime.enable").await;
    assert_eq!(enable["sessionId"], "paused-worker-session");
    reply(&mut peer, &enable, json!({})).await;
    let attach = command(&mut peer, "Target.setAutoAttach").await;
    reply(&mut peer, &attach, json!({})).await;
    let resume = command(&mut peer, "Runtime.runIfWaitingForDebugger").await;
    assert_eq!(resume["sessionId"], "paused-worker-session");
    reply(&mut peer, &resume, json!({})).await;
    let read = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &read,
        json!({"result":{"type":"number","value":42}}),
    )
    .await;
    let mut worker = task.await.unwrap();
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_context_script_identity_and_pending_close_retain_exact_attachment_owners() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let add = tokio::spawn(async move {
        let result = worker
            .init_script(
                Uuid::new_v4(),
                Request::ContextInitScriptAdd {
                    context,
                    source: "globalThis.AcmePrivateInitialization=17".into(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let command = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    assert_eq!(command["params"]["runImmediately"], false);
    reply(
        &mut peer,
        &command,
        json!({"identifier":"Acme-context-script"}),
    )
    .await;
    let (mut worker, result) = add.await.unwrap();
    let result = result.unwrap();
    assert_eq!(result["context"], json!(context));
    let script_id = serde_json::from_value(result["scriptId"].clone()).unwrap();
    assert_eq!(
        worker
            .init_script(
                Uuid::new_v4(),
                Request::PageInitScriptRemove { page, script_id },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "UNKNOWN_INIT_SCRIPT"
    );
    assert!(
        !worker
            .evaluation
            .scripts
            .list(context)
            .to_string()
            .contains("AcmePrivateInitialization")
    );
    worker.startup.enabled = true;
    let info = json!({"targetId":"closing-worker","browserContextId":"context-acme","type":"worker","title":"Acme","url":"https://example.com/worker.js","attached":true,"canAccessOpener":false});
    worker.apply(EventItem::Event(Event{method:"Target.attachedToTarget".into(),session_id:Some("session-acme".into()),params:json!({"sessionId":"closing-worker-session","targetInfo":info,"waitingForDebugger":true})})).unwrap();
    worker.index.mark_closing(context).unwrap();
    worker.fence_init_scripts();
    assert_eq!(
        worker.evaluation.scripts.list(context)["installations"][0]["targets"],
        1
    );
    worker.reconcile(Uuid::new_v4(), deadline()).await.unwrap();
    assert_eq!(worker.startup_summary()["pausedTargets"], 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    worker.index.retire(context).unwrap();
    worker.fence_init_scripts();
    worker.reconcile(Uuid::new_v4(), deadline()).await.unwrap();
    assert_eq!(worker.startup_summary()["pausedTargets"], 0);
    assert!(
        worker.evaluation.scripts.list(context)["scripts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One retained script receipt is followed through uncertain installation and witnessed frame closure"
)]
async fn native_uncertain_frame_script_installation_is_not_replayed_and_cleanup_waits_for_frame_close()
 {
    let (mut worker, mut peer, _root, context, _) = owned_page().await;
    let add = tokio::spawn(async move {
        let result = worker
            .init_script(
                Uuid::new_v4(),
                Request::ContextInitScriptAdd {
                    context,
                    source: "globalThis.Acme=42".into(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let root = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    reply(&mut peer, &root, json!({"identifier":"Acme-root-script"})).await;
    let (mut worker, result) = add.await.unwrap();
    let script_id = serde_json::from_value(result.unwrap()["scriptId"].clone()).unwrap();
    let info = json!({"targetId":"frame-oop","browserContextId":"context-acme","type":"iframe","title":"Acme","url":"https://example.com/frame","attached":true,"canAccessOpener":false});
    worker.apply(EventItem::Event(Event{method:"Target.attachedToTarget".into(),session_id:Some("session-acme".into()),params:json!({"sessionId":"session-frame","targetInfo":info,"waitingForDebugger":true})})).unwrap();
    let shared = worker.shared.clone();
    let operation = Uuid::new_v4();
    shared
        .state
        .lock()
        .unwrap()
        .begin(operation, "fixture".into())
        .unwrap();
    let applying = tokio::spawn(async move {
        let result = worker
            .configure_init_scripts(operation, Kind::Frame, "session-frame", deadline())
            .await;
        (worker, result)
    });
    let frame = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    assert_eq!(frame["params"]["runImmediately"], true);
    shared
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&operation)
        .unwrap()
        .cancel_requested = true;
    shared.changed.notify_waiters();
    let (worker, result) = applying.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    let removing = tokio::spawn(async move {
        let mut worker = worker;
        let result = worker
            .init_script(
                Uuid::new_v4(),
                Request::ContextInitScriptRemove { context, script_id },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let remove = command(&mut peer, "Page.removeScriptToEvaluateOnNewDocument").await;
    assert_eq!(remove["params"]["identifier"], "Acme-root-script");
    reply(&mut peer, &remove, json!({})).await;
    let (mut worker, result) = removing.await.unwrap();
    assert_eq!(result.unwrap_err().code, "INIT_SCRIPT_CLEANUP_PENDING");
    assert_eq!(
        worker
            .init_script(
                Uuid::new_v4(),
                Request::ContextInitScriptRemove { context, script_id },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "INIT_SCRIPT_CLEANUP_PENDING"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    reply(
        &mut peer,
        &frame,
        json!({"identifier":"Acme-frame-script-late"}),
    )
    .await;
    worker
        .apply(EventItem::Event(Event {
            method: "Page.frameDetached".into(),
            session_id: Some("session-acme".into()),
            params: json!({"frameId":"frame-oop","reason":"remove"}),
        }))
        .unwrap();
    assert_eq!(
        worker
            .init_script(
                Uuid::new_v4(),
                Request::ContextInitScriptRemove { context, script_id },
                deadline()
            )
            .await
            .unwrap()["removed"],
        true
    );
    assert!(
        worker.evaluation.scripts.list(context)["scripts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn native_paused_page_init_console_is_collected_without_replaying_existing_page_history() {
    use crate::browser::diagnostics::Query;
    let (mut worker, _peer, _root, context, _) = owned_page().await;
    worker.diagnostics.history.enable(context, true);
    worker.configured.remove("session-acme");
    worker.diagnostic_event(&Event{method:"Runtime.consoleAPICalled".into(),session_id:Some("session-acme".into()),params:json!({"type":"log","args":[{"type":"string","value":"Acme old page"}],"executionContextId":1,"timestamp":1})});
    let info = json!({"targetId":"initializing-page","browserContextId":"context-acme","type":"page","title":"Acme","url":"https://example.com/new","attached":true,"canAccessOpener":false});
    worker.apply(EventItem::Event(Event{method:"Target.attachedToTarget".into(),session_id:None,params:json!({"sessionId":"initializing-session","targetInfo":info,"waitingForDebugger":true})})).unwrap();
    worker.diagnostic_event(&Event{method:"Runtime.consoleAPICalled".into(),session_id:Some("initializing-session".into()),params:json!({"type":"log","args":[{"type":"string","value":"Acme live initialization"}],"executionContextId":1,"timestamp":2})});
    let history = worker
        .diagnostics_read(
            context,
            &Query {
                include_text: true,
                ..Default::default()
            },
        )
        .unwrap();
    let events = history["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["data"]["text"], "Acme live initialization");
    assert_eq!(
        events[0]["data"]["subscriptionBoundary"],
        "owned-target-startup-pending"
    );
}

#[tokio::test]
async fn native_capture_enables_new_owned_workers_without_enabling_diagnostic_history() {
    let (mut worker, mut peer, _root, context, _) = owned_page().await;
    worker
        .har_archives
        .capture_start(
            context,
            None,
            (HarCaptureContent::Embed, HarCaptureMode::Minimal, None),
            None,
            None,
        )
        .unwrap();
    let info = json!({"targetId":"capture-worker","browserContextId":"context-acme","type":"worker","title":"Acme","url":"https://example.com/worker.js","attached":true,"canAccessOpener":false});
    worker.apply(EventItem::Event(Event{method:"Target.attachedToTarget".into(),session_id:Some("session-acme".into()),params:json!({"sessionId":"capture-worker-session","targetInfo":info,"waitingForDebugger":false})})).unwrap();
    let setup = tokio::spawn(async move {
        worker.reconcile(Uuid::new_v4(), deadline()).await.unwrap();
        worker
    });
    let enable = command(&mut peer, "Runtime.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let attach = command(&mut peer, "Target.setAutoAttach").await;
    reply(&mut peer, &attach, json!({})).await;
    let network = command(&mut peer, "Network.enable").await;
    assert_eq!(network["sessionId"], "capture-worker-session");
    assert_eq!(network["params"]["maxPostDataSize"], 65536);
    reply(&mut peer, &network, json!({})).await;
    let mut worker = setup.await.unwrap();
    assert!(!worker.diagnostics.history.active(context));
    worker.diagnostic_event(&Event{method:"Network.requestWillBeSent".into(),session_id:Some("capture-worker-session".into()),params:json!({"requestId":"capture-request","type":"Fetch","timestamp":1,"wallTime":2,"request":{"url":"https://example.com/acme","method":"POST","headers":{},"hasPostData":true,"postData":"lossy","postDataEntries":[{"bytes":"AP+A"}]}})});
    worker.har_archives.capture_stop(context).unwrap();
    let captured = worker
        .har_archives
        .capture_publish(worker.journal.directory(), context)
        .unwrap();
    let archive: Value = serde_json::from_slice(
        &std::fs::read(
            captured["reports"][0]["attachment"]["path"]
                .as_str()
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        archive["log"]["entries"][0]["request"]["postData"]["text"],
        "AP+A"
    );
    assert_eq!(
        archive["log"]["entries"][0]["_testrunCaptureComplete"],
        false
    );
    assert!(!worker.diagnostics.history.active(context));
}

#[tokio::test]
async fn network_response_details_are_private_scoped_and_map_native_cache_and_completion() {
    for cached in [false, true] {
        let (mut worker, _peer, _root, context, _page) = owned_page().await;
        worker.diagnostics.history.enable(context, true);
        let event = |method: &str, params| Event {
            method: method.into(),
            session_id: Some("session-acme".into()),
            params,
        };
        worker.diagnostic_event(&event("Network.requestWillBeSent",json!({"requestId":"Acme-request","type":"Fetch","timestamp":10.0,"wallTime":1000.0,"request":{"url":"https://example.com/private?token=Acme","method":"GET","headers":{}}})));
        if cached {
            worker.diagnostic_event(&event(
                "Network.requestServedFromCache",
                json!({"requestId":"Acme-request"}),
            ));
        }
        worker.diagnostic_event(&event("Network.responseReceived",json!({"requestId":"Acme-request","type":"Fetch","timestamp":10.6,"response":{"url":"https://example.com/private?token=Acme","status":200,"headers":{},"remoteIPAddress":"192.0.2.1","remotePort":443,"securityDetails":{"protocol":"TLS 1.3","subjectName":"example.com","issuer":"Acme CA","validFrom":1,"validTo":2,"keyExchange":"Acme-private"},"timing":{"requestTime":10.5,"dnsStart":1.0,"sendStart":2.0,"receiveHeadersEnd":3.0}}})));
        let history = worker
            .diagnostics
            .history
            .read(context, &Default::default())
            .unwrap();
        let request_id =
            serde_json::from_value(history["events"][0]["data"]["requestId"].clone()).unwrap();
        let plain = worker
            .network_read(
                Uuid::new_v4(),
                context,
                request_id,
                Default::default(),
                deadline(),
            )
            .await
            .unwrap();
        assert!(plain.get("responseDetails").is_none());
        let options = crate::browser::network::Read {
            response_details: true,
            ..Default::default()
        };
        let live = worker
            .network_read(
                Uuid::new_v4(),
                context,
                request_id,
                options.clone(),
                deadline(),
            )
            .await
            .unwrap();
        assert_eq!(live["responseDetails"]["timing"]["responseEnd"], -1.0);
        assert_eq!(
            live["responseDetails"]["timing"]["startTime"],
            if cached { 1000000.0 } else { 1000500.0 }
        );
        assert_eq!(
            live["responseDetails"]["serverAddr"]["ipAddress"],
            "192.0.2.1"
        );
        assert_eq!(
            live["responseDetails"]["securityDetails"]["issuer"],
            "Acme CA"
        );
        assert!(
            !live.to_string().contains("Acme-private") && !live.to_string().contains("token=Acme")
        );
        worker.diagnostic_event(&event(
            "Network.loadingFinished",
            json!({"requestId":"Acme-request","timestamp":11.0,"encodedDataLength":1}),
        ));
        let ended = worker
            .network_read(Uuid::new_v4(), context, request_id, options, deadline())
            .await
            .unwrap();
        assert_eq!(ended["responseDetails"]["timing"]["responseEnd"], 1000.0);
        assert_eq!(
            ended["responseDetails"]["timing"]["responseStart"],
            if cached { 1000.0 } else { 3.0 }
        );
        assert_eq!(ended["complete"], true);
        worker.diagnostics.source_gap(1, vec![]);
        assert_eq!(
            worker
                .network_read(
                    Uuid::new_v4(),
                    context,
                    request_id,
                    Default::default(),
                    deadline()
                )
                .await
                .unwrap_err()
                .code,
            "NETWORK_REQUEST_EXPIRED"
        );
    }
}

#[tokio::test]
async fn network_response_details_keep_no_response_failures_unknown() {
    let (mut worker, _peer, _root, context, _page) = owned_page().await;
    worker.diagnostics.history.enable(context, true);
    for (method, params) in [
        (
            "Network.requestWillBeSent",
            json!({"requestId":"Acme-refused","type":"Fetch","timestamp":10.0,"wallTime":1000.0,"request":{"url":"https://example.com/refused","method":"GET","headers":{}}}),
        ),
        (
            "Network.loadingFailed",
            json!({"requestId":"Acme-refused","timestamp":11.0,"errorText":"net::ERR_CERT_AUTHORITY_INVALID"}),
        ),
    ] {
        worker.diagnostic_event(&Event {
            method: method.into(),
            session_id: Some("session-acme".into()),
            params,
        });
    }
    let history = worker
        .diagnostics
        .history
        .read(context, &Default::default())
        .unwrap();
    let request_id =
        serde_json::from_value(history["events"][0]["data"]["requestId"].clone()).unwrap();
    let read = worker
        .network_read(
            Uuid::new_v4(),
            context,
            request_id,
            crate::browser::network::Read {
                response_details: true,
                ..Default::default()
            },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(read["failed"], true);
    assert_eq!(read["responseDetails"]["responseAvailable"], false);
    assert!(read["responseDetails"]["securityDetails"].is_null());
    assert!(read["responseDetails"]["serverAddr"].is_null());
    assert_eq!(read["responseDetails"]["timing"]["responseEnd"], -1.0);
    assert_eq!(read["responseDetails"]["timingObservedFields"], json!([]));
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one end-to-end capture scenario across a page, its frames and a worker"
)]
async fn native_page_capture_selects_page_frame_requests_and_excludes_workers() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let stale = Reference {
        generation: Uuid::new_v4(),
        ..page
    };
    assert!(
        worker
            .har_capture_start(
                Uuid::new_v4(),
                context,
                Some(stale),
                (HarCaptureContent::Embed, HarCaptureMode::Minimal, None),
                None,
                None,
                deadline()
            )
            .await
            .is_err()
    );
    assert!(!worker.har_archives.capturing(context));
    let started = worker
        .har_archives
        .capture_start(
            context,
            Some(page),
            (HarCaptureContent::Omit, HarCaptureMode::Full, None),
            None,
            None,
        )
        .unwrap();
    assert_eq!(started["page"], json!(page));
    let info = json!({"targetId":"capture-worker","browserContextId":"context-acme","type":"worker","title":"Acme","url":"https://example.com/worker.js","attached":true,"canAccessOpener":false});
    worker.apply(EventItem::Event(Event{method:"Target.attachedToTarget".into(),session_id:Some("session-acme".into()),params:json!({"sessionId":"capture-worker-session","targetInfo":info,"waitingForDebugger":false})})).unwrap();
    let setup = tokio::spawn(async move {
        worker.reconcile(Uuid::new_v4(), deadline()).await.unwrap();
        worker
    });
    let enable = command(&mut peer, "Runtime.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let attach = command(&mut peer, "Target.setAutoAttach").await;
    reply(&mut peer, &attach, json!({})).await;
    let network = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &network, json!({})).await;
    let mut worker = setup.await.unwrap();
    // Chromium can report the sent Cookie header before requestWillBeSent.
    worker.diagnostic_event(&Event {
        method: "Network.requestWillBeSentExtraInfo".into(),
        session_id: Some("session-acme".into()),
        params: json!({"requestId":"page-request","headers":{"Cookie":"acme=one; theme=dark"},"associatedCookies":[]}),
    });
    for (session, id, url) in [
        (
            "session-acme",
            "page-request",
            "https://example.com/acme/page",
        ),
        (
            "capture-worker-session",
            "worker-request",
            "https://example.com/acme/worker",
        ),
    ] {
        worker.diagnostic_event(&Event{method:"Network.requestWillBeSent".into(),session_id:Some(session.into()),params:json!({"requestId":id,"type":"Fetch","timestamp":1,"wallTime":2,"request":{"url":url,"method":"GET","headers":{}}})});
    }
    assert_eq!(
        worker.har_archives.capture_status(context).unwrap()["entries"],
        1
    );
    worker.diagnostic_event(&Event{method:"Network.responseReceived".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"page-request","type":"Fetch","timestamp":10.0,"response":{"url":"https://example.com/acme/page","status":200,"statusText":"OK","headers":{"Content-Type":"text/plain","Set-Cookie":"id=Acme; Path=/"},"mimeType":"text/plain","protocol":"h2","remoteIPAddress":"192.0.2.1","remotePort":443,"securityDetails":{"protocol":"TLS 1.3","subjectName":"example.com","issuer":"Acme CA","validFrom":1.0,"validTo":2.0,"keyExchange":""},"timing":{"requestTime":10.0,"dnsStart":1.0,"dnsEnd":2.0,"connectStart":2.0,"connectEnd":6.0,"sslStart":3.0,"sslEnd":6.0,"sendStart":6.0,"sendEnd":6.5,"receiveHeadersEnd":16.0}}})});
    worker.diagnostic_event(&Event {
        method: "Network.loadingFinished".into(),
        session_id: Some("session-acme".into()),
        params: json!({"requestId":"page-request","timestamp":10.02,"encodedDataLength":120}),
    });
    worker.har_archives.capture_stop(context).unwrap();
    let captured = worker
        .har_archives
        .capture_publish(worker.journal.directory(), context)
        .unwrap();
    let archive: Value = serde_json::from_slice(
        &std::fs::read(
            captured["reports"][0]["attachment"]["path"]
                .as_str()
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let entries = archive["log"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{archive}");
    assert_eq!(
        entries[0]["request"]["url"],
        "https://example.com/acme/page"
    );
    assert_eq!(archive["log"]["_testrunCapture"]["page"], json!(page));
    assert_eq!(archive["log"]["_testrunCapture"]["mode"], "full");
    let pageref = format!("page@{}", page.id);
    assert_eq!(entries[0]["pageref"], pageref);
    assert_eq!(archive["log"]["pages"][0]["id"], pageref);
    assert_eq!(entries[0]["response"]["httpVersion"], "HTTP/2.0");
    assert_eq!(entries[0]["serverIPAddress"], "192.0.2.1");
    assert_eq!(entries[0]["_serverPort"], 443);
    assert_eq!(entries[0]["_securityDetails"]["issuer"], "Acme CA");
    assert!(entries[0]["_securityDetails"].get("keyExchange").is_none());
    assert_eq!(entries[0]["response"]["cookies"][0]["name"], "id");
    assert_eq!(
        entries[0]["request"]["cookies"],
        json!([{"name":"acme","value":"one"},{"name":"theme","value":"dark"}])
    );
    assert!(worker.diagnostics.early_cookies.is_empty());
    assert_eq!(entries[0]["response"]["_transferSize"], 120);
    assert_eq!(entries[0]["timings"]["dns"], 1.0);
    assert_eq!(entries[0]["timings"]["connect"], 4.0);
    assert_eq!(entries[0]["timings"]["ssl"], 3.0);
    assert_eq!(entries[0]["timings"]["wait"], 10.0);
    let receive = entries[0]["timings"]["receive"].as_f64().unwrap();
    assert!((receive - 4.0).abs() < 0.01, "{receive}");
    let sibling = Reference {
        id: Uuid::new_v4(),
        ..page
    };
    worker
        .har_archives
        .capture_start(
            context,
            Some(sibling),
            (HarCaptureContent::Omit, HarCaptureMode::Minimal, None),
            None,
            None,
        )
        .unwrap();
    worker.diagnostic_event(&Event{method:"Network.requestWillBeSent".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"sibling-request","type":"Fetch","timestamp":1,"wallTime":2,"request":{"url":"https://example.com/acme/sibling","method":"GET","headers":{}}})});
    assert_eq!(
        worker.har_archives.capture_status(context).unwrap()["entries"],
        0
    );
}

#[test]
fn native_request_body_prefers_binary_entries_and_does_not_invent_opaque_bytes() {
    let request =
        json!({"hasPostData":true,"postData":"lossy text","postDataEntries":[{"bytes":"AP+A"}]});
    assert_eq!(
        super::super::routing::request_body(&request).unwrap(),
        Some(vec![0, 255, 128])
    );
    assert_eq!(
        super::super::routing::request_body(&json!({"hasPostData":true}))
            .unwrap_err()
            .code,
        "HAR_REQUEST_UNAVAILABLE"
    );
}

#[tokio::test]
async fn native_request_override_omits_unspecified_headers_and_body_on_the_wire() {
    let (mut worker, mut peer, _root, _, _) = owned_page().await;
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"override-acme","request":{"url":"https://example.com/acme","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
    let callback =
        serde_json::from_value(worker.interactions.snapshot()["callbacks"][0]["reference"].clone())
            .unwrap();
    let resolving = tokio::spawn(async move {
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::RouteContinueWith {
                    overrides: contract::RouteOverrides {
                        url: Some("https://example.com/rewritten".into()),
                        method: Some("GET".into()),
                        ..Default::default()
                    },
                },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let request = command(&mut peer, "Fetch.continueRequest").await;
    assert_eq!(
        request["params"],
        json!({"requestId":"override-acme","url":"https://example.com/rewritten","method":"GET"})
    );
    reply(&mut peer, &request, json!({})).await;
    let worker = resolving.await.unwrap();
    assert!(
        worker.interactions.snapshot()["callbacks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn native_route_response_keeps_paused_request_private_and_never_replays_a_fulfillment() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let set = tokio::spawn(async move {
        worker
            .page_route_set(
                Uuid::new_v4(),
                page,
                Some("https://example.com/acme".into()),
                false,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let c = command(&mut peer, "Fetch.enable").await;
    assert_eq!(c["params"]["patterns"][0]["requestStage"], "Request");
    reply(&mut peer, &c, json!({})).await;
    let mut worker = set.await.unwrap();
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"native-paused-acme","request":{"url":"https://example.com/acme?token=Acme-secret","method":"POST","headers":{"Authorization":"Acme-secret"},"postData":"Acme-secret","initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
    let snapshots = worker.interactions.snapshot();
    let callback = serde_json::from_value(snapshots["callbacks"][0]["reference"].clone()).unwrap();
    assert!(!snapshots.to_string().contains("Acme-secret"));
    assert!(!snapshots.to_string().contains("native-paused-acme"));
    assert_eq!(
        worker
            .page_route_set(Uuid::new_v4(), page, None, false, deadline())
            .await
            .unwrap_err()
            .code,
        "ROUTE_CALLBACKS_PENDING"
    );
    let resolve = tokio::spawn(async move {
        let result = worker
            .resolve_route(
                callback,
                contract::CallbackResponse::RouteFulfillBinary {
                    status: 201,
                    body_base64: "AP+A".into(),
                    headers: vec![
                        contract::RouteHeader {
                            name: "content-type".into(),
                            value: "application/octet-stream".into(),
                        },
                        contract::RouteHeader {
                            name: "x-acme".into(),
                            value: "first".into(),
                        },
                        contract::RouteHeader {
                            name: "x-acme".into(),
                            value: "second".into(),
                        },
                    ],
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let c = command(&mut peer, "Fetch.fulfillRequest").await;
    assert_eq!(c["params"]["requestId"], "native-paused-acme");
    assert_eq!(c["params"]["body"], "AP+A");
    assert_eq!(
        c["params"]["responseHeaders"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|header| header["name"] == "x-acme")
            .count(),
        2
    );
    assert_eq!(
        c["params"]["responseHeaders"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["value"],
        "3"
    );
    reply(&mut peer, &c, json!({})).await;
    let (mut worker, result) = resolve.await.unwrap();
    assert_eq!(result.unwrap()["resolved"], true);
    assert_eq!(
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::RouteContinue {},
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
async fn native_evidence_fulfillment_verifies_changed_files_and_sends_large_bodies_once() {
    use base64::Engine as _;
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let bytes = vec![42; crate::browser::routing::MAX_BINARY_BODY];
    let artifact = crate::browser::network::write_body(
        worker.journal.directory(),
        context,
        Uuid::new_v4(),
        crate::browser::network::BodySide::Response,
        &bytes,
        bytes.len(),
    )
    .unwrap();
    let summary = worker.network_evidence.insert(artifact).unwrap();
    let evidence_id = serde_json::from_value(summary["evidenceId"].clone()).unwrap();
    let path = summary["attachment"]["path"].as_str().unwrap();
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"native-evidence-acme","request":{"url":"https://example.com/replayed","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
    let callback =
        serde_json::from_value(worker.interactions.snapshot()["callbacks"][0]["reference"].clone())
            .unwrap();
    let response = move || contract::CallbackResponse::RouteFulfillEvidence {
        status: 201,
        network_evidence_id: evidence_id,
        headers: vec![],
    };
    std::fs::write(path, b"changed").unwrap();
    assert_eq!(
        worker
            .resolve_route(callback, response(), deadline())
            .await
            .unwrap_err()
            .code,
        "EVIDENCE_CHANGED"
    );
    assert_eq!(
        worker.interactions.snapshot()["callbacks"][0]["responding"],
        false
    );
    std::fs::write(path, &bytes).unwrap();
    let mut pending = tokio::spawn(async move {
        let result = worker.resolve_route(callback, response(), deadline()).await;
        (worker, result)
    });
    let c = tokio::select! {
        command = command(&mut peer, "Fetch.fulfillRequest") => command,
        finished = &mut pending => panic!("Evidence dispatch ended before wire: {:?}", finished.unwrap().1),
    };
    let sent = base64::engine::general_purpose::STANDARD
        .decode(c["params"]["body"].as_str().unwrap())
        .unwrap();
    assert_eq!(sent.len(), bytes.len());
    assert_eq!(Sha256::digest(&sent), Sha256::digest(&bytes));
    reply(&mut peer, &c, json!({})).await;
    let (mut worker, result) = pending.await.unwrap();
    assert_eq!(result.unwrap()["resolved"], true);
    assert_eq!(
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::RouteContinue {},
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

fn replay_archive(worker: &mut Worker, root: &tempfile::TempDir) -> Uuid {
    let input = root.path().join("har-import");
    crate::artifacts::directory(&input).unwrap();
    let path = input.join("source.har");
    let value = json!({"log":{"version":"1.2","entries":[{"request":{"url":"https://example.com/acme","method":"GET","headers":[]},"response":{"status":200,"headers":[],"content":{"text":"Acme HAR"}}}]}});
    crate::artifacts::write(&path, &serde_json::to_vec(&value).unwrap()).unwrap();
    serde_json::from_value(
        worker
            .har_archives
            .register(&path, worker.journal.directory())
            .unwrap()["archiveId"]
            .clone(),
    )
    .unwrap()
}

#[tokio::test]
async fn native_page_interception_propagates_to_future_frame_once_and_disables_every_attachment() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let setting = tokio::spawn(async move {
        let result = worker
            .page_route_set(
                Uuid::new_v4(),
                page,
                Some("https://example.com/acme".into()),
                false,
                deadline(),
            )
            .await;
        (worker, result)
    });
    let root = command(&mut peer, "Fetch.enable").await;
    reply(&mut peer, &root, json!({})).await;
    let (mut worker, result) = setting.await.unwrap();
    result.unwrap();
    let info = json!({"targetId":"route-frame","browserContextId":"context-acme","type":"iframe","title":"Acme","url":"https://example.com/frame","attached":true,"canAccessOpener":false});
    worker.apply(EventItem::Event(Event { method:"Target.attachedToTarget".into(),session_id:Some("session-acme".into()),params:json!({"sessionId":"session-route-frame","targetInfo":info,"waitingForDebugger":true})})).unwrap();
    let setting = tokio::spawn(async move {
        worker
            .configure_fetch(Uuid::new_v4(), "session-route-frame", deadline())
            .await
            .unwrap();
        worker
            .configure_fetch(Uuid::new_v4(), "session-route-frame", deadline())
            .await
            .unwrap();
        worker
    });
    let frame = command(&mut peer, "Fetch.enable").await;
    assert_eq!(frame["sessionId"], "session-route-frame");
    assert_eq!(frame["params"], root["params"]);
    reply(&mut peer, &frame, json!({})).await;
    let mut worker = setting.await.unwrap();
    let stopping = tokio::spawn(async move {
        worker
            .page_route_set(Uuid::new_v4(), page, None, false, deadline())
            .await
            .unwrap();
        worker
    });
    let mut disabled = BTreeSet::new();
    for _ in 0..2 {
        let command = command(&mut peer, "Fetch.disable").await;
        disabled.insert(command["sessionId"].as_str().unwrap().to_owned());
        reply(&mut peer, &command, json!({})).await;
    }
    assert_eq!(
        disabled,
        BTreeSet::from(["session-acme".into(), "session-route-frame".into()])
    );
    let mut worker = stopping.await.unwrap();
    worker
        .configure_fetch(Uuid::new_v4(), "session-route-frame", deadline())
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_uncertain_fetch_enable_is_not_repeated_and_uncertain_disable_waits_for_teardown() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let setting = tokio::spawn(async move {
        let result = worker
            .page_route_set(
                Uuid::new_v4(),
                page,
                Some("https://example.com/acme".into()),
                false,
                Instant::now() + Duration::from_secs(1),
            )
            .await;
        (worker, result)
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    let (mut worker, result) = setting.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    assert_eq!(
        worker
            .page_route_set(
                Uuid::new_v4(),
                page,
                Some("https://example.com/acme".into()),
                false,
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "INTERCEPTION_OUTCOME_UNKNOWN"
    );
    reply(&mut peer, &enable, json!({})).await;
    let stopping = tokio::spawn(async move {
        let result = worker
            .page_route_set(
                Uuid::new_v4(),
                page,
                None,
                false,
                Instant::now() + Duration::from_secs(1),
            )
            .await;
        (worker, result)
    });
    let disable = command(&mut peer, "Fetch.disable").await;
    let (mut worker, result) = stopping.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    reply(&mut peer, &disable, json!({})).await;
    assert_eq!(
        worker
            .page_route_set(Uuid::new_v4(), page, None, false, deadline())
            .await
            .unwrap_err()
            .code,
        "INTERCEPTION_OUTCOME_UNKNOWN"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_frame_startup_services_an_inflight_route_policy_before_its_parent_acknowledges() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    worker.startup.enabled = true;
    worker.startup.operation = Uuid::new_v4();
    let setting = tokio::spawn(async move {
        let result = worker
            .page_route_set(
                Uuid::new_v4(),
                page,
                Some("https://example.com/acme".into()),
                false,
                deadline(),
            )
            .await;
        (worker, result)
    });
    let outer = command(&mut peer, "Fetch.enable").await;
    let info = json!({"targetId":"route-startup-frame","browserContextId":"context-acme","type":"iframe","title":"Acme","url":"https://example.com/frame","attached":true,"canAccessOpener":false});
    send(&mut peer,json!({"method":"Target.attachedToTarget","sessionId":"session-acme","params":{"sessionId":"route-startup-session","targetInfo":info,"waitingForDebugger":true}})).await;
    let enable = command(&mut peer, "Page.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let tree = command(&mut peer, "Page.getFrameTree").await;
    reply(&mut peer,&tree,json!({"frameTree":{"frame":{"id":"route-startup-frame","loaderId":"Acme-frame-loader","url":"https://example.com/frame","domainAndRegistry":"example.com","securityOrigin":"https://example.com","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await;
    let runtime = command(&mut peer, "Runtime.enable").await;
    reply(&mut peer, &runtime, json!({})).await;
    let attach = command(&mut peer, "Target.setAutoAttach").await;
    reply(&mut peer, &attach, json!({})).await;
    let script = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    reply(
        &mut peer,
        &script,
        json!({"identifier":"Acme-frame-initialization"}),
    )
    .await;
    let chooser = command(&mut peer, "Page.setInterceptFileChooserDialog").await;
    reply(&mut peer, &chooser, json!({})).await;
    let fetch = command(&mut peer, "Fetch.enable").await;
    assert_eq!(fetch["sessionId"], "route-startup-session");
    assert_eq!(fetch["params"], outer["params"]);
    reply(&mut peer, &fetch, json!({})).await;
    let resume = command(&mut peer, "Runtime.runIfWaitingForDebugger").await;
    reply(&mut peer, &resume, json!({})).await;
    reply(&mut peer, &outer, json!({})).await;
    let (worker, result) = setting.await.unwrap();
    result.unwrap();
    assert!(worker.startup_summary()["failure"].is_null());
    assert_eq!(worker.startup_summary()["pausedTargets"], 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "Independent Page and Context policy lifetimes are exercised against the same paused transport"
)]
async fn native_context_and_page_routes_preserve_precedence_and_independent_cleanup() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let setting = tokio::spawn(async move {
        worker
            .scoped_route_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                Some("https://example.com/acme".into()),
                true,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let mut worker = setting.await.unwrap();
    worker
        .page_route_set(
            Uuid::new_v4(),
            page,
            Some("https://example.com/acme".into()),
            false,
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(
        worker.fetch_owner(context, Some(page), "https://example.com/acme", false),
        Some(page)
    );
    assert_eq!(
        worker.fetch_owner(context, Some(page), "https://example.com/acme", true),
        Some(context)
    );
    worker
        .page_route_set(Uuid::new_v4(), page, None, false, deadline())
        .await
        .unwrap();
    assert_eq!(
        worker.fetch_owner(context, Some(page), "https://example.com/acme", false),
        Some(context)
    );
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"context-acme-paused","request":{"url":"https://example.com/acme","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
    let callback = *worker.interactions.routes.keys().next().unwrap();
    assert_eq!(worker.interactions.routes[&callback].policy_scope, context);
    assert_eq!(
        worker
            .scoped_route_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                None,
                false,
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "ROUTE_CALLBACKS_PENDING"
    );
    worker
        .page_route_set(Uuid::new_v4(), page, None, false, deadline())
        .await
        .unwrap();
    let responding = tokio::spawn(async move {
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::RouteAbort { error_code: None },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let abort = command(&mut peer, "Fetch.failRequest").await;
    reply(&mut peer, &abort, json!({})).await;
    let mut worker = responding.await.unwrap();
    let stopping = tokio::spawn(async move {
        worker
            .scoped_route_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                None,
                false,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let disable = command(&mut peer, "Fetch.disable").await;
    reply(&mut peer, &disable, json!({})).await;
    let _worker = stopping.await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One native paused request verifies fallback identity, original expiry, retained policy owners and final continuation"
)]
async fn native_route_fallback_transfers_once_to_context_before_any_network_dispatch() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let setting = tokio::spawn(async move {
        worker
            .scoped_route_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                Some("https://example.com/acme".into()),
                false,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let mut worker = setting.await.unwrap();
    worker
        .page_route_set(
            Uuid::new_v4(),
            page,
            Some("https://example.com/acme".into()),
            false,
            deadline(),
        )
        .await
        .unwrap();
    // Handler selection must not depend on retaining an oversized HAR body.
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"fallback-acme","request":{"url":"https://example.com/acme","method":"POST","headers":{},"postData":"x".repeat(crate::browser::har::MAX_BODY+1),"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
    let original = *worker.interactions.routes.keys().next().unwrap();
    assert_eq!(worker.interactions.routes[&original].policy_scope, page);
    let expiry = worker.interactions.routes[&original].expires;
    let result = worker
        .resolve_callback(
            Uuid::new_v4(),
            original,
            contract::CallbackResponse::RouteFallback {},
            deadline(),
        )
        .await
        .unwrap();
    let next = serde_json::from_value(result["nextCallback"].clone()).unwrap();
    assert_ne!(original, next);
    assert!(!worker.interactions.routes.contains_key(&original));
    assert_eq!(worker.interactions.routes[&next].policy_scope, context);
    assert_eq!(worker.interactions.routes[&next].expires, expiry);
    assert_eq!(
        worker
            .page_har_set(
                Uuid::new_v4(),
                page,
                None,
                contract::HarNotFound::Abort,
                None,
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "ROUTE_CALLBACKS_PENDING"
    );
    assert_eq!(
        worker
            .page_route_set(Uuid::new_v4(), page, None, false, deadline())
            .await
            .unwrap_err()
            .code,
        "ROUTE_CALLBACKS_PENDING"
    );
    assert_eq!(
        worker
            .resolve_route(
                original,
                contract::CallbackResponse::RouteAbort { error_code: None },
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
    let continuing = tokio::spawn(async move {
        worker
            .resolve_callback(
                Uuid::new_v4(),
                next,
                contract::CallbackResponse::RouteFallback {},
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let request = command(&mut peer, "Fetch.continueRequest").await;
    assert_eq!(request["params"]["requestId"], "fallback-acme");
    reply(&mut peer, &request, json!({})).await;
    let worker = continuing.await.unwrap();
    assert!(worker.interactions.routes.is_empty());
}

#[tokio::test]
async fn native_closing_context_route_events_do_not_allocate_callbacks_or_lose_the_session() {
    let (mut worker, _peer, _root, context, _page) = owned_page().await;
    worker.index.mark_closing(context).unwrap();
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"late-closing-context","request":{"url":"https://example.com/acme","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
    assert!(worker.interactions.routes.is_empty());
    assert!(worker.index.retained(context, Kind::Context));
    assert_eq!(worker.shared.state.lock().unwrap().phase, Phase::Ready);
}

#[tokio::test]
async fn native_clear_scope_har_retains_failed_teardown_and_preserves_siblings() {
    use crate::browser::routing::HandlerAction;
    for context_owned in [false, true] {
        let (mut worker, mut peer, root, context, page) = owned_page().await;
        let archive = replay_archive(&mut worker, &root);
        let scope = if context_owned { context } else { page };
        let sibling = if context_owned { page } else { context };
        let kind = if context_owned {
            Kind::Context
        } else {
            Kind::Page
        };
        let setup = tokio::spawn(async move {
            worker
                .scoped_har_set(
                    Uuid::new_v4(),
                    scope,
                    kind,
                    Some(archive),
                    contract::HarNotFound::Abort,
                    None,
                    deadline(),
                )
                .await
                .unwrap();
            worker
        });
        let enable = command(&mut peer, "Fetch.enable").await;
        reply(&mut peer, &enable, json!({})).await;
        let mut worker = setup.await.unwrap();
        worker.routing_rules.mark_manual(sibling, true);
        // The sibling's logical owner must survive either result. No native
        // sibling configuration is needed for this teardown-failure fixture.
        for fail in [true, false] {
            let clearing = tokio::spawn(async move {
                let result = worker
                    .network_route(
                        Uuid::new_v4(),
                        scope,
                        HandlerAction::Clear { behavior: None },
                        deadline(),
                    )
                    .await;
                (worker, result)
            });
            let disable = command(&mut peer, "Fetch.disable").await;
            if fail {
                send(&mut peer, json!({"id":disable["id"],"sessionId":disable["sessionId"],"error":{"code":-32000,"message":"Acme teardown refused"}})).await;
            } else {
                reply(&mut peer, &disable, json!({})).await;
            }
            let (next, result) = clearing.await.unwrap();
            worker = next;
            assert!(worker.routing_rules.manual(sibling));
            if fail {
                assert!(result.is_err());
                assert!(worker.routing_rules.automatic(scope).is_some());
                assert_eq!(
                    worker.har_remove(archive).unwrap_err().code,
                    "HAR_ARCHIVE_IN_USE"
                );
            } else {
                assert!(result.unwrap()["handlers"].as_array().unwrap().is_empty());
                assert!(worker.routing_rules.automatic(scope).is_none());
                worker.har_remove(archive).unwrap();
            }
        }
    }
}

#[tokio::test]
async fn native_context_har_release_preserves_a_page_owned_paused_request() {
    let (mut worker, mut peer, root, context, page) = owned_page().await;
    let archive = replay_archive(&mut worker, &root);
    let setting = tokio::spawn(async move {
        worker
            .scoped_har_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                Some(archive),
                contract::HarNotFound::Continue,
                None,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let mut worker = setting.await.unwrap();
    assert_eq!(worker.har_list()["routes"][0]["scopeKind"], "context");
    let manual = tokio::spawn(async move {
        worker
            .page_route_set(
                Uuid::new_v4(),
                page,
                Some("https://example.com/acme".into()),
                false,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let mut worker = manual.await.unwrap();
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"page-owned-beside-context-har","request":{"url":"https://example.com/acme","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
    let callback = *worker.interactions.routes.keys().next().unwrap();
    assert_eq!(worker.interactions.routes[&callback].policy_scope, page);
    worker.poll_har_routes().await;
    let stopping = tokio::spawn(async move {
        worker
            .scoped_har_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                None,
                contract::HarNotFound::Abort,
                None,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    assert_eq!(
        enable["params"]["patterns"],
        json!([{"urlPattern":"https://example.com/acme","requestStage":"Request"}])
    );
    reply(&mut peer, &enable, json!({})).await;
    let mut worker = stopping.await.unwrap();
    assert!(worker.interactions.routes.contains_key(&callback));
    worker.har_remove(archive).unwrap();
    let responding = tokio::spawn(async move {
        worker
            .resolve_route(
                callback,
                contract::CallbackResponse::RouteAbort { error_code: None },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let abort = command(&mut peer, "Fetch.failRequest").await;
    reply(&mut peer, &abort, json!({})).await;
    let _worker = responding.await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_uncertain_har_setup_keeps_its_archive_and_teardown_does_not_replay_enable() {
    let (mut worker, mut peer, root, _, page) = owned_page().await;
    let archive_id = replay_archive(&mut worker, &root);
    let setup = tokio::spawn(async move {
        let result = worker
            .page_har_set(
                Uuid::new_v4(),
                page,
                Some(archive_id),
                contract::HarNotFound::Abort,
                None,
                Instant::now() + Duration::from_secs(1),
            )
            .await;
        (worker, result)
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    let (mut worker, result) = setup.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    assert_eq!(worker.har_list()["routes"][0]["phase"], "installing");
    assert_eq!(
        worker.har_remove(archive_id).unwrap_err().code,
        "HAR_ARCHIVE_IN_USE"
    );
    reply(&mut peer, &enable, json!({})).await;
    let stop = tokio::spawn(async move {
        let result = worker
            .page_har_set(
                Uuid::new_v4(),
                page,
                None,
                contract::HarNotFound::Abort,
                None,
                deadline(),
            )
            .await;
        (worker, result)
    });
    let disable = command(&mut peer, "Fetch.disable").await;
    reply(&mut peer, &disable, json!({})).await;
    let (mut worker, result) = stop.await.unwrap();
    assert_eq!(result.unwrap()["enabled"], false);
    assert!(worker.har_list()["routes"].as_array().unwrap().is_empty());
    worker.har_remove(archive_id).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_har_filter_selects_context_and_privately_continues_nonmatches() {
    let (mut worker, mut peer, root, context, page) = owned_page().await;
    let archive = replay_archive(&mut worker, &root);
    let setting = tokio::spawn(async move {
        worker
            .scoped_har_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                Some(archive),
                contract::HarNotFound::Abort,
                Some(crate::browser::routing::Matcher::Regex {
                    pattern: r"example[.]com/(?:acme|context)$".into(),
                    flags: String::new(),
                }),
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let mut worker = setting.await.unwrap();
    worker
        .page_har_set(
            Uuid::new_v4(),
            page,
            Some(archive),
            contract::HarNotFound::Abort,
            Some(crate::browser::routing::Matcher::Glob {
                pattern: "**/page*".into(),
            }),
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(
        worker.fetch_owner(context, Some(page), "https://example.com/acme", false),
        Some(context)
    );
    assert_eq!(
        worker.fetch_owner(context, Some(page), "https://example.com/page", false),
        Some(page)
    );
    assert_eq!(
        worker.fetch_owner(context, Some(page), "https://example.com/other", false),
        None
    );
    assert_eq!(
        worker
            .context_fetch_fallback(page, page, Some("https://example.com/acme"))
            .unwrap(),
        Some(context)
    );
    assert_eq!(
        worker
            .context_fetch_fallback(page, page, Some("https://example.com/other"))
            .unwrap(),
        None
    );
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"filtered-nonmatch","request":{"url":"https://example.com/other","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
    assert!(
        worker
            .interactions
            .routes
            .values()
            .next()
            .unwrap()
            .auto_pass
    );
    let servicing = tokio::spawn(async move {
        worker.poll_har_routes().await;
        worker
    });
    let continued = command(&mut peer, "Fetch.continueRequest").await;
    assert_eq!(continued["params"]["requestId"], "filtered-nonmatch");
    reply(&mut peer, &continued, json!({})).await;
    let worker = servicing.await.unwrap();
    assert!(worker.interactions.routes.is_empty());
    assert_eq!(worker.routing_rules.automatic(context).unwrap().replies, 0);
    assert_eq!(worker.routing_rules.automatic(page).unwrap().replies, 0);
}
#[tokio::test]
async fn native_automatic_har_keeps_dialog_replies_responsive_while_fulfillment_ack_is_held() {
    let (mut worker, mut peer, root, _, page) = owned_page().await;
    let archive_id = replay_archive(&mut worker, &root);
    let rule = worker
        .routing_rules
        .begin(page, archive_id, contract::HarNotFound::Abort)
        .unwrap();
    worker.routing_rules.confirm(page, rule.id).unwrap();
    worker.apply(EventItem::Event(Event{method:"Fetch.requestPaused".into(),session_id:Some("session-acme".into()),params:json!({"requestId":"native-paused-acme","request":{"url":"https://example.com/acme","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"})})).unwrap();
    let shared = worker.shared.clone();
    let (tx, rx) = mpsc::channel(8);
    worker.responses = Some(rx);
    let replay = tokio::spawn(async move {
        worker.poll_har_routes().await;
        worker
    });
    let fulfill = command(&mut peer, "Fetch.fulfillRequest").await;
    send(&mut peer,json!({"method":"Page.javascriptDialogOpening","sessionId":"session-acme","params":{"url":"https://example.com/acme","frameId":"frame-acme","message":"Acme HAR dialog","type":"alert","hasBrowserHandler":true}})).await;
    let until = deadline();
    let dialog = loop {
        let found = shared.details.lock().unwrap()["callbacks"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["kind"] == "dialog"))
            .cloned();
        if let Some(dialog) = found {
            break serde_json::from_value(dialog["reference"].clone()).unwrap();
        }
        assert!(
            Instant::now() < until,
            "Dialog was not indexed during HAR fulfillment"
        );
        tokio::task::yield_now().await;
    };
    let response = response_operation(
        &shared,
        &tx,
        Request::CallbackResolve {
            callback: dialog,
            response: contract::CallbackResponse::Dialog {
                accept: true,
                prompt_text: None,
            },
        },
    )
    .await;
    let accept = command(&mut peer, "Page.handleJavaScriptDialog").await;
    reply(&mut peer, &accept, json!({})).await;
    assert_eq!(
        wait_response(&shared, response).await.outcome,
        Outcome::Succeeded
    );
    assert!(!replay.is_finished());
    reply(&mut peer, &fulfill, json!({})).await;
    let mut worker = replay.await.unwrap();
    assert!(worker.interactions.routes.is_empty());
    assert_eq!(worker.har_list()["routes"][0]["replies"], 1);
    assert_eq!(worker.har_list()["routes"][0]["failures"], 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    worker.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn native_init_script_registration_is_private_and_removal_uses_its_original_identifier() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let add = tokio::spawn(async move {
        let result = worker
            .init_script(
                Uuid::new_v4(),
                Request::PageInitScriptAdd {
                    page,
                    source: "globalThis.acmeSecret=42".into(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let registration = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    assert_eq!(registration["params"]["runImmediately"], false);
    reply(
        &mut peer,
        &registration,
        json!({"identifier":"native-acme-script"}),
    )
    .await;
    let (mut worker, result) = add.await.unwrap();
    let result = result.unwrap();
    let script_id = serde_json::from_value(result["scriptId"].clone()).unwrap();
    let list = worker
        .init_script(
            Uuid::new_v4(),
            Request::PageInitScriptList { page },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(list["scripts"][0]["state"], "registered");
    assert!(!list.to_string().contains("acmeSecret"));
    assert!(!list.to_string().contains("native-acme-script"));
    let remove = tokio::spawn(async move {
        let result = worker
            .init_script(
                Uuid::new_v4(),
                Request::PageInitScriptRemove { page, script_id },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let removal = command(&mut peer, "Page.removeScriptToEvaluateOnNewDocument").await;
    assert_eq!(removal["params"]["identifier"], "native-acme-script");
    reply(&mut peer, &removal, json!({})).await;
    let (mut worker, result) = remove.await.unwrap();
    assert_eq!(result.unwrap()["removed"], true);
    assert!(
        worker
            .init_script(
                Uuid::new_v4(),
                Request::PageInitScriptList { page },
                deadline()
            )
            .await
            .unwrap()["scripts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

async fn response_operation(
    shared: &Arc<Shared>,
    sender: &mpsc::Sender<Command>,
    request: Request,
) -> Uuid {
    let id = Uuid::new_v4();
    shared
        .state
        .lock()
        .unwrap()
        .begin(id, "fixture".into())
        .unwrap();
    sender
        .send(Command {
            queue_slot: None,
            id,
            request: Some(request),
            deadline: deadline(),
        })
        .await
        .unwrap();
    id
}
async fn wait_response(shared: &Arc<Shared>, id: Uuid) -> Operation {
    let until = deadline();
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
            .filter(|o| o.settled)
            .cloned()
        {
            return operation;
        }
        tokio::time::timeout_at(until, changed).await.unwrap();
    }
}

#[tokio::test]
async fn native_developer_pending_attachment_cannot_be_discarded_and_close_is_idempotent() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let shared = worker.shared.clone();
    let (tx, rx) = mpsc::channel(8);
    worker.responses = Some(rx);
    let opening = tokio::spawn(async move {
        let result = worker
            .developer_open(Uuid::new_v4(), page, None, deadline())
            .await;
        (worker, result)
    });
    let get = command(&mut peer, "Target.getDevToolsTarget").await;
    reply(&mut peer, &get, json!({})).await;
    let open = command(&mut peer, "Target.openDevTools").await;
    reply(&mut peer, &open, json!({"targetId":"devtools-acme"})).await;
    let attach = command(&mut peer, "Target.attachToTarget").await;
    send(&mut peer,json!({"method":"Target.attachedToTarget","params":{"sessionId":"devtools-session","targetInfo":{"targetId":"devtools-acme","type":"other","title":"Acme","url":"devtools://devtools/bundled/inspector.html","attached":true,"canAccessOpener":false},"waitingForDebugger":false}})).await;
    let until = deadline();
    let developer = loop {
        if let Some(value) = shared.details.lock().unwrap()["developers"]
            .as_array()
            .and_then(|items| items.first())
            .cloned()
        {
            break serde_json::from_value(value["developer"].clone()).unwrap();
        }
        assert!(Instant::now() < until);
        tokio::task::yield_now().await;
    };
    let closing = response_operation(&shared, &tx, Request::DeveloperClose { developer }).await;
    assert_eq!(
        wait_response(&shared, closing).await.code.as_deref(),
        Some("DEVELOPER_BUSY")
    );
    reply(&mut peer, &attach, json!({"sessionId":"devtools-session"})).await;
    let enable = command(&mut peer, "Runtime.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let ready = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &ready,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let models = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &models,
        json!({"result":{"type":"object","value":{"nativeFrontend":true,"modules":{}}}}),
    )
    .await;
    let (mut worker, result) = opening.await.unwrap();
    assert_eq!(result.unwrap()["developer"], json!(developer));
    let closing = tokio::spawn(async move {
        let result = worker.developer_close(developer, deadline()).await;
        (worker, result)
    });
    let detach = command(&mut peer, "Target.detachFromTarget").await;
    reply(&mut peer, &detach, json!({})).await;
    let close = command(&mut peer, "Target.closeTarget").await;
    reply(&mut peer, &close, json!({"success":true})).await;
    let (mut worker, result) = closing.await.unwrap();
    assert_eq!(result.unwrap()["closed"], true);
    assert_eq!(
        worker.developer_close(developer, deadline()).await.unwrap()["closed"],
        true
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_breakpoint_reply_after_acknowledged_disable_keeps_the_debugger_released() {
    use crate::browser::debugging::{Action, Exceptions};
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let shared = worker.shared.clone();
    let (tx, rx) = mpsc::channel(8);
    worker.responses = Some(rx);
    let enable = tokio::spawn(async move {
        worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                Action::Enable {
                    exceptions: Exceptions::None,
                },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let c = command(&mut peer, "Debugger.enable").await;
    reply(&mut peer, &c, json!({"debuggerId":"debugger-acme"})).await;
    let c = command(&mut peer, "Debugger.setPauseOnExceptions").await;
    reply(&mut peer, &c, json!({})).await;
    let mut worker = enable.await.unwrap();
    let setting = tokio::spawn(async move {
        let result = worker
            .page_debugger(
                Uuid::new_v4(),
                page,
                Action::BreakpointSet {
                    url: "https://example.com/acme.js".into(),
                    line: 0,
                    column: 0,
                    condition: None,
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let set = command(&mut peer, "Debugger.setBreakpointByUrl").await;
    let disable = response_operation(
        &shared,
        &tx,
        Request::PageDebugger {
            page,
            action: Action::Disable,
        },
    )
    .await;
    let c = command(&mut peer, "Debugger.disable").await;
    reply(&mut peer, &c, json!({})).await;
    assert_eq!(
        wait_response(&shared, disable).await.outcome,
        Outcome::Succeeded
    );
    reply(
        &mut peer,
        &set,
        json!({"breakpointId":"breakpoint-acme","locations":[]}),
    )
    .await;
    let (worker, result) = setting.await.unwrap();
    assert_eq!(result.unwrap_err().code, "DEBUGGER_NOT_ENABLED");
    assert_eq!(
        worker.debugger_status(page, false).unwrap()["enabled"],
        false
    );
}

#[tokio::test]
async fn native_trace_late_completion_retains_stream_without_repeating_end() {
    for source_loss in [false, true] {
        let (mut worker, mut peer, _root, context, _) = owned_page().await;
        let start = tokio::spawn(async move {
            worker
                .trace_start(Uuid::new_v4(), context, Default::default(), deadline())
                .await
                .unwrap();
            worker
        });
        let start_command = command(&mut peer, "Tracing.start").await;
        assert_eq!(start_command["params"]["transferMode"], "ReturnAsStream");
        reply(&mut peer, &start_command, json!({})).await;
        let mut worker = start.await.unwrap();
        let original = worker.trace_status()["traceId"].clone();
        let stop = tokio::spawn(async move {
            let result = worker
                .trace_stop(Instant::now() + Duration::from_millis(100))
                .await;
            (worker, result)
        });
        let end = command(&mut peer, "Tracing.end").await;
        reply(&mut peer, &end, json!({})).await;
        let (mut worker, result) = stop.await.unwrap();
        assert_eq!(result.unwrap_err().code, "TRACE_FINALIZATION_PENDING");
        assert_eq!(worker.trace_status()["traceId"], original);
        if source_loss {
            worker.apply(EventItem::Gap { dropped: 1 }).unwrap();
        }
        worker.apply(EventItem::Event(Event{method:"Tracing.tracingComplete".into(),session_id:Some("session-acme".into()),params:json!({"dataLossOccurred":false,"stream":"stream-acme","traceFormat":"json","streamCompression":"none"})})).unwrap();
        let stop = tokio::spawn(async move {
            let result = worker.trace_stop(deadline()).await;
            (worker, result)
        });
        let read = command(&mut peer, "IO.read").await;
        reply(
            &mut peer,
            &read,
            json!({"data":"{\"traceEvents\":[]}","eof":true}),
        )
        .await;
        let close = command(&mut peer, "IO.close").await;
        reply(&mut peer, &close, json!({})).await;
        let (mut worker, result) = stop.await.unwrap();
        let value = if source_loss {
            let error = result.unwrap_err();
            assert_eq!(error.code, "TRACE_INCOMPLETE");
            error.partial.unwrap()
        } else {
            result.unwrap()
        };
        assert_eq!(value["report"]["traceId"], original);
        assert_eq!(value["report"]["complete"], !source_loss);
        assert_eq!(
            worker.trace_status()["state"],
            if source_loss {
                "incomplete"
            } else {
                "complete"
            }
        );
        let again = worker.trace_stop(deadline()).await;
        let repeated = if source_loss {
            let error = again.unwrap_err();
            assert_eq!(error.code, "TRACE_INCOMPLETE");
            error.partial.unwrap()
        } else {
            again.unwrap()
        };
        assert_eq!(repeated, value);
        let path = value["report"]["attachment"]["path"].as_str().unwrap();
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "{\"traceEvents\":[]}"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_profile_reservation_preserves_finalization_when_evidence_table_fills() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let start = tokio::spawn(async move {
        worker
            .profile_start(Uuid::new_v4(), page, Default::default(), deadline())
            .await
            .unwrap();
        worker
    });
    for method in [
        "Profiler.enable",
        "Profiler.setSamplingInterval",
        "Profiler.start",
    ] {
        let c = command(&mut peer, method).await;
        reply(&mut peer, &c, json!({})).await;
    }
    let mut worker = start.await.unwrap();
    for _ in 0..31 {
        let artifact = crate::browser::artifacts::Format {
            prefix: "acme",
            extension: "json",
            mime: "application/json",
            name: "fixture",
            max_bytes: 2,
        }
        .write(worker.journal.directory(), b"{}", json!({}))
        .unwrap();
        worker.developers.evidence.insert(artifact).unwrap();
    }
    assert_eq!(
        worker.developers.evidence.capacity(0).unwrap_err().code,
        "EVIDENCE_CAPACITY"
    );
    let stop = tokio::spawn(async move {
        let result = worker.profile_stop(page, deadline()).await;
        (worker, result)
    });
    let cpu = command(&mut peer, "Profiler.stop").await;
    reply(
        &mut peer,
        &cpu,
        json!({"profile":{"nodes":[],"startTime":1,"endTime":2}}),
    )
    .await;
    let disable = command(&mut peer, "Profiler.disable").await;
    reply(&mut peer, &disable, json!({})).await;
    let (worker, result) = stop.await.unwrap();
    let value = result.unwrap();
    assert_eq!(value["reports"].as_array().unwrap().len(), 1);
    assert_eq!(value["reports"][0]["format"], "v8-cpu-profile");
    assert_eq!(worker.profile_status(page).unwrap()["state"], "idle");
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Finalization repeated a native command"
    );
}

#[tokio::test]
async fn native_trace_transport_loss_preserves_an_explicit_prefix_on_owned_teardown() {
    let (mut worker, mut peer, _root, context, _) = owned_page().await;
    let start = tokio::spawn(async move {
        worker
            .trace_start(Uuid::new_v4(), context, Default::default(), deadline())
            .await
            .unwrap();
        worker
    });
    let c = command(&mut peer, "Tracing.start").await;
    reply(&mut peer, &c, json!({})).await;
    let mut worker = start.await.unwrap();
    worker
        .apply(EventItem::Event(Event {
            method: "Tracing.tracingComplete".into(),
            session_id: Some("session-acme".into()),
            params: json!({"dataLossOccurred":false,"stream":"stream-acme"}),
        }))
        .unwrap();
    let stop = tokio::spawn(async move {
        let result = worker.trace_stop(deadline()).await;
        (worker, result)
    });
    // Completion preceded stop: no second Tracing.end is admitted.
    let read = command(&mut peer, "IO.read").await;
    let prefix = "{\"traceEvents\":[{\"name\":\"Acme\"}";
    reply(&mut peer, &read, json!({"data":prefix,"eof":false})).await;
    let _read = command(&mut peer, "IO.read").await;
    peer.close(None).await.unwrap();
    drop(peer);
    let (mut worker, result) = stop.await.unwrap();
    assert!(result.is_err());
    worker.close(deadline()).await.unwrap();
    let evidence = worker.developers.evidence.list();
    let reports = evidence.as_array().unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["format"], "chrome-trace-event-json-prefix");
    assert_eq!(reports[0]["complete"], false);
    assert_eq!(
        std::fs::read_to_string(reports[0]["attachment"]["path"].as_str().unwrap()).unwrap(),
        prefix
    );
}

#[tokio::test]
async fn native_new_worker_startup_console_does_not_replay_existing_target_history() {
    use crate::browser::diagnostics::Query;
    let (mut worker, _peer, _root, context, _) = owned_page().await;
    worker.diagnostics.history.enable(context, true);
    worker.configured.remove("session-acme");
    worker.diagnostic_event(&Event{method:"Runtime.consoleAPICalled".into(),session_id:Some("session-acme".into()),params:json!({"type":"log","args":[{"type":"string","value":"Acme old page"}],"executionContextId":1,"timestamp":1})});
    let info = json!({"targetId":"acme-service","browserContextId":"context-acme","type":"service_worker","title":"Acme","url":"https://example.com/service.js","attached":false,"canAccessOpener":false});
    worker
        .apply(EventItem::Event(Event {
            method: "Target.targetCreated".into(),
            session_id: None,
            params: json!({"targetInfo":info}),
        }))
        .unwrap();
    worker.apply(EventItem::Event(Event{method:"Target.attachedToTarget".into(),session_id:None,params:json!({"sessionId":"acme-service-session","targetInfo":info,"waitingForDebugger":false})})).unwrap();
    let service = worker.index.target_reference("acme-service").unwrap();
    worker.diagnostic_event(&Event{method:"Runtime.consoleAPICalled".into(),session_id:Some("acme-service-session".into()),params:json!({"type":"log","args":[{"type":"string","value":"Acme worker startup"}],"executionContextId":1,"timestamp":2})});
    let history = worker
        .diagnostics_read(
            context,
            &Query {
                include_text: true,
                ..Default::default()
            },
        )
        .unwrap();
    let events = history["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["data"]["text"], "Acme worker startup");
    assert_eq!(events[0]["data"]["worker"], json!(service));
    assert!(events[0]["page"].is_null());
    assert_eq!(
        events[0]["data"]["subscriptionBoundary"],
        "new-worker-observed-during-subscription"
    );
}

#[tokio::test]
async fn native_reconcile_witnessed_worker_termination_keeps_the_sibling_page_ready() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    let info = json!({"targetId":"acme-short-worker","browserContextId":"context-acme","type":"service_worker","title":"Acme","url":"https://example.com/short.js","attached":false,"canAccessOpener":false});
    worker
        .apply(EventItem::Event(Event {
            method: "Target.targetCreated".into(),
            session_id: None,
            params: json!({"targetInfo":info}),
        }))
        .unwrap();
    let setup = tokio::spawn(async move {
        let result = worker.reconcile(Uuid::new_v4(), deadline()).await;
        (worker, result)
    });
    let c = command(&mut peer, "Target.attachToTarget").await;
    reply(&mut peer, &c, json!({"sessionId":"short-worker-session"})).await;
    let enable = command(&mut peer, "Runtime.enable").await;
    send(
        &mut peer,
        json!({"method":"Target.targetDestroyed","params":{"targetId":"acme-short-worker"}}),
    )
    .await;
    send(&mut peer,json!({"id":enable["id"],"sessionId":"short-worker-session","error":{"code":-32000,"message":"Session closed"}})).await;
    let (worker, result) = setup.await.unwrap();
    result.unwrap();
    assert!(worker.index.route(page, Some(Kind::Page)).is_ok());
    assert_eq!(worker.shared.state.lock().unwrap().phase, Phase::Ready);
}
