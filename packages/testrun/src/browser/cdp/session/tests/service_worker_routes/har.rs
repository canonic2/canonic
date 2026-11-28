//! Automatic context HAR must own worker attachments without page authority.
use super::*;
use crate::browser::contract::HarNotFound;
use base64::{Engine, engine::general_purpose::STANDARD};

fn archive(worker: &mut Worker, root: &tempfile::TempDir) -> Uuid {
    let directory = root.path().join("worker-har");
    crate::artifacts::directory(&directory).unwrap();
    let path = directory.join("source.har");
    let value = json!({"log":{"version":"1.2","entries":[{"request":{"url":"https://example.com/acme","method":"GET","headers":[]},"response":{"status":200,"headers":[],"content":{"text":"Acme worker HAR"}}}]}});
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

fn paused(worker: &mut Worker, path: &str) {
    worker.apply(EventItem::Event(Event {
        method: "Fetch.requestPaused".into(), session_id: Some("Acme-service-session".into()),
        params: json!({"requestId":"Acme-worker-har-request","request":{"url":format!("https://example.com/{path}"),"method":"GET","headers":{},"initialPriority":"High","referrerPolicy":"no-referrer"},"frameId":"Acme-worker-frame","resourceType":"Fetch"}),
    })).unwrap();
}

#[tokio::test]
async fn native_service_worker_har_configures_future_worker_and_disables_exact_attachment() {
    let (mut worker, mut peer, root, context, page) = owned_page().await;
    let archive = archive(&mut worker, &root);
    let setup = tokio::spawn(async move {
        worker
            .scoped_har_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                Some(archive),
                HarNotFound::Abort,
                None,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    assert_eq!(enable["sessionId"], "session-acme");
    reply(&mut peer, &enable, json!({})).await;
    let mut worker = setup.await.unwrap();
    service_worker(&mut worker);
    let configuring = tokio::spawn(async move {
        worker
            .configure_fetch(Uuid::new_v4(), "Acme-service-session", deadline())
            .await
            .unwrap();
        worker
    });
    let enable = command(&mut peer, "Fetch.enable").await;
    assert_eq!(enable["sessionId"], "Acme-service-session");
    assert_eq!(enable["params"]["handleAuthRequests"], false);
    reply(&mut peer, &enable, json!({})).await;
    let mut worker = configuring.await.unwrap();
    worker.index.retire(page).unwrap();
    paused(&mut worker, "acme");
    assert_eq!(
        worker
            .interactions
            .routes
            .values()
            .next()
            .unwrap()
            .policy_scope,
        context
    );
    let servicing = tokio::spawn(async move {
        worker.poll_har_routes().await;
        worker
    });
    let fulfill = command(&mut peer, "Fetch.fulfillRequest").await;
    assert_eq!(fulfill["sessionId"], "Acme-service-session");
    assert_eq!(
        fulfill["params"]["body"],
        STANDARD.encode("Acme worker HAR")
    );
    reply(&mut peer, &fulfill, json!({})).await;
    let mut worker = servicing.await.unwrap();
    assert!(worker.interactions.routes.is_empty());
    let stopping = tokio::spawn(async move {
        worker
            .scoped_har_set(
                Uuid::new_v4(),
                context,
                Kind::Context,
                None,
                HarNotFound::Abort,
                None,
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let disable = command(&mut peer, "Fetch.disable").await;
    assert_eq!(disable["sessionId"], "Acme-service-session");
    reply(&mut peer, &disable, json!({})).await;
    let worker = stopping.await.unwrap();
    assert!(worker.routing_rules.automatic(context).is_none());
    assert!(worker.har_archives.get(archive).is_ok());
}

#[tokio::test]
async fn native_service_worker_har_existing_worker_misses_filtering_and_failed_teardown() {
    for not_found in [
        HarNotFound::Abort,
        HarNotFound::Continue,
        HarNotFound::Fallback,
    ] {
        let (mut worker, mut peer, root, context, page) = owned_page().await;
        let archive = archive(&mut worker, &root);
        let service = service_worker(&mut worker);
        let setup = tokio::spawn(async move {
            worker
                .scoped_har_set(
                    Uuid::new_v4(),
                    context,
                    Kind::Context,
                    Some(archive),
                    not_found,
                    Some(crate::browser::routing::Matcher::Glob {
                        pattern: "**/acme*".into(),
                    }),
                    deadline(),
                )
                .await
                .unwrap();
            worker
        });
        let mut configured = BTreeSet::new();
        for _ in 0..2 {
            let enable = command(&mut peer, "Fetch.enable").await;
            configured.insert(enable["sessionId"].as_str().unwrap().to_owned());
            assert_eq!(enable["params"]["handleAuthRequests"], false);
            reply(&mut peer, &enable, json!({})).await;
        }
        assert_eq!(
            configured,
            BTreeSet::from(["session-acme".to_owned(), "Acme-service-session".to_owned()])
        );
        let mut worker = setup.await.unwrap();
        worker.index.retire(page).unwrap();
        for (path, expected) in [
            (
                "acme-missing",
                if matches!(not_found, HarNotFound::Abort) {
                    "Fetch.failRequest"
                } else {
                    "Fetch.continueRequest"
                },
            ),
            ("other", "Fetch.continueRequest"),
        ] {
            paused(&mut worker, path);
            let route = worker.interactions.routes.values().next().unwrap();
            assert_eq!(route.owner.worker(), Some(service));
            assert_eq!(route.owner.page(), None);
            let servicing = tokio::spawn(async move {
                worker.poll_har_routes().await;
                worker
            });
            let request = command(&mut peer, expected).await;
            assert_eq!(request["sessionId"], "Acme-service-session");
            reply(&mut peer, &request, json!({})).await;
            worker = servicing.await.unwrap();
            assert!(worker.interactions.routes.is_empty());
        }
        let stopping = tokio::spawn(async move {
            let result = worker
                .scoped_har_set(
                    Uuid::new_v4(),
                    context,
                    Kind::Context,
                    None,
                    HarNotFound::Abort,
                    None,
                    deadline(),
                )
                .await;
            (worker, result)
        });
        let disable = command(&mut peer, "Fetch.disable").await;
        send(&mut peer,json!({"id":disable["id"],"sessionId":disable["sessionId"],"error":{"code":-32000,"message":"Acme worker teardown refused"}})).await;
        let (mut worker, result) = stopping.await.unwrap();
        assert!(result.is_err());
        assert!(worker.routing_rules.automatic(context).is_some());
        assert_eq!(
            worker.har_remove(archive).unwrap_err().code,
            "HAR_ARCHIVE_IN_USE"
        );
    }
}

#[tokio::test]
async fn native_service_worker_har_ordered_miss_falls_back_without_losing_worker_owner() {
    let (mut worker, mut peer, root, context, page) = owned_page().await;
    let archive = archive(&mut worker, &root);
    let matcher = crate::browser::routing::Matcher::Glob {
        pattern: "**".into(),
    };
    let setup = tokio::spawn(layered_har_routes(worker, context, archive, matcher));
    let enable = command(&mut peer, "Fetch.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let (mut worker, first, har, newest) = setup.await.unwrap();
    let service = service_worker(&mut worker);
    worker.index.retire(page).unwrap();
    for path in ["acme", "acme-missing"] {
        paused(&mut worker, path);
        let callback = *worker.interactions.routes.keys().next().unwrap();
        assert_eq!(
            worker.interactions.snapshot()["callbacks"][0]["handlerId"],
            newest
        );
        worker
            .resolve_route(callback, CallbackResponse::RouteFallback {}, deadline())
            .await
            .unwrap();
        let next = *worker.interactions.routes.keys().next().unwrap();
        assert_ne!(next, callback);
        assert_eq!(
            worker.interactions.snapshot()["callbacks"][0]["handlerId"],
            har
        );
        if path == "acme" {
            let servicing = tokio::spawn(async move {
                worker.poll_har_routes().await;
                worker
            });
            let fulfill = command(&mut peer, "Fetch.fulfillRequest").await;
            assert_eq!(fulfill["sessionId"], "Acme-service-session");
            reply(&mut peer, &fulfill, json!({})).await;
            worker = servicing.await.unwrap();
        } else {
            worker.poll_har_routes().await;
            let callbacks = worker.interactions.snapshot();
            assert_eq!(callbacks["callbacks"][0]["handlerId"], first);
            assert_eq!(callbacks["callbacks"][0]["worker"], json!(service));
            assert!(callbacks["callbacks"][0]["page"].is_null());
            let last = *worker.interactions.routes.keys().next().unwrap();
            assert_ne!(last, next);
            let servicing = tokio::spawn(async move {
                worker
                    .resolve_route(last, CallbackResponse::RouteContinue {}, deadline())
                    .await
                    .unwrap();
                worker
            });
            let request = command(&mut peer, "Fetch.continueRequest").await;
            assert_eq!(request["sessionId"], "Acme-service-session");
            reply(&mut peer, &request, json!({})).await;
            worker = servicing.await.unwrap();
        }
        assert!(worker.interactions.routes.is_empty());
    }
}

async fn layered_har_routes(
    mut worker: Worker,
    context: Reference,
    archive: Uuid,
    matcher: crate::browser::routing::Matcher,
) -> (Worker, Value, Value, Value) {
    let first = worker
        .network_route(
            Uuid::new_v4(),
            context,
            HandlerAction::AddMatcher {
                matcher: matcher.clone(),
                times: None,
            },
            deadline(),
        )
        .await
        .unwrap()["handlerId"]
        .clone();
    let har = worker
        .network_route(
            Uuid::new_v4(),
            context,
            HandlerAction::AddHar {
                archive_id: archive,
                url_filter: Some(matcher.clone()),
                not_found: HarNotFound::Fallback,
                times: None,
            },
            deadline(),
        )
        .await
        .unwrap()["handlerId"]
        .clone();
    let newest = worker
        .network_route(
            Uuid::new_v4(),
            context,
            HandlerAction::AddMatcher {
                matcher,
                times: None,
            },
            deadline(),
        )
        .await
        .unwrap()["handlerId"]
        .clone();
    (worker, first, har, newest)
}
