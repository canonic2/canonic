//! Installed redirect ownership compared with unchanged pinned APIRequest wire effects.
use super::*;
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod wire;

fn headers() -> Value {
    json!({"Authorization":"Acme-private","Proxy-Authorization":"Acme-proxy-private","Cookie":"caller=Acme-private","Host":"acme.invalid","User-Agent":"Acme-redirect-agent","Accept":"Acme-accept","Accept-Encoding":"identity","X-Acme":"Acme-value","X-Empty":""})
}
fn seed() -> Value {
    json!({"cookies":[{"name":"jar","value":"Acme-jar","domain":"127.0.0.1","path":"/","expires":-1,"httpOnly":false,"secure":false,"sameSite":"Lax"}]})
}
fn selected(row: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for name in [
        "authorization",
        "proxy-authorization",
        "cookie",
        "host",
        "user-agent",
        "accept",
        "accept-encoding",
        "x-acme",
        "x-empty",
    ] {
        if let Some(value) = row["headers"].get(name) {
            out.insert(name.into(), value.clone());
        }
    }
    Value::Object(out)
}
async fn baseline(fixture: &Fixture, site: &wire::Site) -> Value {
    let result = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-http-redirect-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(&site.url)
    .arg(json!({"extraHTTPHeaders":headers(),"storageState":seed()}).to_string())
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PW_LANG_NAME", "javascript")
    .output()
    .await
    .unwrap();
    std::fs::write(
        fixture.workspace.path().join("redirect-sdk-reference.json"),
        &result.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join("redirect-sdk-reference.stderr"),
        &result.stderr,
    )
    .unwrap();
    assert!(
        result.status.success(),
        "SDK redirect reference: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    site: &wire::Site,
    other: &wire::Site,
    reference: &Value,
) {
    let headers: Vec<Value> = headers()
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, value)| json!({"name":name,"value":value}))
        .collect();
    let request = json!({"operation":"http_context_create","options":{"baseUrl":site.url,"headers":headers,"storageState":seed()}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let context = completed(mcp, target, submitted).await["httpContext"].clone();
    for (name, path, shared) in [("same", "/rewrite/307", false), ("cross", "/cross", true)] {
        let before = site.records().len();
        let action = json!({"operation":"http_fetch","httpContext":context,"request":{"url":path}});
        let response = if shared {
            let mut action = action;
            action.as_object_mut().unwrap().remove("operation");
            action["action"] = json!("http_fetch");
            let submitted = mcp
                .tool("act", json!({"targetId":target,"actions":[action]}))
                .await;
            completed(mcp, target, submitted).await["results"][0].clone()
        } else {
            operation(mcp, target, action).await
        };
        assert_eq!(response["status"], 200);
        assert_eq!(response["redirects"], 1);
        assert_eq!(response["authenticationRetries"], 0);
        assert_eq!(response["replayed"], false);
        let rows = site.records();
        assert_eq!(rows[before]["headers"]["host"], "acme.invalid");
        assert_eq!(rows[before]["headers"]["cookie"], "caller=Acme-private");
        let rows = if shared { other.records() } else { rows };
        assert_eq!(selected(rows.last().unwrap()), reference[name]);
        operation(
            mcp,
            target,
            json!({"operation":"http_response_remove","requestId":response["requestId"]}),
        )
        .await;
    }
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":context}),
    )
    .await;
}
#[tokio::test]
async fn installed_step_four_redirect_headers_checkpoint() {
    let fixture = installed_fixture();
    let site_other = wire::Site::start(None).await;
    let site = wire::Site::start(Some(site_other.url.clone())).await;
    let reference = baseline(&fixture, &site).await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(run(
                &fixture,
                &mut mcp,
                target,
                &site,
                &site_other,
                &reference,
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
