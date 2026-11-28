//! SOCKS HTTP semantics are measured against the unchanged pinned SDK first.
use super::socks_fixture as endpoint;
use super::*;

async fn receipt(mcp: &mut Mcp, target: &str, request: Value) -> Value {
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":request}))
        .await;
    let result = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(result["operation"]["settled"], true, "{result}");
    result["operation"].clone()
}

async fn finalize(mcp: &mut Mcp, target: &str, request_id: &Value) -> Value {
    let until = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let result = receipt(
            mcp,
            target,
            json!({"operation":"http_response_finalize","requestId":request_id}),
        )
        .await;
        if result["code"] != "HTTP_RESPONSE_PENDING" {
            return result;
        }
        assert!(
            std::time::Instant::now() < until,
            "Retained request must settle without replay: {result}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

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
        {"name":"socks5","scheme":"socks5"},{"name":"socks5h","scheme":"socks5h"},{"name":"socks","scheme":"socks"},
        {"name":"socks4","scheme":"socks4"},{"name":"socks4a","scheme":"socks4a"},
        {"name":"uri-auth","scheme":"socks5","userinfo":"Acme%3A%C3%A9:p%40ss@"},
        {"name":"object-auth-ignored","scheme":"socks5","objectAuth":true},
        {"name":"socks4-user","scheme":"socks4","userinfo":"Acme%3Auser:ignored@"},
        {"name":"ipv4","scheme":"socks5","host":"127.0.0.1"},
        {"name":"ipv6","scheme":"socks5","host":"[::1]"},{"name":"socks4-ipv6","scheme":"socks4","host":"[::1]"},
        {"name":"forward-bypass","scheme":"socks5","path":"/forward","bypass":"127.0.0.1"},
        {"name":"reverse-bypass","scheme":"socks5","path":"/reverse","host":"127.0.0.1","bypass":"127.0.0.1"},
        {"name":"forced-empty-auth","scheme":"socks5"},
        {"name":"authentication-refused","scheme":"socks5","userinfo":"Acme:example@"},
        {"name":"socks4-connect-refused","scheme":"socks4","userinfo":"Acme:example@"},
        {"name":"socks5-connect-refused","scheme":"socks5","userinfo":"Acme:example@"},
        {"name":"greeting-timeout","scheme":"socks5","userinfo":"Acme:example@"},
        {"name":"tls-omitted","scheme":"socks5"},{"name":"tls-explicit-false","scheme":"socks5","policy":false},
        {"name":"tls-exception","scheme":"socks5","policy":true}
    ]);
    for associated in [false, true] {
        for spec in cases.as_array().unwrap() {
            let name = spec["name"].as_str().unwrap();
            let expected = reference
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["name"] == name)
                .unwrap();
            let proxy_mode = match name {
                "forced-empty-auth" => endpoint::Mode::ForceAuth,
                "authentication-refused" => endpoint::Mode::RefuseAuth,
                "socks4-connect-refused" | "socks5-connect-refused" => {
                    endpoint::Mode::RefuseConnect
                }
                "greeting-timeout" => endpoint::Mode::StallGreeting,
                _ => endpoint::Mode::Forward,
            };
            let tls = name.starts_with("tls-");
            let site = endpoint::Site::start(proxy_mode, tls).await;
            let url = site.url(
                if tls { "https" } else { "http" },
                spec["host"].as_str().unwrap_or("localhost"),
                spec["path"].as_str().unwrap_or("/echo"),
            );
            let mut proxy = json!({"server":site.server(spec["scheme"].as_str().unwrap(), spec["userinfo"].as_str().unwrap_or(""))});
            if let Some(bypass) = spec["bypass"].as_str() {
                proxy["bypass"] = json!(bypass);
            }
            if spec["objectAuth"] == true {
                proxy["username"] = json!("Acme ignored");
                proxy["password"] = json!("ignored example");
            }
            let mut options = json!({"baseUrl":url,"proxy":proxy,"headers":[{"name":"Authorization","value":"Bearer Acme origin"},{"name":"X-Acme","value":"origin"}]});
            if associated {
                options["browserContext"] = source.clone();
                options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
            }
            if !spec["policy"].is_null() {
                options["ignoreHttpsErrors"] = spec["policy"].clone();
            }
            let client = operation(
                mcp,
                target,
                json!({"operation":"http_context_create","options":options}),
            )
            .await["httpContext"]
                .clone();
            let mut request = json!({"url":url,"readBody":true,"maxRetries":2});
            if name == "greeting-timeout" {
                request["timeoutMs"] = json!(80);
            }
            let mut result = receipt(
                mcp,
                target,
                json!({"operation":"http_fetch","httpContext":client,"request":request}),
            )
            .await;
            let initial = result.clone();
            if name == "greeting-timeout" && result["code"] == "HTTP_RESPONSE_PENDING" {
                assert_eq!(result["outcome"], "failed");
                assert_eq!(result["result"]["httpContext"], client);
                assert_eq!(result["result"]["requestRetained"], true);
                assert_eq!(result["result"]["replayed"], false);
                assert_eq!(result["result"]["nextAction"], "http_response_finalize");
                result = finalize(mcp, target, &result["result"]["requestId"]).await;
            }
            if expected["error"].is_string() {
                assert_eq!(
                    result["outcome"], "unknown",
                    "{engine}/{mode}/{name}/{associated}: {result}"
                );
                assert_eq!(result["code"], "HTTP_REQUEST_OUTCOME_UNKNOWN");
                assert_eq!(
                    result["result"]["failureKind"],
                    if tls {
                        "tls"
                    } else if name == "greeting-timeout" {
                        "timeout"
                    } else {
                        "io"
                    }
                );
                assert_eq!(result["result"]["connectionReset"], false);
                assert_eq!(result["result"]["retryCount"], 0);
                assert_eq!(result["result"]["replayed"], false);
            } else {
                assert_eq!(result["outcome"], "succeeded", "{result}");
                let response = &result["result"];
                assert_eq!(response["status"], expected["status"]);
                assert_eq!(response["retryCount"], 0);
                assert_eq!(response["replayed"], false);
                let details = operation(
                    mcp,
                    target,
                    json!({"operation":"http_response_read","requestId":response["requestId"]}),
                )
                .await;
                assert_eq!(details["wireMetadataAvailable"], true);
                let body = operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":response["body"]["evidenceId"],"format":"text"})).await;
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
            let facts = site.settled(1).await;
            std::fs::write(
                fixture.workspace.path().join(format!(
                    "http-socks-{engine}-{mode}-{name}-{associated}.json"
                )),
                serde_json::to_vec_pretty(
                    &json!({"initial":initial,"operation":result,"facts":facts,"prefixes":site.prefixes()}),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                facts["connections"], expected["connections"],
                "{engine}/{mode}/{name}/{associated}"
            );
            assert_eq!(
                facts["origin"], expected["origin"],
                "{engine}/{mode}/{name}/{associated}"
            );
            if tls {
                assert_eq!(site.prefixes(), vec![22]);
            } else if expected["error"].is_string() {
                assert!(site.prefixes().is_empty());
            }
        }
        for scheme in ["socks4", "socks5"] {
            let site = endpoint::Site::start(endpoint::Mode::HoldReply, false).await;
            let url = site.url("http", "localhost", "/echo");
            let mut options = json!({"proxy":{"server":site.server(scheme,"Acme:example@")}});
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
            let submitted = mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_fetch","httpContext":client,"request":{"url":url,"method":"POST","body":{"kind":"text","text":"Acme private body"}}}})).await;
            site.handshake().await;
            let started = std::time::Instant::now();
            mcp.tool(
                "browser_cancel",
                json!({"targetId":target,"operationId":submitted["operation"]["id"]}),
            )
            .await;
            let cancelled = mcp.tool("browser_status",json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":2000})).await;
            assert_eq!(cancelled["operation"]["settled"], true, "{cancelled}");
            assert_eq!(
                cancelled["operation"]["outcome"], "cancelled",
                "{cancelled}"
            );
            assert_eq!(cancelled["operation"]["code"], "CANCELLED");
            assert!(started.elapsed() < Duration::from_secs(2));
            site.grant();
            let facts = site.settled(1).await;
            assert_eq!(facts["origin"], json!([]));
            assert!(site.prefixes().is_empty());
            let finalized =
                finalize(mcp, target, &cancelled["operation"]["result"]["requestId"]).await;
            assert_eq!(
                finalized["code"], "HTTP_REQUEST_OUTCOME_UNKNOWN",
                "{finalized}"
            );
            operation(
                mcp,
                target,
                json!({"operation":"http_context_remove","httpContext":client}),
            )
            .await;
            std::fs::write(fixture.workspace.path().join(format!("http-socks-{engine}-{mode}-{scheme}-cancel-{associated}.json")),serde_json::to_vec_pretty(&json!({"cancelled":cancelled,"finalized":finalized,"facts":facts,"prefixes":site.prefixes()})).unwrap()).unwrap();
        }
    }
}

#[tokio::test]
async fn installed_step_four_http_socks_checkpoint() {
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
                "HTTP SOCKS {engine}/{mode}: 42 isolated/associated reference cases and four held-reply cancellations settled"
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
            .join("tests/fixtures/browser-http-socks-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/browser/http/fixtures"))
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .output()
    .await
    .unwrap();
    std::fs::write(
        fixture.workspace.path().join("http-socks-sdk.json"),
        &result.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture.workspace.path().join("http-socks-sdk.stderr"),
        &result.stderr,
    )
    .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report.as_array().unwrap().len(), 22);
    for row in report.as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        if name.ends_with("refused")
            || name == "greeting-timeout"
            || name == "disposal-cancel"
            || name == "tls-omitted"
            || name == "tls-explicit-false"
        {
            assert!(row["error"].is_string(), "{row}");
            assert!(row["origin"].as_array().unwrap().is_empty());
            assert_eq!(row["connections"].as_array().unwrap().len(), 1);
            if name.starts_with("tls-") {
                assert_eq!(row["prefixes"], json!([22]));
            } else {
                assert_eq!(row["prefixes"], json!([]));
            }
            if name == "greeting-timeout" {
                assert!(row["error"].as_str().unwrap().contains("80ms"));
                assert!(row["elapsedMs"].as_f64().unwrap() < 2000.0);
            }
            if name == "disposal-cancel" {
                assert!(row["elapsedMs"].as_f64().unwrap() < 2000.0);
            }
            if name.ends_with("refused") {
                assert_eq!(
                    row["connections"][0]["rejection"],
                    if name == "authentication-refused" {
                        "authentication"
                    } else {
                        "connect"
                    }
                );
            }
            continue;
        }
        if name == "tls-exception" {
            assert_eq!(row["prefixes"], json!([22]));
        }
        assert!(row["error"].is_null(), "{row}");
        assert_eq!(row["status"], 200);
        assert_eq!(row["body"], "Acme");
        assert_eq!(row["connections"].as_array().unwrap().len(), 1);
        assert_eq!(
            row["origin"].as_array().unwrap().len(),
            if row["name"] == "forward-bypass" || row["name"] == "reverse-bypass" {
                2
            } else {
                1
            }
        );
        for origin in row["origin"].as_array().unwrap() {
            assert_eq!(origin["proxyAuthorization"], "");
            assert_eq!(origin["custom"], "origin");
        }
    }
    report
}

#[tokio::test]
async fn step_four_http_socks_reference_diagnostic() {
    let fixture = installed_fixture();
    let report = reference(&fixture).await;
    eprintln!(
        "Pinned HTTP SOCKS: {} wire/default/authentication/bypass scenarios",
        report.as_array().unwrap().len()
    );
}
