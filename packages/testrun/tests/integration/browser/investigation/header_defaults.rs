//! Installed context headers, target propagation and associated HTTP overrides.
use super::*;
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod wire;

#[tokio::test]
async fn step_four_page_header_sdk_baseline_diagnostic() {
    let fixture = installed_fixture();
    let site = wire::Site::start(None).await;
    for mode in ["headless", "headed"] {
        for engine in ["firefox", "webkit"] {
            let result = Command::new(
                fixture
                    .cache
                    .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
            )
            .arg(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/browser-header-baseline.cjs"),
            )
            .arg(fixture.cache.join("playwright-core-1.64.1/package"))
            .arg(engine)
            .arg(mode)
            .arg(&site.url)
            .current_dir(fixture.workspace.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("PW_LANG_NAME", "javascript")
            .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
            .output()
            .await
            .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let report: Value = serde_json::from_slice(&result.stdout).unwrap();
            eprintln!("Unmodified pinned SDK page headers {engine}/{mode}: {report}");
            assert_eq!(report["initial"], "context");
            assert_eq!(report["page"], "page");
            assert_eq!(report["contextChanged"], "page");
            assert_eq!(report["reset"], "changed");
            assert_eq!(
                report["empty"],
                if engine == "firefox" {
                    Value::Null
                } else {
                    json!("")
                }
            );
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("header-baseline-{engine}-{mode}.json")),
                &result.stdout,
            )
            .unwrap();
        }
    }
}

async fn browser_fetch(
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    tag: &str,
    expected: Option<&str>,
) {
    let response = operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"async url => {const record=await (await fetch(url)).json();return record.headers['x-acme']??null;}","argument":{"kind":"json","value":format!("/echo?browser={tag}")}})).await;
    assert_eq!(response["value"]["value"].as_str(), expected, "{response}");
}
async fn http_fetch(
    mcp: &mut Mcp,
    target: &str,
    client: &Value,
    site: &wire::Site,
    tag: &str,
    expected: Option<&str>,
    headers: Value,
    shared: bool,
) {
    let path = format!("/echo?http={tag}");
    let request = json!({"url":path,"headers":headers});
    let response = if shared {
        let submitted = mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_fetch","httpContext":client,"request":request}]})).await;
        let completed = completed(mcp, target, submitted).await;
        assert_eq!(completed["completedActions"], 1);
        completed["results"][0].clone()
    } else {
        operation(
            mcp,
            target,
            json!({"operation":"http_fetch","httpContext":client,"request":request}),
        )
        .await
    };
    assert_eq!(response["status"], 200);
    assert_eq!(response["replayed"], false);
    let records = site.records();
    let matching = records
        .iter()
        .filter(|record| record["path"] == path)
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "Each HTTP request must dispatch once: {records:?}"
    );
    assert_eq!(
        matching[0]["headers"]["x-acme"].as_str(),
        expected,
        "{:?}",
        matching[0]
    );
    operation(
        mcp,
        target,
        json!({"operation":"http_response_remove","requestId":response["requestId"]}),
    )
    .await;
}
async fn wait_record(site: &wire::Site, path: &str, expected: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let records = site.records();
        if let Some(record) = records.iter().find(|record| record["path"] == path) {
            assert_eq!(record["headers"]["x-acme"], expected, "{record}");
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Missing owned request {path}: {records:?}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    ready: &Value,
    site: &wire::Site,
    engine: &str,
) {
    let context = &ready["browser"]["selectedContext"];
    let page = &ready["browser"]["selectedPage"];
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/header-page?initial=Acme",site.url)}})).await;
    wait_record(site, "/header-page?initial=Acme", "initial").await;
    browser_fetch(mcp, target, page, "initial", Some("initial")).await;
    let mut clients = Vec::new();
    for headers in [
        Value::Null,
        json!([]),
        json!([{"name":"X-Acme","value":"explicit"}]),
    ] {
        let explicit = headers
            .as_array()
            .is_some_and(|headers| !headers.is_empty());
        let mut options = json!({"browserContext":context,"allowUnknownCookiePartitions":engine=="firefox","headers":headers});
        if explicit {
            options["baseUrl"] = json!(site.url);
        }
        let created = operation(
            mcp,
            target,
            json!({"operation":"http_context_create","options":options}),
        )
        .await;
        clients.push(created["httpContext"].clone());
    }
    for (index, expected) in [Some("initial"), None, Some("explicit")]
        .into_iter()
        .enumerate()
    {
        http_fetch(
            mcp,
            target,
            &clients[index],
            site,
            &format!("initial-{index}"),
            expected,
            json!([]),
            false,
        )
        .await;
    }
    let requested = json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"changed"}]}});
    let submitted = fixture
        .cli(&[
            "browser",
            "send",
            "--target",
            target,
            &requested.to_string(),
        ])
        .await;
    let receipt = completed(mcp, target, submitted).await;
    assert!(
        !receipt.to_string().contains("\"changed\""),
        "Header values entered a receipt: {receipt}"
    );
    browser_fetch(mcp, target, page, "changed", Some("changed")).await;
    for (index, expected) in [Some("changed"), None, Some("explicit")]
        .into_iter()
        .enumerate()
    {
        http_fetch(
            mcp,
            target,
            &clients[index],
            site,
            &format!("changed-{index}"),
            expected,
            json!([]),
            true,
        )
        .await;
    }
    http_fetch(
        mcp,
        target,
        &clients[0],
        site,
        "request-override",
        Some(""),
        json!([{"name":"x-aCmE","value":""}]),
        false,
    )
    .await;
    let future = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":future,"navigation":{"kind":"goto","url":format!("{}/header-page?future=Acme",site.url)}})).await;
    wait_record(site, "/header-page?future=Acme", "changed").await;
    browser_fetch(mcp, target, &future, "future", Some("changed")).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"url => !!window.open(url, '_blank')","argument":{"kind":"json","value":format!("{}/header-page?popup=Acme",site.url)}})).await;
    wait_record(site, "/header-page?popup=Acme", "changed").await;
    let cross = site.url.replacen("127.0.0.1", "localhost", 1);
    let frame = operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"url => new Promise(resolve => {const frame=document.createElement('iframe');frame.onload=()=>resolve(true);frame.src=url;document.body.append(frame);})","argument":{"kind":"json","value":format!("{cross}/header-page?frame=Acme")}})).await;
    assert_eq!(frame["value"]["value"], true);
    wait_record(site, "/header-page?frame=Acme", "changed").await;
    let sibling_request = json!({"operation":"context_create","options":{"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"extraHTTPHeaders":[{"name":"X-Acme","value":"sibling"}]}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &sibling_request])
        .await;
    let sibling = completed(mcp, target, submitted).await["context"].clone();
    let sibling_page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":sibling,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":sibling_page,"navigation":{"kind":"goto","url":format!("{}/header-page?sibling=Acme",site.url)}})).await;
    wait_record(site, "/header-page?sibling=Acme", "sibling").await;
    operation(mcp,target,json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[]}})).await;
    browser_fetch(mcp, target, page, "reset", None).await;
    browser_fetch(mcp, target, &future, "future-reset", None).await;
    browser_fetch(
        mcp,
        target,
        &sibling_page,
        "sibling-after-reset",
        Some("sibling"),
    )
    .await;
    for (index, expected) in [None, None, Some("explicit")].into_iter().enumerate() {
        http_fetch(
            mcp,
            target,
            &clients[index],
            site,
            &format!("reset-{index}"),
            expected,
            json!([]),
            false,
        )
        .await;
    }
    collectors::refused(mcp,target,json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"Acme\r\nInjected: private"}]}}),"HTTP_HEADER_INVALID",false).await;
    for client in clients {
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":client}),
        )
        .await;
    }
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":sibling}),
    )
    .await;
    assert_eq!(
        operation(mcp, target, json!({"operation":"http_context_list"})).await["contexts"],
        json!([])
    );
}

