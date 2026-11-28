//! Ordinary and nonmatching RunTo collection delegates readiness to its SDK caller.
use super::*;
use collected_debugger::{PAGE, calls, count, debug, pause};

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-collected-callers-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .output()
    .await
    .unwrap();
    for (extension, bytes) in [("json", &output.stdout), ("stderr", &output.stderr)] {
        std::fs::write(
            fixture.workspace.path().join(format!(
                "collected-callers-reference-{engine}-{mode}.{extension}"
            )),
            bytes,
        )
        .unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ownedProcesses"], 0);
    assert_eq!(report["cases"].as_array().unwrap().len(), 2);
    for row in report["cases"].as_array().unwrap() {
        assert_eq!(row["waiting"]["status"], "running");
        assert_eq!(row["beforeReady"], 0);
        assert_eq!(row["afterReady"], 1);
        assert_eq!(row["terminal"], "success");
    }
    report
}
#[tokio::test]
async fn pinned_collected_callers_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["firefox", "webkit"] {
            reference(&fixture, engine, mode).await;
        }
    }
}
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    expected: &Value,
) {
    for (index, run_to) in [false, true].into_iter().enumerate() {
        let control = debugger_order::PageControl::start(Some(PAGE)).await;
        let context =
            operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
        let page = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":control.url,"waitUntil":"load"}})).await;
        debug(mcp, target, &context, json!({"kind":"enable"})).await;
        if run_to {
            debug(mcp, target, &context, json!({"kind":"pause"})).await;
            let first=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#first"},"action":{"kind":"click"}}})).await;
            let paused = pause(mcp, target, &context).await;
            let requested=fixture.cli(&["browser","send","--target",target,&json!({"operation":"context_debugger","context":context,"action":{"kind":"run_to","pauseId":paused,"location":{"file":"Acme-collected-caller.rs","line":30,"column":3}}}).to_string()]).await;
            completed(mcp, target, requested).await;
            completed(mcp, target, first).await;
        }
        let previous = calls(mcp, target, &context, false).await["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["callId"].clone())
            .collect::<Vec<_>>();
        let request = json!({"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#waiting"},"action":if run_to {json!({"kind":"at_source","source":{"file":"src/Acme-collected-caller.rs","line":20,"column":3},"action":{"kind":"click"}})}else{json!({"kind":"click"})}});
        let pending = if run_to {
            mcp.tool("act",json!({"targetId":target,"actions":[{"action":"page_action","page":page,"locator":{"kind":"css","selector":"#waiting"},"input":{"action":"at_source","source":{"file":"src/Acme-collected-caller.rs","line":20,"column":3},"input":{"action":"click"}}}]})).await
        } else {
            fixture
                .cli(&["browser", "send", "--target", target, &request.to_string()])
                .await
        };
        let evidence = fixture.workspace.path().join(format!(
            "collected-caller-installed-{engine}-{mode}-{index}.json"
        ));
        std::fs::write(
            &evidence,
            serde_json::to_vec_pretty(
                &json!({"request":request,"operationId":pending["operation"]["id"]}),
            )
            .unwrap(),
        )
        .unwrap();
        let until = tokio::time::Instant::now() + Duration::from_secs(5);
        let waiting = loop {
            let observed = calls(mcp, target, &context, true).await;
            if let Some(row) = observed["calls"].as_array().unwrap().iter().find(|row| {
                row["status"] == "running"
                    && !previous.contains(&row["callId"])
                    && row["details"]["title"]
                        .as_str()
                        .is_some_and(|title| title.to_ascii_lowercase().contains("click"))
            }) {
                break row.clone();
            }
            assert!(
                tokio::time::Instant::now() < until,
                "Collected caller unavailable: {observed}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        if run_to {
            assert_eq!(
                waiting["details"]["location"],
                json!({"file":"src/Acme-collected-caller.rs","line":20,"column":3})
            );
        }
        let before = debug(mcp, target, &context, json!({"kind":"status"})).await;
        assert_eq!(before["callCollectionEnabled"], true);
        assert_ne!(before["paused"], true);
        assert_eq!(before["pauseRequested"], run_to);
        assert!(before["pauseId"].is_null());
        let running = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,"operationId":pending["operation"]["id"]}),
            )
            .await;
        assert_eq!(running["operation"]["settled"], false);
        assert_eq!(
            count(mcp, target, &page).await,
            expected["cases"][index]["beforeReady"]
        );
        let metadata = calls(mcp, target, &context, false).await;
        assert!(
            metadata["calls"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row.get("details").is_none() && row.get("logs").is_none())
        );
        control.advance(2);
        let result = std::panic::AssertUnwindSafe(completed(mcp, target, pending))
            .catch_unwind()
            .await;
        if let Err(panic) = result {
            let observation = pause_evidence::readiness(mcp, target, &page).await;
            let button = pause_evidence::read(
                mcp,
                target,
                json!({
                    "operation":"page_read","page":page,
                    "locator":{"kind":"css","selector":"#waiting"},
                    "read":"state","maxBytes":4096
                }),
            )
            .await;
            let status = mcp.tool("browser_status", json!({"targetId":target})).await;
            std::fs::write(
                &evidence,
                serde_json::to_vec_pretty(&json!({
                    "request":request,"waiting":waiting,"before":before,
                    "running":running["operation"],"gateAdvancedTo":2,
                    "readinessObservation":observation,"button":button,
                    "failureStatus":status
                }))
                .unwrap(),
            )
            .unwrap();
            std::panic::resume_unwind(panic);
        }
        assert_eq!(
            count(mcp, target, &page).await,
            expected["cases"][index]["afterReady"]
        );
        let after = debug(mcp, target, &context, json!({"kind":"status"})).await;
        assert_ne!(after["paused"], true);
        assert_eq!(after["pauseRequested"], run_to);
        let until = tokio::time::Instant::now() + Duration::from_secs(5);
        let terminal = loop {
            let observed = calls(mcp, target, &context, false).await;
            if let Some(row) = observed["calls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["callId"] == waiting["callId"] && row["status"] == "success")
            {
                break row.clone();
            }
            assert!(
                tokio::time::Instant::now() < until,
                "Original caller did not settle: {observed}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        std::fs::write(evidence,serde_json::to_vec_pretty(&json!({"request":request,"waiting":waiting,"before":before,"running":running["operation"],"after":after,"terminal":terminal,"reference":expected["cases"][index]})).unwrap()).unwrap();
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
    }
}
#[tokio::test]
async fn installed_step_four_collected_callers_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["firefox", "webkit"] {
            let expected = reference(&fixture, engine, mode).await;
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, target, engine, mode, &expected,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let status = mcp.tool("browser_status", json!({"targetId":target})).await;
                std::fs::write(
                    fixture
                        .workspace
                        .path()
                        .join(format!("collected-caller-failure-{engine}-{mode}.json")),
                    serde_json::to_vec_pretty(&status).unwrap(),
                )
                .unwrap();
            }
            let closed = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                cleanup_failures.push(closed);
            }
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Collected ordinary/nonmatching RunTo callers {engine}/{mode}: strict readiness, stable identity and one unpaused effect"
            );
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        cleanup_failures.is_empty(),
        "Original cleanup failures: {cleanup_failures:?}"
    );
}
