//! Browser-level proxy fallback is measured independently before implementation.
use super::context_proxy::{Origin, basic, proxy_records};
use super::http_proxy::tunnel;
use super::*;

#[tokio::test]
async fn launch_proxy_fixture_refuses_coalesced_background_body_and_keeps_selected_origin() {
    let origin = Origin::start().await;
    let proxy = tunnel::Proxy::start_forwarding(vec![origin.port], None).await;
    let endpoint = url::Url::parse(&proxy.url).unwrap();
    let mut background = tokio::net::TcpStream::connect(("127.0.0.1", endpoint.port().unwrap()))
        .await
        .unwrap();
    background.write_all(b"POST http://example.com/Acme HTTP/1.1\r\nHost: example.com\r\nContent-Length: 4\r\n\r\nAcme").await.unwrap();
    let mut refused = Vec::new();
    background.read_to_end(&mut refused).await.unwrap();
    assert!(refused.starts_with(b"HTTP/1.1 403 Forbidden\r\n"));
    assert!(proxy.requests().is_empty());
    assert!(origin.records().is_empty());
    let mut selected = tokio::net::TcpStream::connect(("127.0.0.1", endpoint.port().unwrap()))
        .await
        .unwrap();
    selected
        .write_all(
            format!(
                "GET {}/page HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
                origin.url, origin.port
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut received = Vec::new();
    selected.read_to_end(&mut received).await.unwrap();
    assert!(received.starts_with(b"HTTP/1.1 200"));
    assert_eq!(proxy.requests().len(), 1);
    assert_eq!(origin.records().len(), 1);
}

fn launch_settings(proxy: &tunnel::Proxy) -> Value {
    json!({"server":format!("{}/ignored?Acme=1",proxy.url.replacen("http://","http://AcmeIgnored:example@",1)),"username":"Acme global","password":"global example"})
}
fn context_settings(name: &str, proxy: &tunnel::Proxy) -> Value {
    let mut options = json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,
        "httpCredentials":{"username":"Acme origin","password":"origin example"},"extraHTTPHeaders":[{"name":"X-Acme","value":"context"}]});
    if name == "override" || name == "bypass" {
        options["proxy"] =
            json!({"server":proxy.url,"username":"Acme context","password":"context example"});
        if name == "bypass" {
            options["proxy"]["bypass"] = json!(" 127.0.0.1 , localhost ");
        }
    }
    options
}
fn facts(origin: &Origin, global: &tunnel::Proxy, overridden: &tunnel::Proxy) -> Value {
    json!({"origin":origin.records(),"globalProxy":proxy_records(global),"contextProxy":proxy_records(overridden)})
}
async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    tag: &str,
    context: &Value,
    page: &Value,
    origin: &Origin,
    global: &tunnel::Proxy,
    overridden: &tunnel::Proxy,
    expected: &Value,
) {
    let before = facts(origin, global, overridden);
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/page",origin.url)}})).await;
    let mut clients = Vec::new();
    for explicit in [false, true] {
        let mut options = json!({"browserContext":context,"baseUrl":origin.url,"allowUnknownCookiePartitions":engine=="firefox"});
        if explicit {
            options["proxy"] = if expected["name"] == "initial"
                || expected["name"] == "later"
                || expected["name"] == "global_bypass"
            {
                json!({"server":overridden.url,"username":"Acme context","password":"context example"})
            } else {
                json!({"server":global.url,"username":"Acme global","password":"global example"})
            };
        }
        let client = operation(
            mcp,
            target,
            json!({"operation":"http_context_create","options":options}),
        )
        .await["httpContext"]
            .clone();
        let response = operation(
            mcp,
            target,
            json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/auth"}}),
        )
        .await;
        assert_eq!(response["status"], 200, "{engine}/{mode}/{tag}: {response}");
        operation(
            mcp,
            target,
            json!({"operation":"http_response_remove","requestId":response["requestId"]}),
        )
        .await;
        clients.push(client);
    }
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":context}),
    )
    .await;
    let closed = facts(origin, global, overridden);
    for client in clients {
        collectors::refused(
            mcp,
            target,
            json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/auth"}}),
            // Pinned BrowserContext close disposes its request context first.
            "HTTP_CONTEXT_DISPOSED",
            true,
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":client}),
        )
        .await;
    }
    let actual = facts(origin, global, overridden);
    assert_eq!(
        actual, closed,
        "Closed source requests must not reach an endpoint"
    );
    let mut observed = json!({});
    for field in ["origin", "globalProxy", "contextProxy"] {
        observed[field] =
            json!(&actual[field].as_array().unwrap()[before[field].as_array().unwrap().len()..]);
    }
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("launch-proxy-wire-{engine}-{mode}-{tag}.json")),
        serde_json::to_vec_pretty(&observed).unwrap(),
    )
    .unwrap();
    for field in ["origin", "globalProxy", "contextProxy"] {
        assert_eq!(
            observed[field], expected[field],
            "{engine}/{mode}/{tag}: {field}"
        );
    }
}

