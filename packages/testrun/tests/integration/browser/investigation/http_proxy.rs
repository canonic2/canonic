//! Installed CONNECT routing and credential ownership against unchanged APIRequest.
use super::*;
#[path = "../../../fixtures/browser_http_proxy.rs"]
pub(super) mod tunnel;
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod wire;

struct Case {
    name: &'static str,
    url: String,
    proxy: Value,
    count: usize,
}
fn selected(row: &Value) -> Value {
    json!({"host":row["headers"]["host"],"proxyAuthorization":row["headers"]["proxy-authorization"]})
}
async fn baseline(fixture: &Fixture, case: &Case) -> Value {
    let proxy = if case.proxy.is_string() {
        json!({"server":case.proxy})
    } else {
        case.proxy.clone()
    };
    let result = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-http-proxy-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(&case.url)
    .arg(json!({"proxy":proxy}).to_string())
    .arg(case.count.to_string())
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PW_LANG_NAME", "javascript")
    .output()
    .await
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("proxy-sdk-{}.json", case.name)),
        &result.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("proxy-sdk-{}.stderr", case.name)),
        &result.stderr,
    )
    .unwrap();
    assert!(
        result.status.success(),
        "{}: {}",
        case.name,
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    cases: &[Case],
    references: &[Value],
    proxy: &tunnel::Proxy,
) {
    for (case, reference) in cases.iter().zip(references) {
        let before = proxy.requests().len();
        let create = json!({"operation":"http_context_create","options":{"proxy":case.proxy}});
        let created = match case.name {
            "uri" => {
                let submitted = fixture
                    .cli(&["browser", "send", "--target", target, &create.to_string()])
                    .await;
                completed(mcp, target, submitted).await
            }
            "object" => {
                let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_context_create","options":{"proxy":case.proxy}}]})).await;
                completed(mcp, target, submitted).await["results"][0].clone()
            }
            _ => operation(mcp, target, create).await,
        };
        let context = created["httpContext"].clone();
        let mut observed = vec![];
        for _ in 0..case.count {
            let response=operation(mcp,target,json!({"operation":"http_fetch","httpContext":context,"request":{"url":case.url,"readBody":true}})).await;
            assert_eq!(response["status"], 200, "{}: {response}", case.name);
            assert_eq!(response["replayed"], false);
            let body=operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":response["body"]["evidenceId"],"format":"text"})).await;
            let row: Value = serde_json::from_str(body["value"].as_str().unwrap()).unwrap();
            observed.push(selected(&row));
            operation(mcp,target,json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]})).await;
            operation(
                mcp,
                target,
                json!({"operation":"http_response_remove","requestId":response["requestId"]}),
            )
            .await;
        }
        assert_eq!(json!(observed), reference["origin"], "{}", case.name);
        assert_eq!(
            json!(&proxy.requests()[before..]),
            reference["connect"],
            "{}",
            case.name
        );
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":context}),
        )
        .await;
    }
    let tls = tunnel::UntrustedTls::start().await;
    for default in [false, true] {
        let context=operation(mcp,target,json!({"operation":"http_context_create","options":{"proxy":{"server":tls.url,"username":"Acme-proxy-private","password":"Acme-proxy-secret"},"ignoreHttpsErrors":default}})).await["httpContext"].clone();
        for selected in [None, Some(false), Some(true)] {
            let mut request = json!({"url":"http://127.0.0.1:9"});
            if let Some(selected) = selected {
                request["ignoreHttpsErrors"] = json!(selected);
            }
            let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_fetch","httpContext":context,"request":request}})).await;
            let status=mcp.tool("browser_status",json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000})).await;
            assert_eq!(status["operation"]["settled"], true, "{status}");
            assert_eq!(
                status["operation"]["code"], "HTTP_REQUEST_OUTCOME_UNKNOWN",
                "{status}"
            );
            assert_eq!(
                status["operation"]["result"]["failureKind"], "tls",
                "{status}"
            );
            assert!(!status.to_string().contains("Acme-proxy-private"));
        }
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":context}),
        )
        .await;
    }
    use std::sync::atomic::Ordering;
    assert_eq!(tls.connections.load(Ordering::SeqCst), 6);
    assert_eq!(tls.requests.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn installed_step_four_http_proxy_checkpoint() {
    let fixture = installed_fixture();
    let other = wire::Site::start(None).await;
    let forward = wire::Site::start(Some(other.url.replace("127.0.0.1", "localhost"))).await;
    let reverse = wire::Site::start(Some(other.url.clone())).await;
    let sites = [&other, &forward, &reverse];
    let ports = sites
        .iter()
        .map(|site| url::Url::parse(&site.url).unwrap().port().unwrap())
        .collect();
    let proxy = tunnel::Proxy::start(ports, None).await;
    let authority = proxy.url.strip_prefix("http://").unwrap();
    let cases = [
        Case {
            name: "uri",
            url: format!("{}/proxy-keepalive", other.url),
            proxy: json!(format!(
                "http://Acme%3Auser:private%40pass@{authority}/ignored"
            )),
            count: 2,
        },
        Case {
            name: "object",
            url: format!("{}/proxy-keepalive", other.url),
            proxy: json!({"server":proxy.url,"username":"Acme-\u{e9}","password":"private%40pass"}),
            count: 2,
        },
        Case {
            name: "replace",
            url: format!("{}/echo", other.url),
            proxy: json!({"server":format!("http://old:private@{authority}"),"username":"Acme-new"}),
            count: 1,
        },
        Case {
            name: "empty",
            url: format!("{}/echo", other.url),
            proxy: json!({"server":format!("http://old:private@{authority}"),"username":"","password":"ignored"}),
            count: 1,
        },
        Case {
            name: "forward",
            url: format!("{}/cross", forward.url),
            proxy: json!({"server":proxy.url,"bypass":"localhost","username":"Acme-proxy-private","password":"Acme-private-pass"}),
            count: 1,
        },
        Case {
            name: "reverse",
            url: format!("{}/cross", reverse.url.replace("127.0.0.1", "localhost")),
            proxy: json!({"server":proxy.url,"bypass":"localhost","username":"Acme-proxy-private","password":"Acme-private-pass"}),
            count: 1,
        },
    ];
    let mut references = vec![];
    for case in &cases {
        let before = proxy.requests().len();
        let origin = baseline(&fixture, case).await;
        references.push(json!({"origin":origin,"connect":&proxy.requests()[before..]}));
    }
    // Keep the independent origin fixture alive and assert the reference was bounded.
    assert_eq!(other.records().len(), 8);
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
                &cases,
                &references,
                &proxy,
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
