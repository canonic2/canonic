//! Stop-time analysis composes the existing native model without recollecting a trace.
#[tokio::test]
async fn pinned_native_trace_start_config_diagnostic() {
    let fixture = installed_fixture();
    let site = native_tracing::CacheSite::start().await;
    let output = Command::new(fixture.cache.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"))
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/browser-trace-start-config.cjs"))
        .arg(fixture.cache.join("playwright-core-1.64.1/package"))
        .arg(fixture.workspace.path()).arg(&site.url)
        .arg(fixture.cache.join("browsers/chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"))
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/src/browser/cdp/session/tracing.rs"))
        .current_dir(fixture.workspace.path()).env_clear().env("PATH", "/usr/bin:/bin")
        .env("PW_LANG_NAME", "javascript").env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
        .output().await.unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join("trace-start-config-observations.json"),
        &output.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture.workspace.path().join("trace-start-config.stderr"),
        &output.stderr,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ownedProcesses"], 0, "{report}");
    assert_eq!(report["observations"].as_array().unwrap().len(), 4);
    eprintln!("Tracing-start native diagnostic: {report}");
}
#[tokio::test]
async fn pinned_native_trace_producer_buffer_diagnostic() {
    let fixture = installed_fixture();
    let site = native_tracing::CacheSite::start().await;
    let output = Command::new(fixture.cache.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"))
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/browser-trace-producer-buffer.cjs"))
        .arg(fixture.cache.join("playwright-core-1.64.1/package"))
        .arg(fixture.workspace.path()).arg(&site.url)
        .arg(fixture.cache.join("browsers/chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"))
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/src/browser/cdp/session/tracing.rs"))
        .current_dir(fixture.workspace.path()).env_clear().env("PATH", "/usr/bin:/bin")
        .env("PW_LANG_NAME", "javascript").env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
        .output().await.unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join("producer-buffer-observations.json"),
        &output.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture.workspace.path().join("producer-buffer.stderr"),
        &output.stderr,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ownedProcesses"], 0, "{report}");
    assert_eq!(report["observations"].as_array().unwrap().len(), 3);
    eprintln!("Producer-buffer native diagnostic: {report}");
}
#[tokio::test]
async fn installed_step_four_trace_frontend_loss_diagnostic() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut observations = Vec::new();
    for frontend_count in 0..=2 {
        let connected = fixture
            .cli(&[
                "browser", "connect", "--engine", "chrome", "--mode", "headless",
            ])
            .await;
        let target = connected["targetId"].as_str().unwrap();
        let ready = fixture.ready_target(target).await;
        let context = &ready["browser"]["selectedContext"];
        let result = std::panic::AssertUnwindSafe(async {
            let site = native_tracing::CacheSite::start().await;
            let page = operation(&mut mcp, target, json!({"operation":"page_create","context":context,"select":true})).await["page"].clone();
            operation(&mut mcp, target, json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
            for index in 0..frontend_count {
                let inspected = if index == 0 { page.clone() } else {
                    operation(&mut mcp, target, json!({"operation":"page_create","context":context,"select":false})).await["page"].clone()
                };
                operation(&mut mcp, target, json!({"operation":"developer_open","page":inspected,"panel":"timeline"})).await;
            }
            operation(&mut mcp, target, json!({"operation":"trace_start","context":context,"options":{"page":page,"performance":{"reload":true,"autoStop":false}}})).await;
            let recording = operation(&mut mcp, target, json!({"operation":"trace_status"})).await;
            assert_eq!(recording["sourceLoss"], false);
            let submitted = mcp.tool("browser_send", json!({"targetId":target,"request":{"operation":"trace_stop"}})).await;
            let settled = mcp.tool("browser_status", json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000})).await;
            assert_eq!(settled["operation"]["settled"], true, "{settled}");
            let receipt = operation(&mut mcp, target, json!({"operation":"trace_status"})).await;
            assert_eq!(receipt["report"]["nativeCompletion"]["received"], true, "{receipt}");
            let native_loss = receipt["report"]["nativeCompletion"]["dataLossOccurred"].as_bool().unwrap();
            let reasons = receipt["report"]["sourceLossReasons"].as_array().unwrap();
            assert_eq!(reasons.contains(&json!("native-data-loss")), native_loss, "{receipt}");
            assert_eq!(receipt["report"]["sourceLoss"], !reasons.is_empty(), "{receipt}");
            let trace: Value = serde_json::from_slice(&std::fs::read(receipt["report"]["attachment"]["path"].as_str().unwrap()).unwrap()).unwrap();
            let observation = json!({"frontends":frontend_count,"operation":settled["operation"],"receipt":receipt,"revision":trace["metadata"]["revision"],"buffers":trace["metadata"]["trace_processor_stats"]["traced_buf"]});
            std::fs::write(fixture.workspace.path().join(format!("trace-frontend-loss-{frontend_count}.json")), serde_json::to_vec_pretty(&observation).unwrap()).unwrap();
            eprintln!("Trace frontend loss diagnostic {observation}");
            observations.push(observation);
        }).catch_unwind().await;
        let closed = performance_lifecycle::disconnect_observed(
            &fixture,
            &mut mcp,
            target,
            "chrome",
            &format!("headless-frontends-{frontend_count}"),
            context,
        )
        .await;
        assert_eq!(
            closed["result"]["structuredContent"]["ok"], true,
            "{closed}"
        );
        if let Err(panic) = result {
            mcp.stop().await;
            std::panic::resume_unwind(panic);
        }
    }
    mcp.stop().await;
    assert_eq!(observations.len(), 3);
    assert_eq!(
        observations[0]["operation"]["outcome"], "succeeded",
        "{observations:?}"
    );
    // The observed variants are evidence, never accepted analysis contracts.
}
use super::*;

