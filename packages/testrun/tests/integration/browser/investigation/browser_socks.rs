//! Browser SOCKS reference establishes creation, address routing and inherited HTTP effects.
use super::socks_fixture as endpoint;
use super::*;

fn settings(site: &endpoint::Site, scheme: &str) -> Value {
    json!({"server":format!("{}/ignored?Acme=1",site.server(scheme,"AcmeIgnored:example@"))})
}
fn context_options(
    origin: &endpoint::Site,
    overridden: &endpoint::Site,
    launch_scheme: &str,
    kind: &str,
) -> Value {
    let other = if launch_scheme == "socks4" {
        "socks5"
    } else {
        "socks4"
    };
    let effective = if kind.starts_with("context_") {
        other
    } else {
        launch_scheme
    };
    let host = if effective == "socks5" && kind != "context_bypass" {
        "acme.invalid"
    } else {
        "127.0.0.1"
    };
    let mut options = json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"baseURL":origin.url("http",host,""),"extraHTTPHeaders":[{"name":"X-Acme","value":"browser"}]});
    if kind.starts_with("context_") {
        options["proxy"] = settings(overridden, other);
    }
    if kind == "context_bypass" {
        options["proxy"]["bypass"] = json!(" 127.0.0.1 ");
    }
    options
}
fn facts(origin: &endpoint::Site, overridden: &endpoint::Site) -> Value {
    json!({"origin":origin.facts()["origin"],"primary":origin.facts()["connections"],"override":overridden.facts()["connections"],"protocolErrors":{"primary":origin.protocol_errors(),"override":overridden.protocol_errors()},"protocolPrefixes":{"primary":origin.protocol_prefixes(),"override":overridden.protocol_prefixes()},"unselectedRefused":{"primary":origin.refused(),"override":overridden.refused()}})
}
fn window(before: &Value, after: &Value) -> Value {
    let mut value = json!({});
    for key in ["origin", "primary", "override"] {
        value[key] =
            json!(&after[key].as_array().unwrap()[before[key].as_array().unwrap().len()..]);
    }
    for field in ["protocolErrors", "protocolPrefixes"] {
        value[field] = json!({});
        for key in ["primary", "override"] {
            value[field][key] = json!(
                &after[field][key].as_array().unwrap()
                    [before[field][key].as_array().unwrap().len()..]
            );
        }
    }
    value
}
fn compare(actual: &Value, expected: &Value) {
    assert_eq!(actual["origin"], expected["origin"], "{actual}");
    assert_eq!(
        actual["protocolPrefixes"], expected["protocolPrefixes"],
        "{actual}"
    );
    assert_eq!(
        actual["protocolErrors"], expected["protocolErrors"],
        "{actual}"
    );
    for key in ["primary", "override"] {
        let packets = |value: &Value| {
            value[key]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["applicationData"] == true)
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(
            packets(actual),
            packets(expected),
            "Application-bearing {key}: {actual}"
        );
        let identity = |row: &Value| {
            let mut row = row.clone();
            row.as_object_mut().unwrap().remove("applicationData");
            row
        };
        let allowed = expected[key]
            .as_array()
            .unwrap()
            .iter()
            .map(identity)
            .collect::<Vec<_>>();
        assert!(actual[key].as_array().unwrap().len() <= 32);
        for idle in actual[key]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["applicationData"] == false)
        {
            assert!(
                allowed.contains(&identity(idle)),
                "Idle handshake changed destination or authentication: {actual}"
            );
        }
    }
}
async fn fetch(mcp: &mut Mcp, target: &str, client: &Value, url: &str) {
    let response=operation(mcp,target,json!({"operation":"http_fetch","httpContext":client,"request":{"url":url,"readBody":true}})).await;
    assert_eq!(response["status"], 200);
    assert_eq!(response["retryCount"], 0);
    assert_eq!(response["replayed"], false);
    let details = operation(
        mcp,
        target,
        json!({"operation":"http_response_read","requestId":response["requestId"]}),
    )
    .await;
    assert_eq!(details["wireMetadataAvailable"], true);
    let body=operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":response["body"]["evidenceId"],"format":"text"})).await;
    assert_eq!(body["value"], "Acme");
    operation(
        mcp,
        target,
        json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"http_response_remove","requestId":response["requestId"]}),
    )
    .await;
}
async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    mode: &str,
    launch_scheme: &str,
    kind: &str,
    context: &Value,
    page: &Value,
    origin: &endpoint::Site,
    overridden: &endpoint::Site,
    expected: &Value,
) {
    let before = facts(origin, overridden);
    let options = context_options(origin, overridden, launch_scheme, kind);
    let base_url = options["baseURL"].clone();
    let navigation = json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/Acme-page",options["baseURL"].as_str().unwrap()),"waitUntil":"load"}});
    if !expected["failure"].is_null() {
        negative_navigation(
            fixture,
            mcp,
            target,
            &format!("{engine}-{mode}-{launch_scheme}-{kind}"),
            navigation,
            origin,
            overridden,
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
    } else {
        operation(mcp, target, navigation).await;
        let text=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"document.querySelector('p').textContent","isFunction":false})).await;
        assert_eq!(text["value"]["value"], "Acme", "{text}");
        let other = if launch_scheme == "socks4" {
            "socks5"
        } else {
            "socks4"
        };
        let mut clients = vec![];
        for explicit in [false, true] {
            let mut options =
                json!({"browserContext":context,"allowUnknownCookiePartitions":engine=="firefox"});
            if launch_scheme == "socks4" && mode == "headless" && kind == "initial" {
                options["baseUrl"] = base_url.clone();
            }
            if explicit {
                options["proxy"] = json!({"server":origin.server(other,"")});
            }
            let client = operation(
                mcp,
                target,
                json!({"operation":"http_context_create","options":options}),
            )
            .await["httpContext"]
                .clone();
            fetch(
                mcp,
                target,
                &client,
                if explicit {
                    "/Acme-override"
                } else {
                    "/Acme-http"
                },
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
        let closed = facts(origin, overridden);
        for client in clients {
            collectors::refused(mcp,target,json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/Acme-after-close"}}),"HTTP_CONTEXT_DISPOSED",true).await;
            operation(
                mcp,
                target,
                json!({"operation":"http_context_remove","httpContext":client}),
            )
            .await;
        }
        let after = facts(origin, overridden);
        std::fs::write(
            fixture.workspace.path().join(format!(
                "browser-socks-{engine}-{mode}-{launch_scheme}-{kind}-source-close.json"
            )),
            serde_json::to_vec_pretty(&json!({"before":closed,"after":after})).unwrap(),
        )
        .unwrap();
        // The live browser may make independently refused background attempts.
        // Preserve those counters, but compare every selected flow and byte flag.
        let mut expected = closed;
        let mut actual = after;
        expected
            .as_object_mut()
            .unwrap()
            .remove("unselectedRefused");
        actual.as_object_mut().unwrap().remove("unselectedRefused");
        assert_eq!(
            actual, expected,
            "Source closure must preserve every selected network effect"
        );
    }
    let observed = window(&before, &facts(origin, overridden));
    std::fs::write(
        fixture.workspace.path().join(format!(
            "browser-socks-{engine}-{mode}-{launch_scheme}-{kind}.json"
        )),
        serde_json::to_vec_pretty(&observed).unwrap(),
    )
    .unwrap();
    compare(&observed, expected);
}

async fn negative_navigation(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    name: &str,
    request: Value,
    origin: &endpoint::Site,
    overridden: &endpoint::Site,
) {
    let before = facts(origin, overridden);
    let submitted = mcp
        .request(
            "tools/call",
            json!({"name":"browser_send","arguments":{"targetId":target,"request":request}}),
        )
        .await;
    // Persist admission before any assertion; a missing operation is evidence too.
    let path = fixture
        .workspace
        .path()
        .join(format!("browser-socks-negative-{name}.json"));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({"submitted":submitted,"before":before})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        submitted["result"]["structuredContent"]["ok"], true,
        "{submitted}"
    );
    let operation = submitted["result"]["structuredContent"]["data"]["operation"]["id"].clone();
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":operation,"waitMs":30000}),
        )
        .await;
    let journal = super::failure_evidence::witness(fixture, target, &status["operation"]);
    std::fs::write(&path,serde_json::to_vec_pretty(&json!({"submitted":submitted,"before":before,"status":status,"journal":journal,"after":facts(origin,overridden)})).unwrap()).unwrap();
    assert_eq!(status["operation"]["settled"], true, "{status}");
    assert_eq!(status["operation"]["outcome"], "failed", "{status}");
    assert_eq!(
        status["operation"]["code"], "BROWSER_METHOD_FAILED",
        "{status}"
    );
}

