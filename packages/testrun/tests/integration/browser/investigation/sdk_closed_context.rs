//! Installed closed SDK contexts release their original automation caller.
use super::*;

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let report_path = fixture
        .workspace
        .path()
        .join(format!("closed-sdk-reference-{engine}-{mode}"));
    let observation = report_path.with_extension("jsonl");
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg("--require")
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-protocol-observations.cjs"
    ))
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-source-input-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(mode)
    .arg("owned")
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/browser/driver/bootstrap.js"
    ))
    .arg(engine)
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .env("TESTRUN_CHILD_OBSERVATIONS", &observation)
    .env("TESTRUN_NATIVE_PROTOCOL_ONLY", "1")
    .output()
    .await
    .unwrap();
    std::fs::write(report_path.with_extension("json"), &output.stdout).unwrap();
    std::fs::write(report_path.with_extension("stderr"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "Owned SDK reference failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ownedProcesses"], 0);
    assert_eq!(report["completedClicks"], 7);
    assert_eq!(report["closedContextResume"], "target-closed");
    assert_eq!(
        report["fixtureCleanup"],
        "owned-driver-context-close-release"
    );
    let records = std::fs::read_to_string(observation)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(
        records.len() < 2048,
        "A truncated transcript cannot prove original reply ownership"
    );
    assert!(!records.iter().any(|row| matches!(
        row["event"].as_str(),
        Some("observer-bounds" | "observer-parse-error")
    )));
    let commands = records
        .iter()
        .filter(|row| {
            row["lane"] == "driver-to-browser"
                && matches!(
                    row["message"]["method"].as_str(),
                    Some("Page.dispatchMouseEvent" | "Input.dispatchMouseEvent")
                )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        commands.len(),
        21,
        "Seven clicks must account for their move/down/up calls, with no native input after closure"
    );
    for command in commands {
        assert!(command["ownerPid"].as_u64().is_some());
        assert!(
            records
                .iter()
                .any(|reply| reply["lane"] == "browser-to-driver"
                    && reply["ownerPid"] == command["ownerPid"]
                    && reply["message"]["id"] == command["message"]["id"]),
            "Original native reply missing: {command}"
        );
    }
    report
}
#[tokio::test]
async fn owned_driver_closed_context_reference_checkpoint() {
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
    sibling: &Value,
    engine: &str,
    mode: &str,
) {
    for collected in [false, true] {
        let context =
            operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
        let page = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        let hidden = if collected { "" } else { "hidden disabled" };
        let html = format!(
            "<!doctype html><button id=Acme-private-step {hidden} onclick='document.querySelector(\"#count\").value=1'>Acme</button><input id=count value=0 readonly>"
        );
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":html,"waitUntil":"load"}})).await;
        if collected {
            operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"enable"}})).await;
        }
        operation(
            mcp,
            target,
            json!({"operation":"context_debugger","context":context,"action":{"kind":"pause"}}),
        )
        .await;
        let request = json!({"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#Acme-private-step"},"action":{"kind":"click"}});
        let pending = mcp
            .tool("browser_send", json!({"targetId":target,"request":request}))
            .await;
        installed_automation_pause(
            fixture,
            mcp,
            target,
            &context,
            None,
            ("closed-sdk-caller", &pending, &page),
        )
        .await;
        assert_eq!(collected_debugger::count(mcp, target, &page).await, 0);
        let before = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,"operationId":pending["operation"]["id"]}),
            )
            .await;
        assert_eq!(before["operation"]["settled"], false);
        let close = if collected {
            mcp.tool(
                "act",
                json!({"targetId":target,"actions":[{"action":"context_close","context":context}]}),
            )
            .await
        } else {
            fixture
                .cli(&[
                    "browser",
                    "send",
                    "--target",
                    target,
                    &json!({"operation":"context_close","context":context}).to_string(),
                ])
                .await
        };
        let receipt = completed(mcp, target, close).await;
        let closed = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,"operationId":pending["operation"]["id"],"waitMs":5000}),
            )
            .await;
        assert_eq!(closed["operation"]["settled"], true, "{closed}");
        assert_ne!(
            closed["operation"]["outcome"], "succeeded",
            "Closing a paused scope must never complete its click"
        );
        let repeated = operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
        assert_eq!(repeated["closed"], true);
        let alive = operation(
            mcp,
            target,
            json!({"operation":"evaluate","scope":sibling,"expression":"6*7","isFunction":false}),
        )
        .await;
        assert_eq!(alive["value"]["value"], 42);
        std::fs::write(fixture.workspace.path().join(format!("closed-sdk-installed-{engine}-{mode}-{collected}.json")),serde_json::to_vec_pretty(&json!({"collected":collected,"request":request,"before":before,"close":receipt,"original":closed,"repeated":repeated,"sibling":alive})).unwrap()).unwrap();
    }
}
#[tokio::test]
async fn installed_step_four_sdk_closed_context_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["firefox", "webkit"] {
            reference(&fixture, engine, mode).await;
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture,
                &mut mcp,
                target,
                &ready["browser"]["selectedPage"],
                engine,
                mode,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let status = mcp
                    .request(
                        "tools/call",
                        json!({"name":"browser_status","arguments":{"targetId":target}}),
                    )
                    .await;
                std::fs::write(
                    fixture
                        .workspace
                        .path()
                        .join(format!("closed-sdk-failure-{engine}-{mode}.json")),
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
                cleanup.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Installed closed SDK context {engine}/{mode}: collected/uncollected pauses, original settlement, repeated close and sibling ownership"
            );
        }
    }
    mcp.stop().await;
    assert!(
        cleanup.is_empty(),
        "Original disconnect failures: {cleanup:?}"
    );
}
