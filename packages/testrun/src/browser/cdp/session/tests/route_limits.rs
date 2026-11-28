//! Native finite handlers release their filters after acknowledged callbacks.
use super::investigation::owned_page;
use super::*;
use crate::browser::routing::HandlerAction;

fn paused(name: &str) -> EventItem {
    EventItem::Event(Event {
        method: "Fetch.requestPaused".into(),
        session_id: Some("session-acme".into()),
        params: json!({"requestId":name,"request":{"url":"https://example.com/acme","method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"frame-acme","resourceType":"Fetch"}),
    })
}

#[tokio::test]
async fn route_reply_ignores_gone_request_errors_like_the_pinned_chromium_route() {
    for (message, resolved) in [
        ("Invalid InterceptionId.", true),
        ("Unsafe header: Host", false),
    ] {
        let (mut worker, mut peer, _root, _context, page) = owned_page().await;
        let starting = tokio::spawn(async move {
            let result = worker
                .network_route(
                    Uuid::new_v4(),
                    page,
                    HandlerAction::Add {
                        url: "https://example.com/acme".into(),
                        times: None,
                    },
                    deadline(),
                )
                .await;
            (worker, result)
        });
        let enable = command(&mut peer, "Fetch.enable").await;
        reply(&mut peer, &enable, json!({})).await;
        let (mut worker, result) = starting.await.unwrap();
        result.unwrap();
        worker.apply(paused("gone")).unwrap();
        let callback = *worker.interactions.routes.keys().next().unwrap();
        let replying = tokio::spawn(async move {
            let result = worker
                .resolve_route(
                    callback,
                    contract::CallbackResponse::RouteContinue {},
                    deadline(),
                )
                .await;
            (worker, result)
        });
        let sent = command(&mut peer, "Fetch.continueRequest").await;
        let mut failure = json!({"id":sent["id"],"error":{"code":-32000,"message":message}});
        if let Some(session) = sent.get("sessionId") {
            failure["sessionId"] = session.clone();
        }
        send(&mut peer, failure).await;
        let (worker, result) = replying.await.unwrap();
        if resolved {
            assert_eq!(result.unwrap()["resolved"], true, "{message}");
            assert!(worker.interactions.routes.is_empty());
        } else {
            assert_eq!(result.unwrap_err().code, "BROWSER_METHOD_FAILED");
            assert!(worker.interactions.routes.contains_key(&callback));
        }
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "Each fault cell retains the same final-use callback through expiry, failed cleanup or unknown acknowledgement"
)]
async fn route_limits_native_exhaustion_is_private_and_cleanup_preserves_acknowledged_resolution() {
    for context_owned in [false, true] {
        for (fail, expire, unknown) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let (mut worker, mut peer, _root, context, page) = owned_page().await;
            let scope = if context_owned { context } else { page };
            let starting = tokio::spawn(async move {
                let result = worker
                    .network_route(
                        Uuid::new_v4(),
                        scope,
                        HandlerAction::Add {
                            url: "https://example.com/acme".into(),
                            times: Some(1),
                        },
                        deadline(),
                    )
                    .await;
                (worker, result)
            });
            let enable = command(&mut peer, "Fetch.enable").await;
            reply(&mut peer, &enable, json!({})).await;
            let (mut worker, result) = starting.await.unwrap();
            let id = serde_json::from_value::<Uuid>(result.unwrap()["handlerId"].clone()).unwrap();
            worker.apply(paused("first")).unwrap();
            let callback = *worker.interactions.routes.keys().next().unwrap();
            assert_eq!(worker.routing_rules.handlers.current(callback), Some(id));
            worker.apply(paused("second")).unwrap();
            assert_eq!(
                worker.interactions.snapshot()["callbacks"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                worker.interactions.snapshot()["routeContinuationsPending"],
                1
            );
            let passing = tokio::spawn(async move {
                worker.poll_har_routes().await;
                worker
            });
            let pass = command(&mut peer, "Fetch.continueRequest").await;
            assert_eq!(pass["params"]["requestId"], "second");
            reply(&mut peer, &pass, json!({})).await;
            worker = passing.await.unwrap();
            assert!(worker.routing_rules.handlers.contains(scope, id));
            if expire {
                worker
                    .interactions
                    .routes
                    .get_mut(&callback)
                    .unwrap()
                    .expires = Instant::now();
            }
            let finishing = tokio::spawn(async move {
                let result = if expire {
                    worker.expire_routes().await;
                    Ok(json!({"resolved":true}))
                } else {
                    worker
                        .resolve_route(
                            callback,
                            contract::CallbackResponse::RouteAbort { error_code: None },
                            deadline(),
                        )
                        .await
                };
                (worker, result)
            });
            let abort = command(&mut peer, "Fetch.failRequest").await;
            assert_eq!(abort["params"]["requestId"], "first");
            reply(&mut peer, &abort, json!({})).await;
            let disable = command(&mut peer, "Fetch.disable").await;
            if fail {
                send(&mut peer,json!({"id":disable["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme cleanup refused"}})).await;
            } else if !unknown {
                reply(&mut peer, &disable, json!({})).await;
            } else {
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(11)).await;
                tokio::time::resume();
            }
            let (mut worker, result) = finishing.await.unwrap();
            let result = result.unwrap();
            assert_eq!(result["resolved"], true);
            if fail || unknown {
                assert!(result["handlerCleanupPending"].is_array());
                if unknown {
                    assert_eq!(
                        result["handlerCleanupPending"][0]["code"],
                        "OUTCOME_UNKNOWN"
                    );
                }
                assert!(worker.routing_rules.handlers.pending(scope));
                assert_eq!(worker.routing_rules.handlers.exhausted(scope), vec![id]);
                worker.apply(paused("late")).unwrap();
                assert!(
                    worker.interactions.snapshot()["callbacks"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
                let passing = tokio::spawn(async move {
                    worker.poll_har_routes().await;
                    worker
                });
                let pass = command(&mut peer, "Fetch.continueRequest").await;
                assert_eq!(pass["params"]["requestId"], "late");
                reply(&mut peer, &pass, json!({})).await;
                worker = passing.await.unwrap();
                assert!(worker.routing_rules.handlers.pending(scope));
                assert!(
                    tokio::time::timeout(Duration::from_millis(25), peer.next())
                        .await
                        .is_err()
                );
            } else {
                assert_eq!(worker.routing_rules.handlers.len(scope), 0);
            }
        }
    }
}