async fn page_cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    ready: &Value,
    site: &wire::Site,
    engine: &str,
) {
    let context = &ready["browser"]["selectedContext"];
    let page = &ready["browser"]["selectedPage"];
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/header-page",site.url)}})).await;
    let sibling = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":sibling,"navigation":{"kind":"goto","url":format!("{}/header-page?sibling",site.url)}})).await;
    let client=operation(mcp,target,json!({"operation":"http_context_create","options":{"browserContext":context,"allowUnknownCookiePartitions":engine=="firefox"}})).await["httpContext"].clone();
    let request=json!({"operation":"page_set_extra_http_headers","page":page,"headers":[{"name":"x-aCmE","value":"page-private"},{"name":"X-Page","value":"private-value"}]}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let receipt = completed(mcp, target, submitted).await;
    assert_eq!(receipt["headerCount"], 2);
    assert!(!receipt.to_string().contains("page-private"));
    browser_fetch(mcp, target, page, "page-override", Some("page-private")).await;
    browser_fetch(mcp, target, &sibling, "sibling-isolation", Some("initial")).await;
    http_fetch(
        mcp,
        target,
        &client,
        site,
        "page-http-isolation",
        Some("initial"),
        json!([]),
        false,
    )
    .await;
    operation(mcp,target,json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"context-changed"}]}})).await;
    browser_fetch(
        mcp,
        target,
        page,
        "context-after-page",
        Some("page-private"),
    )
    .await;
    browser_fetch(
        mcp,
        target,
        &sibling,
        "sibling-context-changed",
        Some("context-changed"),
    )
    .await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"url => !!window.open(url, '_blank')","argument":{"kind":"json","value":format!("{}/header-page?page-popup",site.url)}})).await;
    wait_record(site, "/header-page?page-popup", "context-changed").await;
    http_fetch(
        mcp,
        target,
        &client,
        site,
        "context-http-changed",
        Some("context-changed"),
        json!([]),
        true,
    )
    .await;
    // Reload and cross-origin navigation preserve the page owner, not the old document.
    for (tag, origin) in [
        ("same-origin", site.url.clone()),
        (
            "cross-origin",
            site.url.replacen("127.0.0.1", "localhost", 1),
        ),
    ] {
        let path = format!("/header-page?page-{tag}");
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{origin}{path}")}})).await;
        wait_record(site, &path, "page-private").await;
        browser_fetch(mcp, target, page, tag, Some("page-private")).await;
    }
    let frame_url = format!("{}/header-page?page-frame", site.url);
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"url => new Promise(resolve => {const frame=document.createElement('iframe');frame.onload=()=>resolve(true);frame.src=url;document.body.append(frame);})","argument":{"kind":"json","value":frame_url}})).await;
    wait_record(site, "/header-page?page-frame", "page-private").await;
    operation(mcp,target,json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[]}})).await;
    browser_fetch(
        mcp,
        target,
        page,
        "context-reset-page-retained",
        Some("page-private"),
    )
    .await;
    browser_fetch(mcp, target, &sibling, "context-reset-sibling", None).await;
    operation(mcp,target,json!({"operation":"page_set_extra_http_headers","page":page,"headers":[{"name":"X-Acme","value":""}]})).await;
    // The unchanged pinned SDK independently reproduces Firefox's omission;
    // every engine must still suppress the nonempty context default.
    browser_fetch(
        mcp,
        target,
        page,
        "page-explicit-empty-value",
        if engine == "firefox" { None } else { Some("") },
    )
    .await;
    // Shared act resets only the page layer, revealing the current context list.
    let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"page_set_extra_http_headers","page":page,"headers":[]}]})).await;
    let done = completed(mcp, target, submitted).await;
    assert_eq!(done["completedActions"], 1);
    browser_fetch(mcp, target, page, "page-reset", None).await;
    // Replacement removes X-Page from the prior page list.
    let probe=operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"async () => (await (await fetch('/echo?page-reset-extra')).json()).headers['x-page']??null"})).await;
    assert!(probe["value"]["value"].is_null(), "{probe}");
    collectors::refused(mcp,target,json!({"operation":"page_set_extra_http_headers","page":page,"headers":[{"name":"X-Acme","value":"bad\r\nvalue"}]}),"HTTP_HEADER_INVALID",false).await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    collectors::refused(
        mcp,
        target,
        json!({"operation":"page_set_extra_http_headers","page":page,"headers":[]}),
        "STALE_REFERENCE",
        true,
    )
    .await;
    browser_fetch(mcp, target, &sibling, "sibling-after-close", None).await;
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":client}),
    )
    .await;
}