#[tokio::test]
async fn installed_step_four_browser_socks_negative_diagnostic() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for attempt in 0..3 {
        let origin = endpoint::Site::start(endpoint::Mode::BrowserForward, false).await;
        let overridden = origin.fork_proxy().await;
        let sdk = Command::new(
            fixture
                .cache
                .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
        )
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/browser-socks-negative-probe.cjs"),
        )
        .arg(fixture.cache.join("playwright-core-1.64.1/package"))
        .arg(origin.server("socks4", ""))
        .arg(origin.url("http", "127.0.0.1", "/Acme-page"))
        .current_dir(fixture.workspace.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
        .output()
        .await
        .unwrap();
        let report = json!({"success":sdk.status.success(),"stdout":String::from_utf8_lossy(&sdk.stdout),"stderr":String::from_utf8_lossy(&sdk.stderr),"facts":facts(&origin,&overridden)});
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("browser-socks-negative-sdk-{attempt}.json")),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        assert!(sdk.status.success(), "{report}");
        let sdk_report: Value = serde_json::from_slice(&sdk.stdout).unwrap();
        assert_eq!(sdk_report["ownedProcesses"], 0, "{report}");
        assert_eq!(sdk_report["navigation"]["outcome"], "failed", "{report}");
        assert!(
            sdk_report["navigation"]["message"]
                .as_str()
                .unwrap()
                .contains("The network connection was lost"),
            "{report}"
        );
        let connected=mcp.tool("browser_connect",json!({"engine":"webkit","mode":"headed","proxy":settings(&origin,"socks4"),"context":context_options(&origin,&overridden,"socks4","initial")})).await;
        let target = connected["targetId"].as_str().unwrap();
        let ready = fixture.ready_target(target).await;
        let result=std::panic::AssertUnwindSafe(negative_navigation(&fixture,&mut mcp,target,&format!("diagnostic-{attempt}"),json!({"operation":"page_navigate","page":ready["browser"]["selectedPage"],"navigation":{"kind":"goto","url":origin.url("http","127.0.0.1","/Acme-page"),"waitUntil":"load"}}),&origin,&overridden)).catch_unwind().await;
        let closed = performance_lifecycle::disconnect_observed(
            &fixture,
            &mut mcp,
            target,
            "webkit",
            &format!("headed-negative-{attempt}"),
            &ready["browser"]["selectedContext"],
        )
        .await;
        if result.is_err() {
            failures.push(format!("navigation {attempt}"));
        }
        if closed["result"]["structuredContent"]["ok"] != true {
            failures.push(format!("cleanup {attempt}: {closed}"));
        }
        origin
            .settled(origin.facts()["connections"].as_array().unwrap().len())
            .await;
        overridden
            .settled(overridden.facts()["connections"].as_array().unwrap().len())
            .await;
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original diagnostic failures: {failures:?}"
    );
}

