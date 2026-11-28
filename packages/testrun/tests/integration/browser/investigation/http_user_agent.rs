//! Installed engine/isolated agent defaults compared with independent wire observations.
use super::*;
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod wire;

async fn observed(
    mcp: &mut Mcp,
    target: &str,
    http: &Value,
    site: &wire::Site,
    expected: &str,
    headers: Value,
    shared: bool,
) {
    let before = site.records().len();
    let request = json!({"url":"/echo","headers":headers});
    let result = if shared {
        let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_fetch","httpContext":http,"request":request}]})).await;
        let result = completed(mcp, target, submitted).await;
        assert_eq!(result["completedActions"], 1);
        result["results"][0].clone()
    } else {
        operation(
            mcp,
            target,
            json!({"operation":"http_fetch","httpContext":http,"request":request}),
        )
        .await
    };
    assert_eq!(result["replayed"], false);
    assert_eq!(
        site.records().len(),
        before + 1,
        "Default selection must not send probe requests"
    );
    assert_eq!(
        site.records().last().unwrap()["headers"]["user-agent"],
        expected
    );
    operation(
        mcp,
        target,
        json!({"operation":"http_response_remove","requestId":result["requestId"]}),
    )
    .await;
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

#[expect(
    clippy::too_many_lines,
    reason = "One correlated workflow compares independent wire defaults, overrides, and browser context lifetime before cleanup"
)]
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    ready: &Value,
    engine: &str,
    site: &wire::Site,
    isolated_agent: &str,
) -> Value {
    let page = &ready["browser"]["selectedPage"];
    let context = &ready["browser"]["selectedContext"];
    let selected=operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>{const agent=navigator.userAgent;globalThis.acmeAgentReads=0;Object.defineProperty(navigator,'userAgent',{configurable:true,get(){acmeAgentReads++;return 'Acme page spoof';}});return agent;}"})).await;
    let engine_agent = selected["value"]["value"].as_str().unwrap();
    assert_ne!(engine_agent, "Acme page spoof");
    let start = std::time::Instant::now();
    let submitted = fixture
        .cli(&[
            "browser",
            "send",
            "--target",
            target,
            &json!({"operation":"http_context_create","options":{"baseUrl":site.url}}).to_string(),
        ])
        .await;
    let isolated = completed(mcp, target, submitted).await["httpContext"].clone();
    let fresh_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = std::time::Instant::now();
    observed(
        mcp,
        target,
        &isolated,
        site,
        isolated_agent,
        json!([]),
        false,
    )
    .await;
    observed(
        mcp,
        target,
        &isolated,
        site,
        "Acme request agent",
        json!([{"name":"uSeR-aGeNt","value":"Acme request agent"}]),
        true,
    )
    .await;
    observed(
        mcp,
        target,
        &isolated,
        site,
        isolated_agent,
        json!([]),
        false,
    )
    .await;
    let empty_isolated = create(mcp, target, json!({"baseUrl":site.url,"userAgent":""})).await;
    observed(
        mcp,
        target,
        &empty_isolated,
        site,
        isolated_agent,
        json!([]),
        false,
    )
    .await;
    observed(
        mcp,
        target,
        &empty_isolated,
        site,
        "",
        json!([{"name":"User-Agent","value":""}]),
        true,
    )
    .await;
    if engine == "firefox" {
        collectors::refused(mcp,target,json!({"operation":"http_context_create","options":{"browserContext":context,"baseUrl":site.url}}),"HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE",true).await;
    }
    let associated=create(mcp,target,json!({"browserContext":context,"baseUrl":site.url,"allowUnknownCookiePartitions":engine=="firefox"})).await;
    observed(
        mcp,
        target,
        &associated,
        site,
        engine_agent,
        json!([]),
        false,
    )
    .await;
    observed(
        mcp,
        target,
        &associated,
        site,
        "Acme request agent",
        json!([{"name":"User-Agent","value":"Acme request agent"}]),
        true,
    )
    .await;
    observed(
        mcp,
        target,
        &associated,
        site,
        engine_agent,
        json!([]),
        false,
    )
    .await;
    let explicit=create(mcp,target,json!({"browserContext":context,"baseUrl":site.url,"allowUnknownCookiePartitions":engine=="firefox","userAgent":"Acme HTTP agent","ignoreHttpsErrors":false})).await;
    observed(
        mcp,
        target,
        &explicit,
        site,
        "Acme HTTP agent",
        json!([]),
        false,
    )
    .await;
    let empty_associated=create(mcp,target,json!({"browserContext":context,"baseUrl":site.url,"allowUnknownCookiePartitions":engine=="firefox","userAgent":""})).await;
    observed(
        mcp,
        target,
        &empty_associated,
        site,
        engine_agent,
        json!([]),
        false,
    )
    .await;
    let mut clients = vec![
        isolated,
        empty_isolated,
        associated,
        empty_associated,
        explicit,
    ];
    if engine != "chrome" {
        let configured=operation(mcp,target,json!({"operation":"context_create","options":{"viewport":{"width":800,"height":600},"deviceScaleFactor":1,"userAgent":"Acme context agent","ignoreHTTPSErrors":true}})).await["context"].clone();
        let inherited=create(mcp,target,json!({"browserContext":configured,"baseUrl":site.url,"allowUnknownCookiePartitions":engine=="firefox"})).await;
        observed(
            mcp,
            target,
            &inherited,
            site,
            "Acme context agent",
            json!([]),
            false,
        )
        .await;
        let override_agent=create(mcp,target,json!({"browserContext":configured,"baseUrl":site.url,"allowUnknownCookiePartitions":engine=="firefox","userAgent":"Acme HTTP agent","ignoreHttpsErrors":false})).await;
        observed(
            mcp,
            target,
            &override_agent,
            site,
            "Acme HTTP agent",
            json!([]),
            true,
        )
        .await;
        let contexts = operation(mcp, target, json!({"operation":"http_context_list"})).await;
        assert_eq!(
            contexts["contexts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["httpContext"] == override_agent)
                .unwrap()["tlsVerification"],
            true
        );
        let empty_inherited=create(mcp,target,json!({"browserContext":configured,"baseUrl":site.url,"allowUnknownCookiePartitions":engine=="firefox","userAgent":""})).await;
        observed(
            mcp,
            target,
            &empty_inherited,
            site,
            "Acme context agent",
            json!([]),
            false,
        )
        .await;
        for http in [inherited, override_agent, empty_inherited] {
            operation(
                mcp,
                target,
                json!({"operation":"http_context_remove","httpContext":http}),
            )
            .await;
        }
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":configured}),
        )
        .await;
    }
    let reads=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>acmeAgentReads","isFunction":true})).await;
    assert_eq!(
        reads["value"]["value"], 0,
        "HTTP defaults must never consult page navigator.userAgent"
    );
    for http in clients.drain(..) {
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":http}),
        )
        .await;
    }
    assert_eq!(
        operation(mcp, target, json!({"operation":"http_context_list"})).await["contexts"],
        json!([])
    );
    json!({"freshCliIsolatedCreateMs":fresh_ms,"retainedDefaultsWorkflowWithWireInspectionMs":start.elapsed().as_secs_f64()*1000.0})
}