#[tokio::test]
async fn installed_step_four_page_headers_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let site = wire::Site::start(None).await;
            let connected=mcp.tool("browser_connect",json!({"engine":engine,"mode":mode,"context":{"viewport":{"width":800,"height":600},"deviceScaleFactor":1,"baseURL":site.url,"extraHTTPHeaders":[{"name":"X-Acme","value":"initial"}]}})).await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let scenario = std::panic::AssertUnwindSafe(page_cell(
                &fixture, &mut mcp, target, &ready, &site, engine,
            ))
            .catch_unwind()
            .await;
            let result = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if result["result"]["structuredContent"]["ok"] != true {
                cleanup_failures.push(format!("{engine}/{mode}: {result}"));
            }
            if let Err(panic) = scenario {
                std::panic::resume_unwind(panic);
            }
        }
    }
    assert!(
        cleanup_failures.is_empty(),
        "Original cleanup failures: {cleanup_failures:?}"
    );
    mcp.stop().await;
}

#[tokio::test]
async fn installed_step_four_header_defaults_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let site = wire::Site::start(None).await;
            let options = json!({"viewport":{"width":800,"height":600},"deviceScaleFactor":1,"baseURL":site.url,"extraHTTPHeaders":[{"name":"X-Acme","value":"initial"}]});
            let connected = if mode == "headed" {
                fixture
                    .cli(&[
                        "connect",
                        "--engine",
                        engine,
                        "--mode",
                        mode,
                        "--context",
                        &options.to_string(),
                    ])
                    .await
            } else {
                mcp.tool(
                    "browser_connect",
                    json!({"engine":engine,"mode":mode,"context":options}),
                )
                .await
            };
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let start = std::time::Instant::now();
            let scenario = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, target, &ready, &site, engine,
            ))
            .catch_unwind()
            .await;
            let result = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if result["result"]["structuredContent"]["ok"] != true {
                cleanup_failures.push(format!("{engine}/{mode}: {result}"));
            }
            if let Err(panic) = scenario {
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Header defaults {engine} {mode}: {}ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    assert!(
        cleanup_failures.is_empty(),
        "Original cleanup failures: {cleanup_failures:?}"
    );
    mcp.stop().await;
}