pub(super) fn loss_free_buffers(receipt: &Value) -> Value {
    let trace: Value = serde_json::from_slice(
        &std::fs::read(receipt["report"]["attachment"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    let buffers = trace["metadata"]["trace_processor_stats"]["traced_buf"]
        .as_array()
        .expect("Native trace buffer statistics");
    assert!(!buffers.is_empty());
    for buffer in buffers {
        for field in [
            "trace_writer_packet_loss",
            "sequence_packet_loss",
            "bytes_overwritten",
            "chunks_discarded",
        ] {
            assert_eq!(buffer[field], 0, "Native trace lost data: {buffer}");
        }
    }
    json!(buffers)
}

/// Owner restriction 2026-11-16: when Chrome reports native trace data loss,
/// analysis refuses with `TRACE_INCOMPLETE` and keeps the incomplete report as
/// evidence. Returns false for any other outcome, which the caller then checks.
fn native_loss_refusal(status: &Value) -> bool {
    let operation = &status["operation"];
    assert_eq!(operation["settled"], true, "{status}");
    if operation["code"] != "TRACE_INCOMPLETE" {
        return false;
    }
    let report = &operation["result"]["report"];
    assert_eq!(operation["outcome"], "failed", "{status}");
    assert_eq!(report["complete"], false, "{status}");
    assert_eq!(
        report["nativeCompletion"]["dataLossOccurred"], true,
        "{status}"
    );
    assert!(
        report["sourceLossReasons"]
            .as_array()
            .unwrap()
            .contains(&json!("native-data-loss")),
        "{status}"
    );
    assert!(report["evidenceId"].is_string(), "{status}");
    assert!(operation["result"]["analysis"].is_null(), "{status}");
    true
}

async fn settle(mcp: &mut Mcp, target: &str, submitted: &Value) -> Value {
    mcp.tool(
        "browser_status",
        json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
    )
    .await
}

/// Releases what a restricted cell still owns: models, frontends, evidence and pages.
async fn restricted_cleanup(
    mcp: &mut Mcp,
    target: &str,
    models: &[(Value, Value)],
    developers: &[Value],
    evidence: &[Value],
    pages: &[Value],
) {
    for (developer, analysis) in models {
        operation(
            mcp,
            target,
            json!({"operation":"performance_release","developer":developer,"analysisId":analysis}),
        )
        .await;
    }
    for developer in developers {
        operation(
            mcp,
            target,
            json!({"operation":"developer_close","developer":developer}),
        )
        .await;
    }
    for evidence in evidence {
        operation(
            mcp,
            target,
            json!({"operation":"developer_evidence_remove","evidenceId":evidence}),
        )
        .await;
    }
    for page in pages {
        operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    }
}

async fn native(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    mode: &str,
    retained_preset: bool,
) {
    let site = native_tracing::CacheSite::start().await;
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
    let retained_capture = if retained_preset {
        operation(mcp,target,json!({"operation":"trace_start","context":context,"options":{"page":page,"performance":{"reload":true,"autoStop":false}}})).await;
        let captured = operation(mcp, target, json!({"operation":"trace_stop"})).await;
        assert_eq!(captured["report"]["complete"], true);
        Some(captured["report"].clone())
    } else {
        None
    };
    let developer = operation(
        mcp,
        target,
        json!({"operation":"developer_open","page":page,"panel":"timeline"}),
    )
    .await["developer"]
        .clone();
    let other = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    let other_developer = operation(
        mcp,
        target,
        json!({"operation":"developer_open","page":other,"panel":"timeline"}),
    )
    .await["developer"]
        .clone();
    if !retained_preset {
        operation(mcp,target,json!({"operation":"trace_start","context":context,"options":{"page":page,"performance":{"reload":true,"autoStop":false}}})).await;
    }
    collectors::refused(
        mcp,
        target,
        json!({"operation":"trace_stop","analysisDeveloper":other_developer}),
        "TRACE_ANALYSIS_PAGE_MISMATCH",
        true,
    )
    .await;
    let recording = operation(mcp, target, json!({"operation":"trace_status"})).await;
    if retained_preset {
        assert_eq!(recording["state"], "complete");
        assert_eq!(
            recording["report"],
            retained_capture.as_ref().unwrap().clone()
        );
    } else {
        assert_eq!(recording["state"], "recording");
        assert_eq!(recording["sourceLoss"], false);
    }
    // Keep unrelated producers alive through capture completion. Their teardown
    // belongs outside the interval whose bytes must be complete for analysis.
    let first_path = fixture
        .workspace
        .path()
        .join(format!("Acme-analysis-{mode}.json"));
    let began = std::time::Instant::now();
    let submitted = fixture
        .cli(&[
            "browser",
            "send",
            "--target",
            target,
            &json!({"operation":"trace_stop","analysisDeveloper":developer,"filePath":first_path})
                .to_string(),
        ])
        .await;
    // Only the active-preset capture (frontends open while recording) may hit
    // the restricted native loss; a retained preset must analyze.
    let status = settle(mcp, target, &submitted).await;
    if !retained_preset && native_loss_refusal(&status) {
        let evidence = status["operation"]["result"]["report"]["evidenceId"].clone();
        restricted_cleanup(
            mcp,
            target,
            &[],
            &[other_developer, developer],
            &[evidence],
            &[other, page],
        )
        .await;
        eprintln!("Trace analysis {mode}: native loss refused under the owner restriction");
        return;
    }
    let first = completed(mcp, target, submitted).await;
    if let Some(report) = retained_capture {
        assert_eq!(first["report"], report);
    }
    let fresh_ms = began.elapsed().as_secs_f64() * 1000.0;
    trace_export::exported(&first, &first_path);
    let first_buffers = loss_free_buffers(&first);
    operation(
        mcp,
        target,
        json!({"operation":"developer_close","developer":other_developer}),
    )
    .await;
    operation(mcp, target, json!({"operation":"page_close","page":other})).await;
    assert_eq!(first["analysis"]["developer"], developer);
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
    let query = operation(mcp,target,json!({"operation":"performance_query","developer":developer,"analysisId":first["analysis"]["analysisId"],"query":{"kind":"summary"}})).await;
    assert_eq!(query["result"], first["analysis"]["summary"]);

    // Hold the first model while ending a second capture. Capacity refusal must
    // preserve both that existing model and the second complete evidence owner.
    let options = if retained_preset {
        json!({"page":page,"reload":{"ignoreCache":false},"cpuSampling":true})
    } else {
        json!({"page":page,"performance":{"reload":true,"autoStop":false}})
    };
    operation(
        mcp,
        target,
        json!({"operation":"trace_start","context":context,"options":options}),
    )
    .await;
    let submitted = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":{"operation":"trace_stop","analysisDeveloper":developer}}),
        )
        .await;
    let status = settle(mcp, target, &submitted).await;
    if !retained_preset && native_loss_refusal(&status) {
        let evidence = status["operation"]["result"]["report"]["evidenceId"].clone();
        let model = (developer.clone(), first["analysis"]["analysisId"].clone());
        restricted_cleanup(
            mcp,
            target,
            &[model],
            &[developer],
            &[first["report"]["evidenceId"].clone(), evidence],
            &[page],
        )
        .await;
        eprintln!("Trace analysis {mode}: second capture refused under the owner restriction");
        return;
    }
    assert_eq!(status["operation"]["outcome"], "failed", "{status}");
    assert_eq!(
        status["operation"]["code"], "PERFORMANCE_MODEL_CAPACITY",
        "{status}"
    );
    let pending = operation(mcp, target, json!({"operation":"trace_status"})).await;
    assert_eq!(pending["active"], false);
    assert_eq!(pending["report"]["complete"], true);
    assert_eq!(
        pending["analysisPending"]["code"],
        "PERFORMANCE_MODEL_CAPACITY"
    );
    assert_eq!(
        pending["analysisPending"]["model"]["analysisId"],
        first["analysis"]["analysisId"]
    );
    assert_ne!(
        pending["report"]["evidenceId"],
        first["report"]["evidenceId"]
    );
    let held = operation(mcp,target,json!({"operation":"performance_query","developer":developer,"analysisId":first["analysis"]["analysisId"],"query":{"kind":"summary"}})).await;
    assert_eq!(held["result"], first["analysis"]["summary"]);
    operation(mcp,target,json!({"operation":"performance_release","developer":developer,"analysisId":first["analysis"]["analysisId"]})).await;
    let retained = std::time::Instant::now();
    let second_path = fixture
        .workspace
        .path()
        .join(format!("Acme-analysis-retry-{mode}.json.gz"));
    let submitted = mcp.tool("act",json!({"targetId":target,"actions":[{"action":"trace_stop","analysisDeveloper":developer,"filePath":second_path}]})).await;
    let recovered = completed(mcp, target, submitted).await["results"][0].clone();
    let retained_ms = retained.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(recovered["report"], pending["report"]);
    assert!(recovered["analysisPending"].is_null());
    trace_export::exported(&recovered, &second_path);
    let second_buffers = loss_free_buffers(&recovered);
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("trace-analysis-{mode}-buffer-stats.json")),
        serde_json::to_vec_pretty(&json!({"first":first_buffers,"second":second_buffers})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        operation(mcp, target, json!({"operation":"developer_evidence_list"})).await["evidence"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    operation(mcp,target,json!({"operation":"performance_release","developer":developer,"analysisId":recovered["analysis"]["analysisId"]})).await;

    // Independently invoke the existing manual load on these exact immutable
    // bytes, then compare complete bounded native summaries.
    let manual = operation(mcp,target,json!({"operation":"performance_load","developer":developer,"evidenceId":recovered["report"]["evidenceId"]})).await;
    assert_ne!(manual["analysisId"], recovered["analysis"]["analysisId"]);
    assert_eq!(manual["summary"], recovered["analysis"]["summary"]);
    operation(mcp,target,json!({"operation":"performance_release","developer":developer,"analysisId":manual["analysisId"]})).await;
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"trace_stop","analysisDeveloper":developer})
        )
        .await,
        recovered
    );
    assert!(
        mcp.detailed_browser_status(target).await["browser"]["performance"]["analysis"].is_null()
    );
    operation(
        mcp,
        target,
        json!({"operation":"developer_close","developer":developer}),
    )
    .await;
    // A successful receipt is replayable after the owned frontend is closed.
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"trace_stop","analysisDeveloper":developer})
        )
        .await,
        recovered
    );
    for receipt in [&first, &recovered] {
        operation(mcp,target,json!({"operation":"developer_evidence_remove","evidenceId":receipt["report"]["evidenceId"]})).await;
    }
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    eprintln!(
        "Trace analysis {mode}: freshCliStopExportAnalysisMs={fresh_ms}; retainedExportAnalysisRetryMs={retained_ms}"
    );
}

#[tokio::test]
async fn installed_step_four_trace_analysis_checkpoint() {
    checkpoint(false).await;
}

#[tokio::test]
async fn installed_step_four_retained_trace_analysis_checkpoint() {
    checkpoint(true).await;
}

async fn checkpoint(retained_preset: bool) {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
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
                    native(&fixture, &mut mcp, target, context, mode, retained_preset).await;
                } else {
                    collectors::refused(
                        &mut mcp,
                        target,
                        json!({"operation":"trace_stop","analysisDeveloper":context}),
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
                "Trace analysis {engine} {mode} passed; workspace {}",
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
