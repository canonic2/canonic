//! Distinguish native completion loss from Testrun's own incomplete-stream observations.
use super::*;

#[tokio::test]
async fn native_trace_provenance_foreign_completion_does_not_change_capture() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let starting = tokio::spawn(async move {
        worker
            .trace_start(Uuid::new_v4(), context, Options::default(), deadline())
            .await
            .unwrap();
        worker
    });
    let start = command(&mut peer, "Tracing.start").await;
    reply(&mut peer, &start, json!({})).await;
    let mut worker = starting.await.unwrap();
    worker.trace_event(&Event {
        method: "Tracing.tracingComplete".into(),
        session_id: Some("Acme-unrelated-session".into()),
        params: json!({"dataLossOccurred":true,"stream":"Acme-foreign-private-handle"}),
    });
    assert_eq!(worker.trace_status()["sourceLoss"], false);
    assert_eq!(worker.trace_status()["sourceLossReasons"], json!([]));
    assert_eq!(
        worker.trace_status()["nativeCompletion"],
        json!({"received":false,"dataLossOccurred":null})
    );
    worker.seal_trace_after_exit().unwrap();
    let report = worker.trace_status()["report"].clone();
    assert_eq!(report["sourceLossReasons"], json!(["owned-browser-exited"]));
    assert_eq!(
        report["nativeCompletion"],
        json!({"received":false,"dataLossOccurred":null})
    );
    assert!(!report.to_string().contains("Acme-foreign-private-handle"));
}

