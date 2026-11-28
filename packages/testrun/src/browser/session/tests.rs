//! Controlled real-pipe session fixtures exercise ownership and wire dispatch.
mod automation_debugger;
mod certificate_paths;
mod context_proxy;
mod diagnostic_arguments;
#[cfg(all(feature = "acceptance-browser-public", target_os = "macos"))]
mod driver_close;
mod failure_evidence;
mod frame_element;
mod frame_relationships;
mod header_defaults;
mod http_credentials;
mod http_disposal;
mod http_request_inputs;
mod http_socks;
mod http_storage_state;
mod http_user_agent;
mod network_response;
#[cfg(all(feature = "acceptance-browser-public", target_os = "macos"))]
mod profile_startup;
mod route_clear;
mod route_limits;
mod selector_evaluation;
mod tracing;
mod worker_lifetime;

use super::*;
use crate::browser::{
    connection::tests::{Fixture, fixture},
    contract::{ContextOptions, Engine, Mode},
    dispatcher::{
        Event,
        tests::{created, method_error},
    },
    events::Item,
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "Each scoped transport cell follows matcher removal through acknowledgement or failure and explicit recovery"
)]
async fn matcher_removal_keeps_siblings_and_failed_updates_owned_in_both_scopes() {
    use crate::browser::routing::{HandlerAction, Matcher};
    for context_owned in [false, true] {
        for fail in [true, false] {
            let (session, mut peer, _directory, context, page) = harness().await;
            let scope = if context_owned { context } else { page };
            let matcher = Matcher::Glob {
                pattern: "https://example.com/acme".into(),
            };
            {
                let mut rules = session.0.host.routing.rules.lock().unwrap();
                rules.mark_manual(scope, true);
                rules
                    .handlers
                    .add(scope, "https://example.com/acme".into())
                    .unwrap();
                for _ in 0..2 {
                    rules.handlers.add_matcher(scope, matcher.clone()).unwrap();
                }
            }
            let removing = session
                .submit(
                    Uuid::new_v4(),
                    Request::NetworkRoute {
                        scope,
                        action: HandlerAction::RemoveMatcher {
                            matcher: matcher.clone(),
                        },
                    },
                    5000,
                )
                .unwrap();
            let update = peer.request().await;
            assert_eq!(update["method"], "setNetworkInterceptionPatterns");
            assert_eq!(
                update["guid"],
                if context_owned { "context" } else { "page" }
            );
            assert_eq!(
                update["params"]["patterns"],
                json!([{"regexSource":"^https://example\\.com/acme$"}])
            );
            assert_eq!(
                session
                    .0
                    .host
                    .routing
                    .rules
                    .lock()
                    .unwrap()
                    .handlers
                    .len(scope),
                3
            );
            if fail {
                peer.send(&method_error(update["id"].clone())).await;
            } else {
                peer.send(&json!({"id":update["id"],"result":{}})).await;
            }
            let result = complete(&session, removing).await;
            {
                let rules = session.0.host.routing.rules.lock().unwrap();
                if fail {
                    assert_eq!(result.code.as_deref(), Some("BROWSER_METHOD_FAILED"));
                    assert_eq!(rules.handlers.len(scope), 3);
                    assert!(rules.handlers.pending(scope));
                } else {
                    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
                    assert_eq!(rules.handlers.len(scope), 1);
                    assert!(!rules.handlers.pending(scope));
                }
            }
            if !fail {
                let result = session
                    .0
                    .host
                    .network_route(scope, HandlerAction::RemoveMatcher { matcher }, deadline())
                    .await
                    .unwrap();
                assert_eq!(result["handlers"].as_array().unwrap().len(), 1);
                assert!(
                    tokio::time::timeout(Duration::from_millis(25), peer.request())
                        .await
                        .is_err()
                );
            } else {
                assert_eq!(
                    session
                        .0
                        .host
                        .network_route(scope, HandlerAction::RemoveMatcher { matcher }, deadline())
                        .await
                        .unwrap_err()
                        .code,
                    "ROUTE_POLICY_PENDING"
                );
            }
            let clearing = session
                .submit(
                    Uuid::new_v4(),
                    Request::NetworkRoute {
                        scope,
                        action: HandlerAction::Clear { behavior: None },
                    },
                    5000,
                )
                .unwrap();
            let update = peer.request().await;
            assert_eq!(update["method"], "setNetworkInterceptionPatterns");
            assert_eq!(update["params"]["patterns"], json!([]));
            peer.send(&json!({"id":update["id"],"result":{}})).await;
            assert_eq!(
                complete(&session, clearing).await.outcome,
                Outcome::Succeeded
            );
            close(&session).await;
        }
    }
}

