//! Native trace timers and reloads keep stream and terminal-receipt ownership.
use super::investigation::owned_page;
use super::*;
use crate::browser::tracing::{Options, Reload};
mod analysis;
mod analysis_page;
mod provenance;
mod provenance_tab;

#[tokio::test]
async fn native_trace_refuses_attached_chrome_before_setup() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    worker.attached = true;
    let result = tokio::select! {
        command = peer.next() => panic!("Unexpected CDP command: {command:?}"),
        result = worker.trace_start(
            Uuid::new_v4(), context,
            Options { page: Some(page), reload: Some(Reload::default()), ..Default::default() },
            deadline(),
        ) => result,
    };
    assert_eq!(result.unwrap_err().code, "TRACE_OWNED_PROCESS_REQUIRED");
    assert!(worker.trace.is_none());
}

pub(super) async fn finish(peer: &mut Peer, end: &Value) {
    reply(peer, end, json!({})).await;
    send(peer,json!({"method":"Tracing.tracingComplete","sessionId":end["sessionId"],"params":{"dataLossOccurred":false,"stream":"Acme-stream"}})).await;
    let read = command(peer, "IO.read").await;
    reply(
        peer,
        &read,
        json!({"data":"{\"traceEvents\":[]}","eof":true}),
    )
    .await;
    let close = command(peer, "IO.close").await;
    reply(peer, &close, json!({})).await;
}

