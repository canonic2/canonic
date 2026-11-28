//! Global request headers and zero HTTP-local timeout against the pinned SDK.
use super::*;
#[path = "../../../fixtures/browser_http_defaults.rs"]
mod wire;

async fn reference(
    fixture: &Fixture,
    engine: &str,
    mode: &str,
    first: &wire::Site,
    second: &wire::Site,
) -> Value {
    let input = fixture
        .workspace
        .path()
        .join(format!("http-defaults-input-{engine}-{mode}.json"));
    std::fs::write(
        &input,
        serde_json::to_vec(&json!({"first":first.url,"second":second.url})).unwrap(),
    )
    .unwrap();
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-http-default-semantics-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(input)
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
                .join(format!("http-defaults-sdk-{engine}-{mode}.{suffix}")),
            bytes,
        )
        .unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["ownedProcesses"], 0);
    assert_eq!(result["finiteOverride"], true);
    result
}

#[tokio::test]
async fn pinned_http_request_defaults_reference_checkpoint() {
    let fixture = installed_fixture();
    let second = wire::Site::start(None).await;
    let first = wire::Site::start(Some(second.url.clone())).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let result = reference(&fixture, engine, mode, &first, &second).await;
            assert_eq!(result["contextZero"]["path"], "/delay/40");
            assert_eq!(result["requestZero"]["path"], "/delay/40");
            eprintln!(
                "HTTP defaults SDK {engine}/{mode}: global headers, zero defaults/override and finite timeout"
            );
        }
    }
}

async fn client(mcp: &mut Mcp, target: &str, options: Value) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"http_context_create","options":options}),
    )
    .await["httpContext"]
        .clone()
}
async fn remove(mcp: &mut Mcp, target: &str, client: &Value) {
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":client}),
    )
    .await;
}
async fn fetch(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    client: &Value,
    mut request: Value,
    via: usize,
) -> Value {
    request["readBody"] = json!(true);
    let action = json!({"operation":"http_fetch","httpContext":client,"request":request});
    let response = match via {
        0 => {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &action.to_string()])
                .await;
            completed(mcp, target, submitted).await
        }
        1 => operation(mcp, target, action).await,
        _ => {
            let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_fetch","httpContext":client,"request":request}]})).await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
    };
    assert_eq!(response["status"], 200);
    assert_eq!(response["replayed"], false);
    assert_eq!(response["retryCount"], 0);
    let body=operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":response["body"]["evidenceId"],"format":"json"})).await;
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
    body["value"].clone()
}
fn compare(actual: &Value, expected: &Value) {
    for field in ["method", "path"] {
        assert_eq!(actual[field], expected[field], "{field}");
    }
    for name in ["x-acme", "authorization"] {
        assert_eq!(actual["headers"][name], expected["headers"][name], "{name}");
    }
}
async fn expires(
    mcp: &mut Mcp,
    target: &str,
    client: &Value,
    url: &str,
    local: u64,
    outer: u64,
) -> Value {
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"timeoutMs":outer,
        "request":{"operation":"http_fetch","httpContext":client,"request":{"url":url,"timeoutMs":local}}})).await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(status["operation"]["settled"], true, "{status}");
    let mut outcome = status["operation"].clone();
    if outcome["code"] == "HTTP_RESPONSE_PENDING" {
        assert_eq!(outcome["result"]["nextAction"], "http_response_finalize");
        assert_eq!(outcome["result"]["replayed"], false);
        let id = outcome["result"]["requestId"].clone();
        let until = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_response_finalize","requestId":id}})).await;
            let status=mcp.tool("browser_status",json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000})).await;
            assert_eq!(status["operation"]["settled"], true);
            outcome = status["operation"].clone();
            if outcome["code"] != "HTTP_RESPONSE_PENDING" {
                break;
            }
            assert!(
                std::time::Instant::now() < until,
                "Retained HTTP task must settle without replay"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    assert_eq!(outcome["code"], "HTTP_REQUEST_OUTCOME_UNKNOWN", "{outcome}");
    assert_eq!(outcome["result"]["requestMayHaveBeenSent"], true);
    json!({"initial":status["operation"],"final":outcome})
}

