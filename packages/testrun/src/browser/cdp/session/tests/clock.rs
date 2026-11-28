//! Native clock dispatch is context-scoped and retains failed mutations.
use super::investigation::owned_page;
use super::*;
use crate::browser::clock::Action;

#[tokio::test]
async fn clock_replacement_keeps_bootstrap_during_same_target_navigation() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let task = tokio::spawn(async move {
        let result = worker
            .context_clock(
                Uuid::new_v4(),
                context,
                Action::Install {
                    time_ms: Some(1700000000000),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let source = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    reply(&mut peer, &source, json!({"identifier":"clock-old"})).await;
    let evaluate = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &evaluate,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let (returned, result) = task.await.unwrap();
    result.unwrap();
    worker = returned;
    let task = tokio::spawn(async move {
        let result = worker
            .context_clock(
                Uuid::new_v4(),
                context,
                Action::SetFixedTime {
                    time_ms: 1700000000100,
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    // The replacement is added while clock-old still owns future documents.
    let replacement = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    reply(&mut peer, &replacement, json!({"identifier":"clock-new"})).await;
    let removal = command(&mut peer, "Page.removeScriptToEvaluateOnNewDocument").await;
    assert_eq!(removal["params"]["identifier"], "clock-old");
    // A navigation completes between the add and remove acknowledgments.
    send(
        &mut peer,
        json!({"method":"Runtime.executionContextsCleared","sessionId":"session-acme","params":{}}),
    )
    .await;
    send(&mut peer, json!({"method":"Page.frameNavigated","sessionId":"session-acme","params":{"frame":{"id":"frame-acme","loaderId":"clock-navigation","url":"https://example.com/clock","domainAndRegistry":"example.com","securityOrigin":"https://example.com","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]},"type":"Navigation"}})).await;
    send(&mut peer, json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":2,"origin":"https://example.com","name":"","uniqueId":"world-between-clock-replies","auxData":{"frameId":"frame-acme","isDefault":true}}}})).await;
    reply(&mut peer, &removal, json!({})).await;
    let evaluate = command(&mut peer, "Runtime.evaluate").await;
    assert_eq!(
        evaluate["params"]["uniqueContextId"],
        "world-between-clock-replies"
    );
    reply(
        &mut peer,
        &evaluate,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    task.await.unwrap().1.unwrap();
}

#[tokio::test]
async fn clock_replacement_keeps_earlier_user_init_scripts_after_the_clock() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let task = tokio::spawn(async move {
        let result = worker
            .init_script(
                Uuid::new_v4(),
                Request::ContextInitScriptAdd {
                    context,
                    source: "globalThis.acmeInitTime=Date.now()".into(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let user = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    reply(&mut peer, &user, json!({"identifier":"user-first"})).await;
    let (mut worker, result) = task.await.unwrap();
    result.unwrap();
    for (previous, next) in [(None, "clock-first"), (Some("clock-first"), "clock-second")] {
        let task = tokio::spawn(async move {
            let result = worker
                .context_clock(
                    Uuid::new_v4(),
                    context,
                    Action::SetFixedTime {
                        time_ms: 1700000000000,
                    },
                    deadline(),
                )
                .await;
            (worker, result)
        });
        let clock = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
        assert!(
            clock["params"]["source"]
                .as_str()
                .unwrap()
                .contains("__testrunClock_")
        );
        reply(&mut peer, &clock, json!({"identifier":next})).await;
        let remove = command(&mut peer, "Page.removeScriptToEvaluateOnNewDocument").await;
        assert_eq!(
            remove["params"]["identifier"],
            if previous.is_none() {
                "user-first"
            } else {
                "user-second"
            }
        );
        reply(&mut peer, &remove, json!({})).await;
        let user = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
        assert_eq!(
            user["params"]["source"],
            "globalThis.acmeInitTime=Date.now()"
        );
        assert_eq!(user["params"]["runImmediately"], false);
        reply(&mut peer, &user, json!({"identifier":"user-second"})).await;
        if let Some(identifier) = previous {
            let remove = command(&mut peer, "Page.removeScriptToEvaluateOnNewDocument").await;
            assert_eq!(remove["params"]["identifier"], identifier);
            reply(&mut peer, &remove, json!({})).await;
        }
        let evaluate = command(&mut peer, "Runtime.evaluate").await;
        reply(
            &mut peer,
            &evaluate,
            json!({"result":{"type":"boolean","value":true}}),
        )
        .await;
        let (returned, result) = task.await.unwrap();
        result.unwrap();
        worker = returned;
    }
}

#[tokio::test]
async fn clock_routes_to_exact_document_and_updates_one_ordered_future_script() {
    let (worker, mut peer, _root, context, _page) = owned_page().await;
    let task = tokio::spawn(async move {
        let mut worker = worker;
        let result = worker
            .execute_one(
                Uuid::new_v4(),
                Request::ContextClock {
                    context,
                    action: Action::Install {
                        time_ms: Some(1700000000000),
                    },
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let script = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    assert_eq!(script["sessionId"], "session-acme");
    assert_eq!(script["params"]["runImmediately"], false);
    assert!(script["params"].get("worldName").is_none());
    reply(&mut peer, &script, json!({"identifier":"clock-first"})).await;
    let evaluate = command(&mut peer, "Runtime.evaluate").await;
    assert_eq!(evaluate["params"]["uniqueContextId"], "world-acme");
    reply(
        &mut peer,
        &evaluate,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let (mut worker, result) = task.await.unwrap();
    let result = result.unwrap();
    assert_eq!(result["playwrightClockEquivalent"], false);
    assert_eq!(result["hostDeadlinesUnchanged"], true);
    assert_eq!(result["remainingActions"], 255);
    let mut foreign = context;
    foreign.connection_id = Uuid::new_v4();
    assert_eq!(
        worker
            .context_clock(Uuid::new_v4(), foreign, Action::Resume {}, deadline())
            .await
            .unwrap_err()
            .code,
        "FOREIGN_REFERENCE"
    );
    assert_eq!(
        worker
            .context_clock(
                Uuid::new_v4(),
                context,
                Action::RunFor { duration_ms: 60001 },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "CLOCK_DURATION_BOUNDS"
    );
    let task = tokio::spawn(async move {
        let result = worker
            .context_clock(
                Uuid::new_v4(),
                context,
                Action::RunFor { duration_ms: 20 },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let script = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    let source = script["params"]["source"].as_str().unwrap();
    assert!(source.contains("1700000000000"));
    assert!(source.contains("\"runFor\""));
    reply(&mut peer, &script, json!({"identifier":"clock-second"})).await;
    let remove = command(&mut peer, "Page.removeScriptToEvaluateOnNewDocument").await;
    assert_eq!(remove["params"]["identifier"], "clock-first");
    reply(&mut peer, &remove, json!({})).await;
    let evaluate = command(&mut peer, "Runtime.evaluate").await;
    reply(&mut peer,&evaluate,json!({"result":{"type":"undefined"},"exceptionDetails":{"exceptionId":1,"lineNumber":0,"columnNumber":0,"text":"Uncaught","exception":{"type":"object","subtype":"error","description":"private callback failure"}}})).await;
    let (mut worker, result) = task.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "CLOCK_EXECUTION_FAILED");
    assert_eq!(error.partial.unwrap()["clockPending"], true);
    assert_eq!(
        worker
            .context_clock(Uuid::new_v4(), context, Action::Resume {}, deadline())
            .await
            .unwrap_err()
            .code,
        "CLOCK_PENDING"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), peer.next())
            .await
            .is_err()
    );
    failed_clock_recovery(worker, &mut peer, context).await;
}

async fn failed_clock_recovery(mut worker: Worker, peer: &mut Peer, context: Reference) {
    worker.startup.enabled = true;
    worker.apply(EventItem::Event(Event {
        method: "Target.attachedToTarget".into(),
        session_id: None,
        params: json!({"sessionId":"session-popup","waitingForDebugger":true,"targetInfo":{"targetId":"page-popup","type":"page","title":"Acme","url":"about:blank","attached":true,"canAccessOpener":true,"openerId":"page-acme","browserContextId":"context-acme"}}),
    })).unwrap();
    let task = tokio::spawn(async move {
        let result = worker.reconcile(Uuid::new_v4(), deadline()).await;
        (worker, result)
    });
    loop {
        let message = tokio::time::timeout_at(deadline(), peer.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Text(message) = message else {
            panic!("Expected popup setup command");
        };
        let value: Value = serde_json::from_str(&message).unwrap();
        assert_eq!(value["sessionId"], "session-popup");
        match value["method"].as_str().unwrap() {
            "Page.getFrameTree" => reply(peer, &value, json!({"frameTree":{"frame":{"id":"frame-popup","loaderId":"loader-popup","url":"about:blank","domainAndRegistry":"","securityOrigin":"://","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await,
            "Page.addScriptToEvaluateOnNewDocument" => {
                assert!(!value["params"]["source"].as_str().unwrap().contains("__testrunClock_"));
                reply(peer, &value, json!({"identifier":"popup-runtime"})).await;
            }
            "Runtime.runIfWaitingForDebugger" => {
                reply(peer, &value, json!({})).await;
                break;
            }
            _ => reply(peer, &value, json!({})).await,
        }
    }
    let (mut worker, result) = task.await.unwrap();
    result.unwrap();
    assert!(worker.startup.failed.is_none());
    let task = tokio::spawn(async move {
        let result = worker
            .execute_one(
                Uuid::new_v4(),
                Request::ContextCreate {
                    options: Default::default(),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let create = command(peer, "Target.createBrowserContext").await;
    reply(peer, &create, json!({"browserContextId":"context-other"})).await;
    let downloads = command(peer, "Browser.setDownloadBehavior").await;
    reply(peer, &downloads, json!({})).await;
    let (mut worker, result) = task.await.unwrap();
    let other: Reference = serde_json::from_value(result.unwrap()["context"].clone()).unwrap();
    // The same reconciliation gate used by ordinary requests remains open.
    worker.reconcile(Uuid::new_v4(), deadline()).await.unwrap();
    let task = tokio::spawn(async move {
        let result = worker
            .execute_one(
                Uuid::new_v4(),
                Request::ContextClose { context },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let close = command(peer, "Target.disposeBrowserContext").await;
    assert_eq!(close["params"]["browserContextId"], "context-acme");
    reply(peer, &close, json!({})).await;
    let (mut worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap()["closed"], true);
    worker.reconcile(Uuid::new_v4(), deadline()).await.unwrap();
    assert!(worker.startup.failed.is_none());
    assert!(worker.index.native(context, Kind::Context).is_err());
    assert_eq!(
        worker.index.native(other, Kind::Context).unwrap(),
        "context-other"
    );
}

#[tokio::test]
async fn clock_lost_install_reply_is_not_reinstalled_on_retry_or_future_setup() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let task = tokio::spawn(async move {
        let result = worker
            .context_clock(
                Uuid::new_v4(),
                context,
                Action::Install { time_ms: None },
                Instant::now() + Duration::from_millis(100),
            )
            .await;
        (worker, result)
    });
    let _script = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    let (mut worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap_err().code, "OUTCOME_UNKNOWN");
    worker
        .configure_clock(Uuid::new_v4(), Kind::Page, "session-acme", deadline())
        .await
        .unwrap();
    assert_eq!(
        worker
            .context_clock(Uuid::new_v4(), context, Action::Resume {}, deadline())
            .await
            .unwrap_err()
            .code,
        "CLOCK_PENDING"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn clock_callback_popup_gets_ordered_history_before_resuming_during_an_update() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    worker.startup.enabled = true;
    let task = tokio::spawn(async move {
        let result = worker
            .context_clock(
                Uuid::new_v4(),
                context,
                Action::Install {
                    time_ms: Some(1700000000000),
                },
                deadline(),
            )
            .await;
        (worker, result)
    });
    let registration = command(&mut peer, "Page.addScriptToEvaluateOnNewDocument").await;
    reply(&mut peer, &registration, json!({"identifier":"clock-main"})).await;
    let running = command(&mut peer, "Runtime.evaluate").await;
    let info = json!({"targetId":"page-popup","type":"page","title":"Acme","url":"about:blank","attached":true,"canAccessOpener":true,"openerId":"page-acme","browserContextId":"context-acme"});
    send(&mut peer, json!({"method":"Target.attachedToTarget","params":{"sessionId":"session-popup","targetInfo":info,"waitingForDebugger":true}})).await;
    let mut clock_installed = false;
    loop {
        let message = tokio::time::timeout_at(deadline(), peer.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Text(message) = message else {
            panic!("Expected popup setup command");
        };
        let value: Value = serde_json::from_str(&message).unwrap();
        assert_eq!(value["sessionId"], "session-popup", "{value}");
        match value["method"].as_str().unwrap() {
            "Page.getFrameTree" => reply(&mut peer, &value, json!({"frameTree":{"frame":{"id":"frame-popup","loaderId":"loader-popup","url":"about:blank","domainAndRegistry":"","securityOrigin":"://","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await,
            "Page.addScriptToEvaluateOnNewDocument" => {
                if value["params"]["source"].as_str().unwrap().contains("__testrunClock_") {
                    assert_eq!(value["params"]["runImmediately"], true);
                    assert!(value["params"]["source"].as_str().unwrap().contains("1700000000000"));
                    clock_installed = true;
                }
                reply(&mut peer, &value, json!({"identifier":"clock-popup"})).await;
            }
            "Runtime.runIfWaitingForDebugger" => {
                assert!(clock_installed, "Popup resumed without clock history");
                reply(&mut peer, &value, json!({})).await;
                break;
            }
            _ => reply(&mut peer, &value, json!({})).await,
        }
    }
    reply(
        &mut peer,
        &running,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let (worker, result) = task.await.unwrap();
    result.unwrap();
    assert_eq!(worker.startup_summary()["pausedTargets"], 0);
}
