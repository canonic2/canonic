//! Combined 5.3 installed CLI/MCP checkpoint; requires explicit live acceptance.
use super::*;
mod boundaries;

fn steps() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../fixtures/browser-clock-scenario.json"
    ))
    .unwrap()
}
async fn sdk(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = tokio::process::Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-clock-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .args([engine, mode])
    .arg(fixture.cache.join("browsers"))
    .arg(json!(steps()).to_string())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .stdin(std::process::Stdio::null())
    .kill_on_drop(true)
    .output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(120), output)
        .await
        .expect("Clock SDK baseline exceeded 120 s")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
async fn read(mcp: &mut Mcp, target: &str, page: &Value, expression: &Value) -> Value {
    let value = operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}),
    )
    .await;
    serde_json::from_str(value["value"]["value"].as_str().unwrap()).unwrap()
}
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    ready: &Value,
    expected: &Value,
) {
    let context = &ready["browser"]["selectedContext"];
    let page = &ready["browser"]["selectedPage"];
    if engine == "chrome" {
        operation(mcp,target,json!({"operation":"context_init_script_add","context":context,"source":"globalThis.acmeInitTime=Date.now()"})).await;
    }
    let mut report = Vec::new();
    for (index, step) in steps().into_iter().enumerate() {
        if let Some(action) = step.get("action") {
            let request = json!({"operation":"context_clock","context":context,"action":action});
            let result = if index == 0 {
                let submitted = fixture
                    .cli(&["browser", "send", "--target", target, &request.to_string()])
                    .await;
                completed(mcp, target, submitted).await
            } else {
                operation(mcp, target, request).await
            };
            assert_eq!(result["hostDeadlinesUnchanged"], true);
            if engine == "chrome" {
                assert_eq!(result["playwrightClockEquivalent"], false);
            }
        } else if let Some(expression) = step.get("future") {
            let future = operation(
                mcp,
                target,
                json!({"operation":"page_create","context":context}),
            )
            .await;
            report.push(read(mcp, target, &future["page"], expression).await);
            if engine == "chrome" {
                assert_eq!(
                    read(
                        mcp,
                        target,
                        &future["page"],
                        &json!("()=>JSON.stringify(globalThis.acmeInitTime)")
                    )
                    .await,
                    json!(1700000010000_i64)
                );
            }
            operation(
                mcp,
                target,
                json!({"operation":"page_close","page":future["page"]}),
            )
            .await;
        } else {
            report.push(read(mcp, target, page, &step["evaluate"]).await);
        }
    }
    assert_eq!(report[1]["frames"], json!({"count":1,"aligned":true}));
    assert_eq!(
        json!(report),
        *expected,
        "{engine}: numeric Clock effects against pinned SDK"
    );
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("clock-reference-{engine}-{mode}.json")),
        serde_json::to_vec_pretty(&json!({"expected":expected,"observed":report})).unwrap(),
    )
    .unwrap();
    if engine == "chrome" {
        operation(mcp,target,json!({"operation":"context_clock","context":context,"action":{"kind":"set_fixed_time","timeMs":1700000000000_i64}})).await;
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":"data:text/html,<p>Acme clock</p>","waitUntil":"load"}})).await;
        assert_eq!(
            read(
                mcp,
                target,
                page,
                &json!("()=>JSON.stringify(globalThis.acmeInitTime)")
            )
            .await,
            json!(1700000000000_i64)
        );
    }
    boundaries::exercise(fixture, mcp, target, engine, mode).await;
}

#[tokio::test]
async fn installed_step_five_clock_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let expected = sdk(&fixture, engine, mode).await;
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap().to_owned();
            let ready = fixture.ready_target(&target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, &target, engine, mode, &ready, &expected,
            ))
            .catch_unwind()
            .await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                &target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            assert_eq!(
                closed["result"]["structuredContent"]["ok"], true,
                "{closed}"
            );
            if let Err(panic) = result {
                let message = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                    .unwrap_or_else(|| "non-text assertion".into());
                failures.push(format!("{engine}/{mode}: {message}"));
            } else {
                eprintln!(
                    "Clock acceptance {engine}/{mode} passed; workspace {}",
                    fixture.workspace.path().display()
                );
            }
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Clock acceptance failures: {failures:?}"
    );
}