#[tokio::test]
async fn installed_step_four_launch_proxy_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let reference = reference(&fixture, engine, mode).await;
            let origin = Origin::start().await;
            let global = tunnel::Proxy::start_forwarding(
                vec![origin.port],
                Some(basic("Acme global", "global example")),
            )
            .await;
            let overridden = tunnel::Proxy::start_forwarding(
                vec![origin.port],
                Some(basic("Acme context", "context example")),
            )
            .await;
            for variant in ["default", "initial-override", "global-bypass"] {
                let initial_override = variant == "initial-override";
                let global_bypass = variant == "global-bypass";
                let mut launch = launch_settings(&global);
                if global_bypass {
                    launch["bypass"] = json!(" 127.0.0.1 , localhost ");
                }
                let options = context_settings(
                    if initial_override {
                        "override"
                    } else {
                        "initial"
                    },
                    &overridden,
                );
                let legacy = engine == "chrome" && mode == "headless" && variant == "default";
                let connected = if legacy {
                    fixture
                        .cli(&[
                            "browser",
                            "connect",
                            "--engine",
                            engine,
                            "--mode",
                            mode,
                            "--proxy",
                            &launch.to_string(),
                        ])
                        .await
                } else if mode == "headed" && variant == "default" {
                    fixture
                        .cli(&[
                            "connect",
                            "--engine",
                            engine,
                            "--mode",
                            mode,
                            "--proxy",
                            &launch.to_string(),
                            "--context",
                            &options.to_string(),
                        ])
                        .await
                } else {
                    mcp.tool(
                        if variant != "default" {
                            "connect"
                        } else {
                            "browser_connect"
                        },
                        json!({"engine":engine,"mode":mode,"proxy":launch,"context":options}),
                    )
                    .await
                };
                let target = connected["targetId"].as_str().unwrap();
                let ready = fixture.ready_target(target).await;
                let result=std::panic::AssertUnwindSafe(async {
                    if legacy {
                        operation(&mut mcp,target,json!({"operation":"context_configure","context":ready["browser"]["selectedContext"],"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"context"}]}})).await;
                    }
                    let names=if initial_override {vec!["override"]} else if global_bypass {vec!["global_bypass"]} else {vec!["initial","override","bypass","later"]};
                    for (index,name) in names.iter().enumerate() {
                        let (context,page)=if index==0 {(ready["browser"]["selectedContext"].clone(),ready["browser"]["selectedPage"].clone())}
                            else {
                                let context=operation(&mut mcp,target,json!({"operation":"context_create","options":context_settings(name,&overridden)})).await["context"].clone();
                                let page=operation(&mut mcp,target,json!({"operation":"page_create","context":context,"select":false})).await["page"].clone();
                                (context,page)
                            };
                        let expected=reference["cases"].as_array().unwrap().iter().find(|row|row["name"]==*name).unwrap();
                        let tag=if initial_override {"initial-override"} else {name};
                        exercise(&fixture,&mut mcp,target,engine,mode,tag,&context,&page,&origin,&global,&overridden,expected).await;
                    }
                }).catch_unwind().await;
                if result.is_err() {
                    let status = mcp.tool("browser_status", json!({"targetId":target})).await;
                    std::fs::write(
                        fixture.workspace.path().join(format!(
                            "launch-proxy-failure-{engine}-{mode}-{variant}.json"
                        )),
                        serde_json::to_vec_pretty(
                            &json!({"status":status,"wire":facts(&origin,&global,&overridden)}),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                }
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
                    failures.push(format!("{engine}/{mode}/{variant}: {closed}"));
                }
                if let Err(panic) = result {
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
            }
            eprintln!(
                "Launch proxy {engine}/{mode}: default, override, context/global bypass, later and initial override settled"
            );
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let result = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-launch-proxy-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(fixture.cache.join("browsers"))
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
            .join(format!("launch-proxy-sdk-{engine}-{mode}.json")),
        &result.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("launch-proxy-sdk-{engine}-{mode}.stderr")),
        &result.stderr,
    )
    .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["ownedProcesses"], 0);
    assert_eq!(report["cases"].as_array().unwrap().len(), 5);
    for row in report["cases"].as_array().unwrap() {
        assert!(row["error"].is_null(), "{engine}/{mode}: {row}");
        for field in ["pageStatus", "fetchStatus", "overrideStatus"] {
            assert_eq!(row[field], 200, "{engine}/{mode}: {row}");
        }
        assert_eq!(row["closedRejected"], true, "{engine}/{mode}: {row}");
        assert_eq!(
            row["origin"].as_array().unwrap().len(),
            5,
            "{engine}/{mode}: {row}"
        );
    }
    report
}

#[tokio::test]
async fn step_four_launch_proxy_reference_diagnostic() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let report = reference(&fixture, engine, mode).await;
            eprintln!(
                "Pinned launch proxy {engine}/{mode}: {} scenarios recorded",
                report["cases"].as_array().unwrap().len()
            );
        }
    }
}