#[tokio::test]
async fn route_handler_fallback_keeps_one_wire_and_original_expiry_through_three_handlers() {
    for context_owned in [false, true] {
        let (session, mut peer, _directory, context, page) = harness().await;
        let scope = if context_owned { context } else { page };
        let url = "https://example.com/acme?item=1";
        let host = session.0.host.clone();
        host.routing
            .manual_urls
            .lock()
            .unwrap()
            .insert(scope, url.into());
        let mut ids = Vec::new();
        {
            let mut rules = host.routing.rules.lock().unwrap();
            rules.mark_manual(scope, true);
            for _ in 0..3 {
                ids.push(rules.handlers.add(scope, url.into()).unwrap());
            }
        }
        peer.send(&created("chain-request", "Request", json!({"frame":{"guid":"frame"},"url":url,"resourceType":"fetch","method":"GET","headers":[],"isNavigationRequest":false}))).await;
        peer.send(&created(
            "chain-route",
            "Route",
            json!({"request":{"guid":"chain-request"}}),
        ))
        .await;
        peer.send(&json!({"guid":if context_owned {"context"} else {"page"},"method":"route","params":{"route":{"guid":"chain-route"}}}))
        .await;
        let original = pending_callback(&session).await;
        let expiry = host.callbacks.lock().unwrap()[&original];
        let mut current = original;
        for expected in ids.iter().rev() {
            assert_eq!(
                host.routing.rules.lock().unwrap().handlers.current(current),
                Some(*expected)
            );
            assert_eq!(host.callbacks.lock().unwrap()[&current], expiry);
            if *expected != ids[0] {
                let response = host
                    .callback_resolve(current, CallbackResponse::RouteFallback {}, deadline())
                    .await
                    .unwrap();
                current = serde_json::from_value(response["nextCallback"].clone()).unwrap();
            }
        }
        assert_eq!(
            host.network_route(
                scope,
                crate::browser::routing::HandlerAction::Clear { behavior: None },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
            "ROUTE_CALLBACKS_PENDING"
        );
        if context_owned {
            assert!(host.policy_routes(page).is_empty());
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.request())
                .await
                .is_err()
        );
        host.callbacks
            .lock()
            .unwrap()
            .insert(current, Instant::now());
        let expiring = tokio::spawn(async move {
            host.expire_callback(original).await;
        });
        let request = peer.request().await;
        assert_eq!(request["guid"], "chain-route");
        assert_eq!(request["method"], "abort");
        peer.send(&json!({"id":request["id"],"result":{}})).await;
        expiring.await.unwrap();
        assert!(
            !session
                .0
                .host
                .callbacks
                .lock()
                .unwrap()
                .contains_key(&current)
        );
        close(&session).await;
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One request handoff preserves accumulated overrides and proves exactly one terminal continuation"
)]
async fn route_fallback_overrides_stay_local_until_the_context_continues_once() {
    let (session, mut peer, _directory, context, page) = harness().await;
    session
        .0
        .host
        .diagnostics
        .lock()
        .unwrap()
        .enable(context, true);
    session
        .0
        .host
        .request_inputs
        .lock()
        .unwrap()
        .set(context.connection_id, context, true)
        .unwrap();
    {
        let mut rules = session.0.host.routing.rules.lock().unwrap();
        rules.mark_manual(page, true);
        for _ in 0..2 {
            rules
                .handlers
                .add(page, "https://example.com/acme".into())
                .unwrap();
        }
    }
    session
        .0
        .host
        .routing
        .manual_urls
        .lock()
        .unwrap()
        .insert(page, "https://example.com/acme".into());
    let context_handler = {
        let mut rules = session.0.host.routing.rules.lock().unwrap();
        rules.mark_manual(context, true);
        rules
            .handlers
            .add(context, "https://example.com/other".into())
            .unwrap();
        rules
            .handlers
            .add(context, "https://example.com/acme".into())
            .unwrap()
    };
    session
        .0
        .host
        .routing
        .manual_urls
        .lock()
        .unwrap()
        .insert(context, "https://example.com/other".into());
    peer.send(&created("overridden-request","Request",json!({"frame":{"guid":"frame"},"url":"https://example.com/acme","resourceType":"fetch","method":"GET","headers":[{"name":"Cookie","value":"browser"}],"postData":"AQID","isNavigationRequest":false}))).await;
    peer.send(&json!({"guid":"context","method":"request","params":{"request":{"guid":"overridden-request"},"page":{"guid":"page"}}})).await;
    peer.send(&created(
        "overridden-route",
        "Route",
        json!({"request":{"guid":"overridden-request"}}),
    ))
    .await;
    peer.send(
        &json!({"guid":"page","method":"route","params":{"route":{"guid":"overridden-route"}}}),
    )
    .await;
    let original = pending_callback(&session).await;
    let source_wire = session
        .0
        .host
        .connection()
        .unwrap()
        .resolve("overridden-request", "Request")
        .await
        .unwrap();
    let source_id = session
        .0
        .host
        .diagnostics
        .lock()
        .unwrap()
        .known_request(&source_wire)
        .unwrap();
    let source_lease = session
        .0
        .host
        .request_inputs
        .lock()
        .unwrap()
        .get(context.connection_id, source_id)
        .unwrap();
    let expiry = session.0.host.callbacks.lock().unwrap()[&original];
    let result = session
        .0
        .host
        .callback_resolve(
            original,
            CallbackResponse::RouteFallbackWith {
                overrides: crate::browser::routing::Overrides {
                    url: Some("https://example.com/changed".into()),
                    method: Some("POST".into()),
                    body_base64: Some("AP8=".into()),
                    headers: Some(vec![
                        crate::browser::routing::Header {
                            name: "Cookie".into(),
                            value: "AcmeIntent=private".into(),
                        },
                        crate::browser::routing::Header {
                            name: "X-Acme".into(),
                            value: "routed".into(),
                        },
                    ]),
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    let next = serde_json::from_value(result["nextCallback"].clone()).unwrap();
    assert_eq!(session.0.host.routing.owners.lock().unwrap()[&next], page);
    let result = session
        .0
        .host
        .callback_resolve(next, CallbackResponse::RouteFallback {}, deadline())
        .await
        .unwrap();
    let next = serde_json::from_value(result["nextCallback"].clone()).unwrap();
    assert_eq!(
        session.0.host.routing.owners.lock().unwrap()[&next],
        context
    );
    assert_eq!(session.0.host.callbacks.lock().unwrap()[&next], expiry);
    assert_eq!(
        session
            .0
            .host
            .routing
            .rules
            .lock()
            .unwrap()
            .handlers
            .current(next),
        Some(context_handler)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    let host = session.0.host.clone();
    let continuing = tokio::spawn(async move {
        host.callback_resolve(
            next,
            CallbackResponse::RouteContinueWith {
                overrides: crate::browser::routing::Overrides {
                    method: Some("PUT".into()),
                    ..Default::default()
                },
            },
            deadline(),
        )
        .await
    });
    let request = peer.request().await;
    assert_eq!(request["guid"], "overridden-route");
    assert_eq!(request["method"], "continue");
    assert_eq!(request["params"]["url"], "https://example.com/changed");
    assert_eq!(request["params"]["method"], "PUT");
    assert_eq!(request["params"]["postData"], "AP8=");
    assert_eq!(request["params"]["isFallback"], false);
    assert!(
        request["params"]["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|header| header["name"] == "Cookie" && header["value"] == "browser")
    );
    peer.send(&json!({"id":request["id"],"result":{}})).await;
    continuing.await.unwrap().unwrap();
    let source = session
        .0
        .host
        .request_inputs
        .lock()
        .unwrap()
        .get(context.connection_id, source_id)
        .unwrap()
        .fetch(Default::default())
        .unwrap();
    assert_eq!(source.url, "https://example.com/changed");
    assert_eq!(source.method, "PUT");
    assert_eq!(
        serde_json::to_value(source.body).unwrap(),
        json!({"kind":"base64","data":"AP8="})
    );
    assert!(
        source
            .headers
            .iter()
            .any(|header| header.name == "cookie" && header.value == "AcmeIntent=private")
    );
    assert_eq!(source_lease.body().unwrap(), [1, 2, 3]);
    assert!(session.0.host.routing.overrides.lock().unwrap().is_empty());
    close(&session).await;
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One paused request proves policy races, handoff ownership, expiry and backend identity without replay"
)]
async fn route_fallback_retains_backend_request_source_policy_and_original_expiry() {
    let (session, mut peer, _directory, context, page) = harness().await;
    session
        .0
        .host
        .routing
        .rules
        .lock()
        .unwrap()
        .mark_manual(context, true);
    session
        .0
        .host
        .routing
        .manual_urls
        .lock()
        .unwrap()
        .insert(context, "https://example.com/acme".into());
    peer.send(&created("fallback-request","Request",json!({"frame":{"guid":"frame"},"url":"https://example.com/acme","resourceType":"fetch","method":"GET","headers":[],"isNavigationRequest":false}))).await;
    peer.send(&created(
        "fallback-route",
        "Route",
        json!({"request":{"guid":"fallback-request"}}),
    ))
    .await;
    peer.send(
        &json!({"guid":"page","method":"route","params":{"route":{"guid":"fallback-route"}}}),
    )
    .await;
    let original = pending_callback(&session).await;
    let expiry = session.0.host.callbacks.lock().unwrap()[&original];
    let resource = session
        .0
        .host
        .state
        .lock()
        .unwrap()
        .resource(original, Some(Kind::Route))
        .unwrap()
        .clone();
    session
        .0
        .host
        .routing
        .updating
        .lock()
        .unwrap()
        .insert(context);
    assert_eq!(
        session
            .0
            .host
            .route_fallback_target(&resource)
            .await
            .err()
            .unwrap()
            .code,
        "ROUTE_POLICY_PENDING"
    );
    session
        .0
        .host
        .routing
        .updating
        .lock()
        .unwrap()
        .remove(&context);
    let prepared = session
        .0
        .host
        .route_fallback_target(&resource)
        .await
        .unwrap()
        .unwrap();
    session
        .0
        .host
        .routing
        .manual_urls
        .lock()
        .unwrap()
        .insert(context, "https://example.com/changed".into());
    assert_eq!(
        session
            .0
            .host
            .handoff_route(original, &prepared, &Default::default())
            .unwrap_err()
            .code,
        "ROUTE_POLICY_CHANGED"
    );
    assert_eq!(session.0.host.callbacks.lock().unwrap()[&original], expiry);
    session
        .0
        .host
        .routing
        .manual_urls
        .lock()
        .unwrap()
        .insert(context, "https://example.com/acme".into());
    let reply = session
        .0
        .host
        .callback_resolve(original, CallbackResponse::RouteFallback {}, deadline())
        .await
        .unwrap();
    let next = serde_json::from_value(reply["nextCallback"].clone()).unwrap();
    assert_ne!(original, next);
    assert_eq!(session.0.host.callbacks.lock().unwrap()[&next], expiry);
    assert_eq!(session.0.host.policy_routes(page), vec![next]);
    assert_eq!(session.0.host.policy_routes(context), vec![next]);
    assert_eq!(
        session
            .0
            .host
            .page_har_set(
                page,
                None,
                crate::browser::contract::HarNotFound::Abort,
                None,
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "ROUTE_CALLBACKS_PENDING"
    );
    assert_eq!(
        session
            .0
            .host
            .callback_resolve(
                original,
                CallbackResponse::RouteAbort { error_code: None },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "STALE_REFERENCE"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    session
        .0
        .host
        .callbacks
        .lock()
        .unwrap()
        .insert(next, Instant::now());
    let host = session.0.host.clone();
    let expiring = tokio::spawn(async move {
        host.expire_callback(original).await;
    });
    let abort = peer.request().await;
    assert_eq!(abort["guid"], "fallback-route");
    assert_eq!(abort["method"], "abort");
    peer.send(&json!({"id":abort["id"],"result":{}})).await;
    expiring.await.unwrap();
    assert!(!session.0.host.callbacks.lock().unwrap().contains_key(&next));
    assert!(session.0.host.routing.handoffs.lock().unwrap().is_empty());
    close(&session).await;
}

#[tokio::test]
async fn context_routes_own_unpublished_popup_frames_only_after_typed_graph_membership() {
    for owned in [true, false] {
        let (session, mut peer, _directory, context, page) = harness().await;
        let mut frame = created(
            "unpublished-popup-frame",
            "Frame",
            json!({"url":"about:blank","name":"","loadStates":[]}),
        );
        if owned {
            frame["guid"] = json!("context");
        }
        peer.send(&frame).await;
        peer.send(&created("popup-initial-request","Request",json!({"frame":{"guid":"unpublished-popup-frame"},"url":"https://example.com/popup","resourceType":"document","method":"GET","headers":[],"isNavigationRequest":true}))).await;
        peer.send(&created(
            "popup-initial-route",
            "Route",
            json!({"request":{"guid":"popup-initial-request"}}),
        ))
        .await;
        peer.send(&json!({"guid":"context","method":"route","params":{"route":{"guid":"popup-initial-route"}}})).await;
        if owned {
            let callback = pending_callback(&session).await;
            let status = session.status();
            let route = status["resources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|resource| resource["kind"] == "route")
                .unwrap();
            assert_eq!(route["parent"], json!(context));
            assert_eq!(route["policyScope"], json!(context));
            assert_eq!(status["selectedPage"], json!(page));
            assert_eq!(
                status["resources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|resource| resource["kind"] == "page")
                    .count(),
                1
            );
            crate::schema::validate(&crate::browser::contract::schemas()["status"], &status)
                .unwrap();
            let responding = session
                .submit(
                    Uuid::new_v4(),
                    Request::CallbackResolve {
                        callback,
                        response: CallbackResponse::RouteAbort { error_code: None },
                    },
                    5000,
                )
                .unwrap();
            let request = peer.request().await;
            assert_eq!(request["guid"], "popup-initial-route");
            assert_eq!(request["method"], "abort");
            peer.send(&json!({"id":request["id"],"result":{}})).await;
            assert_eq!(
                complete(&session, responding).await.outcome,
                Outcome::Succeeded
            );
        } else {
            session
                .0
                .host
                .connection()
                .unwrap()
                .event_barrier(deadline())
                .await
                .unwrap();
            assert_eq!(session.status()["failure"], "FOREIGN_CALLBACK");
            assert!(session.0.host.callbacks.lock().unwrap().is_empty());
        }
        close(&session).await;
    }
}

#[tokio::test]
async fn automatic_context_har_redirects_an_initial_navigation_without_a_published_page() {
    let (session, mut peer, directory, context, _page) = harness().await;
    let input = directory.path().join("context-redirect-input");
    crate::artifacts::directory(&input).unwrap();
    let path = input.join("source.har");
    crate::artifacts::write(&path,&serde_json::to_vec(&json!({"log":{"version":"1.2","entries":[
        {"request":{"url":"https://example.com/start","method":"GET","headers":[]},"response":{"status":302,"headers":[{"name":"Location","value":"/end"}],"content":{"text":""}}},
        {"request":{"url":"https://example.com/end","method":"GET","headers":[]},"response":{"status":200,"headers":[],"content":{"text":"Acme destination"}}}
    ]}})).unwrap()).unwrap();
    let archive = session
        .0
        .host
        .har_archives
        .lock()
        .unwrap()
        .register(&path, session.0.host.journal.directory())
        .unwrap();
    let archive_id = serde_json::from_value(archive["archiveId"].clone()).unwrap();
    {
        let mut rules = session.0.host.routing.rules.lock().unwrap();
        let rule = rules
            .begin_context(context, archive_id, contract::HarNotFound::Abort)
            .unwrap();
        rules.confirm(context, rule.id).unwrap();
    }
    let mut frame = created(
        "initial-har-frame",
        "Frame",
        json!({"url":"about:blank","name":"","loadStates":[]}),
    );
    frame["guid"] = json!("context");
    peer.send(&frame).await;
    peer.send(&created("initial-har-request","Request",json!({"frame":{"guid":"initial-har-frame"},"url":"https://example.com/start","resourceType":"document","method":"GET","headers":[],"isNavigationRequest":true}))).await;
    peer.send(&created(
        "initial-har-route",
        "Route",
        json!({"request":{"guid":"initial-har-request"}}),
    ))
    .await;
    peer.send(
        &json!({"guid":"context","method":"route","params":{"route":{"guid":"initial-har-route"}}}),
    )
    .await;
    let redirect = peer.request().await;
    assert_eq!(redirect["guid"], "initial-har-route");
    assert_eq!(redirect["method"], "redirectNavigationRequest");
    assert_eq!(redirect["params"]["url"], "https://example.com/end");
    peer.send(&json!({"id":redirect["id"],"result":{}})).await;
    let until = deadline();
    while session.0.host.har_list()["routes"][0]["replies"] != 1 {
        assert!(
            Instant::now() < until,
            "Context redirect did not settle: {}",
            session.status()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(session.status()["phase"], "ready");
    close(&session).await;
}

#[tokio::test]
async fn closed_popup_subscription_keeps_the_sibling_page_ready() {
    for closed in [true, false] {
        let (session, mut peer, _directory, _context, page) = harness().await;
        peer.send(&created(
            "popup-frame",
            "Frame",
            json!({"url":"about:blank","name":"","loadStates":[]}),
        ))
        .await;
        peer.send(&created(
            "popup",
            "Page",
            json!({"mainFrame":{"guid":"popup-frame"},"isClosed":false}),
        ))
        .await;
        peer.send(&json!({"guid":"context","method":"page","params":{"page":{"guid":"popup"}}}))
            .await;
        let subscription = peer.request().await;
        assert_eq!(subscription["guid"], "popup");
        assert_eq!(subscription["method"], "updateSubscription");
        if closed {
            peer.send(&json!({"guid":"popup","method":"close","params":{}}))
                .await;
        }
        peer.send(&json!({"id":subscription["id"],"error":{"error":{"name":if closed {"TargetClosedError"}else{"Error"},"message":"Acme subscription unavailable"}}})).await;
        let connection = session.0.host.connection().unwrap();
        connection.event_barrier(deadline()).await.unwrap();
        if closed {
            assert_eq!(session.status()["phase"], "ready");
            assert!(
                session
                    .0
                    .host
                    .state
                    .lock()
                    .unwrap()
                    .resource(page, Some(Kind::Page))
                    .is_ok()
            );
            let list = session
                .submit(Uuid::new_v4(), Request::PageList {}, 5000)
                .unwrap();
            let result = complete(&session, list).await;
            assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
            assert_eq!(result.result.unwrap()["items"].as_array().unwrap().len(), 1);
        } else {
            assert_eq!(session.status()["phase"], "lost");
            assert_eq!(session.status()["failure"], "BROWSER_METHOD_FAILED");
        }
        assert!(
            session
                .private_failures()
                .iter()
                .any(|entry| entry.contains("Acme subscription unavailable"))
        );
        assert!(
            !session
                .status()
                .to_string()
                .contains("Acme subscription unavailable")
        );
        close(&session).await;
    }
}

#[tokio::test]
async fn uncertain_script_removal_waits_for_original_disposal_without_replaying() {
    let (session, mut peer, _directory, context, _) = harness().await;
    let adding = tokio::spawn({
        let host = session.0.host.clone();
        async move {
            host.init_script(
                Request::ContextInitScriptAdd {
                    context,
                    source: "globalThis.Acme='Acme private initialization'".into(),
                },
                deadline(),
            )
            .await
        }
    });
    let add = peer.request().await;
    assert_eq!(add["guid"], "context");
    assert_eq!(add["method"], "addInitScript");
    peer.send(&created("acme-script", "Disposable", json!({})))
        .await;
    peer.send(&json!({"id":add["id"],"result":{"disposable":{"guid":"acme-script"}}}))
        .await;
    let added = adding.await.unwrap().unwrap();
    let script_id = serde_json::from_value(added["scriptId"].clone()).unwrap();
    let removing = tokio::spawn({
        let host = session.0.host.clone();
        async move {
            host.init_script(
                Request::ContextInitScriptRemove { context, script_id },
                deadline(),
            )
            .await
        }
    });
    let remove = peer.request().await;
    assert_eq!(remove["guid"], "acme-script");
    assert_eq!(remove["method"], "dispose");
    peer.send(&json!({"id":remove["id"],"error":{"error":{"name":"TimeoutError","message":"Acme cleanup outcome unavailable"}}})).await;
    assert_eq!(removing.await.unwrap().unwrap_err().code, "OUTCOME_UNKNOWN");
    let host = &session.0.host;
    let list = host
        .init_script(Request::ContextInitScriptList { context }, deadline())
        .await
        .unwrap();
    assert_eq!(list["scripts"][0]["state"], "removing");
    assert!(!list.to_string().contains("Acme private initialization"));
    assert_eq!(
        host.init_script(
            Request::ContextInitScriptRemove { context, script_id },
            deadline()
        )
        .await
        .unwrap_err()
        .code,
        "INIT_SCRIPT_CLEANUP_PENDING"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    // The exact original channel closes only after its cleanup, or after its
    // owning scope closes. That witness permits local finalization, not a retry.
    peer.send(&json!({"guid":"acme-script","method":"__dispose__","params":{"reason":"gc"}}))
        .await;
    let finalized = host
        .init_script(
            Request::ContextInitScriptRemove { context, script_id },
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(finalized["removed"], true);
    assert_eq!(finalized["replayed"], false);
    assert_eq!(finalized["cleanupWitness"], "driver-disposable-closed");
    assert!(
        host.init_script(Request::ContextInitScriptList { context }, deadline())
            .await
            .unwrap()["scripts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn script_removal_accepts_the_driver_void_acknowledgement() {
    let (session, mut peer, _directory, context, _) = harness().await;
    let adding = session
        .submit(
            Uuid::new_v4(),
            Request::ContextInitScriptAdd {
                context,
                source: "globalThis.Acme=42".into(),
            },
            5000,
        )
        .unwrap();
    let add = peer.request().await;
    assert_eq!(add["method"], "addInitScript");
    peer.send(&created("acme-void-script", "Disposable", json!({})))
        .await;
    peer.send(&json!({"id":add["id"],"result":{"disposable":{"guid":"acme-void-script"}}}))
        .await;
    let added = complete(&session, adding).await;
    assert_eq!(added.outcome, Outcome::Succeeded, "{added:?}");
    let script_id = serde_json::from_value(added.result.unwrap()["scriptId"].clone()).unwrap();
    let removing = session
        .submit(
            Uuid::new_v4(),
            Request::ContextInitScriptRemove { context, script_id },
            5000,
        )
        .unwrap();
    let remove = peer.request().await;
    assert_eq!(remove["guid"], "acme-void-script");
    assert_eq!(remove["method"], "dispose");
    // Void protocol results normalize to None; an acknowledged call still
    // completed even though there is no JSON result value to inspect.
    peer.send(&json!({"id":remove["id"],"result":{}})).await;
    let removed = complete(&session, removing).await;
    assert_eq!(removed.outcome, Outcome::Succeeded, "{removed:?}");
    assert_eq!(removed.result.unwrap()["removed"], true);
    assert!(
        session
            .0
            .host
            .init_script(Request::ContextInitScriptList { context }, deadline())
            .await
            .unwrap()["scripts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    close(&session).await;
}

#[test]
fn cleanup_stage_report_names_the_unfinished_stage_without_payloads() {
    let mut stages = CleanupStages::new();
    stages.mark("cleanup_lock");
    stages.mark("driver_shutdown");
    stages.browser_close = Some("BROWSER_METHOD_FAILED");
    let report = stages.report();
    let completed = report["cleanupStages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stage| stage["stage"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(completed, ["start", "cleanup_lock"]);
    assert_eq!(report["unfinishedStage"], "driver_shutdown");
    assert_eq!(report["browserCloseOutcome"], "BROWSER_METHOD_FAILED");
    assert!(report["unfinishedElapsedMs"].is_u64());
    assert!(report["totalMs"].as_u64().unwrap() >= report["unfinishedElapsedMs"].as_u64().unwrap());
}

#[tokio::test]
async fn browser_close_outcome_preserves_acknowledgement_and_remote_failure_without_payloads() {
    for fail in [false, true] {
        let (session, mut peer, _directory, _context, _page) = harness().await;
        let host = session.0.host.clone();
        let closing = tokio::spawn(async move { host.close_browser(deadline()).await });
        let command = peer.request().await;
        assert_eq!(command["guid"], "browser");
        assert_eq!(command["method"], "close");
        if fail {
            peer.send(&method_error(command["id"].clone())).await;
        } else {
            peer.send(&json!({"id":command["id"],"result":{}})).await;
        }
        assert_eq!(
            closing.await.unwrap(),
            if fail {
                "BROWSER_METHOD_FAILED"
            } else {
                "ACKNOWLEDGED"
            }
        );
        close(&session).await;
    }
}

#[test]
fn interactive_authentication_is_refused_on_the_pinned_playwright_engines() {
    let page = Reference {
        connection_id: Uuid::new_v4(),
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let request = Request::PageRouteSet {
        page,
        url: Some("https://example.com/acme".into()),
        authentication_challenges: true,
    };
    for engine in [Engine::Firefox, Engine::Webkit] {
        assert_eq!(
            workflows::validate_engine(&request, engine)
                .unwrap_err()
                .code,
            "AUTH_CHALLENGE_UNSUPPORTED"
        );
    }
}

#[test]
fn webkit_fulfillment_status_refusal_precedes_callback_consumption() {
    let callback = Reference {
        connection_id: Uuid::new_v4(),
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    for response in [
        contract::CallbackResponse::RouteFulfill {
            status: Some(302),
            body: Some(String::new()),
            json: None,
            content_type: Some("text/plain".into()),
            headers: vec![],
            path: None,
            file: None,
        },
        contract::CallbackResponse::RouteFulfillBinary {
            status: 304,
            body_base64: String::new(),
            headers: vec![],
        },
        contract::CallbackResponse::RouteFulfillEvidence {
            status: 304,
            network_evidence_id: Uuid::new_v4(),
            headers: vec![],
        },
    ] {
        let request = Request::CallbackResolve { callback, response };
        assert_eq!(
            workflows::validate_engine(&request, Engine::Webkit)
                .unwrap_err()
                .code,
            "ROUTE_STATUS_UNSUPPORTED"
        );
        workflows::validate_engine(&request, Engine::Firefox).unwrap();
        let batch = Request::Batch {
            requests: vec![request],
        };
        assert_eq!(
            workflows::validate_engine(&batch, Engine::Webkit)
                .unwrap_err()
                .code,
            "ROUTE_STATUS_UNSUPPORTED"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn storage_registration_copies_only_private_regular_files_and_preserves_the_source() {
    use std::os::unix::fs::PermissionsExt;
    let (session, _peer, directory, _, _) = harness().await;
    let source = directory.path().join("import");
    crate::artifacts::directory(&source).unwrap();
    let path = source.join("state.json");
    let bytes = br#"{"cookies":[],"origins":[{"origin":"https://example.com","localStorage":[{"name":"acme","value":"private-import"}]}]}"#;
    crate::artifacts::write(&path, bytes).unwrap();
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::StorageRegister {
                path: path.to_str().unwrap().into(),
            },
            5000,
        )
        .unwrap();
    let registered = complete(&session, id).await;
    assert_eq!(registered.outcome, Outcome::Succeeded);
    let metadata = registered.result.unwrap();
    assert!(!metadata.to_string().contains("private-import"));
    assert_ne!(metadata["snapshot"]["artifact"]["path"], json!(path));
    let snapshot = serde_json::from_value(metadata["snapshot"]["reference"].clone()).unwrap();
    let forget = session
        .submit(Uuid::new_v4(), Request::StorageForget { snapshot }, 5000)
        .unwrap();
    assert_eq!(complete(&session, forget).await.outcome, Outcome::Succeeded);
    assert_eq!(std::fs::read(&path).unwrap(), bytes.as_slice());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let refused = session
        .submit(
            Uuid::new_v4(),
            Request::StorageRegister {
                path: path.to_str().unwrap().into(),
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, refused).await.code.as_deref(),
        Some("PRIVATE_STATE_REQUIRED")
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let alias = source.join("alias.json");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    let refused = session
        .submit(
            Uuid::new_v4(),
            Request::StorageRegister {
                path: alias.to_str().unwrap().into(),
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, refused).await.code.as_deref(),
        Some("PRIVATE_STATE_REQUIRED")
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes.as_slice());
    close(&session).await;
}

#[tokio::test]
async fn page_event_wait_barriers_keep_closed_page_witnesses_and_never_replay_the_trigger() {
    use crate::browser::activity::Event;
    let (session, mut peer, _directory, context, page) = harness().await;
    let cursor = session
        .submit(
            Uuid::new_v4(),
            Request::EventsRead {
                context,
                cursor: None,
                limit: 100,
            },
            5000,
        )
        .unwrap();
    let after = complete(&session, cursor).await.result.unwrap()["latestSequence"]
        .as_u64()
        .unwrap();
    // The event deliberately arrives before its wait is submitted.
    peer.send(&json!({"guid":"page","method":"crash","params":{}}))
        .await;
    let wait = session
        .submit(
            Uuid::new_v4(),
            event_wait(context, after, Event::PageCrashed, Some(page)),
            5000,
        )
        .unwrap();
    let matched = complete(&session, wait).await;
    assert_eq!(matched.outcome, Outcome::Succeeded);
    let event = matched.result.unwrap();
    assert_eq!(event["event"]["page"], json!(page));
    assert_eq!(event["event"]["resource"], json!(page));
    assert_eq!(event["contentIncluded"], false);
    let terminated = session
        .submit(
            Uuid::new_v4(),
            event_wait(
                context,
                event["nextCursor"].as_u64().unwrap(),
                Event::Dialog,
                Some(page),
            ),
            5000,
        )
        .unwrap();
    let terminal = complete(&session, terminated).await;
    assert_eq!(terminal.code.as_deref(), Some("PAGE_CRASHED"));
    assert_eq!(terminal.result.unwrap()["nextCursor"], event["nextCursor"]);
    let waiting = session
        .submit(
            Uuid::new_v4(),
            event_wait(
                context,
                event["nextCursor"].as_u64().unwrap(),
                Event::Dialog,
                None,
            ),
            5000,
        )
        .unwrap();
    let read = session
        .submit(Uuid::new_v4(), Request::ContextList {}, 5000)
        .unwrap();
    assert_eq!(complete(&session, read).await.outcome, Outcome::Succeeded);
    session.cancel(waiting).await.unwrap();
    assert_eq!(
        complete(&session, waiting).await.outcome,
        Outcome::Cancelled
    );
    let unknown = Reference {
        generation: Uuid::new_v4(),
        ..page
    };
    let invalid = session
        .submit(
            Uuid::new_v4(),
            event_wait(context, after, Event::PageCrashed, Some(unknown)),
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, invalid).await.code.as_deref(),
        Some("STALE_REFERENCE")
    );
    close(&session).await;
}

fn event_wait(
    context: Reference,
    after: u64,
    event: crate::browser::activity::Event,
    page: Option<Reference>,
) -> Request {
    Request::EventsWait {
        context,
        after,
        conditions: vec![crate::browser::activity::Condition { event, page }],
    }
}

#[tokio::test]
async fn sdk_event_wait_close_witness_survives_either_event_reply_ordering() {
    use crate::browser::activity::{Condition, Event};
    for event_first in [false, true] {
        let (session, mut peer, _root, context, page) = harness().await;
        let after = session
            .0
            .host
            .activity
            .lock()
            .unwrap()
            .read(context, None, 100)
            .unwrap()["latestCursor"]
            .as_u64()
            .unwrap();
        let closing = session
            .submit(Uuid::new_v4(), Request::PageClose { page }, 5000)
            .unwrap();
        let command = peer.request().await;
        assert_eq!(command["method"], "close");
        {
            let state = session.0.host.state.lock().unwrap();
            assert!(state.resource(page, Some(Kind::Page)).is_err());
            assert_eq!(state.retained_ancestor(page, Kind::Context), Some(context));
            assert_eq!(
                state.retained_ancestor(
                    Reference {
                        generation: Uuid::new_v4(),
                        ..page
                    },
                    Kind::Context
                ),
                None
            );
        }
        if event_first {
            peer.send(&json!({"guid":"page","method":"close","params":{}}))
                .await;
        }
        peer.send(&json!({"id":command["id"],"result":{}})).await;
        assert_eq!(
            complete(&session, closing).await.outcome,
            Outcome::Succeeded
        );
        if !event_first {
            peer.send(&json!({"guid":"page","method":"close","params":{}}))
                .await;
        }
        let waiting = session
            .submit(
                Uuid::new_v4(),
                Request::EventsWait {
                    context,
                    after,
                    conditions: vec![Condition {
                        event: Event::PageClosed,
                        page: Some(page),
                    }],
                },
                5000,
            )
            .unwrap();
        let result = complete(&session, waiting).await;
        assert_eq!(
            result.outcome,
            Outcome::Succeeded,
            "event_first={event_first}: {result:?}"
        );
        let history = session
            .0
            .host
            .activity
            .lock()
            .unwrap()
            .read(context, Some(after), 100)
            .unwrap();
        assert_eq!(
            history["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["event"] == "page_closed")
                .count(),
            1
        );
        close(&session).await;
    }
}

#[tokio::test]
async fn batch_engine_preflight_refuses_mobile_emulation_before_any_allocation() {
    let (session, _peer, _directory, _, page) = harness().await;
    session.0.host.state.lock().unwrap().engine = Engine::Firefox;
    let request = Request::Batch {
        requests: vec![
            Request::PageSelect { page },
            Request::ContextCreate {
                options: ContextOptions {
                    is_mobile: Some(true),
                    ..Default::default()
                },
            },
        ],
    };
    let id = Uuid::new_v4();
    assert_eq!(
        session.submit(id, request, 5000).unwrap_err().code,
        "MOBILE_EMULATION_UNSUPPORTED"
    );
    assert!(session.operation(id).is_none());
    close(&session).await;
}

#[tokio::test]
async fn storage_snapshots_keep_values_private_and_restore_once_with_explicit_replacement() {
    use crate::browser::storage::Options;
    let (session, mut peer, directory, context, _) = harness().await;
    let native = json!({"cookies":[{"name":"acme","value":"Acme-private-cookie","domain":"example.com","path":"/","expires":-1,"httpOnly":true,"secure":true,"sameSite":"Lax"}],"origins":[{"origin":"https://example.com","localStorage":[{"name":"account","value":"Acme-private-state"}]}]});
    let export = session
        .submit(
            Uuid::new_v4(),
            Request::StorageExport {
                context,
                options: Options::default(),
            },
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "storageState");
    peer.send(&json!({"id":call["id"],"result":native})).await;
    let exported = complete(&session, export).await;
    assert_eq!(exported.outcome, Outcome::Succeeded);
    let metadata = exported.result.unwrap();
    assert!(!metadata.to_string().contains("Acme-private"));
    assert!(!session.status().to_string().contains("Acme-private"));
    let snapshot: Reference =
        serde_json::from_value(metadata["snapshot"]["reference"].clone()).unwrap();
    let artifact = metadata["snapshot"]["artifact"]["path"].as_str().unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(artifact).unwrap()).unwrap(),
        native
    );
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::StorageApply {
                    context,
                    snapshot,
                    mode: crate::browser::storage::ApplyMode::Merge,
                    install_authenticator: false,
                },
                5000
            )
            .unwrap_err()
            .code,
        "STORAGE_MERGE_UNSUPPORTED"
    );
    let request = Request::StorageApply {
        context,
        snapshot,
        mode: crate::browser::storage::ApplyMode::Replace,
        install_authenticator: false,
    };
    let id = Uuid::new_v4();
    session.submit(id, request.clone(), 5000).unwrap();
    let apply = peer.request().await;
    assert_eq!(apply["method"], "setStorageState");
    assert_eq!(apply["params"]["storageState"], native);
    peer.send(&method_error(apply["id"].clone())).await;
    let failed = complete(&session, id).await;
    assert_eq!(failed.outcome, Outcome::Unknown);
    assert_eq!(failed.result.as_ref().unwrap()["stateMayHaveChanged"], true);
    assert!(!failed.result.unwrap().to_string().contains("Acme-private"));
    assert_eq!(session.submit(id, request, 5000).unwrap(), id);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.request())
            .await
            .is_err()
    );
    // A changed file remains owned and visible; forgetting cannot remove it.
    let original = std::fs::read(artifact).unwrap();
    std::fs::write(artifact, b"Acme replacement").unwrap();
    let forget = session
        .submit(Uuid::new_v4(), Request::StorageForget { snapshot }, 5000)
        .unwrap();
    assert_eq!(
        complete(&session, forget).await.code.as_deref(),
        Some("STORAGE_ARTIFACT_CHANGED")
    );
    std::fs::write(artifact, original).unwrap();
    let forget = session
        .submit(Uuid::new_v4(), Request::StorageForget { snapshot }, 5000)
        .unwrap();
    assert_eq!(complete(&session, forget).await.outcome, Outcome::Succeeded);
    assert!(!Path::new(artifact).exists());
    assert!(directory.path().exists());
    close(&session).await;
}

#[tokio::test]
async fn typed_event_wait_uses_explicit_cursor_keeps_reads_responsive_and_cancels() {
    use crate::browser::diagnostics::{DiagnosticKind, EventCondition};
    let (session, mut peer, _directory, context, page) = harness().await;
    {
        let mut history = session.0.host.diagnostics.lock().unwrap();
        history.enable(context, true);
        history.push(
            context,
            Some(page),
            DiagnosticKind::Console,
            json!({"text":"Acme-old-private"}),
            0,
        );
    }
    let condition = EventCondition {
        web_socket_id: None,
        event: DiagnosticKind::Console,
        page: Some(page),
        request_id: None,
        method: None,
        status: None,
        url: None,
    };
    let waiting = session
        .submit(
            Uuid::new_v4(),
            Request::DiagnosticsWait {
                context,
                after: 1,
                conditions: vec![condition.clone()],
            },
            5000,
        )
        .unwrap();
    let read = session
        .submit(Uuid::new_v4(), Request::PageList {}, 5000)
        .unwrap();
    assert_eq!(complete(&session, read).await.outcome, Outcome::Succeeded);
    assert!(!session.operation(waiting).unwrap().settled);
    peer.send(&json!({"guid":"context","method":"console","params":{"type":"log","text":"Acme-new-private","args":[],"location":{"url":"https://example.com/?secret=Acme","lineNumber":0,"columnNumber":0},"timestamp":1,"page":{"guid":"page"}}})).await;
    let matched = complete(&session, waiting).await;
    assert_eq!(matched.outcome, Outcome::Succeeded);
    let result = matched.result.unwrap();
    assert_eq!(result["event"]["sequence"], 2);
    assert_eq!(result["conditionIndex"], 0);
    assert!(!result.to_string().contains("Acme"));
    let cancelled = session
        .submit(
            Uuid::new_v4(),
            Request::DiagnosticsWait {
                context,
                after: 2,
                conditions: vec![condition],
            },
            5000,
        )
        .unwrap();
    session.cancel(cancelled).await.unwrap();
    assert_eq!(
        complete(&session, cancelled).await.outcome,
        Outcome::Cancelled
    );
    close(&session).await;
}

#[tokio::test]
async fn held_input_expiry_releases_once_and_fences_old_observations() {
    let (session, mut peer, _directory, _, page) = harness().await;
    let epoch = session.0.host.input_epoch.load(Ordering::SeqCst);
    {
        let mut inputs = session.0.host.inputs.lock().unwrap();
        let held = inputs.held.entry(page).or_default();
        held.keys.insert("ShiftLeft".into(), "Shift".into());
        held.expires_at = Some(std::time::Instant::now() - Duration::from_millis(1));
    }
    session.0.host.changed.notify_waiters();
    let release = peer.request().await;
    assert_eq!(release["method"], "keyboardUp");
    assert_eq!(release["params"]["key"], "Shift");
    peer.send(&method_error(release["id"].clone())).await;
    tokio::time::timeout_at(deadline(), async {
        loop {
            let blocked = session.0.host.inputs.lock().unwrap().held[&page]
                .blocked_keys
                .contains("ShiftLeft");
            if blocked {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(session.0.host.input_epoch.load(Ordering::SeqCst) > epoch);
    assert!(session.0.host.inputs.lock().unwrap().held[&page].expiry_attempted);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One control lease verifies mutation fencing and retained reads before and after viewer handoff"
)]
async fn browser_control_release_fences_agent_mutations_but_keeps_observation_and_receipts() {
    let (session, _peer, _directory, context, _) = harness().await;
    let id = Uuid::new_v4();
    session.submit(id, Request::ContextList {}, 5000).unwrap();
    complete(&session, id).await;
    let released = session
        .change_control("browser_control_release", json!({"expectedVersion":1}))
        .await
        .unwrap();
    assert_eq!(released["state"], "released");
    assert!(released.get("leaseId").is_none());
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::ContextDebugger {
                    context,
                    action: crate::browser::automation_debugger::Action::Enable
                },
                5000
            )
            .unwrap_err()
            .code,
        "CONTROL_NOT_OWNED"
    );
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::ContextDebugger {
                    context,
                    action: crate::browser::automation_debugger::Action::RunTo {
                        pause_id: Uuid::new_v4(),
                        location: crate::browser::automation_debugger::Location {
                            file: "Acme-journey.rs".into(),
                            line: Some(30),
                            column: None
                        },
                    },
                },
                5000
            )
            .unwrap_err()
            .code,
        "CONTROL_NOT_OWNED"
    );
    let calls = session
        .submit(
            Uuid::new_v4(),
            Request::ContextDebugger {
                context,
                action: crate::browser::automation_debugger::Action::Calls {
                    offset: 0,
                    limit: 16,
                    include_details: false,
                },
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, calls).await.code.as_deref(),
        Some("AUTOMATION_CALL_COLLECTION_DISABLED")
    );
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::ContextCreate {
                    options: ContextOptions::default()
                },
                5000
            )
            .unwrap_err()
            .code,
        "CONTROL_NOT_OWNED"
    );
    assert_eq!(
        session.submit(id, Request::ContextList {}, 5000).unwrap(),
        id
    );
    let read = session
        .submit(Uuid::new_v4(), Request::PageList {}, 5000)
        .unwrap();
    assert_eq!(complete(&session, read).await.outcome, Outcome::Succeeded);
    assert_eq!(
        session
            .change_control(
                "browser_control_acquire",
                json!({"owner":"agent","expectedVersion":1})
            )
            .await
            .unwrap_err()
            .code,
        "STALE_CONTROL_LEASE"
    );
    let acquired = session
        .change_control(
            "browser_control_acquire",
            json!({"owner":"agent","expectedVersion":released["controlVersion"]}),
        )
        .await
        .unwrap();
    assert_eq!(acquired["owner"], "agent");
    close(&session).await;
}

async fn assert_viewer_grant_replay_refused(socket_path: &std::path::Path, token: &Value) {
    use crate::control::channel::{connect_test, read_frame as read, write_frame as write};
    // A replayed private grant cannot open another host connection.
    let mut duplicate = connect_test(socket_path.to_owned()).await.unwrap();
    write(&mut duplicate, &json!({"token":token}))
        .await
        .unwrap();
    assert_eq!(
        read(&mut duplicate).await.unwrap()["error"]["code"],
        "UNAUTHORIZED"
    );
}

#[tokio::test]
async fn browser_viewer_uses_private_shared_channel_and_cannot_control_another_page() {
    use crate::control::channel::{connect_test, read_frame as read, write_frame as write};
    use std::path::PathBuf;
    let (session, _peer, _directory, _, page) = harness().await;
    let acquired = session
        .change_control(
            "browser_control_acquire",
            json!({"owner":"viewer","viewerId":"acme-viewer","page":page,"expectedVersion":1}),
        )
        .await
        .unwrap();
    assert_eq!(acquired["state"], "viewer_pending");
    assert!(!acquired.to_string().contains("token"));
    assert!(!acquired.to_string().contains("leaseId"));
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::ContextCreate {
                    options: ContextOptions::default()
                },
                5000
            )
            .unwrap_err()
            .code,
        "CONTROL_NOT_OWNED"
    );
    let channel = &acquired["channel"];
    let broker = PathBuf::from(channel["grantBrokerPath"].as_str().unwrap());
    let socket_path = PathBuf::from(channel["controlSocketPath"].as_str().unwrap());
    let mut grant_peer = connect_test(broker).await.unwrap();
    write(
        &mut grant_peer,
        &json!({"kind":"control_grant","channelId":channel["channelId"]}),
    )
    .await
    .unwrap();
    let grant = read(&mut grant_peer).await.unwrap();
    assert_eq!(grant["access"], "input_control");
    drop(grant_peer);
    let mut socket = connect_test(socket_path.clone()).await.unwrap();
    write(&mut socket, &json!({"token":grant["token"]}))
        .await
        .unwrap();
    assert_eq!(read(&mut socket).await.unwrap()["kind"], "ready");
    write(&mut socket, &json!({"kind":"heartbeat"}))
        .await
        .unwrap();
    assert_eq!(read(&mut socket).await.unwrap()["kind"], "heartbeat");
    assert_viewer_grant_replay_refused(&socket_path, &grant["token"]).await;
    let credential = session
        .0
        .host
        .authority
        .credential(std::time::Instant::now());
    let error = session.viewer_input(credential, json!({"commandId":Uuid::new_v4(),"viewerLeaseId":session.0.host.authority.credential(std::time::Instant::now()).id,"viewerVersion":acquired["controlVersion"],"page":Reference { id:Uuid::new_v4(), ..page },"actions":[{"action":"key_up","key":"Shift"}]})).await.unwrap_err();
    assert_eq!(error.code, "INVALID_SCOPE");
    // A handoff fences new viewer input while the current mutation settles.
    // Refusing that input must preserve the healthy socket and its credential.
    let lane = session.0.host.lane.lock().await;
    let takeover = session.clone();
    let version = acquired["controlVersion"].clone();
    let handoff = tokio::spawn(async move {
        takeover
            .change_control(
                "browser_control_acquire",
                json!({"owner":"agent","expectedVersion":version}),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while session.control_status()["state"] != "handoff_pending" {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let command = Uuid::new_v4();
    write(&mut socket, &json!({"kind":"browser_input","commandId":command,"page":page,"actions":[{"action":"key_up","key":"Shift"}]})).await.unwrap();
    let refused = read(&mut socket).await.unwrap();
    assert_eq!(refused["commandId"], json!(command));
    assert_eq!(
        refused["result"]["error"]["code"],
        "CONTROL_HANDOFF_PENDING"
    );
    assert_eq!(
        refused["result"]["error"]["details"]["inputDispatched"],
        false
    );
    write(&mut socket, &json!({"kind":"heartbeat"}))
        .await
        .unwrap();
    assert_eq!(read(&mut socket).await.unwrap()["kind"], "heartbeat");
    drop(lane);
    let restored = handoff.await.unwrap().unwrap();
    assert_eq!(restored["owner"], "agent");
    close(&session).await;
    assert!(!socket_path.exists());
    assert!(!socket_path.parent().unwrap().exists());
}

#[tokio::test]
async fn ordered_batches_retain_partial_results_and_deduplicate_without_replay() {
    let (session, _peer, _directory, context, page) = harness().await;
    let stale = Reference {
        generation: Uuid::new_v4(),
        ..page
    };
    let request = Request::Batch {
        requests: vec![
            Request::ContextList {},
            Request::PageSelect { page: stale },
            Request::ContextSelect { context },
        ],
    };
    let id = Uuid::new_v4();
    session.submit(id, request.clone(), 5000).unwrap();
    let failed = complete(&session, id).await;
    assert_eq!(failed.code.as_deref(), Some("STALE_REFERENCE"));
    let partial = failed.result.unwrap();
    assert_eq!(partial["completedActions"], 1);
    assert_eq!(partial["failedIndex"], 1);
    assert_eq!(partial["results"][0]["items"].as_array().unwrap().len(), 1);
    assert_eq!(partial["replayed"], false);
    assert_eq!(session.submit(id, request, 5000).unwrap(), id);
    assert_eq!(complete(&session, id).await.result.unwrap(), partial);
    assert_eq!(session.status()["selectedPage"], json!(page));
    assert_eq!(
        session
            .submit(id, Request::PageList {}, 5000)
            .unwrap_err()
            .code,
        "OPERATION_CONFLICT"
    );
    assert!(
        session
            .submit(
                Uuid::new_v4(),
                Request::Batch {
                    requests: vec![Request::Batch {
                        requests: vec![Request::PageList {}]
                    }]
                },
                5000
            )
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn cancellation_between_batch_steps_retains_dispatched_result_and_stops_next_step() {
    let (session, mut peer, _directory, _, page) = harness().await;
    let id = Uuid::new_v4();
    session
        .submit(
            id,
            Request::Batch {
                requests: vec![
                    Request::PageRead {
                        page,
                        frame: None,
                        locator: Some(crate::browser::page::Locator::Css {
                            selector: "button".into(),
                        }),
                        read: crate::browser::page::Read::Count,
                        max_bytes: 8192,
                    },
                    Request::PageSelect {
                        page: Reference {
                            generation: Uuid::new_v4(),
                            ..page
                        },
                    },
                ],
            },
            5000,
        )
        .unwrap();
    let read = peer.request().await;
    assert_eq!(read["method"], "queryCount");
    session.cancel(id).await.unwrap();
    peer.send(&json!({"id":read["id"],"result":{"value":2}}))
        .await;
    let cancelled = complete(&session, id).await;
    assert_eq!(cancelled.outcome, Outcome::Cancelled);
    let partial = cancelled.result.unwrap();
    assert_eq!(partial["completedActions"], 1);
    assert_eq!(partial["results"][0]["result"]["value"], 2);
    assert!(cancelled.cancel_requested);
    close(&session).await;
}

#[tokio::test]
async fn batch_preflight_and_uncertain_input_keep_no_replay_and_cleanup_facts() {
    let (session, mut peer, _directory, _, page) = harness().await;
    let before = session.status()["operations"].as_array().unwrap().len();
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::Batch {
                    requests: vec![
                        Request::ContextList {},
                        Request::PageResize {
                            page,
                            viewport: crate::browser::contract::Viewport {
                                width: 0,
                                height: 720
                            }
                        },
                    ]
                },
                5000
            )
            .unwrap_err()
            .code,
        "INVALID_VIEWPORT"
    );
    assert_eq!(
        session.status()["operations"].as_array().unwrap().len(),
        before
    );
    let request = Request::Batch {
        requests: vec![
            Request::ContextList {},
            compound_request(
                page,
                crate::browser::page::Action::Press {
                    key: "Shift+a".into(),
                },
            ),
            Request::PageClose { page },
        ],
    };
    let id = Uuid::new_v4();
    session.submit(id, request.clone(), 5000).unwrap();
    let press = peer.request().await;
    assert_eq!(press["method"], "press");
    peer.send(&method_error(press["id"].clone())).await;
    let failed = complete(&session, id).await;
    assert_eq!(failed.outcome, Outcome::Unknown);
    assert_eq!(failed.code.as_deref(), Some("OUTCOME_UNKNOWN"));
    let partial = failed.result.unwrap();
    assert_eq!(partial["completedActions"], 1);
    assert_eq!(
        partial["partial"]["cleanup"]["requiresOwnedPageClose"],
        true
    );
    assert_eq!(partial["cleanup"]["pages"][0]["cleanup"]["complete"], false);
    assert_eq!(session.submit(id, request, 5000).unwrap(), id);
    assert_eq!(complete(&session, id).await.result.unwrap(), partial);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

async fn harness() -> (Session, Fixture, tempfile::TempDir, Reference, Reference) {
    let directory = tempfile::tempdir().unwrap();
    let (connection, mut events, mut peer) = fixture(65536).await;
    for message in [
        created(
            "browser",
            "Browser",
            json!({"version":"157.0","name":"firefox","browserName":"firefox"}),
        ),
        created("tracing", "Tracing", json!({})),
        created("debugger", "Debugger", json!({})),
        created(
            "request",
            "APIRequestContext",
            json!({"tracing":{"guid":"tracing"}}),
        ),
        created(
            "context",
            "BrowserContext",
            json!({"debugger":{"guid":"debugger"},"requestContext":{"guid":"request"},"tracing":{"guid":"tracing"},"options":{}}),
        ),
        created(
            "frame",
            "Frame",
            json!({"url":"about:blank","name":"","loadStates":[]}),
        ),
        created(
            "page",
            "Page",
            json!({"mainFrame":{"guid":"frame"},"isClosed":false}),
        ),
    ] {
        peer.send(&message).await;
    }
    loop {
        if let Some(Item::Event(Event::Created { kind, .. })) = events.next().await
            && kind == "Page"
        {
            break;
        }
    }
    let options = LaunchOptions {
        media_fixtures: Default::default(),
        proxy: None,
        engine: Engine::Firefox,
        mode: Mode::Headless,
        context: ContextOptions::default(),
    };
    let mut state = State::new(Uuid::new_v4(), Uuid::new_v4(), &options);
    state.phase = Phase::Ready;
    let private = directory.path().join("state");
    crate::artifacts::directory(&private).unwrap();
    let journal = Journal::create(&private, state.connection).unwrap();
    let browser = state
        .issue(
            Kind::Browser,
            connection.resolve("browser", "Browser").await.unwrap(),
            None,
        )
        .unwrap();
    let context = state
        .issue(
            Kind::Context,
            connection
                .resolve("context", "BrowserContext")
                .await
                .unwrap(),
            Some(browser),
        )
        .unwrap();
    let host = Arc::new(Host::new(state, journal, None));
    *host.connection.lock().unwrap() = Some(connection.clone());
    let page_wire = connection.resolve("page", "Page").await.unwrap();
    let indexing = tokio::spawn({
        let host = host.clone();
        let connection = connection.clone();
        async move { host.index_page(&connection, page_wire, context).await }
    });
    let subscription = peer.request().await;
    assert_eq!(subscription["method"], "updateSubscription");
    peer.send(&json!({"id":subscription["id"],"result":{}}))
        .await;
    let page = indexing.await.unwrap().unwrap();
    {
        let mut state = host.state.lock().unwrap();
        state.selected_context = Some(context);
        state.selected_page = Some(page);
    }
    host.start_events(events).await;
    (
        Session(Arc::new(Client {
            host,
            startup: Uuid::new_v4(),
        })),
        peer,
        directory,
        context,
        page,
    )
}
async fn complete(session: &Session, id: Uuid) -> Operation {
    session.wait(id, deadline()).await.unwrap()
}

#[tokio::test]
async fn attached_playwright_refuses_directory_upload_but_accepts_file_payloads() {
    let (session, _peer, _root, _, _) = harness().await;
    let host = &session.0.host;
    let fixture = host
        .fixture_register(
            serde_json::from_value(json!({
                "name":"acme.txt","mimeType":"text/plain","dataBase64":"QWNtZQ==",
                "provenance":"synthetic_generated","cleared":{"by":"Acme","method":"Inline test"}
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let file: Reference = serde_json::from_value(fixture["fixture"]["reference"].clone()).unwrap();
    let fixture = host
        .directory_register(
            "acme".into(),
            vec![crate::browser::directory::Entry {
                path: "acme.txt".into(),
                fixture: file,
            }],
        )
        .await
        .unwrap();
    let directory = serde_json::from_value(fixture["fixture"]["reference"].clone()).unwrap();
    assert!(
        host.attachment_parameters(vec![directory]).await.unwrap().0["localDirectory"].is_string()
    );
    host.attached.store(true, Ordering::SeqCst);
    assert_eq!(
        host.attachment_parameters(vec![directory])
            .await
            .unwrap_err()
            .code,
        "ATTACHMENT_DIRECTORY_UPLOAD_UNSUPPORTED"
    );
    let (params, _) = host.attachment_parameters(vec![file]).await.unwrap();
    assert_eq!(params["payloads"][0]["name"], "acme.txt");
}

#[tokio::test]
async fn large_fixture_playwright_attachment_keeps_verified_bytes_and_bounds() {
    use crate::browser::transfer::{FIXTURE_BYTES, Registration};
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let (session, _peer, _root, _, _) = harness().await;
    let host = &session.0.host;
    let mut fixture: Registration = serde_json::from_value(json!({
        "name":"acme.bin","mimeType":"application/octet-stream",
        "dataBase64":STANDARD.encode(vec![42; FIXTURE_BYTES]),
        "provenance":"synthetic_generated","cleared":{"by":"Acme","method":"Generated"}
    }))
    .unwrap();
    let registered = host.fixture_register(fixture.clone()).await.unwrap();
    let file = serde_json::from_value(registered["fixture"]["reference"].clone()).unwrap();
    let (params, _) = host.attachment_parameters(vec![file]).await.unwrap();
    assert!(params["payloads"][0]["buffer"].as_str() == Some(fixture.data_base64.as_str()));
    assert_eq!(
        host.attachment_parameters(vec![file, file])
            .await
            .unwrap_err()
            .code,
        "FIXTURE_BOUNDS"
    );
    fixture.data_base64 = STANDARD.encode(vec![42; FIXTURE_BYTES + 1]);
    assert_eq!(
        host.fixture_register(fixture).await.unwrap_err().code,
        "FIXTURE_BOUNDS"
    );
    close(&session).await;
}

async fn close(session: &Session) {
    let id = session.disconnect(5000).unwrap();
    assert_eq!(complete(session, id).await.outcome, Outcome::Succeeded);
}

fn input_observation(session: &Session, page: Reference) -> (Reference, Value) {
    let geometry = json!({"url":"about:blank","viewport":{"width":800,"height":600},"dpr":1,"scroll":{"x":0,"y":0},"visualViewport":{"x":0,"y":0,"scale":1},"viewportEpoch":1,"pointerLocked":false});
    let frame = session
        .0
        .host
        .state
        .lock()
        .unwrap()
        .resources
        .values()
        .find(|resource| resource.kind == Kind::Frame && resource.parent == Some(page))
        .unwrap()
        .reference;
    let reference = session.0.host.inputs.lock().unwrap().observe(
        page,
        frame,
        geometry.clone(),
        std::time::Instant::now(),
        session.0.host.input_epoch.load(Ordering::SeqCst),
    );
    (reference, geometry)
}

/// A panicking operation task settles as an unknown outcome and fences the
/// session, instead of leaving the operation unsettled; results pass through.
#[tokio::test]
async fn panicking_operation_settles_unknown_and_fences_the_session() {
    let (session, _peer, _directory, context, _page) = harness().await;
    let host = session.0.host.clone();
    assert_eq!(
        host.unwinding(async { Ok(json!({"acme":true})) })
            .await
            .unwrap(),
        json!({"acme":true})
    );
    assert_eq!(host.state.lock().unwrap().phase, Phase::Ready);
    let error = host
        .unwinding(async { panic!("Acme operation panic") })
        .await
        .unwrap_err();
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    let partial = error.partial.unwrap();
    assert_eq!(partial["internalFailure"], true);
    assert_eq!(partial["replayed"], false);
    assert_eq!(partial["nextAction"], "browser_disconnect");
    {
        let state = host.state.lock().unwrap();
        assert_eq!(state.phase, Phase::Lost);
        assert_eq!(state.failure.as_deref(), Some("OUTCOME_UNKNOWN"));
    }
    let refused = session
        .submit(Uuid::new_v4(), Request::ContextSelect { context }, 5000)
        .unwrap_err();
    assert_eq!(refused.code, "SESSION_NOT_READY");
}

/// A panic that poisons the state lock still ends in a settled operation, a
/// readable lost session and clean refusals rather than a hang or caller panic.
#[tokio::test]
async fn poisoned_state_lock_still_settles_and_refuses() {
    let (session, _peer, _directory, context, _page) = harness().await;
    let host = session.0.host.clone();
    std::thread::scope(|scope| {
        let poisoned = scope
            .spawn(|| {
                let _state = host.state.lock().unwrap();
                panic!("Acme state poison");
            })
            .join();
        assert!(poisoned.is_err());
    });
    assert!(host.state.is_poisoned());
    let id = session
        .submit(Uuid::new_v4(), Request::ContextSelect { context }, 5000)
        .unwrap();
    let operation = session
        .wait(id, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert!(operation.settled, "{operation:?}");
    assert_eq!(operation.outcome, Outcome::Unknown);
    assert_eq!(operation.code.as_deref(), Some("OUTCOME_UNKNOWN"));
    assert_eq!(session.status()["phase"], "lost");
    assert_eq!(
        session
            .submit(Uuid::new_v4(), Request::ContextSelect { context }, 5000)
            .unwrap_err()
            .code,
        "SESSION_NOT_READY"
    );
}

/// A fenced session whose state lock is poisoned still releases: disconnect
/// settles, every owned resource is dropped and the driver connection closes.
#[tokio::test]
async fn poisoned_state_lock_still_disconnects_and_releases_the_driver() {
    let (session, _peer, _directory, _context, _page) = harness().await;
    let host = session.0.host.clone();
    std::thread::scope(|scope| {
        let poisoned = scope
            .spawn(|| {
                let _state = host.state.lock().unwrap();
                panic!("Acme state poison");
            })
            .join();
        assert!(poisoned.is_err());
    });
    assert!(host.state.is_poisoned());
    let id = session.disconnect(5000).unwrap();
    let operation = complete(&session, id).await;
    assert_eq!(operation.outcome, Outcome::Succeeded, "{operation:?}");
    assert_eq!(operation.result.unwrap()["closed"], true);
    assert_eq!(session.status()["phase"], "closed");
    assert!(session.status()["resources"].as_array().unwrap().is_empty());
    assert!(host.connection().unwrap().bootstrap().is_err());
}

/// A single Chrome-only throttling request settles as an engine refusal
/// instead of reaching the resource dispatch, before any driver call.
#[tokio::test]
async fn page_throttling_settles_as_an_engine_refusal_without_a_driver_call() {
    let (session, mut peer, _directory, _context, page) = harness().await;
    let refused = session
        .submit(
            Uuid::new_v4(),
            Request::PageEmulateThrottling {
                page,
                options: crate::browser::settings::ThrottlingOptions {
                    cpu_rate: Some(2.0),
                    network: None,
                },
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, refused).await.code.as_deref(),
        Some("THROTTLING_UNSUPPORTED")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn context_settings_refuse_unsupported_grants_before_dispatch_and_retain_partial_failure() {
    use crate::browser::settings::{ContextSetting, Permission};
    let (session, mut peer, _directory, context, _) = harness().await;
    let refused = session
        .submit(
            Uuid::new_v4(),
            Request::ContextConfigure {
                context,
                setting: ContextSetting::PermissionsGrant {
                    origin: "https://example.com".into(),
                    permissions: vec![Permission::Geolocation, Permission::Camera],
                },
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, refused).await.code.as_deref(),
        Some("PERMISSION_UNSUPPORTED")
    );
    let configure = session
        .submit(
            Uuid::new_v4(),
            Request::ContextConfigure {
                context,
                setting: ContextSetting::Offline { offline: true },
            },
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "setOffline");
    assert_eq!(call["params"], json!({"offline":true}));
    peer.send(&method_error(call["id"].clone())).await;
    let failed = complete(&session, configure).await;
    assert_eq!(failed.outcome, Outcome::Unknown);
    assert_eq!(failed.result.unwrap()["stateMayHaveChanged"], true);
    let clear = session
        .submit(
            Uuid::new_v4(),
            Request::ContextConfigure {
                context,
                setting: ContextSetting::Geolocation { geolocation: None },
            },
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "setGeolocation");
    assert_eq!(call["params"], json!({}));
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, clear).await.result.unwrap()["effectsObserved"],
        false
    );
    close(&session).await;
}
async fn reply_geometry(peer: &mut Fixture, geometry: &Value) {
    let call = peer.request().await;
    assert_eq!(call["method"], "evaluateExpression");
    peer.send(&json!({"id":call["id"],"result":{"value":crate::browser::values::argument(geometry).unwrap()["value"]}})).await;
}

fn compound_request(page: Reference, action: crate::browser::page::Action) -> Request {
    Request::PageAction {
        page,
        frame: None,
        locator: crate::browser::page::Locator::Css {
            selector: "#acme".into(),
        },
        action,
        force: false,
        postcondition: None,
    }
}

#[tokio::test]
async fn compound_failure_reserves_holds_until_reply_and_never_replays_possible_releases() {
    use crate::browser::page::Action;
    let (session, mut peer, _directory, _, page) = harness().await;
    let operation = session
        .submit(
            Uuid::new_v4(),
            compound_request(
                page,
                Action::Press {
                    key: "Shift+a".into(),
                },
            ),
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "press");
    let held = session.0.host.inputs.lock().unwrap().held[&page].clone();
    assert_eq!(held.inflight, 1);
    assert_eq!(held.keys.len(), 2);
    let release = session
        .submit(Uuid::new_v4(), Request::InputRelease {}, 5000)
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.request())
            .await
            .is_err()
    );
    peer.send(&method_error(call["id"].clone())).await;
    let failed = complete(&session, operation).await;
    assert_eq!(failed.outcome, Outcome::Unknown);
    assert_eq!(
        failed.result.unwrap()["cleanup"]["requiresOwnedPageClose"],
        true
    );
    let cleanup = complete(&session, release).await.result.unwrap();
    assert_eq!(cleanup["pages"][0]["cleanup"]["complete"], false);
    assert_eq!(
        cleanup["pages"][0]["cleanup"]["state"]["unknownKeyReleases"],
        2
    );
    let next = session
        .submit(
            Uuid::new_v4(),
            compound_request(page, Action::Click {}),
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, next).await.code.as_deref(),
        Some("INPUT_STATE_CONFLICT")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.request())
            .await
            .is_err()
    );
    let closing = session
        .submit(Uuid::new_v4(), Request::PageClose { page }, 5000)
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "close");
    peer.send(&json!({"guid":"page","method":"close","params":{}}))
        .await;
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, closing).await.outcome,
        Outcome::Succeeded
    );
    assert!(
        !session
            .0
            .host
            .inputs
            .lock()
            .unwrap()
            .held
            .contains_key(&page)
    );
    close(&session).await;
}

#[tokio::test]
async fn compound_validation_and_success_leave_no_possible_holds() {
    use crate::browser::page::Action;
    let (session, mut peer, _directory, _, page) = harness().await;
    let invalid = session
        .submit(
            Uuid::new_v4(),
            compound_request(
                page,
                Action::Press {
                    key: "Shift+NotAKey".into(),
                },
            ),
            5000,
        )
        .unwrap_err();
    assert_eq!(invalid.code, "INVALID_BROWSER_KEY");
    assert!(
        !session
            .0
            .host
            .inputs
            .lock()
            .unwrap()
            .held
            .contains_key(&page)
    );
    let operation = session
        .submit(
            Uuid::new_v4(),
            compound_request(
                page,
                Action::Press {
                    key: "Control++".into(),
                },
            ),
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "press");
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, operation).await.outcome,
        Outcome::Succeeded
    );
    assert!(session.0.host.inputs.lock().unwrap().held[&page].empty());
    close(&session).await;
}

#[tokio::test]
async fn compound_expired_before_dispatch_clears_its_reservation() {
    use crate::browser::page::Action;
    let (session, mut peer, _directory, _, page) = harness().await;
    let wire = session
        .0
        .host
        .connection()
        .unwrap()
        .resolve("frame", "Frame")
        .await
        .unwrap();
    let error = session
        .0
        .host
        .compound_input(
            page,
            &wire,
            &Action::Click {},
            "click",
            json!({"selector":"css=#acme","strict":true}),
            Instant::now() - Duration::from_millis(1),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "NOT_DISPATCHED");
    assert!(session.0.host.inputs.lock().unwrap().held[&page].empty());
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn input_release_fences_a_batch_but_waits_for_dispatched_down_before_up() {
    let (session, mut peer, _directory, _, page) = harness().await;
    let (observation, geometry) = input_observation(&session, page);
    let operation = session
        .submit(
            Uuid::new_v4(),
            Request::PageInput {
                page,
                observation: Some(observation),
                actions: vec![crate::browser::input::Action::KeyPress {
                    key: "Shift+A".into(),
                    delay_ms: 0,
                }],
            },
            5000,
        )
        .unwrap();
    reply_geometry(&mut peer, &geometry).await;
    let down = peer.request().await;
    assert_eq!(down["method"], "keyboardDown");
    assert_eq!(down["params"]["key"], "Shift");
    let release = session
        .submit(Uuid::new_v4(), Request::InputRelease {}, 5000)
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.request())
            .await
            .is_err()
    );
    peer.send(&json!({"id":down["id"],"result":{}})).await;
    let up = peer.request().await;
    assert_eq!(up["method"], "keyboardUp");
    assert_eq!(up["params"]["key"], "Shift");
    peer.send(&json!({"id":up["id"],"result":{}})).await;
    let result = complete(&session, operation).await;
    assert_eq!(result.code.as_deref(), Some("INPUT_PARTIAL"));
    assert_eq!(result.result.unwrap()["cleanup"]["complete"], true);
    assert_eq!(
        complete(&session, release).await.outcome,
        Outcome::Succeeded
    );
    canonic_mcp_kit::schema::validate(&contract::schemas()["status"], &session.status()).unwrap();
    close(&session).await;
}

#[tokio::test]
async fn uncertain_key_release_is_retained_and_never_replayed() {
    let (session, mut peer, _directory, _, page) = harness().await;
    let (observation, geometry) = input_observation(&session, page);
    let operation = session
        .submit(
            Uuid::new_v4(),
            Request::PageInput {
                page,
                observation: Some(observation),
                actions: vec![crate::browser::input::Action::KeyPress {
                    key: "a".into(),
                    delay_ms: 0,
                }],
            },
            5000,
        )
        .unwrap();
    reply_geometry(&mut peer, &geometry).await;
    let down = peer.request().await;
    peer.send(&json!({"id":down["id"],"result":{}})).await;
    let up = peer.request().await;
    assert_eq!(up["method"], "keyboardUp");
    peer.send(&method_error(up["id"].clone())).await;
    let result = complete(&session, operation).await;
    assert_eq!(result.code.as_deref(), Some("INPUT_PARTIAL"));
    assert_eq!(
        result.result.unwrap()["cleanup"]["requiresOwnedPageClose"],
        true
    );
    let release = session
        .submit(Uuid::new_v4(), Request::InputRelease {}, 5000)
        .unwrap();
    assert_eq!(
        complete(&session, release).await.result.unwrap()["pages"][0]["cleanup"]["complete"],
        false
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn stale_input_observation_refuses_dispatch_after_viewport_epoch_changes() {
    let (session, mut peer, _directory, _, page) = harness().await;
    let (observation, _geometry) = input_observation(&session, page);
    session.0.host.inputs.lock().unwrap().invalidate(page);
    let operation = session
        .submit(
            Uuid::new_v4(),
            Request::PageInput {
                page,
                observation: Some(observation),
                actions: vec![crate::browser::input::Action::InsertText {
                    text: "Acme".into(),
                }],
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, operation).await.code.as_deref(),
        Some("INPUT_OBSERVATION_STALE")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

#[tokio::test]
async fn closing_the_selected_page_never_selects_an_arbitrary_survivor() {
    let (session, mut peer, _directory, _context, page) = harness().await;
    let id = session
        .submit(Uuid::new_v4(), Request::PageClose { page }, 5000)
        .unwrap();
    let request = peer.request().await;
    assert_eq!(request["method"], "close");
    peer.send(&json!({"guid":"page","method":"close","params":{}}))
        .await;
    peer.send(&json!({"id":request["id"],"result":{}})).await;
    assert_eq!(complete(&session, id).await.outcome, Outcome::Succeeded);
    assert!(session.status()["selectedPage"].is_null());
    let id = session
        .submit(Uuid::new_v4(), Request::PageSelect { page }, 5000)
        .unwrap();
    assert_eq!(
        complete(&session, id).await.code.as_deref(),
        Some("STALE_REFERENCE")
    );
    close(&session).await;
}

#[tokio::test]
async fn duplicate_operation_ids_and_dropped_waits_do_not_replay_allocations() {
    let (session, mut peer, _directory, context, _page) = harness().await;
    let id = Uuid::new_v4();
    let request = Request::PageCreate {
        context,
        select: false,
    };
    session.submit(id, request.clone(), 5000).unwrap();
    let wire = peer.request().await;
    assert_eq!(wire["method"], "newPage");
    let mut waiting = Box::pin(session.wait(id, deadline()));
    assert!(futures_util::poll!(waiting.as_mut()).is_pending());
    drop(waiting);
    session.cancel(id).await.unwrap();
    assert_eq!(session.submit(id, request, 5000).unwrap(), id);
    assert_eq!(
        session
            .submit(id, Request::PageList {}, 5000)
            .unwrap_err()
            .code,
        "OPERATION_CONFLICT"
    );
    peer.send(&created(
        "next-frame",
        "Frame",
        json!({"url":"about:blank","name":"","loadStates":[]}),
    ))
    .await;
    peer.send(&created(
        "next-page",
        "Page",
        json!({"mainFrame":{"guid":"next-frame"},"isClosed":false}),
    ))
    .await;
    peer.send(&json!({"id":wire["id"],"result":{"page":{"guid":"next-page"}}}))
        .await;
    let subscription = peer.request().await;
    assert_eq!(subscription["method"], "updateSubscription");
    peer.send(&json!({"id":subscription["id"],"result":{}}))
        .await;
    let outcome = complete(&session, id).await;
    assert_eq!(outcome.outcome, Outcome::Succeeded);
    let id = session
        .submit(Uuid::new_v4(), Request::PageList {}, 5000)
        .unwrap();
    assert_eq!(
        complete(&session, id).await.result.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    close(&session).await;
}

#[tokio::test]
async fn a_driver_call_that_never_returns_frees_the_lane_at_the_ceiling() {
    let (session, mut peer, _directory, context, _page) = harness().await;
    session.0.host.ceiling_ms.store(300, Ordering::SeqCst);
    let create = Request::PageCreate {
        context,
        select: false,
    };
    let stuck = session.submit(Uuid::new_v4(), create.clone(), 100).unwrap();
    assert_eq!(peer.request().await["method"], "newPage");
    // Queued behind it, a cancellation or the operation's own deadline ends
    // the wait for the lane.
    let cancelled = session
        .submit(Uuid::new_v4(), create.clone(), 5000)
        .unwrap();
    let expired = session.submit(Uuid::new_v4(), create, 50).unwrap();
    session.cancel(cancelled).await.unwrap();
    assert_eq!(
        complete(&session, cancelled).await.outcome,
        Outcome::Cancelled
    );
    assert_eq!(
        complete(&session, expired).await.code.as_deref(),
        Some("NOT_DISPATCHED")
    );
    // The driver never answers: at the ceiling the effect is unknown and the
    // session admits no further work.
    assert_eq!(complete(&session, stuck).await.outcome, Outcome::Unknown);
    assert_eq!(session.status()["failure"], "OUTCOME_UNKNOWN");
    assert_eq!(
        session
            .submit(Uuid::new_v4(), Request::PageList {}, 5000)
            .unwrap_err()
            .code,
        "SESSION_NOT_READY"
    );
    close(&session).await;
}

#[test]
fn journal_records_keep_result_references_but_no_content() {
    let page = Reference {
        connection_id: Uuid::new_v4(),
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let result = json!({"page":page,"result":{"value":"acme-private-page-text","items":[{"reference":page,"title":"Acme form"}]}});
    let journaled = json!(references(Some(&result))).to_string();
    assert!(!journaled.contains("acme-private-page-text"));
    assert!(!journaled.contains("Acme form"));
    assert_eq!(references(Some(&result)), vec![json!(page), json!(page)]);
    assert!(references(None).is_empty());
}

#[tokio::test]
async fn settled_operations_journal_no_result_content() {
    let (session, _peer, _directory, _context, page) = harness().await;
    let id = session
        .submit(Uuid::new_v4(), Request::PageList {}, 5000)
        .unwrap();
    assert_eq!(complete(&session, id).await.outcome, Outcome::Succeeded);
    let mut settled = Vec::new();
    for entry in std::fs::read_dir(session.journal_directory()).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let record: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            if record["settled"] == true {
                settled.push(record);
            }
        }
    }
    let record = settled
        .iter()
        .find(|record| record["operationId"] == json!(id))
        .unwrap();
    assert!(record.get("result").is_none(), "{record}");
    assert!(
        record["references"]
            .as_array()
            .unwrap()
            .contains(&json!(page)),
        "{record}"
    );
    close(&session).await;
}

#[tokio::test]
async fn a_foreign_context_is_refused_before_sending_any_wire_request() {
    let (session, _peer, _directory, mut context, _) = harness().await;
    context.connection_id = Uuid::new_v4();
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::PageCreate {
                context,
                select: false,
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, id).await.code.as_deref(),
        Some("FOREIGN_REFERENCE")
    );
    close(&session).await;
}

#[tokio::test]
async fn a_closing_context_fences_new_allocations_but_status_and_lists_stay_available() {
    let (session, mut peer, _directory, context, _) = harness().await;
    let closing = session
        .submit(Uuid::new_v4(), Request::ContextClose { context }, 5000)
        .unwrap();
    let close = peer.request().await;
    let allocation = session
        .submit(
            Uuid::new_v4(),
            Request::PageCreate {
                context,
                select: false,
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, allocation).await.code.as_deref(),
        Some("SCOPE_CLOSING")
    );
    let listing = session
        .submit(Uuid::new_v4(), Request::PageList {}, 5000)
        .unwrap();
    assert_eq!(
        complete(&session, listing).await.outcome,
        Outcome::Succeeded
    );
    peer.send(&json!({"id":close["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, closing).await.outcome,
        Outcome::Succeeded
    );
    let disconnect = session.disconnect(5000).unwrap();
    assert_eq!(
        complete(&session, disconnect).await.outcome,
        Outcome::Succeeded
    );
}

#[tokio::test]
async fn cancelled_queued_work_never_reaches_the_protocol() {
    let (session, _peer, _directory, context, _) = harness().await;
    let lane = session.0.host.lane.lock().await;
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::PageCreate {
                context,
                select: false,
            },
            5000,
        )
        .unwrap();
    session.cancel(id).await.unwrap();
    drop(lane);
    let result = complete(&session, id).await;
    assert_eq!(result.outcome, Outcome::Cancelled);
    assert!(result.cancel_requested);
    close(&session).await;
}

#[tokio::test]
async fn expired_dialogs_are_dismissed_once_without_blocking_the_operation_lane() {
    let (session, mut peer, _directory, _, _) = harness().await;
    peer.send(&created(
        "expired-dialog",
        "Dialog",
        json!({"page":{"guid":"page"},"type":"confirm","message":"Acme","defaultValue":""}),
    ))
    .await;
    peer.send(
        &json!({"guid":"context","method":"dialog","params":{"dialog":{"guid":"expired-dialog"}}}),
    )
    .await;
    let reference = tokio::time::timeout_at(deadline(), async {
        loop {
            if let Some(reference) = session
                .0
                .host
                .callbacks
                .lock()
                .unwrap()
                .keys()
                .next()
                .copied()
            {
                break reference;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    session
        .0
        .host
        .callbacks
        .lock()
        .unwrap()
        .insert(reference, Instant::now());
    let error = session
        .0
        .host
        .callback_resolve(
            reference,
            CallbackResponse::Dialog {
                accept: true,
                prompt_text: None,
            },
            deadline(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "CALLBACK_EXPIRED");
    let host = session.0.host.clone();
    let expiration = tokio::spawn(async move {
        host.expire_callback(reference).await;
    });
    let dismiss = peer.request().await;
    assert_eq!(dismiss["method"], "dismiss");
    peer.send(&json!({"id":dismiss["id"],"result":{}})).await;
    expiration.await.unwrap();
    assert!(
        !session
            .0
            .host
            .callbacks
            .lock()
            .unwrap()
            .contains_key(&reference)
    );
    close(&session).await;
}

#[tokio::test]
async fn dialog_responses_remain_responsive_while_a_page_allocation_waits() {
    let (session, mut peer, _directory, context, _) = harness().await;
    let pending = session
        .submit(
            Uuid::new_v4(),
            Request::PageCreate {
                context,
                select: false,
            },
            5000,
        )
        .unwrap();
    let allocation = peer.request().await;
    peer.send(&created(
        "dialog",
        "Dialog",
        json!({"page":{"guid":"page"},"type":"alert","message":"Acme","defaultValue":""}),
    ))
    .await;
    peer.send(&json!({"guid":"context","method":"dialog","params":{"dialog":{"guid":"dialog"}}}))
        .await;
    let callback = tokio::time::timeout_at(deadline(), async {
        loop {
            if let Some(callback) = session
                .0
                .host
                .callbacks
                .lock()
                .unwrap()
                .keys()
                .next()
                .copied()
            {
                break callback;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::CallbackResolve {
                callback,
                response: CallbackResponse::Dialog {
                    accept: true,
                    prompt_text: None,
                },
            },
            5000,
        )
        .unwrap();
    let response = peer.request().await;
    assert_eq!(response["method"], "accept");
    peer.send(&json!({"id":response["id"],"result":{}})).await;
    assert_eq!(complete(&session, id).await.outcome, Outcome::Succeeded);
    peer.send(&method_error(allocation["id"].clone())).await;
    assert_eq!(complete(&session, pending).await.outcome, Outcome::Failed);
    assert!(
        session
            .private_failures()
            .iter()
            .any(|failure| failure.contains("Acme fixture refusal"))
    );
    assert!(
        !session
            .status()
            .to_string()
            .contains("Acme fixture refusal")
    );
    close(&session).await;
}

#[tokio::test]
async fn navigation_invalidates_document_handles_and_journals_omit_requests() {
    let (session, mut peer, _directory, _, page) = harness().await;
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::HandleCreate {
                page,
                expression: "({privateValue:'acme-private-sentinel'})".into(),
            },
            5000,
        )
        .unwrap();
    let request = peer.request().await;
    assert_eq!(request["method"], "evaluateExpressionHandle");
    peer.send(&created("handle", "JSHandle", json!({"preview":"Object"})))
        .await;
    peer.send(&json!({"id":request["id"],"result":{"handle":{"guid":"handle"}}}))
        .await;
    let handle: Reference =
        serde_json::from_value(complete(&session, id).await.result.unwrap()["handle"].clone())
            .unwrap();
    peer.send(&json!({"guid":"frame","method":"navigated","params":{"url":"https://example.com","name":"","newDocument":{}}})).await;
    let disposal = peer.request().await;
    assert_eq!(disposal["method"], "dispose");
    peer.send(&json!({"id":disposal["id"],"result":{}})).await;
    tokio::time::timeout_at(deadline(), async {
        while session
            .0
            .host
            .state
            .lock()
            .unwrap()
            .resource(handle, None)
            .is_ok()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let id = session
        .submit(Uuid::new_v4(), Request::HandleDispose { handle }, 5000)
        .unwrap();
    assert_eq!(
        complete(&session, id).await.code.as_deref(),
        Some("STALE_REFERENCE")
    );
    close(&session).await;
    for entry in std::fs::read_dir(session.journal_directory()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("acme-private-sentinel"));
    }
}

#[tokio::test]
async fn evaluation_retains_handles_and_refuses_foreign_arguments_before_dispatch() {
    use crate::browser::evaluation::EvaluationValue;
    let (session, mut peer, _directory, _, page) = harness().await;
    let evaluate = |scope, expression: &str, argument, return_handle| Request::Evaluate {
        scope,
        expression: expression.into(),
        is_function: Some(true),
        argument,
        return_handle,
    };
    let id = session
        .submit(
            Uuid::new_v4(),
            evaluate(
                page,
                "() => ({name:'Acme-private-evaluation'})",
                EvaluationValue::default(),
                true,
            ),
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "evaluateExpressionHandle");
    assert_eq!(
        call["params"]["arg"],
        json!({"value":{"v":"undefined"},"handles":[]})
    );
    peer.send(&created(
        "evaluated",
        "JSHandle",
        json!({"preview":"Object"}),
    ))
    .await;
    peer.send(&json!({"id":call["id"],"result":{"handle":{"guid":"evaluated"}}}))
        .await;
    let result = complete(&session, id).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    let handle: Reference =
        serde_json::from_value(result.result.unwrap()["handle"].clone()).unwrap();
    let id = session
        .submit(
            Uuid::new_v4(),
            evaluate(
                page,
                "value => value.name",
                EvaluationValue::Handle { handle },
                false,
            ),
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(
        call["params"]["arg"],
        json!({"value":{"h":0},"handles":[{"guid":"evaluated"}]})
    );
    peer.send(&json!({"id":call["id"],"result":{"value":{"s":"Acme-private-evaluation"}}}))
        .await;
    assert_eq!(
        complete(&session, id).await.result.unwrap()["value"],
        json!({"kind":"json","value":"Acme-private-evaluation"})
    );

    let id = session
        .submit(
            Uuid::new_v4(),
            evaluate(
                handle,
                "object => object.name",
                EvaluationValue::default(),
                false,
            ),
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["guid"], "evaluated");
    assert_eq!(
        call["params"]["arg"],
        json!({"value":{"v":"undefined"},"handles":[]})
    );
    peer.send(&json!({"id":call["id"],"result":{"value":{"v":"undefined"}}}))
        .await;
    assert_eq!(
        complete(&session, id).await.result.unwrap()["value"]["kind"],
        "undefined"
    );

    foreign_evaluation_handle_is_refused(&session, page, handle).await;

    let id = session
        .submit(Uuid::new_v4(), Request::HandleRead { handle }, 5000)
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "jsonValue");
    peer.send(&json!({"id":call["id"],"result":{"value":{"id":1,"a":[{"ref":1}]}}}))
        .await;
    assert_eq!(
        complete(&session, id).await.result.unwrap()["value"],
        json!({"kind":"array","id":1,"items":[{"kind":"reference","id":1}]})
    );
    close(&session).await;
    assert_evaluation_journal_private(&session);
}

fn assert_evaluation_journal_private(session: &Session) {
    for entry in std::fs::read_dir(session.journal_directory()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("Acme-private-evaluation"));
    }
}

async fn foreign_evaluation_handle_is_refused(
    session: &Session,
    page: Reference,
    handle: Reference,
) {
    let foreign = Reference {
        connection_id: Uuid::new_v4(),
        ..handle
    };
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::Evaluate {
                scope: page,
                expression: "value => value".into(),
                is_function: Some(true),
                argument: crate::browser::evaluation::EvaluationValue::Handle { handle: foreign },
                return_handle: false,
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(session, id).await.code.as_deref(),
        Some("FOREIGN_REFERENCE")
    );
}

#[tokio::test]
async fn evaluation_result_refusal_preserves_completed_dispatch() {
    let (session, mut peer, _directory, _, page) = harness().await;
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::Evaluate {
                scope: page,
                expression: "() => 1".into(),
                is_function: Some(true),
                argument: Default::default(),
                return_handle: false,
            },
            5000,
        )
        .unwrap();
    let call = peer.request().await;
    // The wire schema permits this shape, but the value codec refuses a
    // dangling reference rather than returning undefined or replaying input.
    peer.send(&json!({"id":call["id"],"result":{"value":{"ref":99}}}))
        .await;
    let result = complete(&session, id).await;
    assert_eq!(result.code.as_deref(), Some("INVALID_VALUE_REFERENCE"));
    assert_eq!(result.result.unwrap()["dispatchCompleted"], true);
    close(&session).await;
}

async fn diagnostic_subscription(
    session: &Session,
    peer: &mut Fixture,
    context: Reference,
    enabled: bool,
) {
    let request = if enabled {
        Request::DiagnosticsStart { context }
    } else {
        Request::DiagnosticsStop { context }
    };
    let id = session.submit(Uuid::new_v4(), request, 5000).unwrap();
    for event in crate::browser::diagnostics::SUBSCRIPTIONS {
        let call = peer.request().await;
        assert_eq!(call["method"], "updateSubscription");
        assert_eq!(call["params"], json!({"event":event,"enabled":enabled}));
        peer.send(&json!({"id":call["id"],"result":{}})).await;
    }
    assert_eq!(complete(session, id).await.outcome, Outcome::Succeeded);
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One captured history verifies exact request identities, scoped correlation and metadata privacy"
)]
async fn diagnostics_correlate_exact_requests_and_default_to_metadata() {
    let (session, mut peer, _directory, context, page) = harness().await;
    diagnostic_subscription(&session, &mut peer, context, true).await;
    peer.send(&created("diagnostic-request", "Request", json!({"frame":{"guid":"frame"},"url":"https://example.com/conflict?token=Acme-query-secret","method":"POST","resourceType":"fetch","headers":[{"name":"Authorization","value":"Bearer Acme-header-secret"}],"postData":"QWNtZS1ib2R5LXNlY3JldA==","isNavigationRequest":false}))).await;
    peer.send(&json!({"guid":"context","method":"request","params":{"request":{"guid":"diagnostic-request"},"page":{"guid":"page"}}})).await;
    peer.send(&created("diagnostic-response", "Response", json!({"request":{"guid":"diagnostic-request"},"url":"https://example.com/conflict?token=Acme-query-secret","status":409,"statusText":"Conflict","headers":[{"name":"Set-Cookie","value":"Acme-cookie-secret"}],"timing":{"startTime":0,"domainLookupStart":-1,"domainLookupEnd":-1,"connectStart":-1,"secureConnectionStart":-1,"connectEnd":-1,"requestStart":0,"responseStart":1},"fromServiceWorker":false}))).await;
    peer.send(&json!({"guid":"context","method":"response","params":{"response":{"guid":"diagnostic-response"},"page":{"guid":"page"}}})).await;
    peer.send(&created(
        "console-argument",
        "JSHandle",
        json!({"preview":"Acme private object"}),
    ))
    .await;
    peer.send(&created(
        "acme-console-worker",
        "Worker",
        json!({"url":"https://example.com/worker.js"}),
    ))
    .await;
    peer.send(&json!({"guid":"context","method":"console","params":{"type":"error","text":"Acme-console-private-text","args":[{"guid":"console-argument"}],"location":{"url":"https://example.com/?token=Acme-query-secret","lineNumber":0,"columnNumber":0},"timestamp":0,"page":{"guid":"page"},"worker":{"guid":"acme-console-worker"}}})).await;
    let disposal = peer.request().await;
    assert_eq!(disposal["method"], "dispose");
    assert_eq!(disposal["guid"], "console-argument");
    peer.send(&json!({"id":disposal["id"],"result":{}})).await;
    peer.send(&json!({"guid":"context","method":"pageError","params":{"error":{"error":{"name":"Error","message":"Acme-private-error-message","stack":"Acme-private-stack\n at acmeDiagnosticFailure (https://example.com/app.js:4:2)"}},"page":{"guid":"page"},"location":{"url":"https://example.com/app.js","line":4,"column":2}}})).await;
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::DiagnosticsRead {
                context,
                query: Default::default(),
            },
            5000,
        )
        .unwrap();
    let result = complete(&session, id).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    let result = result.result.unwrap();
    let events = result["events"].as_array().unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(
        events[0]["data"]["requestId"],
        events[1]["data"]["requestId"]
    );
    assert_eq!(events[1]["data"]["status"], 409);
    assert_eq!(events[0]["page"], json!(page));
    let worker: Reference = serde_json::from_value(events[2]["data"]["worker"].clone()).unwrap();
    assert!(
        session
            .0
            .host
            .state
            .lock()
            .unwrap()
            .resource(worker, Some(Kind::Worker))
            .is_ok()
    );
    assert_eq!(
        session
            .0
            .host
            .state
            .lock()
            .unwrap()
            .ancestor(worker, Kind::Page),
        Some(page)
    );
    for sentinel in [
        "Acme-query-secret",
        "Acme-header-secret",
        "Acme-cookie-secret",
        "Acme-console-private-text",
        "Acme-private-stack",
        "Acme-private-error-message",
        "postData",
        "console-argument",
        "diagnostic-request",
    ] {
        assert!(
            !result.to_string().contains(sentinel),
            "{sentinel}: {result}"
        );
        assert!(!session.status().to_string().contains(sentinel));
    }
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::DiagnosticsRead {
                context,
                query: crate::browser::diagnostics::Query {
                    include_text: true,
                    kind: Some(crate::browser::diagnostics::DiagnosticKind::Console),
                    worker: Some(worker),
                    levels: vec!["ERROR".into()],
                    ..Default::default()
                },
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, id).await.result.unwrap()["events"][0]["data"]["text"],
        "Acme-console-private-text"
    );
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::DiagnosticsRead {
                context,
                query: crate::browser::diagnostics::Query {
                    include_stack: true,
                    kind: Some(crate::browser::diagnostics::DiagnosticKind::PageError),
                    ..Default::default()
                },
            },
            5000,
        )
        .unwrap();
    let stacks = complete(&session, id).await.result.unwrap();
    assert!(
        stacks["events"][0]["data"]["stack"]["text"]
            .as_str()
            .unwrap()
            .contains("Acme-private-stack")
    );
    assert!(stacks["events"][0]["data"].get("text").is_none());
    diagnostic_subscription(&session, &mut peer, context, false).await;
    peer.send(&created(
        "late-console-argument",
        "JSHandle",
        json!({"preview":"Acme late object"}),
    ))
    .await;
    peer.send(&json!({"guid":"context","method":"console","params":{"type":"log","text":"Acme stopped private text","args":[{"guid":"late-console-argument"}],"location":{"url":"https://example.com/","lineNumber":0,"columnNumber":0},"timestamp":1,"page":{"guid":"page"}}})).await;
    let disposal = peer.request().await;
    assert_eq!(disposal["guid"], "late-console-argument");
    assert_eq!(disposal["method"], "dispose");
    peer.send(&json!({"id":disposal["id"],"result":{}})).await;
    assert!(
        !session
            .0
            .host
            .diagnostics
            .lock()
            .unwrap()
            .read(
                context,
                &crate::browser::diagnostics::Query {
                    include_text: true,
                    ..Default::default()
                }
            )
            .unwrap()
            .to_string()
            .contains("Acme stopped private text")
    );
    close(&session).await;
}

#[tokio::test]
async fn har_filter_rechecks_installation_candidates_before_scheduling_archive_replies() {
    let (session, mut peer, root, _, page) = harness().await;
    let input = root.path().join("filter-installation");
    crate::artifacts::directory(&input).unwrap();
    let path = input.join("source.har");
    crate::artifacts::write(&path, br#"{"log":{"version":"1.2","entries":[]}}"#).unwrap();
    let archive = session
        .0
        .host
        .har_archives
        .lock()
        .unwrap()
        .register(&path, session.journal_directory())
        .unwrap();
    let archive_id = serde_json::from_value(archive["archiveId"].clone()).unwrap();
    let setting = session
        .submit(
            Uuid::new_v4(),
            Request::PageHarSet {
                page,
                archive_id: Some(archive_id),
                not_found: contract::HarNotFound::Abort,
                url_filter: Some(crate::browser::routing::Matcher::Regex {
                    pattern: "/selected$".into(),
                    flags: String::new(),
                }),
            },
            5000,
        )
        .unwrap();
    let enable = peer.request().await;
    assert_eq!(enable["method"], "setNetworkInterceptionPatterns");
    peer.send(&created("install-request","Request",json!({"frame":{"guid":"frame"},"url":"https://example.com/nonmatch","resourceType":"fetch","method":"GET","headers":[],"isNavigationRequest":false}))).await;
    peer.send(&created(
        "install-route",
        "Route",
        json!({"request":{"guid":"install-request"}}),
    ))
    .await;
    peer.send(&json!({"guid":"page","method":"route","params":{"route":{"guid":"install-route"}}}))
        .await;
    session
        .0
        .host
        .connection()
        .unwrap()
        .event_barrier(deadline())
        .await
        .unwrap();
    let until = deadline();
    loop {
        if session.0.host.policy_routes(page).len() == 1
            && session.0.host.routing.pending.lock().unwrap().is_empty()
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "Candidate did not settle before setup acknowledgement"
        );
        tokio::task::yield_now().await;
    }
    peer.send(&json!({"id":enable["id"],"result":{}})).await;
    let continued = peer.request().await;
    assert_eq!(continued["guid"], "install-route");
    assert_eq!(continued["method"], "continue");
    assert_eq!(continued["params"]["isFallback"], true);
    peer.send(&json!({"id":continued["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, setting).await.outcome,
        Outcome::Succeeded
    );
    assert_eq!(session.0.host.har_list()["routes"][0]["replies"], 0);
    assert_eq!(session.0.host.har_list()["routes"][0]["failures"], 0);
    close(&session).await;
}
#[tokio::test]
async fn har_filter_nonmatch_continues_without_archive_lookup_or_reply_count() {
    let (session, mut peer, _root, context, _) = harness().await;
    {
        let mut rules = session.0.host.routing.rules.lock().unwrap();
        let rule = rules
            .begin_filtered(
                context,
                crate::browser::routing::HarScope::Context,
                Uuid::new_v4(),
                contract::HarNotFound::Abort,
                Some(crate::browser::routing::Matcher::Regex {
                    pattern: "/selected$".into(),
                    flags: String::new(),
                }),
            )
            .unwrap();
        rules.confirm(context, rule.id).unwrap();
    }
    peer.send(&created("filtered-request","Request",json!({"frame":{"guid":"frame"},"url":"https://example.com/nonmatch?private=Acme","resourceType":"fetch","method":"GET","headers":[],"isNavigationRequest":false}))).await;
    peer.send(&created(
        "filtered-route",
        "Route",
        json!({"request":{"guid":"filtered-request"}}),
    ))
    .await;
    peer.send(
        &json!({"guid":"context","method":"route","params":{"route":{"guid":"filtered-route"}}}),
    )
    .await;
    let continued = peer.request().await;
    assert_eq!(continued["guid"], "filtered-route");
    assert_eq!(continued["method"], "continue");
    assert_eq!(continued["params"]["isFallback"], true);
    peer.send(&json!({"id":continued["id"],"result":{}})).await;
    session
        .0
        .host
        .connection()
        .unwrap()
        .event_barrier(deadline())
        .await
        .unwrap();
    let listed = session.0.host.har_list();
    assert_eq!(listed["routes"][0]["replies"], 0);
    assert_eq!(listed["routes"][0]["failures"], 0);
    close(&session).await;
}
#[tokio::test]
async fn har_setup_remote_failure_retains_interception_until_explicit_cleanup() {
    let (session, mut peer, directory, _, page) = harness().await;
    let input = directory.path().join("har-input");
    crate::artifacts::directory(&input).unwrap();
    let path = input.join("source.har");
    crate::artifacts::write(&path, br#"{"log":{"version":"1.2","entries":[]}}"#).unwrap();
    let registered = session
        .0
        .host
        .har_archives
        .lock()
        .unwrap()
        .register(&path, session.journal_directory())
        .unwrap();
    let archive_id = serde_json::from_value(registered["archiveId"].clone()).unwrap();
    let setting = session
        .submit(
            Uuid::new_v4(),
            Request::PageHarSet {
                page,
                archive_id: Some(archive_id),
                not_found: contract::HarNotFound::Abort,
                url_filter: None,
            },
            5000,
        )
        .unwrap();
    let enable = peer.request().await;
    assert_eq!(enable["method"], "setNetworkInterceptionPatterns");
    peer.send(&method_error(enable["id"].clone())).await;
    let failed = complete(&session, setting).await;
    assert_eq!(failed.code.as_deref(), Some("BROWSER_METHOD_FAILED"));
    assert_eq!(failed.result.unwrap()["setupPending"], true);
    assert_eq!(
        session.0.host.har_list()["routes"][0]["phase"],
        "installing"
    );
    assert_eq!(
        session.0.host.har_remove(archive_id).unwrap_err().code,
        "HAR_ARCHIVE_IN_USE"
    );
    let stopping = session
        .submit(
            Uuid::new_v4(),
            Request::PageHarSet {
                page,
                archive_id: None,
                not_found: contract::HarNotFound::Abort,
                url_filter: None,
            },
            5000,
        )
        .unwrap();
    let disable = peer.request().await;
    assert_eq!(disable["method"], "setNetworkInterceptionPatterns");
    assert_eq!(disable["params"]["patterns"], json!([]));
    peer.send(&json!({"id":disable["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, stopping).await.outcome,
        Outcome::Succeeded
    );
    session.0.host.har_remove(archive_id).unwrap();
    close(&session).await;
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One reserved response lane verifies route and binding callback consumption without replay"
)]
async fn typed_route_and_binding_callbacks_use_the_reserved_lane_once() {
    let (session, mut peer, _directory, _, _) = harness().await;
    peer.send(&created(
        "network-request",
        "Request",
        json!({
            "frame":{"guid":"frame"},"url":"https://example.com/fixture",
            "resourceType":"document","method":"GET","headers":[],"isNavigationRequest":false
        }),
    ))
    .await;
    for (guid, kind, method, init, response, expected) in [
        (
            "route",
            "Route",
            "route",
            json!({"request":{"guid":"network-request"}}),
            CallbackResponse::RouteAbort { error_code: None },
            "abort",
        ),
        (
            "binding",
            "BindingCall",
            "bindingCall",
            json!({"frame":{"guid":"frame"},"name":"acme","args":[]}),
            CallbackResponse::BindingResolve {
                value: json!({"label":"Acme","optional":null}),
            },
            "resolve",
        ),
        (
            "typed-binding",
            "BindingCall",
            "bindingCall",
            json!({"frame":{"guid":"frame"},"name":"acme","args":[]}),
            CallbackResponse::BindingResolveValue {
                value: serde_json::from_value(json!({"kind":"bigint","value":"9007199254740993"}))
                    .unwrap(),
            },
            "resolve",
        ),
        (
            "rejected-binding",
            "BindingCall",
            "bindingCall",
            json!({"frame":{"guid":"frame"},"name":"acme","args":[]}),
            CallbackResponse::BindingRejectValue {
                value: serde_json::from_value(json!({"kind":"undefined"})).unwrap(),
            },
            "reject",
        ),
    ] {
        peer.send(&created(guid, kind, init)).await;
        let field = if method == "route" {
            "route"
        } else {
            "binding"
        };
        peer.send(&json!({"guid":"context","method":method,"params":{field:{"guid":guid}}}))
            .await;
        let callback = pending_callback(&session).await;
        let id = session
            .submit(
                Uuid::new_v4(),
                Request::CallbackResolve {
                    callback,
                    response: CallbackResponse::Dialog {
                        accept: false,
                        prompt_text: None,
                    },
                },
                5000,
            )
            .unwrap();
        assert_eq!(
            complete(&session, id).await.code.as_deref(),
            Some("CALLBACK_RESPONSE_MISMATCH")
        );
        let expired = session
            .0
            .host
            .callback_resolve(
                callback,
                response.clone(),
                Instant::now() - Duration::from_millis(1),
            )
            .await
            .unwrap_err();
        assert_eq!(expired.code, "NOT_DISPATCHED");
        assert!(
            !session
                .0
                .host
                .responding
                .lock()
                .unwrap()
                .contains(&callback)
        );
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
        let wire = peer.request().await;
        assert_eq!(wire["method"], expected);
        if guid == "typed-binding" {
            assert_eq!(
                wire["params"]["result"],
                json!({"value":{"bi":"9007199254740993"},"handles":[]})
            );
        }
        if guid == "rejected-binding" {
            assert_eq!(wire["params"]["error"], json!({"value":{"v":"undefined"}}));
        }
        peer.send(&json!({"id":wire["id"],"result":{}})).await;
        assert_eq!(complete(&session, id).await.outcome, Outcome::Succeeded);
        let id = session
            .submit(
                Uuid::new_v4(),
                Request::CallbackResolve { callback, response },
                5000,
            )
            .unwrap();
        assert_eq!(
            complete(&session, id).await.code.as_deref(),
            Some("STALE_REFERENCE")
        );
    }
    close(&session).await;
}

async fn pending_callback(session: &Session) -> Reference {
    tokio::time::timeout_at(deadline(), async {
        loop {
            if let Some(reference) = session
                .0
                .host
                .callbacks
                .lock()
                .unwrap()
                .keys()
                .next()
                .copied()
            {
                break reference;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn journal_failure_cannot_prevent_owned_connection_cleanup() {
    let (session, _peer, directory, _, _) = harness().await;
    std::fs::rename(
        session.journal_directory(),
        directory.path().join("retained-journal"),
    )
    .unwrap();
    let read = session
        .submit(Uuid::new_v4(), Request::PageList {}, 5000)
        .unwrap();
    assert_eq!(
        complete(&session, read).await.code.as_deref(),
        Some("STATE_IO")
    );
    assert_eq!(session.status()["phase"], "lost");
    let id = session.disconnect(5000).unwrap();
    let outcome = complete(&session, id).await;
    assert_eq!(outcome.code, None);
    assert_eq!(outcome.outcome, Outcome::Succeeded);
    let receipt = outcome.result.unwrap();
    assert_eq!(receipt["closed"], true);
    assert_eq!(receipt["cleanup"]["journalFailure"], "STATE_IO");
    assert_eq!(session.status()["phase"], "closed");
    assert!(session.status()["resources"].as_array().unwrap().is_empty());
    assert!(session.0.host.connection().unwrap().bootstrap().is_err());
}

#[tokio::test]
async fn unfinished_playwright_disconnect_preserves_journal_failure() {
    let (session, _peer, root, _, _) = harness().await;
    std::fs::rename(
        session.journal_directory(),
        root.path().join("retained-journal"),
    )
    .unwrap();
    let driver = session.0.host.driver.lock().await;
    let id = session.disconnect(1).unwrap();
    let outcome = complete(&session, id).await;
    assert!(outcome.settled);
    assert_eq!(outcome.outcome, Outcome::Unknown);
    assert_eq!(outcome.code.as_deref(), Some("STATE_IO"));
    assert_eq!(outcome.result.unwrap()["unfinishedStage"], "driver_lock");
    assert_eq!(session.status()["phase"], "cleanup_pending");
    assert_eq!(session.status()["failure"], "STATE_IO");
    drop(driver);
    close(&session).await;
}

#[tokio::test]
async fn driver_lifecycle_evidence_terminal_receipt_requires_completed_owned_cleanup() {
    let (session, _peer, _root, _context, _page) = harness().await;
    let id = session.disconnect(5000).unwrap();
    let done = complete(&session, id).await;
    assert_eq!(done.outcome, Outcome::Succeeded);
    let result = done.result.unwrap();
    assert_eq!(result["closed"], true);
    let report = &result["cleanup"];
    assert_eq!(report["complete"], true);
    assert!(report["unfinishedStage"].is_null());
    assert!(report["unfinishedElapsedMs"].is_null());
    assert!(
        report["cleanupStages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|stage| stage["stage"] == "control_http_and_archives")
    );
    assert!(
        report["driverShutdown"].is_null(),
        "An in-memory protocol fixture must not fabricate process-release evidence"
    );
    assert_eq!(session.status()["phase"], "closed");
    assert!(session.status()["resources"].as_array().unwrap().is_empty());
    assert!(session.0.host.connection().unwrap().bootstrap().is_err());
}

#[tokio::test]
async fn page_waits_keep_reads_responsive_and_navigation_barriers_reject_old_frames() {
    use crate::browser::page::{Action, Condition, Locator, Read};
    let (session, mut peer, _directory, _, page) = harness().await;
    let frame = session
        .0
        .host
        .state
        .lock()
        .unwrap()
        .resources
        .values()
        .find(|r| r.kind == Kind::Frame)
        .unwrap()
        .reference;
    let waiting = session
        .submit(
            Uuid::new_v4(),
            Request::PageWait {
                page,
                frame: None,
                locator: Some(Locator::Css {
                    selector: "#ready".into(),
                }),
                condition: Condition::Visible {},
            },
            5000,
        )
        .unwrap();
    let wire = peer.request().await;
    assert_eq!(wire["method"], "evaluateExpression");
    let reading = session
        .submit(
            Uuid::new_v4(),
            Request::PageRead {
                page,
                frame: None,
                locator: Some(Locator::Css {
                    selector: "button".into(),
                }),
                read: Read::Count,
                max_bytes: 8192,
            },
            5000,
        )
        .unwrap();
    let read = peer.request().await;
    assert_eq!(read["method"], "queryCount");
    assert_eq!(wire["metadata"]["internal"], true);
    assert_eq!(read["metadata"]["internal"], true);
    peer.send(&json!({"id":read["id"],"result":{"value":2}}))
        .await;
    assert_eq!(
        complete(&session, reading).await.result.unwrap()["result"]["value"],
        2
    );
    peer.send(&json!({"id":wire["id"],"result":{"value":crate::browser::values::argument(&json!({"satisfied":true})).unwrap()["value"]}})).await;
    assert_eq!(
        complete(&session, waiting).await.outcome,
        Outcome::Succeeded
    );
    peer.send(&json!({"guid":"frame","method":"navigated","params":{"url":"https://example.com/next","name":"","newDocument":{}}})).await;
    let stale = session
        .submit(
            Uuid::new_v4(),
            Request::PageAction {
                page,
                frame: Some(frame),
                locator: Locator::Css {
                    selector: "button".into(),
                },
                action: Action::Click {},
                force: false,
                postcondition: None,
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, stale).await.code.as_deref(),
        Some("STALE_REFERENCE")
    );
    close(&session).await;
}

#[tokio::test]
async fn a_failed_chooser_resolve_can_be_retried() {
    let (session, mut peer, _directory, _, _) = harness().await;
    peer.send(&created(
        "chooser-element",
        "ElementHandle",
        json!({"preview":"input"}),
    ))
    .await;
    peer.send(&json!({"guid":"page","method":"fileChooser","params":{"element":{"guid":"chooser-element"},"isMultiple":false}})).await;
    let owner = peer.request().await;
    assert_eq!(owner["method"], "ownerFrame");
    peer.send(&json!({"id":owner["id"],"result":{"frame":{"guid":"frame"}}}))
        .await;
    let chooser: Reference = tokio::time::timeout_at(deadline(), async {
        loop {
            let reference = session.status()["transfers"]["choosers"][0]["reference"].clone();
            if !reference.is_null() {
                return serde_json::from_value(reference).unwrap();
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let resolve = Request::ChooserResolve {
        chooser,
        fixtures: Vec::new(),
    };
    let first = session
        .submit(Uuid::new_v4(), resolve.clone(), 5000)
        .unwrap();
    let attach = peer.request().await;
    assert_eq!(attach["method"], "setInputFiles");
    peer.send(&method_error(attach["id"].clone())).await;
    assert_ne!(complete(&session, first).await.outcome, Outcome::Succeeded);
    // The failure released the chooser: a retry attaches instead of being
    // refused as already resolving.
    let retry = session.submit(Uuid::new_v4(), resolve, 5000).unwrap();
    let attach = peer.request().await;
    assert_eq!(attach["method"], "setInputFiles");
    peer.send(&json!({"id":attach["id"],"result":{}})).await;
    let disposal = peer.request().await;
    assert_eq!(disposal["method"], "dispose");
    peer.send(&json!({"id":disposal["id"],"result":{}})).await;
    assert_eq!(complete(&session, retry).await.outcome, Outcome::Succeeded);
    close(&session).await;
}

async fn refusals(session: &Session, count: usize) -> Value {
    tokio::time::timeout_at(deadline(), async {
        loop {
            let refused = session.status()["transfers"]["refused"].clone();
            if refused
                .as_array()
                .is_some_and(|refused| refused.len() == count)
            {
                return refused;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn an_unacceptable_download_is_refused_alone_and_the_session_continues() {
    let (session, mut peer, _directory, _, _) = harness().await;
    let incoming = session.journal_directory().join("incoming");
    crate::artifacts::directory(&incoming).unwrap();
    let download = |guid: &str| json!({"guid":"page","method":"download","params":{"url":"https://example.com/acme","suggestedFilename":"acme.bin","artifact":{"guid":guid}}});
    // A download outside the session's own folder is cancelled and recorded.
    peer.send(&created(
        "foreign",
        "Artifact",
        json!({"absolutePath":"/example/elsewhere/acme.bin"}),
    ))
    .await;
    peer.send(&download("foreign")).await;
    let cancel = peer.request().await;
    assert_eq!(cancel["method"], "cancel");
    peer.send(&json!({"id":cancel["id"],"result":{}})).await;
    assert_eq!(
        refusals(&session, 1).await[0]["code"],
        "FOREIGN_DOWNLOAD_PATH"
    );
    // With every download slot held, the next download is refused the same
    // way; a deleted record makes room again.
    let host = &session.0.host;
    let wire = host
        .connection()
        .unwrap()
        .resolve("page", "Page")
        .await
        .unwrap();
    let connection_id = host.state.lock().unwrap().connection;
    {
        let mut transfers = host.transfers.lock().unwrap();
        for _ in 0..crate::browser::transfer::DOWNLOADS {
            let reference = Reference {
                connection_id,
                id: Uuid::new_v4(),
                generation: Uuid::new_v4(),
            };
            transfers.downloads.insert(
                reference,
                crate::browser::transfer::Download {
                    wire: wire.clone(),
                    incoming: std::path::PathBuf::new(),
                    summary: json!({"reference":reference,"state":"complete"}),
                },
            );
        }
    }
    peer.send(&created(
        "full",
        "Artifact",
        json!({"absolutePath":incoming.join("full.bin")}),
    ))
    .await;
    peer.send(&download("full")).await;
    let cancel = peer.request().await;
    assert_eq!(cancel["method"], "cancel");
    peer.send(&json!({"id":cancel["id"],"result":{}})).await;
    assert_eq!(refusals(&session, 2).await[1]["code"], "DOWNLOAD_CAPACITY");
    host.transfers
        .lock()
        .unwrap()
        .downloads
        .values_mut()
        .next()
        .unwrap()
        .summary["state"] = json!("deleted");
    peer.send(&created(
        "room",
        "Artifact",
        json!({"absolutePath":incoming.join("room.bin")}),
    ))
    .await;
    peer.send(&download("room")).await;
    let allocation = peer.request().await;
    assert_eq!(allocation["method"], "saveAsStream");
    peer.send(&method_error(allocation["id"].clone())).await;
    let cleanup = peer.request().await;
    assert_eq!(cleanup["method"], "cancel");
    peer.send(&json!({"id":cleanup["id"],"result":{}})).await;
    assert!(session.status()["failure"].is_null());
    let list = session
        .submit(Uuid::new_v4(), Request::DownloadList {}, 5000)
        .unwrap();
    assert_eq!(complete(&session, list).await.outcome, Outcome::Succeeded);
    close(&session).await;
}

#[tokio::test]
async fn pending_download_allocation_keeps_cancel_responsive_and_retains_its_reply() {
    let (session, mut peer, _directory, _, _) = harness().await;
    let incoming = session.journal_directory().join("incoming");
    crate::artifacts::directory(&incoming).unwrap();
    peer.send(&created(
        "artifact",
        "Artifact",
        json!({"absolutePath":incoming.join("owned.bin")}),
    ))
    .await;
    peer.send(&json!({"guid":"page","method":"download","params":{"url":"https://example.com/fixture","suggestedFilename":"../../Acme.csv","artifact":{"guid":"artifact"}}})).await;
    let allocation = peer.request().await;
    assert_eq!(allocation["method"], "saveAsStream");
    let download =
        serde_json::from_value(session.status()["transfers"]["downloads"][0]["reference"].clone())
            .unwrap();
    let cancel = session
        .submit(Uuid::new_v4(), Request::DownloadCancel { download }, 5000)
        .unwrap();
    let cancellation = peer.request().await;
    assert_eq!(cancellation["method"], "cancel");
    peer.send(&json!({"id":cancellation["id"],"result":{}}))
        .await;
    assert_eq!(complete(&session, cancel).await.outcome, Outcome::Succeeded);
    peer.send(&method_error(allocation["id"].clone())).await;
    // Collector retains the allocation result, then cancels the failed artifact.
    let cleanup = peer.request().await;
    assert_eq!(cleanup["method"], "cancel");
    peer.send(&json!({"id":cleanup["id"],"result":{}})).await;
    tokio::time::timeout_at(deadline(), async {
        loop {
            if session.status()["transfers"]["downloads"][0]["state"] == "partial_or_failed" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        session.status()["transfers"]["downloads"][0]
            .get("artifact")
            .is_none()
    );
    close(&session).await;
}