#[tokio::test]
async fn native_trace_export_failure_retries_retained_evidence_without_recollecting() {
    let (mut worker, mut peer, root, context, _page) = owned_page().await;
    let directory = root.path().join("exports");
    crate::artifacts::directory(&directory).unwrap();
    let blocked = directory
        .join("blocked.json")
        .to_string_lossy()
        .into_owned();
    let requested = blocked.clone();
    let start = tokio::spawn(async move {
        worker
            .trace_start(
                Uuid::new_v4(),
                context,
                Options {
                    file_path: Some(requested),
                    ..Default::default()
                },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let request = command(&mut peer, "Tracing.start").await;
    reply(&mut peer, &request, json!({})).await;
    let mut worker = start.await.unwrap();
    std::fs::write(&blocked, b"Acme-existing").unwrap();
    assert_eq!(
        worker
            .trace_stop_to(Some(blocked.clone()), deadline())
            .await
            .unwrap_err()
            .code,
        "TRACE_EXPORT_EXISTS"
    );
    assert_eq!(worker.trace_status()["state"], "recording");
    let stop = tokio::spawn(async move {
        let error = worker.trace_stop(deadline()).await.unwrap_err();
        (worker, error)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let (mut worker, error) = stop.await.unwrap();
    assert_eq!(error.code, "TRACE_EXPORT_EXISTS");
    let report = error.partial.unwrap()["report"].clone();
    assert_eq!(report["complete"], true);
    assert_eq!(worker.trace_status()["active"], false);
    assert_eq!(std::fs::read(&blocked).unwrap(), b"Acme-existing");
    std::fs::remove_file(&blocked).unwrap();
    let same_path = worker.trace_stop(deadline()).await.unwrap();
    assert_eq!(same_path["report"], report);
    assert_eq!(
        same_path["export"]["path"],
        std::fs::canonicalize(&blocked).unwrap().to_str().unwrap()
    );
    let output = directory
        .join("retry.json.gz")
        .to_string_lossy()
        .into_owned();
    let exported = worker
        .trace_stop_to(Some(output.clone()), deadline())
        .await
        .unwrap();
    assert_eq!(exported["report"], report);
    assert!(exported["exportPending"].is_null());
    assert_eq!(
        exported["export"]["path"],
        std::fs::canonicalize(&output).unwrap().to_str().unwrap()
    );
    assert_eq!(worker.trace_stop(deadline()).await.unwrap(), exported);
    assert_eq!(
        worker
            .trace_stop_to(Some(output), deadline())
            .await
            .unwrap(),
        exported
    );
    assert_eq!(
        worker.developers.evidence.list().as_array().unwrap().len(),
        1
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_trace_incomplete_capture_never_publishes_a_caller_export() {
    let (mut worker, mut peer, root, context, _page) = owned_page().await;
    let directory = root.path().join("exports");
    crate::artifacts::directory(&directory).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o1777)).unwrap();
    }
    let path = directory.join("missing/incomplete.json");
    let file_path = path.to_string_lossy().into_owned();
    let start = tokio::spawn(async move {
        worker
            .trace_start(
                Uuid::new_v4(),
                context,
                Options {
                    file_path: Some(file_path),
                    ..Default::default()
                },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let call = command(&mut peer, "Tracing.start").await;
    reply(&mut peer, &call, json!({})).await;
    let mut worker = start.await.unwrap();
    assert!(!directory.join("missing").exists());
    worker.trace_gap();
    let stop = tokio::spawn(async move {
        let error = worker.trace_stop(deadline()).await.unwrap_err();
        (worker, error)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let (mut worker, error) = stop.await.unwrap();
    assert_eq!(error.code, "TRACE_INCOMPLETE");
    assert_eq!(error.partial.as_ref().unwrap()["report"]["complete"], false);
    assert!(!path.exists());
    assert!(!directory.join("missing").exists());
    assert_eq!(
        worker.trace_stop(deadline()).await.unwrap_err().partial,
        error.partial
    );
    assert!(!path.exists());
    assert!(!directory.join("missing").exists());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn native_trace_export_shared_parent_policy_changes_retain_evidence_for_retry() {
    use std::os::unix::fs::PermissionsExt;
    let (mut worker, mut peer, root, context, _page) = owned_page().await;
    let directory = root.path().join("shared");
    crate::artifacts::directory(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = directory.join("missing/Acme.json");
    let requested = path.to_str().unwrap().to_owned();
    let start = tokio::spawn(async move {
        worker
            .trace_start(
                Uuid::new_v4(),
                context,
                Options {
                    file_path: Some(requested),
                    ..Default::default()
                },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let call = command(&mut peer, "Tracing.start").await;
    reply(&mut peer, &call, json!({})).await;
    let worker = start.await.unwrap();
    assert!(!directory.join("missing").exists());
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o777)).unwrap();
    let stop = tokio::spawn(async move {
        let mut worker = worker;
        let error = worker.trace_stop(deadline()).await.unwrap_err();
        (worker, error)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let (mut worker, error) = stop.await.unwrap();
    assert_eq!(error.code, "TRACE_EXPORT_PRIVATE_DIRECTORY_REQUIRED");
    let report = error.partial.unwrap()["report"].clone();
    assert_eq!(report["complete"], true);
    assert_eq!(
        worker.trace_status()["exportPending"]["code"],
        "TRACE_EXPORT_PRIVATE_DIRECTORY_REQUIRED"
    );
    assert!(!directory.join("missing").exists());
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o1777)).unwrap();
    let recovered = worker.trace_stop(deadline()).await.unwrap();
    assert_eq!(recovered["report"], report);
    assert!(path.is_file());
    assert_eq!(
        std::fs::metadata(directory.join("missing"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(worker.trace_stop(deadline()).await.unwrap(), recovered);
    assert_eq!(
        worker.developers.evidence.list().as_array().unwrap().len(),
        1
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err(),
        "Retry must not send another trace end/read"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn native_trace_export_filename_repeat_keeps_the_receipt_after_an_ancestor_alias_changes() {
    use std::os::unix::fs::symlink;
    let (mut worker, mut peer, root, context, _page) = owned_page().await;
    let first = root.path().join("first");
    let second = root.path().join("second");
    for directory in [&first, &second] {
        crate::artifacts::directory(directory).unwrap();
        crate::artifacts::directory(&directory.join("private")).unwrap();
    }
    let alias = root.path().join("alias");
    symlink(&first, &alias).unwrap();
    let requested = alias.join("private/Acme.json").to_str().unwrap().to_owned();
    let bound = requested.clone();
    let start = tokio::spawn(async move {
        worker
            .trace_start(
                Uuid::new_v4(),
                context,
                Options {
                    file_path: Some(bound),
                    ..Default::default()
                },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let starting = command(&mut peer, "Tracing.start").await;
    reply(&mut peer, &starting, json!({})).await;
    let mut worker = start.await.unwrap();
    let stop = tokio::spawn(async move {
        let stopped = worker.trace_stop(deadline()).await.unwrap();
        (worker, stopped)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let (mut worker, stopped) = stop.await.unwrap();
    assert!(first.join("private/Acme.json").is_file());
    std::fs::remove_file(&alias).unwrap();
    symlink(&second, &alias).unwrap();
    let repeated = worker
        .trace_stop_to(Some(requested), deadline())
        .await
        .unwrap();
    assert!(
        !second.join("private/Acme.json").exists(),
        "A repeated successful export must not publish a second copy after an alias changes"
    );
    assert_eq!(repeated, stopped);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_trace_explicit_page_and_boolean_stop_keep_selection_and_original_capture() {
    for auto_stop in [true, false] {
        let (mut worker, mut peer, _root, context, page) = owned_page().await;
        worker.shared.state.lock().unwrap().selected_page = None;
        let starting = tokio::spawn(async move {
            let value = worker
                .trace_start(
                    Uuid::new_v4(),
                    context,
                    Options {
                        page: Some(page),
                        auto_stop: Some(auto_stop),
                        ..Default::default()
                    },
                    deadline(),
                )
                .await
                .unwrap();
            (worker, value)
        });
        let start = command(&mut peer, "Tracing.start").await;
        reply(&mut peer, &start, json!({})).await;
        let (mut worker, value) = starting.await.unwrap();
        assert_eq!(value["metadataPage"], json!(page));
        assert_eq!(worker.shared.state.lock().unwrap().selected_page, None);
        assert_eq!(worker.trace_timer_pending(), auto_stop);
        let worker = if auto_stop {
            // No I/O is pending while the clock advances. Keep the existing
            // five-second transport deadline independent of this five-second
            // capture window instead of racing those identical deadlines.
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(4)).await;
            assert!(!worker.trace_due());
            tokio::time::advance(Duration::from_secs(1)).await;
            assert!(worker.trace_due());
            tokio::time::resume();
            let stopping = tokio::spawn(async move {
                worker.poll_trace().await;
                let stopped = worker.trace_stop(deadline()).await.unwrap();
                (worker, stopped)
            });
            let end = command(&mut peer, "Tracing.end").await;
            finish(&mut peer, &end).await;
            let (mut worker, stopped) = stopping.await.unwrap();
            assert_eq!(stopped["report"]["stopReason"], "auto-stop");
            assert_eq!(worker.trace_stop(deadline()).await.unwrap(), stopped);
            worker
        } else {
            assert!(value["durationRemainingMs"].is_null());
            let stopping = tokio::spawn(async move {
                let value = worker.trace_stop(deadline()).await.unwrap();
                (worker, value)
            });
            let end = command(&mut peer, "Tracing.end").await;
            finish(&mut peer, &end).await;
            let (worker, stopped) = stopping.await.unwrap();
            assert_eq!(stopped["report"]["stopReason"], "requested");
            worker
        };
        assert!(!worker.trace_timer_pending());
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_trace_wrong_context_and_stale_explicit_page_refuse_before_io() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    let foreign_page = Reference {
        connection_id: Uuid::new_v4(),
        ..page
    };
    let error = worker
        .trace_start(
            Uuid::new_v4(),
            context,
            Options {
                page: Some(foreign_page),
                ..Default::default()
            },
            deadline(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "FOREIGN_REFERENCE");
    let other = worker.index.register_context("Acme-other-context").unwrap();
    let error = worker
        .trace_start(
            Uuid::new_v4(),
            other,
            Options {
                page: Some(page),
                ..Default::default()
            },
            deadline(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "INVALID_SCOPE");
    worker.index.retire(page).unwrap();
    let error = worker
        .trace_start(
            Uuid::new_v4(),
            context,
            Options {
                page: Some(page),
                ..Default::default()
            },
            deadline(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "STALE_REFERENCE");
    assert_eq!(worker.trace_status()["state"], "idle");
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_trace_duration_finishes_while_an_evaluation_waits_and_replays_only_the_receipt() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let starting = tokio::spawn(async move {
        let value = worker
            .trace_start(
                Uuid::new_v4(),
                context,
                Options {
                    max_duration_ms: Some(1000),
                    ..Default::default()
                },
                deadline(),
            )
            .await
            .unwrap();
        (worker, value)
    });
    let request = command(&mut peer, "Tracing.start").await;
    reply(&mut peer, &request, json!({})).await;
    let (mut worker, start) = starting.await.unwrap();
    assert!(worker.trace_timer_pending());
    let shared = worker.shared.clone();
    let waiting = tokio::spawn(async move {
        let value = worker
            .call_read(
                Uuid::new_v4(),
                Some("session-acme"),
                "Runtime.evaluate",
                json!({"expression":"Acme pending"}),
                deadline(),
            )
            .await;
        (worker, value)
    });
    let evaluation = command(&mut peer, "Runtime.evaluate").await;
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let until = deadline();
    while shared.details.lock().unwrap()["trace"]["state"] != "complete" {
        assert!(Instant::now() < until);
        tokio::task::yield_now().await;
    }
    assert!(!waiting.is_finished());
    reply(
        &mut peer,
        &evaluation,
        json!({"result":{"type":"number","value":42}}),
    )
    .await;
    let (mut worker, result) = waiting.await.unwrap();
    assert_eq!(result.unwrap()["result"]["value"], 42);
    let first = worker.trace_stop(deadline()).await.unwrap();
    assert_eq!(first["report"]["traceId"], start["traceId"]);
    assert_eq!(first["report"]["stopReason"], "duration");
    assert_eq!(first["report"]["complete"], true);
    assert_eq!(worker.trace_stop(deadline()).await.unwrap(), first);
    assert!(!worker.trace_timer_pending());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    let starting = tokio::spawn(async move {
        worker
            .trace_start(Uuid::new_v4(), context, Default::default(), deadline())
            .await
            .unwrap();
        worker
    });
    let request = command(&mut peer, "Tracing.start").await;
    reply(&mut peer, &request, json!({})).await;
    let mut worker = starting.await.unwrap();
    assert_ne!(worker.trace_status()["traceId"], start["traceId"]);
    assert_eq!(worker.trace_status()["state"], "recording");
    assert!(!worker.trace_timer_pending());
    let stopping = tokio::spawn(async move {
        let value = worker.trace_stop(deadline()).await.unwrap();
        (worker, value)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let (worker, result) = stopping.await.unwrap();
    assert_eq!(result["report"]["stopReason"], "requested");
    assert_eq!(worker.trace_status()["report"], result["report"]);
}

#[tokio::test]
async fn native_trace_failed_automatic_end_stays_owned_without_automatic_retry() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let starting = tokio::spawn(async move {
        worker
            .trace_start(
                Uuid::new_v4(),
                context,
                Options {
                    max_duration_ms: Some(1000),
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
    let mut worker = starting.await.unwrap();
    let waiting = tokio::spawn(async move {
        worker
            .call_read(
                Uuid::new_v4(),
                Some("session-acme"),
                "Runtime.evaluate",
                json!({"expression":"Acme pending"}),
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let evaluation = command(&mut peer, "Runtime.evaluate").await;
    let end = command(&mut peer, "Tracing.end").await;
    send(&mut peer,json!({"id":end["id"],"sessionId":end["sessionId"],"error":{"code":-32000,"message":"Acme end failed"}})).await;
    reply(
        &mut peer,
        &evaluation,
        json!({"result":{"type":"number","value":42}}),
    )
    .await;
    let mut worker = waiting.await.unwrap();
    assert_eq!(worker.trace_status()["state"], "finalization_pending");
    assert_eq!(
        worker.trace_status()["automaticFinalizationAttempted"],
        true
    );
    assert_eq!(worker.trace_status()["stopRequested"], true);
    assert!(!worker.trace_timer_pending());
    worker.poll_trace().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    let stopping = tokio::spawn(async move {
        let result = worker.trace_stop(deadline()).await.unwrap();
        (worker, result)
    });
    send(&mut peer,json!({"method":"Tracing.tracingComplete","sessionId":end["sessionId"],"params":{"dataLossOccurred":false,"stream":"Acme-late-stream"}})).await;
    let read = command(&mut peer, "IO.read").await;
    assert_eq!(read["params"]["handle"], "Acme-late-stream");
    reply(
        &mut peer,
        &read,
        json!({"data":"{\"traceEvents\":[]}","eof":true}),
    )
    .await;
    let close = command(&mut peer, "IO.close").await;
    reply(&mut peer, &close, json!({})).await;
    let (mut worker, result) = stopping.await.unwrap();
    assert_eq!(result["report"]["stopReason"], "duration");
    assert_eq!(worker.trace_stop(deadline()).await.unwrap(), result);
}

#[tokio::test]
async fn native_trace_reload_follows_start_ack_and_preserves_capture_on_reload_failure() {
    for (ignore_cache, auto_stop) in [
        (false, None),
        (true, None),
        (false, Some(true)),
        (true, Some(true)),
    ] {
        let (mut worker, mut peer, _root, context, _page) = owned_page().await;
        let task = tokio::spawn(async move {
            let result = worker
                .trace_start(
                    Uuid::new_v4(),
                    context,
                    Options {
                        reload: Some(Reload { ignore_cache }),
                        auto_stop,
                        ..Default::default()
                    },
                    deadline(),
                )
                .await;
            (worker, result)
        });
        let start = command(&mut peer, "Tracing.start").await;
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
        reply(&mut peer, &start, json!({})).await;
        let world = command(&mut peer, "Page.createIsolatedWorld").await;
        send(&mut peer,json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":2,"uniqueId":"Acme-isolated","origin":"://","name":page::WORLD,"auxData":{"frameId":"frame-acme","isDefault":false}}}})).await;
        reply(&mut peer, &world, json!({"executionContextId":2})).await;
        let initialize = command(&mut peer, "Runtime.evaluate").await;
        reply(
            &mut peer,
            &initialize,
            json!({"result":{"type":"object","value":null}}),
        )
        .await;
        for method in ["Page.setLifecycleEventsEnabled", "Network.enable"] {
            let call = command(&mut peer, method).await;
            reply(&mut peer, &call, json!({})).await;
        }
        let reload = command(&mut peer, "Page.reload").await;
        assert_eq!(reload["params"], json!({"ignoreCache":ignore_cache}));
        send(&mut peer,json!({"id":reload["id"],"sessionId":reload["sessionId"],"error":{"code":-32000,"message":"Acme rejected reload"}})).await;
        let (mut worker, result) = task.await.unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.code, "BROWSER_METHOD_FAILED");
        assert_eq!(error.partial.unwrap()["traceStarted"], true);
        assert_eq!(worker.trace_status()["state"], "recording");
        assert!(
            !worker.trace_timer_pending(),
            "Failed reload must not arm the five-second observation window"
        );
        let stop = tokio::spawn(async move {
            worker.trace_stop(deadline()).await.unwrap();
            worker
        });
        let end = command(&mut peer, "Tracing.end").await;
        finish(&mut peer, &end).await;
        let worker = stop.await.unwrap();
        assert_eq!(worker.trace_status()["state"], "complete");
    }
}

#[tokio::test]
async fn native_trace_reload_without_a_page_refuses_before_trace_allocation() {
    let (mut worker, mut peer, _root) = fixture().await;
    let task = tokio::spawn(async move {
        let context = worker
            .context_create(Uuid::new_v4(), deadline())
            .await
            .unwrap();
        let error = worker
            .trace_start(
                Uuid::new_v4(),
                context,
                Options {
                    reload: Some(Reload::default()),
                    ..Default::default()
                },
                deadline(),
            )
            .await
            .unwrap_err();
        (worker, error)
    });
    let context = command(&mut peer, "Target.createBrowserContext").await;
    reply(
        &mut peer,
        &context,
        json!({"browserContextId":"Acme-empty"}),
    )
    .await;
    let downloads = command(&mut peer, "Browser.setDownloadBehavior").await;
    reply(&mut peer, &downloads, json!({})).await;
    let (worker, error) = task.await.unwrap();
    assert_eq!(error.code, "TRACE_PAGE_REQUIRED");
    assert_eq!(worker.trace_status()["state"], "idle");
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}
