//! Installed explicit and inherited HTTP authentication, with independent wire counts.
use super::*;
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod wire;

#[tokio::test]
async fn step_four_http_credentials_sdk_baseline_diagnostic() {
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
                    .join("tests/fixtures/browser-http-credentials-baseline.cjs"),
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
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("credentials-sdk-{engine}-{mode}.json")),
                &result.stdout,
            )
            .unwrap();
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("credentials-wire-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(&site.records()).unwrap(),
            )
            .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let report: Value = serde_json::from_slice(&result.stdout).unwrap();
            eprintln!("Pinned SDK credential observations {engine}/{mode}: {report}");
            for observed in report.as_array().unwrap() {
                assert_eq!(observed["apiStatus"], 200, "{observed}");
            }
        }
    }
}

fn credential(origin: &str, send: &str) -> Value {
    json!({"origin":origin,"username":"Acme","password":"pass","send":send})
}
async fn create(mcp: &mut Mcp, target: &str, options: Value) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"http_context_create","options":options}),
    )
    .await["httpContext"]
        .clone()
}
async fn observed(
    mcp: &mut Mcp,
    target: &str,
    http: &Value,
    site: &wire::Site,
    request: Value,
    status: u64,
    retries: usize,
    shared: bool,
) {
    let before = site.records().len();
    let response = if shared {
        let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_fetch","httpContext":http,"request":request}]})).await;
        completed(mcp, target, submitted).await["results"][0].clone()
    } else {
        operation(
            mcp,
            target,
            json!({"operation":"http_fetch","httpContext":http,"request":request}),
        )
        .await
    };
    assert_eq!(response["status"], status, "{response}");
    assert_eq!(response["retryCount"], 0);
    assert_eq!(response["authenticationRetries"], retries);
    assert_eq!(response["replayed"], retries > 0);
    let records = site.records();
    let observed = &records[before..];
    assert_eq!(observed.len(), retries + 1, "{observed:?}");
    if retries > 0 {
        assert!(observed[0]["headers"]["authorization"].is_null());
        assert_eq!(
            observed[1]["headers"]["authorization"],
            "Basic QWNtZTpwYXNz"
        );
    }
    assert!(!response.to_string().contains("QWNtZTpwYXNz"));
    operation(
        mcp,
        target,
        json!({"operation":"http_response_remove","requestId":response["requestId"]}),
    )
    .await;
}
async fn remove(mcp: &mut Mcp, target: &str, http: &Value) {
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":http}),
    )
    .await;
}
async fn cell(fixture: &Fixture, mcp: &mut Mcp, target: &str, engine: &str, site: &wire::Site) {
    let configured = json!([
        credential("http://example.invalid", "always"),
        credential(&site.url, "unauthorized")
    ]);
    let submitted=fixture.cli(&["browser","send","--target",target,&json!({"operation":"http_context_create","options":{"baseUrl":site.url,"httpCredentials":configured}}).to_string()]).await;
    let explicit = completed(mcp, target, submitted).await["httpContext"].clone();
    let before = site.records().len();
    observed(mcp,target,&explicit,site,json!({"url":"/auth/basic","method":"POST","body":{"kind":"text","text":"Acme immutable body"}}),200,1,true).await;
    for record in &site.records()[before..] {
        assert_eq!(record["method"], "POST");
        assert_eq!(record["body"], "Acme immutable body");
    }
    observed(mcp,target,&explicit,site,json!({"url":"/auth/basic","headers":[{"name":"Authorization","value":"Basic explicit-wrong"}]}),401,0,false).await;
    remove(mcp, target, &explicit).await;
    let always = create(
        mcp,
        target,
        json!({"baseUrl":site.url,"httpCredentials":credential(&site.url,"always")}),
    )
    .await;
    observed(mcp,target,&always,site,json!({"url":"/auth/basic","headers":[{"name":"Authorization","value":"Basic explicit-wrong"}]}),200,0,false).await;
    assert_eq!(
        site.records().last().unwrap()["headers"]["authorization"],
        "Basic QWNtZTpwYXNz"
    );
    remove(mcp, target, &always).await;
    let context=operation(mcp,target,json!({"operation":"context_create","options":{"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"baseURL":site.url,"httpCredentials":configured}})).await["context"].clone();
    for override_value in [Value::Null, json!([])] {
        let inherited=create(mcp,target,json!({"browserContext":context,"httpCredentials":override_value,"allowUnknownCookiePartitions":engine=="firefox"})).await;
        let enabled = override_value.is_null();
        observed(
            mcp,
            target,
            &inherited,
            site,
            json!({"url":"/auth/basic"}),
            if enabled { 200 } else { 401 },
            usize::from(enabled),
            true,
        )
        .await;
        remove(mcp, target, &inherited).await;
    }
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    let before_navigation = site.records().len();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/auth/basic",site.url)}})).await;
    let records = site.records();
    let navigation = records[before_navigation..]
        .iter()
        .filter(|record| record["path"] == "/auth/basic")
        .last()
        .expect("The navigation must reach its selected auth resource");
    assert_eq!(
        navigation["headers"]["authorization"],
        "Basic QWNtZTpwYXNz",
        "Navigation wire: {:?}",
        &records[before_navigation..]
    );
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":context}),
    )
    .await;
}
#[tokio::test]
async fn installed_step_four_http_credentials_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let site = wire::Site::start(None).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result =
                std::panic::AssertUnwindSafe(cell(&fixture, &mut mcp, target, engine, &site))
                    .catch_unwind()
                    .await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("credentials-wire-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(&site.records()).unwrap(),
            )
            .unwrap();
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
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = result {
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