#[tokio::test]
async fn installed_step_four_http_user_agent_checkpoint() {
    let fixture = installed_fixture();
    let site = wire::Site::start(None).await;
    for option in ["default", "empty"] {
        let baseline = Command::new(
            fixture
                .cache
                .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
        )
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/browser-http-agent-baseline.cjs"),
        )
        .arg(fixture.cache.join("playwright-core-1.64.1/package"))
        .arg(format!("{}/echo?baseline=Acme", site.url))
        .arg(option)
        .current_dir(fixture.workspace.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("PW_LANG_NAME", "javascript")
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
        assert!(
            baseline.status.success(),
            "{}",
            String::from_utf8_lossy(&baseline.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&baseline.stdout).unwrap()["status"],
            200
        );
    }
    assert_eq!(site.records().len(), 2);
    let isolated_agent = site.records()[0]["headers"]["user-agent"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        site.records()[1]["headers"]["user-agent"],
        isolated_agent,
        "The unchanged SDK treats an empty userAgent option as default"
    );
    assert!(
        isolated_agent.starts_with("Playwright/1.64.1 ("),
        "{isolated_agent}"
    );
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let start = std::time::Instant::now();
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let connect_ms = start.elapsed().as_secs_f64() * 1000.0;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture,
                &mut mcp,
                target,
                &ready,
                engine,
                &site,
                &isolated_agent,
            ))
            .catch_unwind()
            .await;
            let start = std::time::Instant::now();
            let disconnect = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if disconnect["result"]["structuredContent"]["ok"] != true {
                cleanup_failures.push(disconnect);
            }
            let cleanup_ms = start.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(metrics) => eprintln!(
                    "HTTP user-agent {engine} {mode}: {metrics}; connectMs={connect_ms}; cleanupMs={cleanup_ms}"
                ),
                Err(panic) => {
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.stop().await;
    assert!(
        cleanup_failures.is_empty(),
        "Original HTTP user-agent disconnects failed: {cleanup_failures:?}"
    );
}
