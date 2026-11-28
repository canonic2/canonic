//! Exact element relationships through CLI, browser_send and shared act.
use super::*;

async fn retain(mcp: &mut Mcp, target: &str, scope: &Value, expression: &str) -> Value {
    operation(mcp,target,json!({"operation":"evaluate","scope":scope,"expression":expression,"isFunction":true,"returnHandle":true})).await["handle"].clone()
}
async fn relationship(mcp: &mut Mcp, target: &str, handle: &Value, owner: bool) -> Value {
    operation(mcp,target,json!({"operation":if owner {"handle_owner_frame"} else {"handle_content_frame"},"handle":handle})).await
}
async fn exercise(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value) {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":"<!doctype html><button id=adopt>Acme</button><iframe id=child srcdoc='<body>Acme child</body>'></iframe>","waitUntil":"load"}})).await;
    let button = retain(mcp, target, &page, "()=>document.querySelector('#adopt')").await;
    let iframe = retain(mcp, target, &page, "()=>document.querySelector('#child')").await;
    let document = retain(mcp, target, &page, "()=>document").await;
    let main = relationship(mcp, target, &button, true).await;
    assert_eq!(main["page"], page);
    assert!(!main["frame"].is_null());
    assert_eq!(main["sourcePage"], page);
    assert_eq!(main["relationshipCleanupPending"], false);
    assert_eq!(
        relationship(mcp, target, &button, false).await["frame"],
        Value::Null
    );
    assert_eq!(
        relationship(mcp, target, &document, true).await["frame"],
        main["frame"]
    );
    assert_eq!(
        relationship(mcp, target, &document, false).await["frame"],
        Value::Null
    );
    let child = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    let query = json!({"operation":"handle_content_frame","handle":iframe}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &query])
        .await;
    assert_eq!(completed(mcp, target, submitted).await["frame"], child);
    let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"handle_owner_frame","handle":iframe},{"action":"handle_content_frame","handle":iframe}]})).await;
    let result = completed(mcp, target, submitted).await;
    assert_eq!(result["results"][0]["frame"], main["frame"]);
    assert_eq!(result["results"][1]["frame"], child);
    let inner = retain(mcp, target, &child, "()=>document.body").await;
    assert_eq!(
        relationship(mcp, target, &inner, true).await["frame"],
        child
    );
    operation(mcp,target,json!({"operation":"evaluate","scope":button,"expression":"node=>node.remove()","isFunction":true})).await;
    assert_eq!(
        relationship(mcp, target, &button, true).await["frame"],
        main["frame"]
    );
    operation(mcp,target,json!({"operation":"evaluate","scope":button,"expression":"node=>{document.body.append(node)}","isFunction":true})).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{globalThis.acmePopup=window.open('about:blank');return !!acmePopup}","isFunction":true})).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{acmePopup.document.body.append(acmePopup.document.adoptNode(document.querySelector('#adopt')))}","isFunction":true})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let adopted = loop {
        let value = relationship(mcp, target, &button, true).await;
        if !value["page"].is_null() {
            break value;
        }
        assert!(tokio::time::Instant::now() < until, "{value}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert_ne!(adopted["page"], page);
    assert_eq!(adopted["sourcePage"], page);
    assert_eq!(
        relationship(mcp, target, &button, false).await["frame"],
        Value::Null
    );
    let before = mcp.tool("browser_status", json!({"targetId":target})).await;
    let query = json!({"operation":"evaluate","scope":adopted["page"],"expression":"()=>!!document.querySelector('#adopt')","isFunction":true});
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":query}))
        .await;
    let observed = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    std::fs::write(
        fixture
            .workspace
            .path()
            .join("frame-relationship-popup.json"),
        serde_json::to_vec_pretty(
            &json!({"adopted":adopted,"before":before,"query":query,"observed":observed}),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        observed["operation"]["outcome"], "succeeded",
        "Popup evaluation: {observed}"
    );
    assert_eq!(observed["operation"]["result"]["value"]["value"], true);
    let primitive = retain(mcp, target, &page, "()=>7").await;
    for operation_name in ["handle_owner_frame", "handle_content_frame"] {
        collectors::refused(
            mcp,
            target,
            json!({"operation":operation_name,"handle":primitive}),
            "INVALID_SCOPE",
            true,
        )
        .await;
    }
    for handle in [&button, &iframe, &document, &inner, &primitive] {
        operation(
            mcp,
            target,
            json!({"operation":"handle_dispose","handle":handle}),
        )
        .await;
    }
    collectors::refused(
        mcp,
        target,
        json!({"operation":"handle_owner_frame","handle":button}),
        "STALE_REFERENCE",
        true,
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"page_close","page":adopted["page"]}),
    )
    .await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
}

#[tokio::test]
async fn installed_step_four_frame_relationships_checkpoint() {
    let fixture = installed_fixture();
    let root = std::path::PathBuf::from(std::env::var_os("TESTRUN_BROWSER_ROOT").unwrap());
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let baseline =
                Command::new(root.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"))
                    .arg("tests/fixtures/browser-frame-relationships.cjs")
                    .arg(root.join("playwright-core-1.64.1/package"))
                    .arg(if engine == "chrome" {
                        "chromium"
                    } else {
                        engine
                    })
                    .arg(mode)
                .arg(root.join("browsers/chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"))
                    .env("PLAYWRIGHT_BROWSERS_PATH", root.join("browsers"))
                    .output()
                    .await
                    .unwrap();
            assert!(
                baseline.status.success(),
                "Pinned {engine}/{mode}: {}",
                String::from_utf8_lossy(&baseline.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&baseline.stdout).unwrap(),
                json!({"owner":true,"content":true,"document":true,"detached":true,"crossPageAdoption":true})
            );
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let context = &ready["browser"]["selectedContext"];
            let result =
                std::panic::AssertUnwindSafe(exercise(&fixture, &mut mcp, target, context))
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
                "Frame relationships {engine}/{mode} passed; workspace {}",
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
