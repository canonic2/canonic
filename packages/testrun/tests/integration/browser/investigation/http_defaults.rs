//! Installed request-default and redirect transitions, observed by an owned wire fixture.
use super::*;
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod wire;

async fn fetch(mcp: &mut Mcp, target: &str, http: &Value, request: Value, shared: bool) -> Value {
    let value = if shared {
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
    assert_eq!(value["retryCount"], 0);
    assert_eq!(value["replayed"], false);
    operation(
        mcp,
        target,
        json!({"operation":"http_response_remove","requestId":value["requestId"]}),
    )
    .await;
    value
}
async fn failed(mcp: &mut Mcp, target: &str, http: &Value, request: Value) -> Value {
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_fetch","httpContext":http,"request":request}})).await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(status["operation"]["settled"], true);
    assert_eq!(
        status["operation"]["code"], "HTTP_REDIRECT_LIMIT",
        "{status}"
    );
    status["operation"]["result"].clone()
}
#[expect(
    clippy::too_many_lines,
    reason = "One installed client workflow checks wire effects, redirect counts and header policies before owned cleanup"
)]
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    engine: &str,
    site: &wire::Site,
    other: &wire::Site,
) -> Value {
    if engine == "firefox" {
        collectors::refused(mcp,target,json!({"operation":"http_context_create","options":{"browserContext":context,"baseUrl":site.url}}),"HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE",true).await;
    }
    let options = json!({"browserContext":context,"baseUrl":site.url,"allowUnknownCookiePartitions":engine=="firefox"});
    let before = site.records().len();
    let request = json!({"operation":"http_context_create","options":options}).to_string();
    let start = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let http = completed(mcp, target, submitted).await["httpContext"].clone();
    let fresh_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = std::time::Instant::now();
    let response = fetch(mcp, target, &http, json!({"url":"/chain/20"}), false).await;
    assert_eq!(response["redirects"], 20);
    assert_eq!(site.records().len(), before + 21);
    let before = site.records().len();
    let response = fetch(
        mcp,
        target,
        &http,
        json!({"url":"/chain/20","maxRedirects":0}),
        true,
    )
    .await;
    assert_eq!(response["status"], 302);
    assert_eq!(response["redirectLimitReached"], false);
    assert_eq!(site.records().len(), before + 1);
    let before = site.records().len();
    let exhausted = failed(mcp, target, &http, json!({"url":"/chain/21"})).await;
    assert_eq!(exhausted["redirects"], 20);
    assert_eq!(exhausted["replayed"], false);
    assert_eq!(site.records().len(), before + 21);
    for path in ["/empty", "/missing"] {
        let before = site.records().len();
        let response = fetch(mcp, target, &http, json!({"url":path}), false).await;
        assert_eq!(response["status"], 302);
        assert_eq!(response["redirects"], 0);
        assert_eq!(site.records().len(), before + 1);
    }
    let before = site.records().len();
    let exhausted = failed(
        mcp,
        target,
        &http,
        json!({"url":"/exhaust","maxRedirects":1}),
    )
    .await;
    assert_eq!(exhausted["redirects"], 1);
    assert_eq!(site.records().len(), before + 2);
    fetch(mcp, target, &http, json!({"url":"/echo"}), false).await;
    let records = site.records();
    let headers = &records.last().unwrap()["headers"];
    assert_eq!(headers["accept"], "*/*");
    assert_eq!(headers["accept-encoding"], "gzip,deflate,br");
    let headers = json!([{"name":"Content-Type","value":"Acme"},{"name":"Content-Encoding","value":"identity"},{"name":"Content-Language","value":"Acme"},{"name":"Content-Location","value":"/Acme"},{"name":"Accept","value":"Acme context"},{"name":"Accept-Encoding","value":"identity"},{"name":"X-Acme","value":"Acme private"}]);
    let mut rich = options.clone();
    rich["headers"] = headers;
    rich["maxRedirects"] = json!(1);
    let created = mcp
        .tool(
            "act",
            json!({"targetId":target,"actions":[{"action":"http_context_create","options":rich}]}),
        )
        .await;
    let created = completed(mcp, target, created).await;
    let rich = created["results"][0]["httpContext"].clone();
    assert!(rich.is_object());
    let before = site.records().len();
    let exhausted = failed(mcp, target, &rich, json!({"url":"/chain/2"})).await;
    assert_eq!(exhausted["redirects"], 1);
    assert_eq!(site.records().len(), before + 2);
    let before = site.records().len();
    let response = fetch(
        mcp,
        target,
        &rich,
        json!({"url":"/chain/2","maxRedirects":2}),
        true,
    )
    .await;
    assert_eq!(response["redirects"], 2);
    assert_eq!(site.records().len(), before + 3);
    for (code, method, rewritten) in [
        (301, "pOsT", true),
        (302, "post", true),
        (303, "pUt", true),
        (303, "get", false),
        (303, "GET", false),
        (303, "head", false),
        (307, "POST", false),
        (308, "PUT", false),
    ] {
        let mut request = json!({"url":format!("/rewrite/{code}"),"method":method});
        if !matches!(method, "get" | "head") {
            request["body"] = json!({"kind":"text","text":"Acme body"});
        }
        let before = site.records().len();
        fetch(mcp, target, &rich, request, true).await;
        let records = site.records();
        assert_eq!(records.len(), before + 2);
        let last = records.last().unwrap();
        assert_eq!(
            last["method"],
            if rewritten {
                "GET".into()
            } else {
                method.to_ascii_uppercase()
            },
            "{last}"
        );
        assert_eq!(
            last["body"],
            if rewritten || matches!(method, "get" | "head") {
                ""
            } else {
                "Acme body"
            },
            "{last}"
        );
        for name in [
            "content-type",
            "content-encoding",
            "content-language",
            "content-location",
        ] {
            assert_eq!(last["headers"].get(name).is_some(), !rewritten, "{last}");
        }
        if rewritten {
            assert!(last["headers"].get("content-length").is_none(), "{last}");
        }
    }
    fetch(mcp,target,&rich,json!({"url":"/echo","headers":[{"name":"aCcEpT","value":"Acme request"},{"name":"aCcEpT-EnCoDiNg","value":""}]}),true).await;
    let records = site.records();
    let headers = &records.last().unwrap()["headers"];
    assert_eq!(headers["accept"], "Acme request");
    assert_eq!(headers["accept-encoding"], "");
    fetch(mcp, target, &rich, json!({"url":"/cross"}), false).await;
    let records = other.records();
    let headers = &records.last().unwrap()["headers"];
    assert_eq!(headers["accept"], "Acme context");
    assert_eq!(headers["accept-encoding"], "identity");
    assert_eq!(headers["x-acme"], "Acme private");
    for http in [&rich, &http] {
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":http}),
        )
        .await;
    }
    json!({"freshCliCreateMs":fresh_ms,"retainedWorkflowWithWireInspectionMs":start.elapsed().as_secs_f64()*1000.0})
}
#[tokio::test]
async fn installed_step_four_http_defaults_checkpoint() {
    let other = wire::Site::start(None).await;
    let site = wire::Site::start(Some(other.url.clone())).await;
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
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
                &ready["browser"]["selectedContext"],
                engine,
                &site,
                &other,
            ))
            .catch_unwind()
            .await;
            let start = std::time::Instant::now();
            performance_lifecycle::disconnect_with_diagnostics(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            let cleanup_ms = start.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(metrics) => eprintln!(
                    "HTTP defaults {engine} {mode}: {metrics}; connectMs={connect_ms}; cleanupMs={cleanup_ms}"
                ),
                Err(panic) => {
                    fixture.cli(&["disconnect"]).await;
                    mcp.tool("disconnect", json!({})).await;
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
}