async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    context: &Value,
    first: &wire::Site,
    second: &wire::Site,
    expected: &Value,
) -> Value {
    let first_before = first.records().len();
    let second_before = second.records().len();
    let global =
        json!([{"name":"X-Acme","value":"global"},{"name":"Authorization","value":"Acme global"}]);
    operation(mcp,target,json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":global}})).await;
    let mut reports = Vec::new();
    for name in ["isolated", "based", "associated"] {
        let mut options = json!({});
        if name == "associated" {
            options["browserContext"] = context.clone();
            options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
        } else {
            options["headers"] = global.clone();
        }
        if name == "based" {
            options["baseUrl"] = json!(first.url);
        }
        let client = client(mcp, target, options).await;
        let oracle = &expected[if name == "associated" && engine == "chrome" {
            "isolated"
        } else {
            name
        }];
        for (index, (kind, url)) in [
            ("first", format!("{}/echo", first.url)),
            ("second", format!("{}/echo", second.url)),
            ("override", format!("{}/echo", second.url)),
            ("redirect", format!("{}/cross", first.url)),
        ]
        .iter()
        .enumerate()
        {
            let mut request = json!({"url":url});
            if *kind == "override" {
                request["headers"] = json!([{"name":"x-acme","value":"request"}]);
            }
            let actual = fetch(fixture, mcp, target, &client, request, index % 3).await;
            compare(&actual, &oracle[*kind]);
            reports.push(actual);
        }
        if name == "associated" {
            for (kind, headers, url) in [
                (
                    "changed",
                    json!([{"name":"X-Acme","value":"changed"}]),
                    format!("{}/echo", second.url),
                ),
                ("reset", json!([]), format!("{}/echo", first.url)),
            ] {
                operation(mcp,target,json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":headers}})).await;
                let actual = fetch(fixture, mcp, target, &client, json!({"url":url}), 1).await;
                compare(&actual, &expected[kind]);
                reports.push(actual);
            }
        }
        remove(mcp, target, &client).await;
    }
    let mut failures = Vec::new();
    for associated in [false, true] {
        for (context_timeout, request_timeout, kind) in
            [(0, None, "contextZero"), (1, Some(0), "requestZero")]
        {
            let mut options = json!({"timeoutMs":context_timeout});
            if associated {
                options["browserContext"] = context.clone();
                options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
            }
            let client = client(mcp, target, options).await;
            let mut request = json!({"url":format!("{}/delay/40",first.url)});
            if let Some(timeout) = request_timeout {
                request["timeoutMs"] = json!(timeout);
            }
            let actual = fetch(fixture, mcp, target, &client, request, 2).await;
            compare(&actual, &expected[kind]);
            reports.push(actual);
            remove(mcp, target, &client).await;
        }
        let mut options = json!({"timeoutMs":0});
        if associated {
            options["browserContext"] = context.clone();
            options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
        }
        let client = client(mcp, target, options).await;
        for (local, outer) in [(1000, 5000), (0, 1000)] {
            let before = first.records().len();
            failures.push(
                expires(
                    mcp,
                    target,
                    &client,
                    &format!("{}/delay/1500", first.url),
                    local,
                    outer,
                )
                .await,
            );
            assert_eq!(
                first.records().len(),
                before + 1,
                "Deadline does not dispatch twice"
            );
        }
        remove(mcp, target, &client).await;
    }
    assert_eq!(first.records().len(), first_before + 15);
    assert_eq!(second.records().len(), second_before + 10);
    assert_eq!(
        operation(mcp, target, json!({"operation":"http_context_list"})).await["contexts"],
        json!([])
    );
    let evidence = operation(mcp, target, json!({"operation":"http_evidence_list"})).await;
    assert_eq!(evidence["pending"], json!([]));
    assert_eq!(evidence["evidence"], json!([]));
    json!({"responses":reports,"timeouts":failures,"firstRequests":15,"secondRequests":10})
}

#[tokio::test]
async fn installed_step_four_http_request_defaults_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let second = wire::Site::start(None).await;
            let first = wire::Site::start(Some(second.url.clone())).await;
            let expected = reference(&fixture, engine, mode, &first, &second).await;
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
                &first,
                &second,
                &expected,
            ))
            .catch_unwind()
            .await;
            std::fs::write(fixture.workspace.path().join(format!("http-defaults-installed-{engine}-{mode}.json")),serde_json::to_vec_pretty(&json!({"reference":expected,"result":result.as_ref().ok(),"first":first.records(),"second":second.records()})).unwrap()).unwrap();
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
            eprintln!(
                "HTTP request defaults {engine}/{mode}: global/live headers, zero timeout, finite override and outer deadline"
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