#[tokio::test]
async fn native_trace_provenance_completion_flag_preserves_false_unknown_and_true() {
    for flag in [None, Some(false), Some(true)] {
        let (mut worker, mut peer, _root, context, _page) = owned_page().await;
        let starting = tokio::spawn(async move {
            worker
                .trace_start(Uuid::new_v4(), context, Options::default(), deadline())
                .await
                .unwrap();
            worker
        });
        let start = command(&mut peer, "Tracing.start").await;
        assert_trace_config(&start);
        reply(&mut peer, &start, json!({})).await;
        let mut worker = starting.await.unwrap();
        assert_eq!(
            worker.trace_status()["nativeCompletion"],
            json!({"received":false,"dataLossOccurred":null})
        );
        let stopping = tokio::spawn(async move {
            let outcome = worker.trace_stop(deadline()).await;
            (worker, outcome)
        });
        let end = command(&mut peer, "Tracing.end").await;
        reply(&mut peer, &end, json!({})).await;
        let mut params = json!({"stream":"Acme-provenance"});
        if let Some(value) = flag {
            params["dataLossOccurred"] = json!(value);
        }
        send(&mut peer, json!({"method":"Tracing.tracingComplete","sessionId":end["sessionId"],"params":params})).await;
        if flag.is_none() {
            // The pinned CDP schema requires the loss flag. A malformed event
            // fences the transport; it must not manufacture a false flag.
            let (mut worker, outcome) = stopping.await.unwrap();
            assert!(outcome.is_err());
            assert_eq!(
                worker.trace_status()["nativeCompletion"],
                json!({"received":false,"dataLossOccurred":null})
            );
            worker.seal_trace_after_exit().unwrap();
            assert_eq!(
                worker.trace_status()["report"]["nativeCompletion"],
                json!({"received":false,"dataLossOccurred":null})
            );
            assert_eq!(worker.trace_status()["report"]["complete"], false);
            continue;
        }
        let read = command(&mut peer, "IO.read").await;
        reply(
            &mut peer,
            &read,
            json!({"data":"{\"traceEvents\":[]}","eof":true}),
        )
        .await;
        let close = command(&mut peer, "IO.close").await;
        reply(&mut peer, &close, json!({})).await;
        let (mut worker, outcome) = stopping.await.unwrap();
        let receipt = if flag == Some(true) {
            let error = outcome.unwrap_err();
            assert_eq!(error.code, "TRACE_INCOMPLETE");
            error.partial.unwrap()
        } else {
            outcome.unwrap()
        };
        assert_eq!(
            receipt["report"]["nativeCompletion"],
            json!({"received":true,"dataLossOccurred":flag})
        );
        assert_eq!(
            receipt["report"]["sourceLossReasons"],
            if flag == Some(true) {
                json!(["native-data-loss"])
            } else {
                json!([])
            }
        );
        assert_eq!(receipt["report"]["complete"], flag != Some(true));
        let replay = worker.trace_stop(deadline()).await;
        assert_eq!(
            if flag == Some(true) {
                replay.unwrap_err().partial.unwrap()
            } else {
                replay.unwrap()
            },
            receipt
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.next())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_trace_provenance_gap_does_not_invent_native_data_loss() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let starting = tokio::spawn(async move {
        worker
            .trace_start(Uuid::new_v4(), context, Options::default(), deadline())
            .await
            .unwrap();
        worker
    });
    let start = command(&mut peer, "Tracing.start").await;
    reply(&mut peer, &start, json!({})).await;
    let mut worker = starting.await.unwrap();
    worker.trace_gap();
    worker.trace_gap();
    assert_eq!(
        worker.trace_status()["sourceLossReasons"],
        json!(["event-gap"])
    );
    assert_eq!(
        worker.trace_status()["nativeCompletion"],
        json!({"received":false,"dataLossOccurred":null})
    );
    let stopping = tokio::spawn(async move {
        let error = worker.trace_stop(deadline()).await.unwrap_err();
        (worker, error)
    });
    let end = command(&mut peer, "Tracing.end").await;
    finish(&mut peer, &end).await;
    let (mut worker, error) = stopping.await.unwrap();
    let report = &error.partial.as_ref().unwrap()["report"];
    assert_eq!(error.code, "TRACE_INCOMPLETE");
    assert_eq!(
        report["nativeCompletion"],
        json!({"received":true,"dataLossOccurred":false})
    );
    assert_eq!(report["sourceLossReasons"], json!(["event-gap"]));
    assert_eq!(
        worker.trace_stop(deadline()).await.unwrap_err().partial,
        error.partial
    );
}

#[tokio::test]
async fn native_trace_provenance_failed_read_and_exit_preserve_native_completion() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let starting = tokio::spawn(async move {
        worker
            .trace_start(Uuid::new_v4(), context, Options::default(), deadline())
            .await
            .unwrap();
        worker
    });
    let start = command(&mut peer, "Tracing.start").await;
    reply(&mut peer, &start, json!({})).await;
    let mut worker = starting.await.unwrap();
    let stopping = tokio::spawn(async move {
        let error = worker.trace_stop(deadline()).await.unwrap_err();
        (worker, error)
    });
    let end = command(&mut peer, "Tracing.end").await;
    reply(&mut peer, &end, json!({})).await;
    send(&mut peer,json!({"method":"Tracing.tracingComplete","sessionId":end["sessionId"],"params":{"dataLossOccurred":false,"stream":"Acme-unread"}})).await;
    let read = command(&mut peer, "IO.read").await;
    send(&mut peer,json!({"id":read["id"],"sessionId":read["sessionId"],"error":{"code":-32000,"message":"Acme stream refused"}})).await;
    let (mut worker, _error) = stopping.await.unwrap();
    assert_eq!(
        worker.trace_status()["sourceLossReasons"],
        json!(["stream-read-failed"])
    );
    assert_eq!(
        worker.trace_status()["nativeCompletion"],
        json!({"received":true,"dataLossOccurred":false})
    );
    worker.seal_trace_after_exit().unwrap();
    let report = worker.trace_status()["report"].clone();
    assert_eq!(report["complete"], false);
    assert_eq!(
        report["nativeCompletion"],
        json!({"received":true,"dataLossOccurred":false})
    );
    assert_eq!(
        report["sourceLossReasons"],
        json!(["owned-browser-exited", "stream-read-failed"])
    );
    assert_eq!(
        worker
            .trace_stop(deadline())
            .await
            .unwrap_err()
            .partial
            .unwrap()["report"],
        report
    );
}

fn assert_trace_config(start: &Value) {
    assert_eq!(
        start["params"]["traceConfig"]["excludedCategories"],
        json!(["*"])
    );
    assert!(
        !start["params"]["traceConfig"]["includedCategories"]
            .as_array()
            .unwrap()
            .contains(&json!("-*"))
    );
    assert_eq!(start["params"]["traceConfig"]["traceBufferSizeInKb"], 16384);
    assert_eq!(
        start["params"]["traceConfig"]["recordMode"],
        "recordUntilFull"
    );
}
