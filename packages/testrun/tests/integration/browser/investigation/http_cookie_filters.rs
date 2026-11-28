//! Cookie filter semantics and expiry ordering through public request transports.
use super::*;

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-http-cookie-filter-baseline.cjs"
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
                .join(format!("cookie-filters-sdk-{engine}-{mode}.{suffix}")),
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
async fn pinned_http_cookie_filters_reference_checkpoint() {
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
    let data: Value = serde_json::from_str(include_str!(
        "../../../fixtures/browser-cookie-filter-cases.json"
    ))
    .unwrap();
    let mut reports = vec![];
    for (index, case) in data["cases"].as_array().unwrap().iter().enumerate() {
        let clear = json!({"operation":"http_context_cookies_clear","httpContext":client});
        let add = json!({"operation":"http_context_cookies_add","httpContext":client,"cookies":data["cookies"]});
        let filtered = json!({"operation":"http_context_cookies_clear","httpContext":client,"options":case["options"]});
        let read = json!({"operation":"http_context_cookies_read","httpContext":client});
        let invalid = case["name"].as_str().unwrap().ends_with("invalid")
            || ["duplicate-flag", "incompatible-flags", "unknown-flag"]
                .contains(&case["name"].as_str().unwrap());
        let (outcome, remaining) = if index > 1 && !invalid {
            let mut actions = vec![clear, add, filtered, read];
            for action in &mut actions {
                let operation = action.as_object_mut().unwrap().remove("operation").unwrap();
                action["action"] = operation;
            }
            let submitted = mcp
                .tool("act", json!({"targetId":target,"actions":actions}))
                .await;
            let result = completed(mcp, target, submitted).await;
            ("cleared", result["results"][3]["cookies"].clone())
        } else {
            send(fixture, mcp, target, clear, index % 3).await;
            send(fixture, mcp, target, add, index % 3).await;
            if invalid {
                let submitted = mcp
                    .tool(
                        "browser_send",
                        json!({"targetId":target,"request":filtered}),
                    )
                    .await;
                let status = mcp
                    .tool(
                        "browser_status",
                        json!({"targetId":target,
                    "operationId":submitted["operation"]["id"],"waitMs":30000}),
                    )
                    .await;
                assert_eq!(status["operation"]["settled"], true, "{status}");
                assert_eq!(
                    status["operation"]["code"], "HTTP_COOKIE_REGEX_INVALID",
                    "{status}"
                );
            } else {
                send(fixture, mcp, target, filtered, index % 3).await;
            }
            let read = send(fixture, mcp, target, read, index % 3).await;
            (
                if invalid { "invalid" } else { "cleared" },
                read["cookies"].clone(),
            )
        };
        reports.push(json!({"name":case["name"],"outcome":outcome,"remaining":remaining}));
    }
    json!(reports)
}
async fn ordering(mcp: &mut Mcp, target: &str, client: &Value) -> Value {
    let cookie = |name: &str, expires: f64| {
        json!({"name":name,"value":"Acme",
        "domain":"example.com","path":"/","expires":expires})
    };
    let mut report = json!({});
    for variant in [
        "beforeRead",
        "afterRead",
        "afterNoMatchClear",
        "expiredMember",
    ] {
        operation(
            mcp,
            target,
            json!({"operation":"http_context_cookies_clear","httpContext":client}),
        )
        .await;
        let expires = if variant == "expiredMember" {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64()
                + 0.1
        } else {
            0.0
        };
        operation(
            mcp,
            target,
            json!({"operation":"http_context_cookies_add","httpContext":client,
            "cookies":[cookie("first",expires),cookie("second",-1.0)]}),
        )
        .await;
        if variant == "afterRead" {
            operation(
                mcp,
                target,
                json!({"operation":"http_context_cookies_read","httpContext":client}),
            )
            .await;
        }
        if variant == "expiredMember" {
            while std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64()
                <= expires
            {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            operation(
                mcp,
                target,
                json!({"operation":"http_context_cookies_add","httpContext":client,
                "cookies":[cookie("third",-1.0)]}),
            )
            .await;
        }
        if ["afterNoMatchClear", "expiredMember"].contains(&variant) {
            operation(
                mcp,
                target,
                json!({"operation":"http_context_cookies_clear","httpContext":client,
                "options":{"name":"missing"}}),
            )
            .await;
        }
        operation(
            mcp,
            target,
            json!({"operation":"http_context_cookies_add","httpContext":client,
            "cookies":[cookie("first",-1.0)]}),
        )
        .await;
        let read = operation(
            mcp,
            target,
            json!({"operation":"http_context_cookies_read","httpContext":client}),
        )
        .await;
        report[variant] = json!(
            read["cookies"]
                .as_array()
                .unwrap()
                .iter()
                .map(|cookie| cookie["name"].clone())
                .collect::<Vec<_>>()
        );
    }
    report
}
async fn cancellation(mcp: &mut Mcp, target: &str, client: &Value) {
    operation(
        mcp,
        target,
        json!({"operation":"http_context_cookies_clear","httpContext":client}),
    )
    .await;
    let name = format!("{}!", "a".repeat(4000));
    operation(
        mcp,
        target,
        json!({"operation":"http_context_cookies_add","httpContext":client,
        "cookies":[{"name":name,"value":"Acme","domain":"example.com","path":"/"}]}),
    )
    .await;
    let before = operation(
        mcp,
        target,
        json!({"operation":"http_context_cookies_read","httpContext":client}),
    )
    .await;
    let submitted = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":{
        "operation":"http_context_cookies_clear","httpContext":client,
        "options":{"name":{"regex":"^(a+)+$"}}}}),
        )
        .await;
    let operation_id = &submitted["operation"]["id"];
    let until = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let status = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,"operationId":operation_id,"waitMs":25}),
            )
            .await;
        assert_ne!(status["operation"]["settled"], true, "{status}");
        // The active context lease proves the evaluator actually owns work;
        // operation admission or a timed status receipt cannot prove that.
        let contexts = operation(mcp, target, json!({"operation":"http_context_list"})).await;
        let entry = contexts["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["httpContext"] == *client)
            .unwrap();
        if entry["active"] == true {
            break;
        }
        assert!(
            std::time::Instant::now() < until,
            "Filter must start before cancellation: {}",
            status["operation"]
        );
    }
    mcp.tool(
        "browser_cancel",
        json!({"targetId":target,"operationId":operation_id}),
    )
    .await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":operation_id,"waitMs":2000}),
        )
        .await;
    assert_eq!(status["operation"]["settled"], true, "{status}");
    assert_eq!(status["operation"]["code"], "CANCELLED", "{status}");
    let until = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let contexts = operation(mcp, target, json!({"operation":"http_context_list"})).await;
        let entry = contexts["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["httpContext"] == *client)
            .unwrap();
        if entry["active"] == false {
            break;
        }
        assert!(
            std::time::Instant::now() < until,
            "Cancelled filter still owns resources: {contexts}"
        );
        tokio::task::yield_now().await;
    }
    let after = operation(
        mcp,
        target,
        json!({"operation":"http_context_cookies_read","httpContext":client}),
    )
    .await;
    assert_eq!(
        after["cookies"], before["cookies"],
        "Cancellation cannot mutate the jar"
    );
}
#[tokio::test]
async fn installed_step_four_http_cookie_filters_checkpoint() {
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
                        let expected_cases = expected
                            [if associated { "associated" } else { "isolated" }]
                        .as_array()
                        .unwrap();
                        assert_eq!(report.as_array().unwrap().len(), expected_cases.len());
                        for (actual, expected_case) in
                            report.as_array().unwrap().iter().zip(expected_cases)
                        {
                            assert_eq!(actual["name"], expected_case["name"]);
                            assert_eq!(actual["outcome"], expected_case["outcome"]);
                            assert_eq!(
                                project(&actual["remaining"]),
                                project(&expected_case["remaining"]),
                                "{engine}/{mode} associated={associated} {}",
                                actual["name"]
                            );
                        }
                        if !associated {
                            assert_eq!(
                                ordering(&mut mcp, target, client).await,
                                expected["ordering"],
                                "{engine}/{mode} isolated insertion order"
                            );
                            cancellation(&mut mcp, target, client).await;
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
                    .join(format!("cookie-filters-installed-{engine}-{mode}.json")),
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
                "HTTP cookie filters {engine}/{mode}: isolated and associated strings, ECMAScript filters, invalid syntax and isolated expiry order"
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
