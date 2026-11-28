//! Caller getters/proxy enumeration must reach the execution they can release.
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
async fn hold(mcp: &mut Mcp, target: &str, page: &Value) -> Value {
    value(
        mcp,
        target,
        page,
        "()=>{globalThis.acmeWaiting=false;return true}",
    )
    .await;
    let held=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":"()=>new Promise(resolve=>{globalThis.acmeRelease=()=>resolve('Acme released');acmeWaiting=true})","isFunction":true}})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if value(mcp, target, page, "()=>acmeWaiting").await == true {
            break;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Caller was not witnessed before property admission"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    held
}
async fn exercise(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value) -> Value {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    let parent=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{globalThis.acmeReads=0;globalThis.acmeThenReads=0;globalThis.acmePromise=Promise.resolve(42);return Object.defineProperties({},{release:{enumerable:true,get(){acmeReads++;acmeRelease();return acmePromise}},thenable:{get(){return {get then(){acmeThenReads++;throw Error('Acme then')}}}}})}","isFunction":true,"returnHandle":true})).await["handle"].clone();
    let held = hold(mcp, target, &page).await;
    let started = std::time::Instant::now();
    let request =
        json!({"operation":"handle_property","handle":parent,"name":"release"}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let property = completed(mcp, target, submitted).await["handle"].clone();
    let property_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        completed(mcp, target, held).await["value"]["value"],
        "Acme released"
    );
    assert_eq!(
        value(mcp, target, &property, "value=>value===acmePromise").await,
        true
    );
    let thenable = operation(
        mcp,
        target,
        json!({"operation":"handle_property","handle":parent,"name":"thenable"}),
    )
    .await["handle"]
        .clone();
    assert_eq!(
        value(
            mcp,
            target,
            &thenable,
            "value=>typeof Object.getOwnPropertyDescriptor(value,'then').get"
        )
        .await,
        "function"
    );
    assert_eq!(value(mcp, target, &page, "()=>acmeReads").await, 1);
    assert_eq!(value(mcp, target, &page, "()=>acmeThenReads").await, 0);
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":parent}),
    )
    .await;
    assert_eq!(
        value(mcp, target, &property, "value=>value===acmePromise").await,
        true
    );
    for handle in [&property, &thenable] {
        operation(
            mcp,
            target,
            json!({"operation":"handle_dispose","handle":handle}),
        )
        .await;
    }
    let proxy=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{globalThis.acmeEnumerationReads=0;return new Proxy({Acme:1,Beta:2},{ownKeys(target){acmeEnumerationReads++;acmeRelease();return Reflect.ownKeys(target)}})}","isFunction":true,"returnHandle":true})).await["handle"].clone();
    let held = hold(mcp, target, &page).await;
    let started = std::time::Instant::now();
    let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"handle_properties","handle":proxy,"query":{"limit":1}}]})).await;
    let names = completed(mcp, target, submitted).await["results"][0]["properties"].clone();
    let enumeration_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(names["names"], json!(["Acme"]));
    assert_eq!(names["hasMore"], true);
    assert_eq!(names["nextOffset"], 1);
    assert_eq!(names["snapshot"], false);
    assert_eq!(
        completed(mcp, target, held).await["value"]["value"],
        "Acme released"
    );
    assert_eq!(
        value(mcp, target, &page, "()=>acmeEnumerationReads").await,
        1
    );
    let next = operation(
        mcp,
        target,
        json!({"operation":"handle_properties","handle":proxy,"query":{"offset":1,"limit":1}}),
    )
    .await;
    assert_eq!(next["properties"]["names"], json!(["Beta"]));
    assert_eq!(next["properties"]["hasMore"], false);
    assert_eq!(
        value(mcp, target, &page, "()=>acmeEnumerationReads").await,
        2
    );
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":"data:text/html,<body>Acme replacement</body>","waitUntil":"load"}})).await;
    collectors::refused(
        mcp,
        target,
        json!({"operation":"handle_properties","handle":proxy}),
        "STALE_REFERENCE",
        true,
    )
    .await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    json!({"freshCliPropertyMs":property_ms,"retainedActEnumerationMs":enumeration_ms})
}

#[tokio::test]
async fn installed_step_four_property_callers_checkpoint() {
    let fixture = installed_fixture();
    let root = std::path::PathBuf::from(std::env::var_os("TESTRUN_BROWSER_ROOT").unwrap());
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let baseline=Command::new(root.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node")).arg("tests/fixtures/browser-property-callers.cjs").arg(root.join("playwright-core-1.64.1/package")).arg(if engine=="chrome" {"chromium"} else {engine}).arg(mode).arg(root.join("browsers/chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing")).env("PLAYWRIGHT_BROWSERS_PATH",root.join("browsers")).output().await.unwrap();
            assert!(
                baseline.status.success(),
                "Pinned {engine}/{mode}: {}",
                String::from_utf8_lossy(&baseline.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&baseline.stdout).unwrap(),
                json!({"propertyUnblocks":true,"promisePreserved":true,"thenablePreserved":true,"parentDisposal":true,"sdkValueMap":true,"accessorEntry":engine=="firefox"})
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
            let metrics = match result {
                Ok(value) => value,
                Err(panic) => {
                    mcp.stop().await;
                    std::panic::resume_unwind(panic)
                }
            };
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("property-callers-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(&metrics).unwrap(),
            )
            .unwrap();
            eprintln!(
                "Property callers {engine}/{mode} passed: {metrics}; workspace {}",
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
