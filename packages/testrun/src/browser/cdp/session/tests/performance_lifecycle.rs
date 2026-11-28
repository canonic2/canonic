//! The performance lifecycle resets before capture and owns failed return navigation.
use super::investigation::owned_page;
use super::*;
use crate::browser::tracing::{Options, Performance};

async fn scope(peer: &mut Peer, fresh: bool) {
    let call = command(peer, "Page.createIsolatedWorld").await;
    if fresh {
        send(peer,json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":2,"uniqueId":"Acme-performance-isolated","origin":"://","name":page::WORLD,"auxData":{"frameId":"frame-acme","isDefault":false}}}})).await;
    }
    reply(peer, &call, json!({"executionContextId":2})).await;
    let initialize = command(peer, "Runtime.evaluate").await;
    reply(
        peer,
        &initialize,
        json!({"result":{"type":"object","value":null}}),
    )
    .await;
}
async fn navigation(peer: &mut Peer, url: &str, fresh: bool, fail: bool) {
    scope(peer, fresh).await;
    for method in ["Page.setLifecycleEventsEnabled", "Network.enable"] {
        let call = command(peer, method).await;
        reply(peer, &call, json!({})).await;
    }
    let navigate = command(peer, "Page.navigate").await;
    assert_eq!(navigate["params"], json!({"url":url}));
    if fail {
        send(peer,json!({"id":navigate["id"],"sessionId":navigate["sessionId"],"error":{"code":-32000,"message":"Acme navigation refused"}})).await;
        return;
    }
    reply(
        peer,
        &navigate,
        json!({"frameId":"frame-acme","loaderId":"loader-acme"}),
    )
    .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Navigation must reach load before tracing or further preparation"
    );
    send(peer,json!({"method":"Page.lifecycleEvent","sessionId":"session-acme","params":{"frameId":"frame-acme","loaderId":"loader-acme","name":"load","timestamp":1.0}})).await;
    scope(peer, false).await;
    let geometry = command(peer, "Runtime.evaluate").await;
    reply(
        peer,
        &geometry,
        json!({"result":{"type":"object","value":{"url":url}}}),
    )
    .await;
}
async fn saved_url(peer: &mut Peer, url: &str) {
    let tree = command(peer, "Page.getFrameTree").await;
    let (base, fragment) = url
        .split_once('#')
        .map_or((url, None), |(base, fragment)| (base, Some(fragment)));
    let mut result = json!({"frameTree":{"frame":{"id":"frame-acme","loaderId":"loader-acme","url":base,"domainAndRegistry":"example.com","securityOrigin":"https://example.com","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}});
    if let Some(fragment) = fragment {
        result["frameTree"]["frame"]["urlFragment"] = json!(format!("#{fragment}"));
    }
    reply(peer, &tree, result).await;
}
#[tokio::test]
async fn performance_reset_load_precedes_trace_start_and_original_url_load_precedes_timer() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let original = "https://example.com/Acme?chosen=1#retained";
    let task = tokio::spawn(async move {
        let value = worker
            .trace_start(
                Uuid::new_v4(),
                context,
                Options {
                    performance: Some(Performance::default()),
                    ..Default::default()
                },
                deadline(),
            )
            .await
            .unwrap();
        (worker, value)
    });
    saved_url(&mut peer, original).await;
    navigation(&mut peer, "about:blank", true, false).await;
    let start = command(&mut peer, "Tracing.start").await;
    let categories = start["params"]["traceConfig"]["includedCategories"]
        .as_array()
        .unwrap();
    // Independent pinned DevTools category witnesses, including categories
    // absent from the raw trace, plus both enabled-by-default optional sets.
    for category in [
        "blink.console",
        "disabled-by-default-devtools.target-rundown",
        "disabled-by-default-devtools.v8-source-rundown-sources",
        "disabled-by-default-layout_shift.debug",
        "disabled-by-default-v8.inspector",
        "disabled-by-default-v8.cpu_profiler.hires",
        "disabled-by-default-lighthouse",
        "v8",
        "cppgc",
        "navigation",
        "rail",
        "disabled-by-default-v8.cpu_profiler",
        "disabled-by-default-devtools.screenshot",
    ] {
        assert!(
            categories.contains(&json!(category)),
            "Missing {category}: {start}"
        );
    }
    assert_eq!(categories.len(), 21);
    assert!(!categories.contains(&json!("-*")));
    assert_eq!(
        start["params"]["traceConfig"]["excludedCategories"],
        json!(["*"])
    );
    assert_eq!(start["params"]["traceConfig"]["enableSampling"], true);
    reply(&mut peer, &start, json!({})).await;
    navigation(&mut peer, original, false, false).await;
    let (mut worker, value) = task.await.unwrap();
    assert_eq!(value["reset"]["url"], "about:blank");
    assert_eq!(value["reload"]["url"], original);
    assert_eq!(value["metadataPage"], json!(page));
    assert!(worker.trace_timer_pending());
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(4)).await;
    assert!(!worker.trace_due());
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(worker.trace_due());
    tokio::time::resume();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "No unrequested cache command or repeated navigation"
    );
    let stopping = tokio::spawn(async move {
        worker.poll_trace().await;
        worker.trace_stop(deadline()).await.unwrap()
    });
    let end = command(&mut peer, "Tracing.end").await;
    super::native_tracing::finish(&mut peer, &end).await;
    assert_eq!(stopping.await.unwrap()["report"]["stopReason"], "auto-stop");
}

async fn cache(peer: &mut Peer, disabled: bool, fail: bool) {
    let call = command(peer, "Network.setCacheDisabled").await;
    assert_eq!(call["sessionId"], "session-acme");
    assert_eq!(call["params"], json!({"cacheDisabled":disabled}));
    if fail {
        send(peer,json!({"id":call["id"],"sessionId":call["sessionId"],"error":{"code":-32000,"message":"Acme cache command refused"}})).await;
    } else {
        reply(peer, &call, json!({})).await;
    }
}

#[tokio::test]
async fn performance_cache_restores_after_navigation_failure_and_retains_failed_restoration() {
    for (disable_failure, navigation_failure, restore_failure) in [
        (false, false, false),
        (false, true, false),
        (true, false, false),
        (false, true, true),
    ] {
        let (mut worker, mut peer, _root, context, _page) = owned_page().await;
        let task = tokio::spawn(async move {
            let result = worker
                .trace_start(
                    Uuid::new_v4(),
                    context,
                    Options {
                        performance: Some(Performance {
                            ignore_cache: true,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    deadline(),
                )
                .await;
            (worker, result)
        });
        saved_url(&mut peer, "https://example.com/Acme").await;
        navigation(&mut peer, "about:blank", true, false).await;
        let start = command(&mut peer, "Tracing.start").await;
        reply(&mut peer, &start, json!({})).await;
        cache(&mut peer, true, disable_failure).await;
        if !disable_failure {
            navigation(
                &mut peer,
                "https://example.com/Acme",
                false,
                navigation_failure,
            )
            .await;
        }
        cache(&mut peer, false, restore_failure).await;
        let (mut worker, result) = task.await.unwrap();
        assert_eq!(
            worker.trace_status()["cacheRestorePending"],
            restore_failure
        );
        assert_eq!(
            worker.trace_timer_pending(),
            !disable_failure && !navigation_failure && !restore_failure
        );
        if disable_failure || navigation_failure {
            let error = result.unwrap_err();
            assert_eq!(
                error.code,
                if restore_failure {
                    "TRACE_CACHE_RESTORE_PENDING"
                } else {
                    "BROWSER_METHOD_FAILED"
                }
            );
            assert_eq!(error.partial.unwrap()["traceStarted"], true);
        } else {
            result.unwrap();
        }
        let stop = tokio::spawn(async move { worker.trace_stop(deadline()).await.unwrap() });
        if restore_failure {
            cache(&mut peer, false, false).await;
        }
        let end = command(&mut peer, "Tracing.end").await;
        super::native_tracing::finish(&mut peer, &end).await;
        assert_eq!(stop.await.unwrap()["report"]["complete"], true);
    }
}

#[tokio::test]
async fn performance_cache_without_reload_has_no_cache_or_navigation_effects() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let task = tokio::spawn(async move {
        worker
            .trace_start(
                Uuid::new_v4(),
                context,
                Options {
                    performance: Some(Performance {
                        reload: false,
                        auto_stop: false,
                        ignore_cache: true,
                    }),
                    ..Default::default()
                },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let start = command(&mut peer, "Tracing.start").await;
    reply(&mut peer, &start, json!({})).await;
    let mut worker = task.await.unwrap();
    assert_eq!(worker.trace_status()["cacheRestorePending"], false);
    let stop = tokio::spawn(async move { worker.trace_stop(deadline()).await.unwrap() });
    let end = command(&mut peer, "Tracing.end").await;
    super::native_tracing::finish(&mut peer, &end).await;
    assert_eq!(stop.await.unwrap()["report"]["complete"], true);
}
#[tokio::test]
async fn performance_navigation_failures_distinguish_no_capture_from_owned_recording() {
    for reset_failure in [true, false] {
        let (mut worker, mut peer, _root, context, _page) = owned_page().await;
        let task = tokio::spawn(async move {
            let result = worker
                .trace_start(
                    Uuid::new_v4(),
                    context,
                    Options {
                        performance: Some(Performance::default()),
                        ..Default::default()
                    },
                    deadline(),
                )
                .await;
            (worker, result)
        });
        saved_url(&mut peer, "https://example.com/Acme").await;
        navigation(&mut peer, "about:blank", true, reset_failure).await;
        if !reset_failure {
            let start = command(&mut peer, "Tracing.start").await;
            reply(&mut peer, &start, json!({})).await;
            navigation(&mut peer, "https://example.com/Acme", false, true).await;
        }
        let (mut worker, result) = task.await.unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.code, "BROWSER_METHOD_FAILED");
        assert_eq!(error.partial.unwrap()["traceStarted"], !reset_failure);
        assert_eq!(worker.trace_status()["active"], !reset_failure);
        assert!(
            !worker.trace_timer_pending(),
            "A failed return load cannot arm the observation window"
        );
        let worker = if !reset_failure {
            let stopping = tokio::spawn(async move {
                worker.trace_stop(deadline()).await.unwrap();
                worker
            });
            let end = command(&mut peer, "Tracing.end").await;
            super::native_tracing::finish(&mut peer, &end).await;
            let worker = stopping.await.unwrap();
            assert_eq!(worker.trace_status()["active"], false);
            worker
        } else {
            worker
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
        drop(worker);
    }
}
