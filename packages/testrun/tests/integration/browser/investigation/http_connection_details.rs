//! Final-hop connection facts and retained private details against the pinned SDK.
use super::client_certificates::wire as tls;
use super::socks_fixture as socks;
use super::*;
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod plain;
#[path = "../../../fixtures/browser_http_proxy_refusal.rs"]
mod refusal;
#[path = "../../../fixtures/browser_http_connection_tunnel.rs"]
mod tunnel;

async fn reference(
    fixture: &Fixture,
    engine: &str,
    mode: &str,
    http: &plain::Site,
    secure: &tls::Site,
    proxy: &refusal::Refusal,
    tunnel: &tunnel::Tunnel,
    socks: &socks::Site,
) -> Value {
    let input = fixture
        .workspace
        .path()
        .join(format!("http-connection-input-{engine}-{mode}.json"));
    std::fs::write(
        &input,
        serde_json::to_vec(&json!({"http":http.url,"tls":secure.url,
        "proxy":proxy.proxy,"refusalUrl":proxy.url("https","/Acme"),"tunnel":tunnel.url,
        "socks":socks.server("socks5",""),"socksUrl":socks.url("https","localhost","/Acme")}))
        .unwrap(),
    )
    .unwrap();
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-http-connection-details-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(&input)
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PW_LANG_NAME", "javascript")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .output()
    .await
    .unwrap();
    for (suffix, bytes) in [("json", &output.stdout), ("stderr", &output.stderr)] {
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("http-connection-sdk-{engine}-{mode}.{suffix}")),
            bytes,
        )
        .unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ownedProcesses"], 0);
    assert_eq!(report["refusal"]["status"], 403);
    report
}

