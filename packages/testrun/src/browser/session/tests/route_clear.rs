//! Clearing scope-wide replay retains failed teardown and independent scopes.
use super::*;
use crate::browser::routing::{HandlerAction, HarScope};

#[tokio::test]
async fn clear_scope_har_does_not_abort_a_callback_arriving_during_teardown() {
    let (session, mut peer, _directory, _context, page) = harness().await;
    {
        let mut rules = session.0.host.routing.rules.lock().unwrap();
        let rule = rules
            .begin(page, Uuid::new_v4(), contract::HarNotFound::Abort)
            .unwrap();
        rules.confirm(page, rule.id).unwrap();
    }
    let clearing = session
        .submit(
            Uuid::new_v4(),
            Request::NetworkRoute {
                scope: page,
                action: HandlerAction::Clear { behavior: None },
            },
            5000,
        )
        .unwrap();
    let disable = peer.request().await;
    assert_eq!(disable["params"]["patterns"], json!([]));
    peer.send(&created("clear-request", "Request", json!({"frame":{"guid":"frame"},"url":"https://example.com/acme","resourceType":"fetch","method":"GET","headers":[],"isNavigationRequest":false}))).await;
    peer.send(&created(
        "clear-route",
        "Route",
        json!({"request":{"guid":"clear-request"}}),
    ))
    .await;
    peer.send(&json!({"guid":"page","method":"route","params":{"route":{"guid":"clear-route"}}}))
        .await;
    let callback = pending_callback(&session).await;
    peer.send(&json!({"id":disable["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, clearing).await.code.as_deref(),
        Some("ROUTE_CALLBACKS_PENDING")
    );
    assert!(
        session
            .0
            .host
            .routing
            .rules
            .lock()
            .unwrap()
            .automatic(page)
            .is_some()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    let host = session.0.host.clone();
    let resolving = tokio::spawn(async move {
        host.callback_resolve(callback, CallbackResponse::RouteContinue {}, deadline())
            .await
            .unwrap();
    });
    let continued = peer.request().await;
    assert_eq!(continued["method"], "continue");
    peer.send(&json!({"id":continued["id"],"result":{}})).await;
    resolving.await.unwrap();
    let clearing = session
        .submit(
            Uuid::new_v4(),
            Request::NetworkRoute {
                scope: page,
                action: HandlerAction::Clear { behavior: None },
            },
            5000,
        )
        .unwrap();
    let disable = peer.request().await;
    peer.send(&json!({"id":disable["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, clearing).await.outcome,
        Outcome::Succeeded
    );
    close(&session).await;
}

#[tokio::test]
async fn clear_scope_har_retains_failed_teardown_and_preserves_siblings() {
    for context_owned in [false, true] {
        let (session, mut peer, directory, context, page) = harness().await;
        let scope = if context_owned { context } else { page };
        let sibling = if context_owned { page } else { context };
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
        let archive = serde_json::from_value(registered["archiveId"].clone()).unwrap();
        {
            let mut rules = session.0.host.routing.rules.lock().unwrap();
            for (reference, kind) in [(context, HarScope::Context), (page, HarScope::Page)] {
                let rule = rules
                    .begin_filtered(reference, kind, archive, contract::HarNotFound::Abort, None)
                    .unwrap();
                rules.confirm(reference, rule.id).unwrap();
            }
        }
        for fail in [true, false] {
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
            let command = peer.request().await;
            assert_eq!(command["method"], "setNetworkInterceptionPatterns");
            assert_eq!(
                command["guid"],
                if context_owned { "context" } else { "page" }
            );
            assert_eq!(command["params"]["patterns"], json!([]));
            assert_eq!(
                session.0.host.har_remove(archive).unwrap_err().code,
                "HAR_ARCHIVE_IN_USE"
            );
            if fail {
                peer.send(&method_error(command["id"].clone())).await;
            } else {
                peer.send(&json!({"id":command["id"],"result":{}})).await;
            }
            let result = complete(&session, clearing).await;
            let rules = session.0.host.routing.rules.lock().unwrap();
            assert!(rules.automatic(sibling).is_some());
            if fail {
                assert_eq!(result.code.as_deref(), Some("BROWSER_METHOD_FAILED"));
                assert!(rules.automatic(scope).is_some());
            } else {
                assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
                assert!(rules.automatic(scope).is_none());
            }
        }
        // Repeated clear is an acknowledged no-op and does not disable a sibling.
        session
            .0
            .host
            .network_route(scope, HandlerAction::Clear { behavior: None }, deadline())
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.request())
                .await
                .is_err()
        );
        close(&session).await;
    }
}
