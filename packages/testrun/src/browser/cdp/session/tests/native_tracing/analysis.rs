//! Complete capture receipts and exact frontend owners survive analysis failures.
use super::*;

pub(super) async fn frontend(
    mut worker: Worker,
    peer: &mut Peer,
    page: Reference,
    borrowed: bool,
) -> (Worker, Reference) {
    let opening = tokio::spawn(async move {
        let value = worker
            .developer_open(Uuid::new_v4(), page, Some("timeline".into()), deadline())
            .await
            .unwrap();
        (
            worker,
            serde_json::from_value(value["developer"].clone()).unwrap(),
        )
    });
    frontend_replies(peer, borrowed).await;
    opening.await.unwrap()
}

pub(super) async fn frontend_replies(peer: &mut Peer, borrowed: bool) {
    let call = command(peer, "Target.getDevToolsTarget").await;
    reply(
        peer,
        &call,
        if borrowed {
            json!({"targetId":"Acme-analysis-frontend"})
        } else {
            json!({})
        },
    )
    .await;
    let call = command(peer, "Target.openDevTools").await;
    reply(peer, &call, json!({"targetId":"Acme-analysis-frontend"})).await;
    let call = command(peer, "Target.attachToTarget").await;
    reply(peer, &call, json!({"sessionId":"Acme-analysis-session"})).await;
    let call = command(peer, "Runtime.enable").await;
    reply(peer, &call, json!({})).await;
    let call = command(peer, "Runtime.evaluate").await;
    reply(
        peer,
        &call,
        json!({"result":{"type":"boolean","value":true}}),
    )
    .await;
    let call = command(peer, "Runtime.evaluate").await;
    reply(
        peer,
        &call,
        json!({"result":{"type":"object","value":{"nativeFrontend":true,"modules":{}}}}),
    )
    .await;
}
pub(super) async fn recording(
    mut worker: Worker,
    peer: &mut Peer,
    context: Reference,
    page: Reference,
) -> Worker {
    let starting = tokio::spawn(async move {
        worker
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
            .unwrap();
        worker
    });
    let call = command(peer, "Tracing.start").await;
    reply(peer, &call, json!({})).await;
    starting.await.unwrap()
}
pub(super) async fn model(peer: &mut Peer, fail: bool) {
    for action in ["initialize", "append", "finish"] {
        let call = command(peer, "Runtime.evaluate").await;
        assert_eq!(call["sessionId"], "Acme-analysis-session");
        assert!(
            call["params"]["expression"]
                .as_str()
                .unwrap()
                .contains(&format!(",\"{action}\","))
        );
        if action == "append" {
            assert!(
                call["params"]["expression"]
                    .as_str()
                    .unwrap()
                    .contains(&serde_json::to_string("{\"traceEvents\":[]}").unwrap())
            );
        }
        if action == "finish" && fail {
            reply(peer,&call,json!({"result":{"type":"object","subtype":"error"},"exceptionDetails":{"exceptionId":1,"text":"Uncaught","lineNumber":0,"columnNumber":0,"exception":{"type":"object","subtype":"error","description":"Error: canonic:PERFORMANCE_TRACE_METADATA_UNAVAILABLE"}}})).await;
        } else {
            reply(peer,&call,json!({"result":{"type":if action=="finish" {"object"} else {"boolean"},"value":if action=="finish" {json!({"data":{"insightSets":[]},"truncated":false})} else {json!(true)}}})).await;
        }
    }
}
async fn release(
    mut worker: Worker,
    peer: &mut Peer,
    developer: Reference,
    analysis: Value,
    already_disposed: bool,
) -> Worker {
    let releasing = tokio::spawn(async move {
        worker
            .performance_operation(
                Uuid::new_v4(),
                Request::PerformanceRelease {
                    developer,
                    analysis_id: serde_json::from_value(analysis).unwrap(),
                },
                deadline(),
            )
            .await
            .unwrap();
        worker
    });
    let call = command(peer, "Runtime.evaluate").await;
    assert!(
        call["params"]["expression"]
            .as_str()
            .unwrap()
            .contains(",'release',")
    );
    if already_disposed {
        reply(peer,&call,json!({"result":{"type":"object","subtype":"error"},"exceptionDetails":{"exceptionId":1,"text":"Uncaught","lineNumber":0,"columnNumber":0,"exception":{"type":"object","subtype":"error","description":"Error: canonic:UNKNOWN_PERFORMANCE_ANALYSIS"}}})).await;
    } else {
        reply(
            peer,
            &call,
            json!({"result":{"type":"boolean","value":true}}),
        )
        .await;
    }
    releasing.await.unwrap()
}

