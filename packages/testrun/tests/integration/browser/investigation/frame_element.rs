//! Installed embedding-element retrieval and parent-document lifetime.
use super::*;

async fn value(mcp: &mut Mcp, target: &str, scope: &Value, expression: &str) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":scope,"expression":expression,"isFunction":true}),
    )
    .await["value"]["value"]
        .clone()
}
async fn exercise(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value) -> Value {
    let workflow_started = std::time::Instant::now();
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":"<!doctype html><iframe id=child srcdoc='<body>Acme child</body>'></iframe>","waitUntil":"load"}})).await;
    let child = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    let request = json!({"operation":"frame_element","frame":child}).to_string();
    let cli_started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let first = completed(mcp, target, submitted).await;
    let fresh_cli_ms = cli_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(first["page"], page);
    assert_eq!(first["childFrame"], child);
    assert_eq!(first["potentiallyMutating"], false);
    let handle = first["handle"].clone();
    assert_eq!(value(mcp, target, &handle, "node=>node.id").await, "child");
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"handle_content_frame","handle":handle})
        )
        .await["frame"],
        child
    );
    collectors::refused(
        mcp,
        target,
        json!({"operation":"frame_element","frame":first["frame"]}),
        "INVALID_SCOPE",
        true,
    )
    .await;
    assert_eq!(value(mcp,target,&handle,"node=>{const doc=node.contentDocument;doc.open();doc.write('<body>Acme replacement</body>');doc.close();return true}").await,true);
    assert_eq!(value(mcp, target, &handle, "node=>node.id").await, "child");
    let fresh = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    let retained_started = std::time::Instant::now();
    let submitted = mcp
        .tool(
            "act",
            json!({"targetId":target,"actions":[{"action":"frame_element","frame":fresh}]}),
        )
        .await;
    let second = completed(mcp, target, submitted).await["results"][0].clone();
    let retained_ms = retained_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(second["frame"], first["frame"]);
    assert_eq!(
        value(mcp, target, &second["handle"], "node=>node.id").await,
        "child"
    );
    value(mcp, target, &handle, "node=>{node.remove();return true}").await;
    assert_eq!(
        value(mcp, target, &second["handle"], "node=>node.isConnected").await,
        false
    );
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"handle_owner_frame","handle":second["handle"]})
        )
        .await["frame"],
        first["frame"]
    );
    collectors::refused(
        mcp,
        target,
        json!({"operation":"frame_element","frame":fresh}),
        "STALE_REFERENCE",
        true,
    )
    .await;
    for handle in [&handle, &second["handle"]] {
        operation(
            mcp,
            target,
            json!({"operation":"handle_dispose","handle":handle}),
        )
        .await;
    }
    let child2=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{const f=document.createElement('iframe');f.id='next';document.body.append(f);return true}","isFunction":true})).await;
    assert_eq!(child2["value"]["value"], true);
    let next = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#next"}}),
    )
    .await["frame"]
        .clone();
    let retained = operation(
        mcp,
        target,
        json!({"operation":"frame_element","frame":next}),
    )
    .await["handle"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":"data:text/html,<body>Acme parent navigation</body>","waitUntil":"load"}})).await;
    collectors::refused(mcp,target,json!({"operation":"evaluate","scope":retained,"expression":"node=>node.id","isFunction":true}),"STALE_REFERENCE",true).await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    json!({"freshCliEmbeddingMs":fresh_cli_ms,"retainedActEmbeddingMs":retained_ms,"workflowWithLifetimesAndPageCleanupMs":workflow_started.elapsed().as_secs_f64()*1000.0})
}

#[tokio::test]
async fn installed_step_four_frame_element_checkpoint() {
    let fixture = installed_fixture();
    let root = std::path::PathBuf::from(std::env::var_os("TESTRUN_BROWSER_ROOT").unwrap());
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let cli = fixture
                .cli(&["browser", "doctor", "--engine", engine])
                .await;
            let remote = mcp.tool("browser_doctor", json!({"engine":engine})).await;
            assert_eq!(
                cli["capabilities"], remote["capabilities"],
                "CLI and MCP must expose the same admission contract"
            );
            let relationships = &cli["capabilities"]["evaluation"]["elementRelationships"];
            assert_eq!(relationships["embeddingElement"], true);
            assert_eq!(relationships["embeddingHandleLifetime"], "parent-document");
            assert_eq!(relationships["fullSdkParity"], false);
            if engine == "chrome" {
                let cleanup =
                    &cli["capabilities"]["evaluation"]["handleAllocation"]["cleanupAdmission"];
                assert_eq!(cleanup["highWater"], 256);
                assert_eq!(cleanup["error"], "EXECUTION_CLEANUP_PENDING");
                assert_eq!(cleanup["existingHandleReadsAndDisposal"], true);
            }
            let baseline=Command::new(root.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"))
            .arg("tests/fixtures/browser-frame-element.cjs").arg(root.join("playwright-core-1.64.1/package"))
            .arg(if engine=="chrome" {"chromium"} else {engine}).arg(mode)
            .arg(root.join("browsers/chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"))
            .env("PLAYWRIGHT_BROWSERS_PATH",root.join("browsers")).output().await.unwrap();
            assert!(
                baseline.status.success(),
                "Pinned {engine}/{mode}: {}",
                String::from_utf8_lossy(&baseline.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&baseline.stdout).unwrap(),
                json!({"embedding":true,"parentRealm":true,"childReplacement":true,"detachedElement":true,"parentNavigation":true})
            );
            let connect_started = std::time::Instant::now();
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let connect_ms = connect_started.elapsed().as_secs_f64() * 1000.0;
            let context = &ready["browser"]["selectedContext"];
            let result =
                std::panic::AssertUnwindSafe(exercise(&fixture, &mut mcp, target, context))
                    .catch_unwind()
                    .await;
            let cleanup_started = std::time::Instant::now();
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, context,
            )
            .await;
            let cleanup_ms = cleanup_started.elapsed().as_secs_f64() * 1000.0;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            let mut metrics = match result {
                Ok(metrics) => metrics,
                Err(panic) => {
                    mcp.stop().await;
                    std::panic::resume_unwind(panic)
                }
            };
            metrics["connectMs"] = json!(connect_ms);
            metrics["disconnectMs"] = json!(cleanup_ms);
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("frame-element-{engine}-{mode}-timing.json")),
                serde_json::to_vec_pretty(&metrics).unwrap(),
            )
            .unwrap();
            eprintln!(
                "Frame element {engine}/{mode} passed: {metrics}; workspace {}",
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