#[tokio::test]
async fn pinned_http_connection_details_reference_checkpoint() {
    let fixture = installed_fixture();
    let keys = tls::Identities::new().await;
    let http = plain::Site::start(None).await;
    let secure = tls::Site::start(&keys).await;
    secure.redirect_to(&format!("{}/echo", http.url));
    let proxy = refusal::Refusal::start(vec![refusal::response(403, "", "Acme"); 6]).await;
    let socks = socks::Site::start(socks::Mode::Forward, true).await;
    let tunnel = tunnel::Tunnel::start([
        url::Url::parse(&http.url).unwrap().port().unwrap(),
        url::Url::parse(&secure.url).unwrap().port().unwrap(),
    ])
    .await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let report = reference(
                &fixture, engine, mode, &http, &secure, &proxy, &tunnel, &socks,
            )
            .await;
            eprintln!(
                "Connection metadata SDK {engine}/{mode}: refusal={}",
                report["refusal"]
            );
            eprintln!(
                "Connection metadata SDK {engine}/{mode}: tunnel={}",
                report["tunnel"]
            );
            eprintln!(
                "Connection metadata SDK {engine}/{mode}: socks={}",
                report["socks"]
            );
        }
    }
    assert_eq!(proxy.settled(6).await.len(), 6);
    tunnel.settled(12).await;
    assert_eq!(
        socks.settled(6).await["connections"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
}

async fn create_client(mcp: &mut Mcp, target: &str, options: Value) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"http_context_create","options":options}),
    )
    .await["httpContext"]
        .clone()
}
async fn fetched(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    client: &Value,
    url: &str,
    read_body: bool,
    via: usize,
) -> (Value, Value) {
    let request = json!({"url":url,"readBody":read_body});
    let response = match via {
        0 => {
            let submitted = fixture
                .cli(&[
                    "browser",
                    "send",
                    "--target",
                    target,
                    &json!({"operation":"http_fetch","httpContext":client,"request":request})
                        .to_string(),
                ])
                .await;
            completed(mcp, target, submitted).await
        }
        1 => {
            operation(
                mcp,
                target,
                json!({"operation":"http_fetch","httpContext":client,"request":request}),
            )
            .await
        }
        _ => {
            let submitted = mcp
                .tool(
                    "act",
                    json!({"targetId":target,"actions":[{
                "action":"http_fetch","httpContext":client,"request":request}]}),
                )
                .await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
    };
    assert_eq!(response["replayed"], false);
    assert_eq!(response["retryCount"], 0);
    let details = operation(
        mcp,
        target,
        json!({"operation":"http_response_read","requestId":response["requestId"]}),
    )
    .await;
    if read_body {
        assert!(response["body"]["evidenceId"].is_string());
        operation(
            mcp,
            target,
            json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]}),
        )
        .await;
    } else {
        assert!(response["body"].is_null());
    }
    (response, details)
}
fn timing(details: &Value, complete: bool) {
    let timing = &details["timing"];
    assert!(timing["startTime"].as_f64().unwrap() > 1_000_000_000_000.0);
    for field in [
        "domainLookupStart",
        "domainLookupEnd",
        "connectStart",
        "secureConnectionStart",
        "connectEnd",
        "requestStart",
        "responseStart",
        "responseEnd",
    ] {
        let number = timing[field].as_f64().expect(field);
        assert!(number == -1.0 || number >= 0.0, "{field}: {timing}");
    }
    assert!(timing["responseStart"].as_f64().unwrap() >= 0.0);
    if complete {
        assert!(
            timing["responseEnd"].as_f64().unwrap() >= timing["responseStart"].as_f64().unwrap()
        );
    } else {
        assert_eq!(timing["responseEnd"], -1.0);
    }
}
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    context: &Value,
    http: &plain::Site,
    secure: &tls::Site,
    proxy: &refusal::Refusal,
    tunnel: &tunnel::Tunnel,
    socks: &socks::Site,
    reference: &Value,
) -> Value {
    let mut reports = Vec::new();
    for associated in [false, true] {
        let mut options = json!({"ignoreHttpsErrors":true});
        if associated {
            options["browserContext"] = context.clone();
            options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
        }
        let client = create_client(mcp, target, options).await;
        let expected = &reference["cases"][if associated && engine != "chrome" {
            "associated"
        } else {
            "isolated"
        }];
        let urls = [
            ("http", format!("{}/echo", http.url)),
            ("tls", format!("{}/first", secure.url)),
            ("reused", format!("{}/second", secure.url)),
            ("redirect", format!("{}/redirect", secure.url)),
        ];
        let mut retained = Vec::new();
        for (index, (name, url)) in urls.iter().enumerate() {
            let before = (http.records().len(), secure.requests.lock().unwrap().len());
            let (response, details) =
                fetched(fixture, mcp, target, &client, url, true, index % 3).await;
            assert_eq!(response["status"], 200);
            for field in ["url", "status", "serverAddr", "securityDetails"] {
                assert_eq!(
                    details[field], expected[*name][field],
                    "{name}/{associated}/{field}"
                );
            }
            timing(&details, true);
            let after = (http.records().len(), secure.requests.lock().unwrap().len());
            let delta = match *name {
                "http" => (1, 0),
                "redirect" => (1, 1),
                _ => (0, 1),
            };
            assert_eq!(after, (before.0 + delta.0, before.1 + delta.1));
            if *name == "reused" {
                for field in [
                    "domainLookupStart",
                    "domainLookupEnd",
                    "connectStart",
                    "secureConnectionStart",
                    "connectEnd",
                ] {
                    assert_eq!(details["timing"][field], -1.0, "Reused TLS {field}");
                }
            }
            retained.push((response["requestId"].clone(), details));
        }
        let (response, details) = fetched(
            fixture,
            mcp,
            target,
            &client,
            &format!("{}/echo", http.url),
            false,
            1,
        )
        .await;
        timing(&details, false);
        assert_eq!(details["securityDetails"], Value::Null);
        retained.push((response["requestId"].clone(), details));
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":client}),
        )
        .await;
        for (id, expected) in retained {
            let details = operation(
                mcp,
                target,
                json!({"operation":"http_response_read","requestId":id}),
            )
            .await;
            assert_eq!(
                details, expected,
                "Context removal preserves immutable private details"
            );
            let removed = operation(
                mcp,
                target,
                json!({"operation":"http_response_remove","requestId":id}),
            )
            .await;
            assert_eq!(removed["removed"], true);
            collectors::refused(
                mcp,
                target,
                json!({"operation":"http_response_read","requestId":id}),
                "UNKNOWN_HTTP_RESPONSE_DETAILS",
                true,
            )
            .await;
            reports.push(details);
        }
        let mut options = json!({"proxy":{"server":proxy.proxy}});
        if associated {
            options["browserContext"] = context.clone();
            options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
        }
        let proxy_client = create_client(mcp, target, options).await;
        let (response, details) = fetched(
            fixture,
            mcp,
            target,
            &proxy_client,
            &proxy.url("https", "/Acme"),
            true,
            2,
        )
        .await;
        assert_eq!(response["status"], 403);
        for field in ["serverAddr", "securityDetails"] {
            assert_eq!(
                details[field], reference["refusal"][field],
                "Proxy refusal {field}"
            );
        }
        for field in [
            "domainLookupStart",
            "domainLookupEnd",
            "connectStart",
            "secureConnectionStart",
            "connectEnd",
            "requestStart",
        ] {
            assert_eq!(
                details["timing"][field].as_f64(),
                reference["refusal"]["timing"][field].as_f64(),
                "Synthetic refusal {field}"
            );
        }
        timing(&details, true);
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":proxy_client}),
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"http_response_remove","requestId":response["requestId"]}),
        )
        .await;
        reports.push(details);
    }
    for associated in [false, true] {
        let mut options = json!({"ignoreHttpsErrors":true,"proxy":{"server":tunnel.url}});
        if associated {
            options["browserContext"] = context.clone();
            options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
        }
        let client = create_client(mcp, target, options).await;
        for (name, url) in [
            ("http", format!("{}/echo", http.url)),
            ("tls", format!("{}/tunnel", secure.url)),
        ] {
            let (response, details) = fetched(fixture, mcp, target, &client, &url, true, 1).await;
            for field in ["serverAddr", "securityDetails"] {
                assert_eq!(
                    details[field], reference["tunnel"][name][field],
                    "Positive tunnel {name}/{field}"
                );
            }
            timing(&details, true);
            for field in [
                "domainLookupStart",
                "domainLookupEnd",
                "connectStart",
                "secureConnectionStart",
            ] {
                assert_eq!(
                    details["timing"][field].as_f64(),
                    reference["tunnel"][name]["timing"][field].as_f64(),
                    "Tunnel {name}/{field}"
                );
            }
            for field in ["connectEnd", "requestStart"] {
                assert_eq!(
                    details["timing"][field].as_f64().unwrap() >= 0.0,
                    reference["tunnel"][name]["timing"][field].as_f64().unwrap() >= 0.0,
                    "Tunnel {name}/{field}"
                );
            }
            reports.push(details);
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
    }
    for associated in [false, true] {
        let mut options = json!({"ignoreHttpsErrors":true,"proxy":socks.server("socks5","")});
        if associated {
            options["browserContext"] = context.clone();
            options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
        }
        let client = create_client(mcp, target, options).await;
        let (response, details) = fetched(
            fixture,
            mcp,
            target,
            &client,
            &socks.url("https", "localhost", "/Acme"),
            true,
            2,
        )
        .await;
        for field in ["serverAddr", "securityDetails"] {
            assert_eq!(details[field], reference["socks"][field], "SOCKS {field}");
        }
        for field in [
            "domainLookupStart",
            "domainLookupEnd",
            "connectStart",
            "secureConnectionStart",
        ] {
            assert_eq!(
                details["timing"][field].as_f64(),
                reference["socks"]["timing"][field].as_f64(),
                "SOCKS {field}"
            );
        }
        timing(&details, true);
        reports.push(details);
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":client}),
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"http_response_remove","requestId":response["requestId"]}),
        )
        .await;
    }
    let evidence = operation(mcp, target, json!({"operation":"http_evidence_list"})).await;
    assert!(evidence["pending"].as_array().unwrap().is_empty());
    assert!(evidence["evidence"].as_array().unwrap().is_empty());
    json!(reports)
}
#[tokio::test]
async fn installed_step_four_http_connection_details_checkpoint() {
    let fixture = installed_fixture();
    let keys = tls::Identities::new().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let http = plain::Site::start(None).await;
            let secure = tls::Site::start(&keys).await;
            secure.redirect_to(&format!("{}/echo", http.url));
            let proxy = refusal::Refusal::start(vec![refusal::response(403, "", "Acme"); 3]).await;
            let socks = socks::Site::start(socks::Mode::Forward, true).await;
            let tunnel = tunnel::Tunnel::start([
                url::Url::parse(&http.url).unwrap().port().unwrap(),
                url::Url::parse(&secure.url).unwrap().port().unwrap(),
            ])
            .await;
            let expected = reference(
                &fixture, engine, mode, &http, &secure, &proxy, &tunnel, &socks,
            )
            .await;
            let connected = mcp
                .tool(
                    if mode == "headless" {
                        "browser_connect"
                    } else {
                        "connect"
                    },
                    json!({"engine":engine,"mode":mode}),
                )
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture,
                &mut mcp,
                target,
                engine,
                &ready["browser"]["selectedContext"],
                &http,
                &secure,
                &proxy,
                &tunnel,
                &socks,
                &expected,
            ))
            .catch_unwind()
            .await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("http-connection-installed-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(
                    &json!({"reference":expected,"details":result.as_ref().ok(),
                    "http":http.records(),"tls":*secure.requests.lock().unwrap()}),
                )
                .unwrap(),
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
                failures.push(closed);
            }
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            assert_eq!(proxy.settled(3).await.len(), 3);
            tunnel.settled(6).await;
            assert_eq!(
                socks.settled(3).await["connections"]
                    .as_array()
                    .unwrap()
                    .len(),
                3
            );
            eprintln!(
                "HTTP connection details {engine}/{mode}: TLS, address, timing, reuse, redirect and private lifetime"
            );
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original disconnect failures: {failures:?}"
    );
}