#[tokio::test]
async fn installed_step_four_browser_socks_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let reference = reference(&fixture, engine, mode).await;
            let origin = endpoint::Site::start(endpoint::Mode::BrowserForward, false).await;
            let overridden = origin.fork_proxy().await;
            for scheme in ["socks4", "socks5"] {
                let options = context_options(&origin, &overridden, scheme, "initial");
                let proxy = settings(&origin, scheme);
                let connected = if scheme == "socks4" && mode == "headless" {
                    fixture
                        .cli(&[
                            "browser",
                            "connect",
                            "--engine",
                            engine,
                            "--mode",
                            mode,
                            "--proxy",
                            &proxy.to_string(),
                        ])
                        .await
                } else if scheme == "socks5" && mode == "headed" {
                    fixture
                        .cli(&[
                            "connect",
                            "--engine",
                            engine,
                            "--mode",
                            mode,
                            "--proxy",
                            &proxy.to_string(),
                            "--context",
                            &options.to_string(),
                        ])
                        .await
                } else {
                    mcp.tool(
                        if scheme == "socks4" {
                            "browser_connect"
                        } else {
                            "connect"
                        },
                        json!({"engine":engine,"mode":mode,"proxy":proxy,"context":options}),
                    )
                    .await
                };
                let target = connected["targetId"].as_str().unwrap();
                let ready = fixture.ready_target(target).await;
                let result=std::panic::AssertUnwindSafe(async {
                if scheme=="socks4" && mode=="headless" {
                    operation(&mut mcp,target,json!({"operation":"context_configure","context":ready["browser"]["selectedContext"],"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"browser"}]}})).await;
                }
                for kind in ["initial","context_override","context_bypass","later"] {
                    let (context,page)=if kind=="initial" {(ready["browser"]["selectedContext"].clone(),ready["browser"]["selectedPage"].clone())}
                        else {let context=operation(&mut mcp,target,json!({"operation":"context_create","options":context_options(&origin,&overridden,scheme,kind)})).await["context"].clone();let page=operation(&mut mcp,target,json!({"operation":"page_create","context":context,"select":false})).await["page"].clone();(context,page)};
                    let name=format!("{scheme}-{kind}");let expected=reference["cases"].as_array().unwrap().iter().find(|row|row["name"]==name).unwrap();
                    exercise(&fixture,&mut mcp,target,engine,mode,scheme,kind,&context,&page,&origin,&overridden,expected).await;
                }
                for row in reference["validation"].as_array().unwrap().iter().filter(|row|row["kind"]=="context").take(6) {
                    let mut options=context_options(&origin,&overridden,scheme,"later");let mut proxy=row["credentials"].clone();proxy["server"]=json!(overridden.server(row["scheme"].as_str().unwrap(),""));options["proxy"]=proxy;
                    if row["accepted"]==true {let context=operation(&mut mcp,target,json!({"operation":"context_create","options":options})).await["context"].clone();operation(&mut mcp,target,json!({"operation":"context_close","context":context})).await;}
                    else {
                        let before=operation(&mut mcp,target,json!({"operation":"context_list"})).await;
                        collectors::refused(&mut mcp,target,json!({"operation":"context_create","options":options}),"HTTP_PROXY_AUTH_UNSUPPORTED",false).await;
                        assert_eq!(operation(&mut mcp,target,json!({"operation":"context_list"})).await,before,"Authentication refusal must allocate no context");
                    }
                }
            }).catch_unwind().await;
                let closed = performance_lifecycle::disconnect_observed(
                    &fixture,
                    &mut mcp,
                    target,
                    engine,
                    &format!("{mode}-{scheme}"),
                    &ready["browser"]["selectedContext"],
                )
                .await;
                if closed["result"]["structuredContent"]["ok"] != true {
                    failures.push(format!("{engine}/{mode}/{scheme}: {closed}"));
                }
                if let Err(panic) = result {
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
            }
            for row in reference["validation"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["kind"] == "launch")
            {
                let mut proxy = row["credentials"].clone();
                proxy["server"] = json!(origin.server(row["scheme"].as_str().unwrap(), ""));
                let refused=mcp.request("tools/call",json!({"name":"connect","arguments":{"engine":engine,"mode":mode,"proxy":proxy}})).await;
                assert_eq!(
                    refused["result"]["structuredContent"]["ok"], false,
                    "{refused}"
                );
                assert_eq!(
                    refused["result"]["structuredContent"]["code"], "HTTP_PROXY_AUTH_UNSUPPORTED",
                    "{refused}"
                );
            }
            let first = origin.facts()["connections"].as_array().unwrap().len();
            let second = overridden.facts()["connections"].as_array().unwrap().len();
            origin.settled(first).await;
            overridden.settled(second).await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("browser-socks-{engine}-{mode}-final.json")),
                serde_json::to_vec_pretty(&facts(&origin, &overridden)).unwrap(),
            )
            .unwrap();
            eprintln!(
                "Browser SOCKS {engine}/{mode}: eight reference workflows, sixteen authentication cases and both original disconnects retained"
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
            .join("tests/fixtures/browser-socks-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(fixture.cache.join("browsers"))
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .output()
    .await
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("browser-socks-sdk-{engine}-{mode}.json")),
        &result.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("browser-socks-sdk-{engine}-{mode}.stderr")),
        &result.stderr,
    )
    .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["cases"].as_array().unwrap().len(), 8);
    for row in report["cases"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let webkit_four = engine == "webkit"
            && matches!(
                name,
                "socks4-initial" | "socks4-later" | "socks5-context_override"
            );
        if webkit_four {
            for key in ["primary", "override"] {
                let prefixes = row["protocolPrefixes"][key].as_array().unwrap();
                assert_eq!(
                    prefixes.len(),
                    row["protocolErrors"][key].as_array().unwrap().len()
                );
                assert!(
                    prefixes
                        .iter()
                        .all(|prefix| *prefix == json!([71, 69, 84, 32])),
                    "{row}"
                );
            }
            assert_eq!(row["failure"]["phase"], "page_navigate", "{row}");
            assert!(
                row["failure"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("network connection was lost"),
                "{row}"
            );
            assert_eq!(row["origin"], json!([]));
            assert_eq!(row["primary"], json!([]));
            assert_eq!(row["override"], json!([]));
            assert!(
                row["protocolErrors"]["primary"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .chain(row["protocolErrors"]["override"].as_array().unwrap())
                    .all(|byte| byte == 71),
                "{row}"
            );
            assert!(
                !row["protocolErrors"]["primary"]
                    .as_array()
                    .unwrap()
                    .is_empty()
                    || !row["protocolErrors"]["override"]
                        .as_array()
                        .unwrap()
                        .is_empty()
            );
            continue;
        }
        assert!(row["failure"].is_null(), "{engine}/{mode}: {row}");
        assert_eq!(
            row["protocolPrefixes"],
            json!({"primary":[],"override":[]}),
            "{row}"
        );
        assert_eq!(
            row["protocolErrors"],
            json!({"primary":[],"override":[]}),
            "{row}"
        );
        assert_eq!(row["pageStatus"], 200);
        assert_eq!(row["pageText"], "Acme");
        assert_eq!(row["httpStatus"], 200);
        assert_eq!(row["httpBody"], "Acme");
        assert_eq!(row["overrideStatus"], 200);
        assert_eq!(row["sourceClosed"], true);
        let bypass = name.ends_with("context_bypass");
        assert_eq!(row["origin"].as_array().unwrap().len(), 3, "{row}");
        for origin in row["origin"].as_array().unwrap() {
            assert_eq!(origin["custom"], "browser");
            assert_eq!(origin["authorization"], "");
            assert_eq!(origin["proxyAuthorization"], "");
        }
        let main = row["primary"].as_array().unwrap();
        let other = row["override"].as_array().unwrap();
        let forwarded = main
            .iter()
            .chain(other)
            .filter(|connection| connection["applicationData"] == true)
            .count();
        assert_eq!(forwarded, if bypass { 1 } else { 3 }, "{row}");
        assert!(
            main.len() + other.len() <= 32,
            "Owned reference connection bound: {row}"
        );
        for connection in main.iter().chain(other) {
            if connection["version"] == 4 {
                assert_eq!(connection["userId"], "");
            } else {
                assert_eq!(connection["version"], 5);
                assert_eq!(connection["methods"], json!([0]));
                assert!(connection["authentication"].is_null());
            }
        }
    }
    assert_eq!(report["validation"].as_array().unwrap().len(), 16);
    for row in report["validation"].as_array().unwrap() {
        let empty = row["credentials"]["username"] == "" && row["credentials"]["password"] == "";
        assert_eq!(row["accepted"], empty, "{engine}/{mode}: {row}");
        if !empty {
            assert!(
                row["error"]
                    .as_str()
                    .unwrap()
                    .contains(if row["scheme"] == "socks4" {
                        "does not support authentication"
                    } else {
                        "does not support socks5 proxy authentication"
                    }),
                "{row}"
            );
        }
    }
    report
}

#[tokio::test]
async fn step_four_browser_socks_reference_diagnostic() {
    let fixture = installed_fixture();
    eprintln!(
        "Browser SOCKS reference workspace {}",
        fixture.workspace.path().display()
    );
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let report = reference(&fixture, engine, mode).await;
            eprintln!(
                "Pinned browser SOCKS {engine}/{mode}: {} creation/routing/inheritance scenarios",
                report["cases"].as_array().unwrap().len()
            );
        }
    }
}
