//! CONNECT refusal bodies and origin isolation use an unchanged SDK reference.
use super::*;
#[path = "../../../fixtures/browser_http_proxy_refusal.rs"]
mod endpoint;

async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    source: &Value,
    reference: &Value,
) {
    let cases = json!([
        {"name":"authentication","scheme":"http","status":407,"headers":"Proxy-Authenticate: Basic realm=\"Acme\"\r\n"},
        {"name":"https-post","scheme":"https","status":502,"post":true},
        {"name":"redirect-disabled","scheme":"https","status":301,"headers":"Location: /Acme-next\r\n","maxRedirects":0},
        {"name":"status-refusal","scheme":"http","status":503,"failOnStatusCode":true},
        {"name":"created","scheme":"https","status":201},
        {"name":"empty","scheme":"http","status":204,"body":""},
        {"name":"redirect","scheme":"http","status":302,"headers":"Location: /Acme-next\r\n","next":403},
        {"name":"chunked","scheme":"https","status":502,"chunked":true},
        {"name":"head","scheme":"https","status":502,"head":true},
        {"name":"head-body","scheme":"https","status":502,"head":true,"invalidHead":true}
    ]);
    for associated in [false, true] {
        for spec in cases.as_array().unwrap() {
            let name = spec["name"].as_str().unwrap();
            let expected = reference["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["name"] == name)
                .unwrap();
            let mut replies = vec![endpoint::response(
                spec["status"].as_u64().unwrap() as u16,
                spec["headers"].as_str().unwrap_or(""),
                spec["body"].as_str().unwrap_or("Acme"),
            )];
            if spec["head"] == true && spec["invalidHead"] != true {
                replies[0] =
                    b"HTTP/1.1 502 Acme refusal\r\nContent-Length: 4\r\nConnection: close\r\n\r\n"
                        .to_vec();
            }
            if spec["chunked"] == true {
                replies[0]=b"HTTP/1.1 502 Acme refusal\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\nAcme\r\n0\r\n\r\n".to_vec();
            }
            if let Some(next) = spec["next"].as_u64() {
                replies.push(endpoint::response(next as u16, "", "Acme"));
            }
            let proxy = endpoint::Refusal::start(replies).await;
            let url = proxy.url(spec["scheme"].as_str().unwrap(), "/Acme");
            let mut options = json!({"baseUrl":url,"proxy":{"server":proxy.proxy,"username":"Acme proxy","password":"proxy example"},"headers":[{"name":"Authorization","value":"Bearer Acme origin secret"},{"name":"X-Acme","value":"origin secret"},{"name":"Cookie","value":"Acme=private"}]});
            if associated {
                options["browserContext"] = source.clone();
                options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
            }
            let client = operation(
                mcp,
                target,
                json!({"operation":"http_context_create","options":options}),
            )
            .await["httpContext"]
                .clone();
            let mut request = json!({"url":url,"readBody":true,"maxRetries":2});
            for key in ["maxRedirects", "failOnStatusCode"] {
                if !spec[key].is_null() {
                    request[key] = spec[key].clone();
                }
            }
            if spec["post"] == true {
                request["method"] = json!("POST");
                request["body"] = json!({"kind":"text","text":"Acme private body"});
            }
            if spec["head"] == true {
                request["method"] = json!("HEAD");
            }
            let command = json!({"operation":"http_fetch","httpContext":client,"request":request});
            if spec["invalidHead"] == true {
                let submitted = mcp
                    .tool("browser_send", json!({"targetId":target,"request":command}))
                    .await;
                let status=mcp.tool("browser_status",json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000})).await;
                std::fs::write(
                    fixture.workspace.path().join(format!(
                        "http-proxy-refusal-{engine}-{mode}-head-body-outcome-{associated}.json"
                    )),
                    serde_json::to_vec_pretty(&status["operation"]).unwrap(),
                )
                .unwrap();
                let receipt = &status["operation"];
                assert_eq!(receipt["settled"], true);
                assert_eq!(receipt["outcome"], "unknown");
                assert_eq!(receipt["code"], "HTTP_REQUEST_OUTCOME_UNKNOWN");
                assert_eq!(receipt["result"]["failureKind"], "io");
                assert_eq!(receipt["result"]["connectionReset"], false);
                assert_eq!(receipt["result"]["replayed"], false);
                assert_eq!(receipt["result"]["retryCount"], 0);
            } else if spec["failOnStatusCode"] == true {
                collectors::refused(mcp, target, command, "HTTP_STATUS", true).await;
            } else {
                let response = operation(mcp, target, command).await;
                assert_eq!(
                    response["status"], expected["status"],
                    "{engine}/{mode}/{name}/{associated}: {response}"
                );
                let details = operation(
                    mcp,
                    target,
                    json!({"operation":"http_response_read","requestId":response["requestId"]}),
                )
                .await;
                assert_eq!(details["wireMetadataAvailable"], true);
                assert_eq!(details["statusText"], expected["statusText"]);
                assert_eq!(details["headers"], expected["headers"]);
                assert_eq!(
                    url::Url::parse(response["url"].as_str().unwrap())
                        .unwrap()
                        .path(),
                    expected["url"].as_str().unwrap()
                );
                assert_eq!(response["retryCount"], 0);
                assert_eq!(response["replayed"], false);
                let body=operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":response["body"]["evidenceId"],"format":"text"})).await;
                assert_eq!(body["value"], expected["body"]);
                operation(mcp,target,json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]})).await;
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
                json!({"operation":"http_context_remove","httpContext":client}),
            )
            .await;
            let connections = proxy
                .settled(expected["connections"].as_array().unwrap().len())
                .await;
            std::fs::write(
                fixture.workspace.path().join(format!(
                    "http-proxy-refusal-{engine}-{mode}-{name}-{associated}.json"
                )),
                serde_json::to_vec_pretty(&connections).unwrap(),
            )
            .unwrap();
            assert_eq!(
                connections,
                expected["connections"].as_array().unwrap().clone(),
                "{engine}/{mode}/{name}/{associated}"
            );
        }
    }
}

