//! Cookie addition and URL selection through every public request transport.
use super::*;

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-http-cookie-admin-baseline.cjs"
    ))
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
    for (suffix, bytes) in [("json", &output.stdout), ("stderr", &output.stderr)] {
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("cookie-admin-sdk-{engine}-{mode}.{suffix}")),
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
    result
}
#[tokio::test]
async fn pinned_http_cookie_admin_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            reference(&fixture, engine, mode).await;
        }
    }
}
fn project(values: &Value) -> Value {
    let mut result = values
        .as_array()
        .unwrap()
        .iter()
        .map(|value| {
            let mut selected = json!({});
            for field in [
                "name", "value", "domain", "path", "expires", "httpOnly", "secure", "sameSite",
            ] {
                selected[field] = value[field].clone();
            }
            selected
        })
        .collect::<Vec<_>>();
    result.sort_by_key(Value::to_string);
    json!(result)
}
async fn send(fixture: &Fixture, mcp: &mut Mcp, target: &str, request: Value, via: usize) -> Value {
    match via {
        0 => {
            let response = fixture
                .cli(&["browser", "send", "--target", target, &request.to_string()])
                .await;
            completed(mcp, target, response).await
        }
        1 => operation(mcp, target, request).await,
        _ => {
            let mut action = request;
            let name = action.as_object_mut().unwrap().remove("operation").unwrap();
            action["action"] = name;
            let response = mcp
                .tool("act", json!({"targetId":target,"actions":[action]}))
                .await;
            completed(mcp, target, response).await["results"][0].clone()
        }
    }
}
async fn exercise(fixture: &Fixture, mcp: &mut Mcp, target: &str, client: &Value) -> Value {
    let initial = json!([
        {"name":"acme","value":"url","url":"http://127.0.0.1/acme/page","secure":true},
        {"name":"scope","value":"secure","domain":"example.com","path":"/acme","secure":true},
        {"name":"local","value":"secure","domain":"localhost","path":"/","secure":true},
        {"name":"acme","value":"root","domain":"127.0.0.1","path":"/"}
    ]);
    send(
        fixture,
        mcp,
        target,
        json!({"operation":"http_context_cookies_add","httpContext":client,"cookies":initial}),
        0,
    )
    .await;
    send(fixture,mcp,target,json!({"operation":"http_context_cookies_add","httpContext":client,"cookies":[{"name":"acme","value":"replacement","domain":"127.0.0.1","path":"/"}]}),2).await;
    let mut report = json!({});
    for (index, (name, urls)) in [
        ("all", None),
        (
            "string",
            Some(json!("https://child.example.com/acme-extra")),
        ),
        (
            "union",
            Some(json!(["http://127.0.0.1/acme/page", "http://localhost/"])),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut request = json!({"operation":"http_context_cookies_read","httpContext":client});
        if let Some(urls) = urls {
            request["urls"] = urls;
        }
        let result = send(fixture, mcp, target, request, index).await;
        assert_eq!(result["secretBearing"], true);
        report[name] = project(&result["cookies"]);
    }
    let submitted = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":{
            "operation":"http_context_cookies_add","httpContext":client,"cookies":[
                {"name":"must-not-appear","value":"atomic","domain":"127.0.0.1","path":"/"},
                {"name":"invalid","value":"expiry","domain":"127.0.0.1","path":"/","expires":-2}
            ]}}),
        )
        .await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(status["operation"]["settled"], true);
    assert_eq!(status["operation"]["code"], "HTTP_COOKIE_INVALID");
    let unchanged = send(
        fixture,
        mcp,
        target,
        json!({"operation":"http_context_cookies_read","httpContext":client}),
        1,
    )
    .await;
    assert_eq!(project(&unchanged["cookies"]), report["all"]);
    send(fixture,mcp,target,json!({"operation":"http_context_cookies_add","httpContext":client,"cookies":[{"name":"acme","value":"deleted","domain":"127.0.0.1","path":"/","expires":0}]}),1).await;
    let result = send(
        fixture,
        mcp,
        target,
        json!({"operation":"http_context_cookies_read","httpContext":client,"urls":[]}),
        2,
    )
    .await;
    report["expired"] = project(&result["cookies"]);
    report
}
#[tokio::test]
async fn installed_step_four_http_cookie_admin_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let expected = reference(&fixture, engine, mode).await;
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
            let context = &ready["browser"]["selectedContext"];
            let result =
                std::panic::AssertUnwindSafe(async {
                    let mut reports = vec![];
                    for associated in [false, true] {
                        let mut options = json!({});
                        if associated {
                            options["browserContext"] = context.clone();
                            options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
                        }
                        let created = operation(
                            &mut mcp,
                            target,
                            json!({"operation":"http_context_create","options":options}),
                        )
                        .await;
                        let client = &created["httpContext"];
                        let report = exercise(&fixture, &mut mcp, target, client).await;
                        for field in ["all", "string", "union", "expired"] {
                            assert_eq!(
                                project(&report[field]),
                                project(
                                    &expected[if associated { "associated" } else { "isolated" }]
                                        [field]
                                ),
                                "{engine}/{mode} associated={associated} {field}"
                            );
                        }
                        operation(
                            &mut mcp,
                            target,
                            json!({"operation":"http_context_cookies_clear","httpContext":client}),
                        )
                        .await;
                        let empty = operation(
                            &mut mcp,
                            target,
                            json!({"operation":"http_context_cookies_read","httpContext":client}),
                        )
                        .await;
                        assert_eq!(empty["cookies"], json!([]));
                        operation(
                            &mut mcp,
                            target,
                            json!({"operation":"http_context_remove","httpContext":client}),
                        )
                        .await;
                        reports.push(report);
                    }
                    assert_eq!(
                        operation(&mut mcp, target, json!({"operation":"http_context_list"})).await
                            ["contexts"],
                        json!([])
                    );
                    reports
                })
                .catch_unwind()
                .await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("cookie-admin-installed-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(
                    &json!({"reference":expected,"result":result.as_ref().ok()}),
                )
                .unwrap(),
            )
            .unwrap();
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, context,
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
                "HTTP cookie administration {engine}/{mode}: isolated and associated addition, replacement, URL selection and expiry"
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
