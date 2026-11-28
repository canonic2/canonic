//! Finite handlers retain paused requests and acknowledged transport cleanup.
use super::*;
use crate::browser::routing::{HandlerAction, Matcher};

async fn request(peer: &mut Fixture, scope: &str, name: &str) {
    peer.send(&created(&format!("{name}-request"),"Request",json!({"frame":{"guid":"frame"},"url":"https://example.com/acme","resourceType":"fetch","method":"GET","headers":[],"isNavigationRequest":false}))).await;
    peer.send(&created(
        &format!("{name}-route"),
        "Route",
        json!({"request":{"guid":format!("{name}-request")}}),
    ))
    .await;
    peer.send(
        &json!({"guid":scope,"method":"route","params":{"route":{"guid":format!("{name}-route")}}}),
    )
    .await;
}

#[tokio::test]
async fn route_fulfill_text_reaches_the_driver_as_text_like_the_pinned_client() {
    let (session, mut peer, _directory, _context, page) = harness().await;
    let host = session.0.host.clone();
    {
        let mut rules = host.routing.rules.lock().unwrap();
        rules.mark_manual(page, true);
        rules
            .handlers
            .add_limited(
                page,
                Matcher::Exact {
                    url: "https://example.com/acme".into(),
                },
                None,
            )
            .unwrap();
    }
    host.routing
        .manual_urls
        .lock()
        .unwrap()
        .insert(page, "https://example.com/acme".into());
    request(&mut peer, "page", "text").await;
    let callback = pending_callback(&session).await;
    let replying = tokio::spawn({
        let host = host.clone();
        async move {
            host.callback_resolve(
                callback,
                CallbackResponse::RouteFulfill {
                    status: None,
                    body: Some("Acme é".into()),
                    json: None,
                    content_type: None,
                    headers: vec![],
                    path: None,
                    file: None,
                },
                deadline(),
            )
            .await
        }
    });
    let fulfill = peer.request().await;
    assert_eq!(fulfill["method"], "fulfill");
    assert_eq!(fulfill["params"]["isBase64"], false);
    assert_eq!(fulfill["params"]["body"], "Acme é");
    assert_eq!(fulfill["params"]["status"], 200);
    assert!(
        fulfill["params"]["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|header| header["name"] == "content-length" && header["value"] == "7"),
        "{fulfill}"
    );
    peer.send(&json!({"id":fulfill["id"],"result":{}})).await;
    replying.await.unwrap().unwrap();
    close(&session).await;
}

#[tokio::test]
async fn route_clear_default_continues_paused_requests_and_refuses_late_replies() {
    use crate::browser::routing::ClearBehavior;
    for behavior in [ClearBehavior::Default, ClearBehavior::IgnoreErrors] {
        let (session, mut peer, _directory, _context, page) = harness().await;
        let host = session.0.host.clone();
        {
            let mut rules = host.routing.rules.lock().unwrap();
            rules.mark_manual(page, true);
            rules
                .handlers
                .add_limited(
                    page,
                    Matcher::Exact {
                        url: "https://example.com/acme".into(),
                    },
                    None,
                )
                .unwrap();
        }
        host.routing
            .manual_urls
            .lock()
            .unwrap()
            .insert(page, "https://example.com/acme".into());
        request(&mut peer, "page", "first").await;
        let callback = pending_callback(&session).await;
        let clearing = tokio::spawn({
            let host = host.clone();
            async move {
                host.network_route(
                    page,
                    HandlerAction::Clear {
                        behavior: Some(behavior),
                    },
                    deadline(),
                )
                .await
            }
        });
        // Pinned: ending interception sends the paused request to the network.
        let continued = peer.request().await;
        assert_eq!(continued["guid"], "first-route");
        assert_eq!(continued["method"], "continue");
        peer.send(&json!({"id":continued["id"],"result":{}})).await;
        let update = peer.request().await;
        assert_eq!(update["method"], "setNetworkInterceptionPatterns");
        assert_eq!(update["params"]["patterns"], json!([]));
        peer.send(&json!({"id":update["id"],"result":{}})).await;
        clearing.await.unwrap().unwrap();
        assert_eq!(host.routing.rules.lock().unwrap().handlers.len(page), 0);
        // A late reply refuses like any answered callback, without another command.
        assert!(
            host.callback_resolve(callback, CallbackResponse::RouteFallback {}, deadline())
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.request())
                .await
                .is_err()
        );
        close(&session).await;
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "Each Page or Context fault cell preserves its original callback through final-use teardown, late acknowledgement and explicit clear"
)]
async fn route_limits_driver_exhaustion_falls_through_and_cleanup_retains_failure_or_expiry() {
    for context_owned in [false, true] {
        for (fail, expire, unknown) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let (session, mut peer, _directory, context, page) = harness().await;
            let scope = if context_owned { context } else { page };
            let wire = if context_owned { "context" } else { "page" };
            let host = session.0.host.clone();
            let id = {
                let mut rules = host.routing.rules.lock().unwrap();
                rules.mark_manual(scope, true);
                rules
                    .handlers
                    .add_limited(
                        scope,
                        Matcher::Exact {
                            url: "https://example.com/acme".into(),
                        },
                        Some(1),
                    )
                    .unwrap()
            };
            host.routing
                .manual_urls
                .lock()
                .unwrap()
                .insert(scope, "https://example.com/acme".into());
            request(&mut peer, wire, "first").await;
            let callback = pending_callback(&session).await;
            let expiry = host.callbacks.lock().unwrap()[&callback];
            peer.send(
                &json!({"guid":wire,"method":"route","params":{"route":{"guid":"first-route"}}}),
            )
            .await;
            host.connection()
                .unwrap()
                .event_barrier(deadline())
                .await
                .unwrap();
            assert_eq!(host.callbacks.lock().unwrap()[&callback], expiry);
            assert_eq!(
                host.routing
                    .rules
                    .lock()
                    .unwrap()
                    .handlers
                    .current(callback),
                Some(id)
            );
            assert!(
                !host
                    .routing
                    .rules
                    .lock()
                    .unwrap()
                    .handlers
                    .matches(scope, "https://example.com/acme")
            );
            request(&mut peer, wire, "second").await;
            let continuation = peer.request().await;
            assert_eq!(continuation["guid"], "second-route");
            assert_eq!(continuation["method"], "continue");
            peer.send(&json!({"id":continuation["id"],"result":{}}))
                .await;
            // Finish the automatic fallthrough before injecting a lost cleanup
            // reply into the separately owned final-use resolution below.
            tokio::time::timeout_at(deadline(), async {
                loop {
                    if host.routing.pending.lock().unwrap().is_empty() {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            // The final use still owns its original callback, even though
            // another request is already falling through without using it.
            assert!(
                host.routing
                    .rules
                    .lock()
                    .unwrap()
                    .handlers
                    .contains(scope, id)
            );
            let finishing = tokio::spawn({
                let host = host.clone();
                async move {
                    if expire {
                        host.callbacks
                            .lock()
                            .unwrap()
                            .insert(callback, Instant::now());
                        host.expire_callback(callback).await;
                        Ok(json!({"resolved":true}))
                    } else {
                        host.callback_resolve(
                            callback,
                            CallbackResponse::RouteAbort { error_code: None },
                            deadline(),
                        )
                        .await
                    }
                }
            });
            let abort = peer.request().await;
            assert_eq!(abort["guid"], "first-route");
            assert_eq!(abort["method"], "abort");
            peer.send(&json!({"id":abort["id"],"result":{}})).await;
            let update = peer.request().await;
            assert_eq!(update["guid"], wire);
            assert_eq!(update["method"], "setNetworkInterceptionPatterns");
            assert_eq!(update["params"]["patterns"], json!([]));
            assert!(
                host.routing
                    .rules
                    .lock()
                    .unwrap()
                    .handlers
                    .contains(scope, id)
            );
            if fail {
                peer.send(&method_error(update["id"].clone())).await;
            } else if !unknown {
                peer.send(&json!({"id":update["id"],"result":{}})).await;
            } else {
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(11)).await;
                tokio::time::resume();
                assert!(!finishing.is_finished());
                assert!(host.routing.updating.lock().unwrap().contains(&scope));
                assert_eq!(
                    host.routing.rules.lock().unwrap().handlers.exhausted(scope),
                    vec![id]
                );
                assert!(
                    tokio::time::timeout(Duration::from_millis(25), peer.request())
                        .await
                        .is_err()
                );
                // The original retained command is still owned. Its late
                // acknowledgement must finalize without another pattern write.
                peer.send(&json!({"id":update["id"],"result":{}})).await;
            }
            let result = finishing.await.unwrap().unwrap();
            assert_eq!(result["resolved"], true);
            if fail {
                assert!(result["handlerCleanupPending"].is_array());
                assert_eq!(
                    host.routing.rules.lock().unwrap().handlers.exhausted(scope),
                    vec![id]
                );
                assert!(host.routing.rules.lock().unwrap().handlers.pending(scope));
                assert_eq!(
                    host.cleanup_exhausted_handlers(vec![scope], deadline())
                        .await[0]["code"],
                    "ROUTE_POLICY_PENDING"
                );
                assert!(
                    tokio::time::timeout(Duration::from_millis(25), peer.request())
                        .await
                        .is_err()
                );
                let clearing = tokio::spawn({
                    let host = host.clone();
                    async move {
                        host.network_route(
                            scope,
                            HandlerAction::Clear { behavior: None },
                            deadline(),
                        )
                        .await
                    }
                });
                let update = peer.request().await;
                assert_eq!(update["params"]["patterns"], json!([]));
                peer.send(&json!({"id":update["id"],"result":{}})).await;
                clearing.await.unwrap().unwrap();
            } else {
                assert!(result.get("handlerCleanupPending").is_none(), "{result}");
            }
            assert_eq!(host.routing.rules.lock().unwrap().handlers.len(scope), 0);
            close(&session).await;
        }
    }
}