#[tokio::test]
async fn installed_step_four_http_proxy_refusal_checkpoint() {
    let fixture = installed_fixture();
    let reference = reference(&fixture).await;
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
                engine,
                mode,
                &ready["browser"]["selectedContext"],
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
            eprintln!(
                "CONNECT refusal {engine}/{mode}: isolated and associated response/isolation/status/redirect contracts settled"
            );
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}

async fn reference(fixture: &Fixture) -> Value {
    let result = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-http-proxy-refusal-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .output()
    .await
    .unwrap();
    std::fs::write(
        fixture.workspace.path().join("http-proxy-refusal-sdk.json"),
        &result.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join("http-proxy-refusal-sdk.stderr"),
        &result.stderr,
    )
    .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["originConnections"], 0);
    assert_eq!(report["cases"].as_array().unwrap().len(), 10);
    for row in report["cases"].as_array().unwrap() {
        if row["name"] == "head-body" {
            assert!(
                row["error"]
                    .as_str()
                    .unwrap()
                    .contains("Data after `Connection: close`")
            );
        } else if row["name"] == "status-refusal" {
            assert!(row["error"].as_str().unwrap().contains("503 Acme refusal"));
        } else {
            assert!(row["error"].is_null(), "{row}");
            assert_eq!(
                row["body"],
                if row["name"] == "empty" || row["name"] == "head" {
                    ""
                } else {
                    "Acme"
                }
            );
        }
        let connections = row["connections"].as_array().unwrap();
        assert_eq!(
            connections.len(),
            if row["name"] == "redirect" { 2 } else { 1 }
        );
        for connection in connections {
            assert_eq!(connection["trailingBytes"], 0);
            assert_eq!(connection["headers"].as_object().unwrap().len(), 3);
            assert_eq!(connection["headers"]["proxy-connection"], "close");
            assert_eq!(connection["headers"]["host"], "127.0.0.1:PORT");
        }
    }
    report
}

#[tokio::test]
async fn step_four_http_proxy_refusal_reference_diagnostic() {
    let fixture = installed_fixture();
    let report = reference(&fixture).await;
    eprintln!(
        "Pinned CONNECT refusal: {} scenarios; origin connections {}",
        report["cases"].as_array().unwrap().len(),
        report["originConnections"]
    );
}
