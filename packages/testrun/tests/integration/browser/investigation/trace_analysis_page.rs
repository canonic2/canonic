//! Exact-page automatic analysis begins after complete capture, never inside its source interval.
use super::*;

async fn native(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value, mode: &str) {
    let site = native_tracing::CacheSite::start().await;
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    let other = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
    operation(mcp,target,json!({"operation":"trace_start","context":context,"options":{"page":page,"performance":{"reload":true,"autoStop":false}}})).await;
    collectors::refused(
        mcp,
        target,
        json!({"operation":"trace_stop","analysisPage":other}),
        "TRACE_ANALYSIS_PAGE_MISMATCH",
        true,
    )
    .await;
    collectors::refused(
        mcp,
        target,
        json!({"operation":"trace_stop","analysisPage":page,"analysisDeveloper":other}),
        "TRACE_ANALYSIS_OPTIONS_CONFLICT",
        false,
    )
    .await;
    let occupied = fixture
        .workspace
        .path()
        .join(format!("Acme-post-capture-occupied-{mode}.json"));
    std::fs::write(&occupied, b"Acme existing").unwrap();
    collectors::refused(
        mcp,
        target,
        json!({"operation":"trace_stop","analysisPage":page,"filePath":occupied}),
        "TRACE_EXPORT_EXISTS",
        true,
    )
    .await;
    assert_eq!(
        operation(mcp, target, json!({"operation":"trace_status"})).await["state"],
        "recording"
    );
    let path = fixture
        .workspace
        .path()
        .join(format!("Acme-post-capture-{mode}.json"));
    let started = std::time::Instant::now();
    let submitted = fixture
        .cli(&[
            "browser",
            "send",
            "--target",
            target,
            &json!({"operation":"trace_stop","analysisPage":page,"filePath":path}).to_string(),
        ])
        .await;
    let first = completed(mcp, target, submitted).await;
    let fresh_ms = started.elapsed().as_secs_f64() * 1000.0;
    trace_export::exported(&first, &path);
    let first_buffers = trace_analysis::loss_free_buffers(&first);
    let developer = first["analysis"]["developer"].clone();
    assert_eq!(first["analysisFrontend"]["developer"], developer);
    assert_eq!(first["analysisFrontend"]["page"], page);
    assert_eq!(first["analysisFrontend"]["createdByStop"], true);
    assert_eq!(
        first["analysis"]["evidenceId"],
        first["report"]["evidenceId"]
    );
    assert!(
        first["analysis"]["summary"]["data"]["insightSets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|set| set["url"] == format!("{}/", site.url) && set["navigationObserved"] == true),
        "{first}"
    );
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"developer_status","developer":developer})
        )
        .await["frontendOwned"],
        true
    );

    // Existing owned frontends are reused for raw captures; the first model
    // remains owned until explicit release following the capacity refusal.
    operation(mcp,target,json!({"operation":"trace_start","context":context,"options":{"page":page,"reload":{},"cpuSampling":true}})).await;
    collectors::refused(
        mcp,
        target,
        json!({"operation":"trace_stop","analysisPage":page}),
        "PERFORMANCE_MODEL_CAPACITY",
        true,
    )
    .await;
    let pending = operation(mcp, target, json!({"operation":"trace_status"})).await;
    assert_eq!(pending["report"]["complete"], true);
    assert_eq!(
        pending["analysisPending"]["model"]["analysisId"],
        first["analysis"]["analysisId"]
    );
    assert_eq!(pending["analysisFrontend"]["createdByStop"], false);
    assert_eq!(pending["analysisFrontend"]["developer"], developer);
    let second_buffers = trace_analysis::loss_free_buffers(&pending);
    operation(mcp,target,json!({"operation":"performance_release","developer":developer,"analysisId":first["analysis"]["analysisId"]})).await;
    let second_path = fixture
        .workspace
        .path()
        .join(format!("Acme-post-capture-retry-{mode}.json.gz"));
    let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"trace_stop","analysisPage":page,"filePath":second_path}]})).await;
    let second = completed(mcp, target, submitted).await["results"][0].clone();
    assert_eq!(second["report"], pending["report"]);
    assert!(second["analysisPending"].is_null());
    trace_export::exported(&second, &second_path);
    operation(mcp,target,json!({"operation":"performance_release","developer":developer,"analysisId":second["analysis"]["analysisId"]})).await;
    operation(
        mcp,
        target,
        json!({"operation":"developer_close","developer":developer}),
    )
    .await;
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"trace_stop","analysisPage":page})
        )
        .await,
        second
    );

    // Timed finalization is raw. Later automatic allocation has a real native
    // capacity refusal, retains complete bytes, then succeeds without recollection.
    operation(mcp,target,json!({"operation":"trace_start","context":context,"options":{"page":page,"performance":{}}})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(15);
    let timed = loop {
        let status = operation(mcp, target, json!({"operation":"trace_status"})).await;
        if status["active"] == false {
            assert_eq!(status["state"], "complete", "{status}");
            break status;
        }
        assert!(tokio::time::Instant::now() < until, "{status}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(timed["analysis"].is_null());
    assert_eq!(timed["report"]["stopReason"], "auto-stop");
    let third_buffers = trace_analysis::loss_free_buffers(&timed);
    let mut frontends = Vec::new();
    for _ in 0..4 {
        let extra = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        let opened = operation(
            mcp,
            target,
            json!({"operation":"developer_open","page":extra,"panel":"timeline"}),
        )
        .await;
        assert_eq!(opened["frontendOwned"], true);
        frontends.push((extra, opened["developer"].clone()));
    }
    collectors::refused(
        mcp,
        target,
        json!({"operation":"trace_stop","analysisPage":page}),
        "DEVELOPER_SESSION_CAPACITY",
        true,
    )
    .await;
    let allocation = operation(mcp, target, json!({"operation":"trace_status"})).await;
    assert_eq!(allocation["report"], timed["report"]);
    assert_eq!(
        allocation["analysisPending"]["frontendCreationUncertain"],
        false
    );
    assert_eq!(
        allocation["analysisPending"]["frontends"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let (extra, opened) = frontends.pop().unwrap();
    operation(
        mcp,
        target,
        json!({"operation":"developer_close","developer":opened}),
    )
    .await;
    operation(mcp, target, json!({"operation":"page_close","page":extra})).await;
    let third = operation(
        mcp,
        target,
        json!({"operation":"trace_stop","analysisPage":page}),
    )
    .await;
    assert_eq!(third["report"], timed["report"]);
    assert_eq!(third["analysisFrontend"]["createdByStop"], true);
    assert!(third["analysisPending"].is_null());
    let final_developer = &third["analysis"]["developer"];
    operation(mcp,target,json!({"operation":"performance_release","developer":final_developer,"analysisId":third["analysis"]["analysisId"]})).await;
    let manual=operation(mcp,target,json!({"operation":"performance_load","developer":final_developer,"evidenceId":third["report"]["evidenceId"]})).await;
    assert_eq!(manual["summary"], third["analysis"]["summary"]);
    assert_ne!(manual["analysisId"], third["analysis"]["analysisId"]);
    operation(mcp,target,json!({"operation":"performance_release","developer":final_developer,"analysisId":manual["analysisId"]})).await;
    operation(
        mcp,
        target,
        json!({"operation":"developer_close","developer":final_developer}),
    )
    .await;
    for (extra, opened) in frontends {
        operation(
            mcp,
            target,
            json!({"operation":"developer_close","developer":opened}),
        )
        .await;
        operation(mcp, target, json!({"operation":"page_close","page":extra})).await;
    }
    assert_eq!(
        operation(mcp, target, json!({"operation":"developer_evidence_list"})).await["evidence"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("post-capture-analysis-{mode}-buffers.json")),
        serde_json::to_vec_pretty(&json!([first_buffers, second_buffers, third_buffers])).unwrap(),
    )
    .unwrap();
    operation(mcp, target, json!({"operation":"page_close","page":other})).await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"trace_stop","analysisPage":page})
        )
        .await,
        third
    );
    assert_eq!(std::fs::read(&occupied).unwrap(), b"Acme existing");
    for capture in [&first, &second, &third] {
        assert_eq!(
            capture["report"]["nativeCompletion"],
            json!({"received":true,"dataLossOccurred":false})
        );
        assert_eq!(capture["report"]["sourceLossReasons"], json!([]));
        operation(mcp,target,json!({"operation":"developer_evidence_remove","evidenceId":capture["report"]["evidenceId"]})).await;
    }
    eprintln!("Post-capture analysis {mode}: freshCliCaptureExportFrontendAnalysisMs={fresh_ms}");
}

#[tokio::test]
async fn installed_step_four_post_capture_analysis_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let context = &ready["browser"]["selectedContext"];
            let result = std::panic::AssertUnwindSafe(async {
                if engine == "chrome" {
                    native(&fixture, &mut mcp, target, context, mode).await;
                } else {
                    collectors::refused(
                        &mut mcp,
                        target,
                        json!({"operation":"trace_stop","analysisPage":context}),
                        "TRACE_NATIVE_OPTIONS_UNSUPPORTED",
                        false,
                    )
                    .await;
                }
            })
            .catch_unwind()
            .await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, context,
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = result {
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Post-capture analysis {engine} {mode} passed; workspace {}",
                fixture.workspace.path().display()
            );
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