#[tokio::test]
async fn native_trace_analysis_returns_a_cached_receipt_after_model_release() {
    let (worker, mut peer, _root, context, page) = owned_page().await;
    let (worker, developer) = frontend(worker, &mut peer, page, false).await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    let stopping = tokio::spawn(async move {
        let result = worker
            .trace_stop_with_analysis(Uuid::new_v4(), None, Some(developer), deadline())
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    model(&mut peer, false).await;
    let (worker, result) = stopping.await.unwrap();
    let receipt = result.unwrap();
    assert_eq!(receipt["analysis"]["developer"], json!(developer));
    assert_eq!(
        receipt["analysis"]["evidenceId"],
        receipt["report"]["evidenceId"]
    );
    let mut worker = release(
        worker,
        &mut peer,
        developer,
        receipt["analysis"]["analysisId"].clone(),
        false,
    )
    .await;
    // Neither replay reopens the released model or rereads the native stream.
    assert_eq!(
        worker
            .trace_stop_with_analysis(Uuid::new_v4(), None, Some(developer), deadline())
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(
        worker
            .trace_stop_with_analysis(Uuid::new_v4(), None, None, deadline())
            .await
            .unwrap(),
        receipt
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
async fn native_trace_analysis_failure_retries_the_same_capture_after_explicit_model_release() {
    let (worker, mut peer, _root, context, page) = owned_page().await;
    let (worker, developer) = frontend(worker, &mut peer, page, false).await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    let stopping = tokio::spawn(async move {
        let result = worker
            .trace_stop_with_analysis(Uuid::new_v4(), None, Some(developer), deadline())
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    model(&mut peer, true).await;
    let (mut worker, result) = stopping.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "PERFORMANCE_TRACE_METADATA_UNAVAILABLE");
    let retained = error.partial.unwrap();
    assert_eq!(retained["report"]["complete"], true);
    assert_eq!(retained["analysisPending"]["recollected"], false);
    let failed_id = retained["analysisPending"]["model"]["analysisId"].clone();
    assert!(failed_id.is_string());
    assert_eq!(
        worker
            .trace_stop_with_analysis(Uuid::new_v4(), None, Some(developer), deadline())
            .await
            .unwrap_err()
            .code,
        "PERFORMANCE_MODEL_CAPACITY"
    );
    assert_eq!(
        worker.trace_status()["analysisPending"]["model"]["analysisId"],
        failed_id
    );
    worker = release(worker, &mut peer, developer, failed_id.clone(), true).await;
    let retry = tokio::spawn(async move {
        let result = worker
            .trace_stop_with_analysis(Uuid::new_v4(), None, Some(developer), deadline())
            .await;
        (worker, result)
    });
    model(&mut peer, false).await;
    let (worker, result) = retry.await.unwrap();
    let recovered = result.unwrap();
    assert_eq!(recovered["report"], retained["report"]);
    assert!(recovered["analysisPending"].is_null());
    assert_ne!(recovered["analysis"]["analysisId"], failed_id);
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
async fn native_trace_analysis_refuses_wrong_page_or_borrowed_frontends_before_trace_end() {
    for borrowed in [false, true] {
        let (mut worker, mut peer, _root, context, page) = owned_page().await;
        let selected = if borrowed {
            page
        } else {
            worker.index.apply(Event{method:"Target.targetCreated".into(),session_id:None,params:json!({"targetInfo":{"targetId":"page-other","type":"page","title":"Acme other","url":"about:blank","attached":false,"canAccessOpener":false,"browserContextId":"context-acme"}})}).unwrap();
            worker.index.target_reference("page-other").unwrap()
        };
        let (worker, developer) = frontend(worker, &mut peer, selected, borrowed).await;
        let mut worker = recording(worker, &mut peer, context, page).await;
        let error = worker
            .trace_stop_with_analysis(Uuid::new_v4(), None, Some(developer), deadline())
            .await
            .unwrap_err();
        assert_eq!(
            error.code,
            if borrowed {
                "PERFORMANCE_OWNED_FRONTEND_REQUIRED"
            } else {
                "TRACE_ANALYSIS_PAGE_MISMATCH"
            }
        );
        assert_eq!(worker.trace_status()["state"], "recording");
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_trace_analysis_never_parses_incomplete_evidence() {
    let (worker, mut peer, _root, context, page) = owned_page().await;
    let (worker, developer) = frontend(worker, &mut peer, page, false).await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    worker.trace_gap();
    let stopping = tokio::spawn(async move {
        let result = worker
            .trace_stop_with_analysis(Uuid::new_v4(), None, Some(developer), deadline())
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let (_worker, result) = stopping.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "TRACE_INCOMPLETE");
    assert_eq!(error.partial.unwrap()["report"]["complete"], false);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_trace_analysis_export_refusal_retries_complete_evidence_before_parsing() {
    let (worker, mut peer, root, context, page) = owned_page().await;
    let (worker, developer) = frontend(worker, &mut peer, page, false).await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    let stopping = tokio::spawn(async move {
        let capture = worker.trace_stop(deadline()).await.unwrap();
        (worker, capture)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let (mut worker, capture) = stopping.await.unwrap();
    let blocked = root.path().join("Acme-existing.json");
    std::fs::write(&blocked, b"Acme existing").unwrap();
    let error = worker
        .trace_stop_with_analysis(
            Uuid::new_v4(),
            Some(blocked.to_str().unwrap().to_owned()),
            Some(developer),
            deadline(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "TRACE_EXPORT_EXISTS");
    assert_eq!(worker.trace_status()["report"], capture["report"]);
    assert!(worker.performance_status()["analysis"].is_null());
    let output = root.path().join("Acme-analysis.json");
    let selected = output.to_str().unwrap().to_owned();
    let retry = tokio::spawn(async move {
        let result = worker
            .trace_stop_with_analysis(Uuid::new_v4(), Some(selected), Some(developer), deadline())
            .await;
        (worker, result)
    });
    model(&mut peer, false).await;
    let (worker, result) = retry.await.unwrap();
    let recovered = result.unwrap();
    assert_eq!(recovered["report"], capture["report"]);
    assert_eq!(std::fs::read(&output).unwrap(), b"{\"traceEvents\":[]}");
    assert_eq!(std::fs::read(&blocked).unwrap(), b"Acme existing");
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
async fn native_trace_analysis_uses_ordinary_capacity_without_claiming_the_cleanup_lane() {
    let (worker, _peer, _root, _, page) = owned_page().await;
    let (session, mut ordinary, _executions, mut responses, _controls) =
        super::super::execution_concurrency::client(&worker);
    let developer = Reference {
        id: Uuid::new_v4(),
        ..page
    };
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::TraceStop {
                file_path: None,
                analysis_developer: Some(developer),
                analysis_page: None,
            },
            5000,
        )
        .unwrap();
    assert_eq!(ordinary.try_recv().unwrap().id, id);
    assert!(responses.try_recv().is_err());
    let batch = Request::Batch {
        requests: vec![Request::TraceStop {
            file_path: None,
            analysis_developer: Some(developer),
            analysis_page: None,
        }],
    };
    let id = session.submit(Uuid::new_v4(), batch.clone(), 5000).unwrap();
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
            .submit(Uuid::new_v4(), batch, 5000)
            .unwrap_err()
            .code,
        "OPERATION_CAPACITY"
    );
    assert_eq!(
        session
            .submit(
                Uuid::new_v4(),
                Request::TraceStop {
                    file_path: None,
                    analysis_developer: Some(developer),
                    analysis_page: None
                },
                5000
            )
            .unwrap_err()
            .code,
        "OPERATION_CAPACITY"
    );
    let raw = session
        .submit(
            Uuid::new_v4(),
            Request::TraceStop {
                file_path: None,
                analysis_developer: None,
                analysis_page: None,
            },
            5000,
        )
        .unwrap();
    assert_eq!(responses.try_recv().unwrap().id, raw);
    drop(capacity);
}

#[tokio::test]
async fn native_trace_analysis_rechecks_frontend_loss_before_allocating_a_model() {
    let (worker, mut peer, _root, context, page) = owned_page().await;
    let (worker, developer) = frontend(worker, &mut peer, page, false).await;
    let mut worker = recording(worker, &mut peer, context, page).await;
    let (session, _ordinary, _executions, responses, _controls) =
        super::super::execution_concurrency::client(&worker);
    worker.responses = Some(responses);
    let stopping = tokio::spawn(async move {
        let result = worker
            .trace_stop_with_analysis(Uuid::new_v4(), None, Some(developer), deadline())
            .await;
        (worker, result)
    });
    let end = command(&mut peer, "Tracing.end").await;
    send(
        &mut peer,
        json!({"method":"Target.targetDestroyed","params":{"targetId":"Acme-analysis-frontend"}}),
    )
    .await;
    let until = deadline();
    loop {
        let read = session
            .submit(Uuid::new_v4(), Request::DeveloperStatus { developer }, 5000)
            .unwrap();
        let observed = session.wait(read, until).await.unwrap();
        assert_eq!(observed.outcome, Outcome::Succeeded);
        if serde_json::to_value(observed).unwrap()["result"]["closed"] == true {
            break;
        }
        assert!(Instant::now() < until);
    }
    finish(&mut peer, &end).await;
    let (worker, result) = stopping.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "DEVELOPER_SESSION_LOST");
    assert_eq!(error.partial.unwrap()["report"]["complete"], true);
    assert!(worker.performance_status()["analysis"].is_null());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
}
