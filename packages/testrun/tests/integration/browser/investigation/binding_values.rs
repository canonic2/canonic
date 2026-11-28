//! Pinned binding value serialization and installed, privately disclosed settlements.
use super::*;

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-binding-values.json"
    )))
    .unwrap()
}
async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-binding-values-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(fixture.cache.join("browsers"))
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .output()
    .await
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("binding-reference-{engine}-{mode}.json")),
        &output.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("binding-reference-{engine}-{mode}.stderr")),
        &output.stderr,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["calls"], 28);
    assert_eq!(report["cases"].as_array().unwrap().len(), 28);
    assert_eq!(report["ownedProcesses"], 0);
    report
}
#[tokio::test]
async fn binding_values_unchanged_sdk_reference() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            reference(&fixture, engine, mode).await;
            eprintln!("Pinned binding values {engine}/{mode}: 28 argument/settlement checks");
        }
    }
}
pub(super) async fn pending(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    binding: &Value,
    disclosed: &std::collections::BTreeSet<String>,
) -> Value {
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = mcp.detailed_browser_status(target).await;
        let mut metadata = status.clone();
        if let Some(operations) = metadata["browser"]["operations"].as_array_mut() {
            for operation in operations {
                if operation["id"]
                    .as_str()
                    .is_some_and(|id| disclosed.contains(id))
                {
                    assert_eq!(operation["result"]["private"], true);
                    operation["result"] = Value::Null;
                }
            }
        }
        if metadata.to_string().contains("Acme private") {
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join("binding-status-private-failure.json"),
                serde_json::to_vec_pretty(&status).unwrap(),
            )
            .unwrap();
        }
        assert!(
            !metadata.to_string().contains("Acme private"),
            "Binding arguments leaked outside explicitly requested private-read results"
        );
        let browser = &status["browser"];
        let rows = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        // This fixture owns one binding. SDK resource metadata omits bindingId;
        // the explicit private read below verifies the exact registration.
        if let Some(callback) = rows.iter().find(|row| {
            row["kind"] == "binding" && (row["bindingId"].is_null() || row["bindingId"] == *binding)
        }) {
            return callback["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Binding callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn exercise(fixture: &Fixture, mcp: &mut Mcp, target: &str, page: &Value) {
    let mut disclosed = std::collections::BTreeSet::new();
    let binding = operation(
        mcp,
        target,
        json!({"operation":"binding_add","scope":page,"name":"AcmeTypedBridge"}),
    )
    .await["bindingId"]
        .clone();
    for mode in ["resolve", "reject"] {
        for (index, item) in cases().iter().enumerate() {
            let expression = format!(
                "async()=>{{const check=({});try{{const value=await AcmeTypedBridge(({})());return JSON.stringify({{outcome:'resolved',matches:check(value)}});}}catch(value){{return JSON.stringify({{outcome:'rejected',matches:check(value)}});}}}}",
                item["check"].as_str().unwrap(),
                item["make"].as_str().unwrap()
            );
            let evaluation=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
            let callback = pending(fixture, mcp, target, &binding, &disclosed).await;
            let read = json!({"operation":"binding_read","callback":callback}).to_string();
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &read])
                .await;
            disclosed.insert(submitted["operation"]["id"].as_str().unwrap().to_owned());
            let read = completed(mcp, target, submitted).await;
            assert_eq!(read["bindingId"], binding);
            assert_eq!(read["private"], true);
            assert_eq!(
                read["arguments"],
                json!([item["typed"]]),
                "Argument {}",
                item["name"]
            );
            let response = json!({"kind":format!("binding_{mode}_value"),"value":item["typed"]});
            let request =
                json!({"operation":"callback_resolve","callback":callback,"response":response});
            let result = match index % 3 {
                0 => {
                    let text = request.to_string();
                    let submitted = fixture
                        .cli(&["browser", "send", "--target", target, &text])
                        .await;
                    completed(mcp, target, submitted).await
                }
                1 => operation(mcp, target, request.clone()).await,
                _ => {
                    let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"callback_resolve","callback":callback,"response":response}]})).await;
                    completed(mcp, target, submitted).await["results"][0].clone()
                }
            };
            assert_eq!(result["resolved"], true);
            let result = completed(mcp, target, evaluation).await;
            let result: Value =
                serde_json::from_str(result["value"]["value"].as_str().unwrap()).unwrap();
            assert_eq!(
                result,
                json!({"outcome":if mode=="resolve"{"resolved"}else{"rejected"},"matches":true}),
                "{mode}/{}",
                item["name"]
            );
            if index == 0 || index == 13 {
                collectors::refused(mcp, target, request, "STALE_REFERENCE", true).await;
            }
        }
    }
    operation(
        mcp,
        target,
        json!({"operation":"binding_remove","scope":page,"bindingId":binding}),
    )
    .await;
    assert!(
        operation(
            mcp,
            target,
            json!({"operation":"binding_list","scope":page})
        )
        .await["bindings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let removed=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"typeof AcmeTypedBridge","isFunction":false})).await;
    assert_eq!(removed["value"]["value"], "undefined");
}
#[tokio::test]
async fn installed_step_four_binding_values_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(exercise(
                &fixture,
                &mut mcp,
                target,
                &ready["browser"]["selectedPage"],
            ))
            .catch_unwind()
            .await;
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
                cleanup_failures.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Binding values {engine}/{mode}: 28 private argument reads and typed settlements"
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
