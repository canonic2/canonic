//! Post-capture frontend allocation preserves the complete trace and uncertain owners.
#[tokio::test]
async fn native_trace_analysis_page_lost_discovery_does_not_invent_a_creation_owner() {
    let (worker, mut peer, _root, context, page) = owned_page().await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    let stopping = tokio::spawn(async move {
        let result = worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let lookup = command(&mut peer, "Target.getDevToolsTarget").await;
    reply(&mut peer, &lookup, json!({"targetId":7})).await;
    let (worker, result) = stopping.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    let receipt = error.partial.unwrap();
    assert_eq!(receipt["report"]["complete"], true);
    assert_eq!(
        receipt["analysisPending"]["frontendCreationUncertain"],
        false
    );
    assert!(worker.developers.analysis_uncertain.is_empty());
    assert!(worker.developer_snapshots().as_array().unwrap().is_empty());
    assert_eq!(
        worker.connection.as_ref().unwrap().status().loss,
        Some(crate::browser::cdp::Loss::Protocol)
    );
}
#[tokio::test]
async fn native_trace_analysis_page_existing_frontend_refuses_before_open_or_attach() {
    let (worker, mut peer, _root, context, page) = owned_page().await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    let stopping = tokio::spawn(async move {
        let result = worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let lookup = command(&mut peer, "Target.getDevToolsTarget").await;
    reply(
        &mut peer,
        &lookup,
        json!({"targetId":"Acme-unowned-frontend"}),
    )
    .await;
    let (mut worker, result) = tokio::select! {
        result = stopping => result.unwrap(),
        request = peer.next() => panic!("Automatic setup touched an unowned frontend before refusing: {request:?}"),
    };
    let error = result.unwrap_err();
    assert_eq!(error.code, "PERFORMANCE_OWNED_FRONTEND_REQUIRED");
    let receipt = error.partial.unwrap();
    assert_eq!(receipt["report"]["complete"], true);
    assert!(receipt["analysisFrontend"].is_null());
    assert!(
        receipt["analysisPending"]["frontends"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        receipt["analysisPending"]["frontendCreationUncertain"],
        false
    );
    // Its external owner closes that frontend before the next discovery read.
    let retry = tokio::spawn(async move {
        let result = worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
            .await;
        (worker, result)
    });
    frontend_replies(&mut peer, false).await;
    model(&mut peer, false).await;
    let (worker, result) = retry.await.unwrap();
    assert_eq!(result.unwrap()["report"], receipt["report"]);
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
async fn native_trace_analysis_page_borrowed_frontend_retains_report_and_attachment_cleanup() {
    let (worker, mut peer, _root, context, page) = owned_page().await;
    // An explicit developer_open may attach a borrowed frontend. Automatic
    // analysis must refuse it without another open or attachment operation.
    let (worker, developer) = super::analysis::frontend(worker, &mut peer, page, true).await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    let stopping = tokio::spawn(async move {
        let result = worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let (mut worker, result) = stopping.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "PERFORMANCE_OWNED_FRONTEND_REQUIRED");
    let receipt = error.partial.unwrap();
    assert_eq!(receipt["report"]["complete"], true);
    assert!(receipt["analysisFrontend"].is_null());
    assert_eq!(
        receipt["analysisPending"]["frontends"][0]["developer"],
        json!(developer)
    );
    assert_eq!(
        worker.developer_status(developer).unwrap()["frontendOwned"],
        false
    );
    let closing = tokio::spawn(async move {
        let result = worker.developer_close(developer, deadline()).await.unwrap();
        assert_eq!(result["borrowedFrontendPreserved"], true);
        worker
    });
    let detach = command(&mut peer, "Target.detachFromTarget").await;
    reply(&mut peer, &detach, json!({})).await;
    let mut worker = closing.await.unwrap();
    // The external owner closes its frontend before the next discovery read.
    let retry = tokio::spawn(async move {
        let result = worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
            .await;
        (worker, result)
    });
    frontend_replies(&mut peer, false).await;
    model(&mut peer, false).await;
    let (worker, result) = retry.await.unwrap();
    assert_eq!(result.unwrap()["report"], receipt["report"]);
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
use super::analysis::{frontend_replies, model, recording};
use super::*;

#[tokio::test]
async fn native_trace_analysis_page_allocates_only_after_complete_export_and_replays() {
    let (worker, mut peer, root, context, page) = owned_page().await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    let path = root.path().join("Acme-post-capture.json");
    let output = path.to_str().unwrap().to_owned();
    let stopping = tokio::spawn(async move {
        let result = worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), Some(output), page, deadline())
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    // The first frontend call can only occur after IO.close and publication.
    let get = command(&mut peer, "Target.getDevToolsTarget").await;
    assert_eq!(std::fs::read(&path).unwrap(), b"{\"traceEvents\":[]}");
    reply(&mut peer, &get, json!({})).await;
    let open = command(&mut peer, "Target.openDevTools").await;
    reply(
        &mut peer,
        &open,
        json!({"targetId":"Acme-analysis-frontend"}),
    )
    .await;
    let attach = command(&mut peer, "Target.attachToTarget").await;
    reply(
        &mut peer,
        &attach,
        json!({"sessionId":"Acme-analysis-session"}),
    )
    .await;
    let enable = command(&mut peer, "Runtime.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let ready = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &ready,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let probe = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &probe,
        json!({"result":{"type":"object","value":{"nativeFrontend":true,"modules":{}}}}),
    )
    .await;
    model(&mut peer, false).await;
    let (mut worker, result) = stopping.await.unwrap();
    let receipt = result.unwrap();
    assert_eq!(receipt["analysisFrontend"]["createdByStop"], true);
    assert_eq!(
        receipt["analysisFrontend"]["developer"],
        receipt["analysis"]["developer"]
    );
    assert_eq!(
        receipt["analysis"]["evidenceId"],
        receipt["report"]["evidenceId"]
    );
    assert_eq!(
        worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
            .await
            .unwrap(),
        receipt
    );
    let developer = serde_json::from_value(receipt["analysis"]["developer"].clone()).unwrap();
    let closing = tokio::spawn(async move {
        worker.developer_close(developer, deadline()).await.unwrap();
        worker.index.retire(page).unwrap();
        let replay = worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
            .await
            .unwrap();
        (worker, replay)
    });
    let release = command(&mut peer, "Runtime.evaluate").await;
    reply(
        &mut peer,
        &release,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let detach = command(&mut peer, "Target.detachFromTarget").await;
    reply(&mut peer, &detach, json!({})).await;
    let close = command(&mut peer, "Target.closeTarget").await;
    reply(&mut peer, &close, json!({"success":true})).await;
    let (worker, replay) = closing.await.unwrap();
    assert_eq!(replay, receipt);
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
async fn native_trace_analysis_page_refuses_wrong_page_conflicts_and_export_before_ending() {
    let (worker, mut peer, root, context, page) = owned_page().await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    let other = Reference {
        id: Uuid::new_v4(),
        ..page
    };
    assert_eq!(
        worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, other, deadline())
            .await
            .unwrap_err()
            .code,
        "TRACE_ANALYSIS_PAGE_MISMATCH"
    );
    assert_eq!(
        supported(&Request::TraceStop {
            file_path: None,
            analysis_developer: Some(other),
            analysis_page: Some(page)
        })
        .unwrap_err()
        .code,
        "TRACE_ANALYSIS_OPTIONS_CONFLICT"
    );
    let path = root.path().join("Acme-occupied.json");
    std::fs::write(&path, b"Acme existing").unwrap();
    assert_eq!(
        worker
            .trace_stop_for_analysis_page(
                Uuid::new_v4(),
                Some(path.to_str().unwrap().to_owned()),
                page,
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "TRACE_EXPORT_EXISTS"
    );
    assert_eq!(worker.trace_status()["state"], "recording");
    assert!(worker.developer_snapshots().as_array().unwrap().is_empty());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_trace_analysis_page_allocation_failure_retries_complete_bytes_without_recollecting()
{
    let (worker, mut peer, _root, context, page) = owned_page().await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    let stopping = tokio::spawn(async move {
        let result = worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let get = command(&mut peer, "Target.getDevToolsTarget").await;
    send(
        &mut peer,
        json!({"id":get["id"],"error":{"code":-32000,"message":"Acme lookup refusal"}}),
    )
    .await;
    let (mut worker, result) = stopping.await.unwrap();
    let failed = result.unwrap_err();
    assert_eq!(failed.code, "BROWSER_METHOD_FAILED");
    let receipt = failed.partial.unwrap();
    assert_eq!(receipt["report"]["complete"], true);
    assert_eq!(
        receipt["analysisPending"]["frontendCreationUncertain"],
        false
    );
    let retry = tokio::spawn(async move {
        let result = worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
            .await;
        (worker, result)
    });
    frontend_replies(&mut peer, false).await;
    model(&mut peer, false).await;
    let (worker, result) = retry.await.unwrap();
    let recovered = result.unwrap();
    assert_eq!(recovered["report"], receipt["report"]);
    assert!(recovered["analysisPending"].is_null());
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
async fn native_trace_analysis_page_never_opens_a_frontend_for_incomplete_bytes() {
    let (worker, mut peer, _root, context, page) = owned_page().await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    worker.trace_gap();
    let stopping = tokio::spawn(async move {
        let result = worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let (worker, result) = stopping.await.unwrap();
    assert_eq!(result.unwrap_err().code, "TRACE_INCOMPLETE");
    assert!(worker.developer_snapshots().as_array().unwrap().is_empty());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_trace_analysis_page_unknown_frontend_creation_never_automatically_reallocates() {
    unconfirmed_frontend(false).await;
    unconfirmed_frontend(true).await;
}

async fn unconfirmed_frontend(malformed_reply: bool) {
    let (worker, mut peer, _root, context, page) = owned_page().await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    let stopping = tokio::spawn(async move {
        let result = worker
            .trace_stop_for_analysis_page(
                Uuid::new_v4(),
                None,
                page,
                Instant::now() + Duration::from_millis(500),
            )
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let get = command(&mut peer, "Target.getDevToolsTarget").await;
    reply(&mut peer, &get, json!({})).await;
    let opening = command(&mut peer, "Target.openDevTools").await;
    if malformed_reply {
        reply(&mut peer, &opening, json!({})).await;
    }
    let (mut worker, result) = stopping.await.unwrap();
    let error = result.unwrap_err();
    // Schema validation also fences a malformed mutation reply as unknown;
    // neither missing acknowledgement nor missing target identity permits replay.
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    let first = error.partial.unwrap();
    assert_eq!(first["report"]["complete"], true);
    assert_eq!(first["analysisPending"]["frontendCreationUncertain"], true);
    let retry = worker
        .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
        .await
        .unwrap_err();
    assert_eq!(retry.code, "TRACE_ANALYSIS_FRONTEND_RECOVERY_REQUIRED");
    assert_eq!(retry.partial.unwrap()["report"], first["report"]);
    if malformed_reply {
        // The malformed native reply closes this transport epoch. Its writer
        // is gone before the loss witness; complete evidence and the uncertain
        // allocation reservation still survive without another native dispatch.
        let transport = worker.connection.as_ref().unwrap().status();
        assert_eq!(transport.loss, Some(crate::browser::cdp::Loss::Protocol));
        assert!(!transport.open);
        assert!(worker.developers.analysis_uncertain.contains(&page));
        assert_eq!(
            worker.developers.evidence.list().as_array().unwrap().len(),
            1
        );
        return;
    }
    assert!(worker.connection.as_ref().unwrap().status().open);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    // Replacing the terminal trace receipt must not erase the uncertain native
    // allocation owner or permit a second automatic frontend allocation.
    let mut worker = recording(worker, &mut peer, context, page).await;
    let next = tokio::spawn(async move {
        let result = worker
            .trace_stop_for_analysis_page(Uuid::new_v4(), None, page, deadline())
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let (worker, result) = next.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "TRACE_ANALYSIS_FRONTEND_RECOVERY_REQUIRED");
    let second = error.partial.unwrap();
    assert_eq!(second["report"]["complete"], true);
    assert_ne!(
        second["report"]["evidenceId"],
        first["report"]["evidenceId"]
    );
    assert_eq!(second["analysisPending"]["frontendCreationUncertain"], true);
    assert_eq!(
        worker.developers.evidence.list().as_array().unwrap().len(),
        2
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_trace_analysis_page_direct_and_batch_keep_ordinary_admission() {
    let (worker, _peer, _root, _, page) = owned_page().await;
    let (session, mut ordinary, _executions, mut responses, _controls) =
        super::super::execution_concurrency::client(&worker);
    let request = Request::TraceStop {
        file_path: None,
        analysis_developer: None,
        analysis_page: Some(page),
    };
    for request in [
        request.clone(),
        Request::Batch {
            requests: vec![request.clone()],
        },
    ] {
        let id = session
            .submit(Uuid::new_v4(), request.clone(), 5000)
            .unwrap();
        assert_eq!(ordinary.try_recv().unwrap().id, id);
        assert!(responses.try_recv().is_err());
        let capacity = session
            .0
            .capacity
            .clone()
            .try_acquire_many_owned(32)
            .unwrap();
        assert_eq!(
            session
                .submit(Uuid::new_v4(), request, 5000)
                .unwrap_err()
                .code,
            "OPERATION_CAPACITY"
        );
        drop(capacity);
    }
}
