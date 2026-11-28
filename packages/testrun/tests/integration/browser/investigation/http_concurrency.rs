//! Independent pinned HTTP concurrency and installed ownership checkpoints.
use super::*;

#[path = "../../../fixtures/browser_http_concurrency.rs"]
mod wire;

async fn submit(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    mut request: Value,
    via: usize,
) -> Value {
    match via {
        0 => {
            fixture
                .cli(&["browser", "send", "--target", target, &request.to_string()])
                .await
        }
        1 => {
            mcp.tool("browser_send", json!({"targetId":target,"request":request}))
                .await
        }
        _ => {
            let name = request
                .as_object_mut()
                .unwrap()
                .remove("operation")
                .unwrap();
            request["action"] = name;
            mcp.tool("act", json!({"targetId":target,"actions":[request]}))
                .await
        }
    }
}
async fn finish(mcp: &mut Mcp, target: &str, submitted: Value, via: usize) -> Value {
    let value = completed(mcp, target, submitted).await;
    if via == 2 {
        value["results"][0].clone()
    } else {
        value
    }
}
async fn active(mcp: &mut Mcp, target: &str, client: &Value) -> Value {
    operation(mcp, target, json!({"operation":"http_context_list"})).await["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["httpContext"] == *client)
        .unwrap()
        .clone()
}
async fn project(mcp: &mut Mcp, target: &str, response: Value) -> Value {
    assert_eq!(response["replayed"], false);
    let details = operation(
        mcp,
        target,
        json!({"operation":"http_response_read","requestId":response["requestId"]}),
    )
    .await;
    assert_eq!(details["wireMetadataAvailable"], true, "{details}");
    let header = details["headers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|header| header["name"] == "X-Acme")
        .unwrap()["value"]
        .clone();
    let text = operation(
        mcp,
        target,
        json!({"operation":"http_evidence_read",
        "evidenceId":response["body"]["evidenceId"],"format":"text"}),
    )
    .await;
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
    json!({"status":response["status"],"body":text["value"],"header":header})
}
async fn positive(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    client: &Value,
    mode: &str,
) -> Value {
    let mut site = wire::Site::start().await;
    let url = site.url.clone();
    let fetch = |name: &str| {
        json!({"operation":"http_fetch","httpContext":client,
        "request":{"url":format!("{}/{}",url,name),"readBody":true}})
    };
    let a = submit(fixture, mcp, target, fetch("A"), 0).await;
    let b = submit(fixture, mcp, target, fetch("B"), 1).await;
    let mut held = site.held(2).await;
    assert_eq!(active(mcp, target, client).await["activeRequests"], 2);
    let (order, first, first_via, other, other_via) = if mode == "headless" {
        (["B", "A"], b, 1, a, 0)
    } else {
        (["A", "B"], a, 0, b, 1)
    };
    held.remove(&format!("/{}", order[0]))
        .unwrap()
        .send(())
        .unwrap();
    let response = finish(mcp, target, first, first_via).await;
    let first = project(mcp, target, response).await;
    let pending = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":other["operation"]["id"],"waitMs":0}),
        )
        .await;
    assert_eq!(pending["operation"]["settled"], false, "{pending}");
    assert_eq!(active(mcp, target, client).await["activeRequests"], 1);
    held.remove(&format!("/{}", order[1]))
        .unwrap()
        .send(())
        .unwrap();
    let response = finish(mcp, target, other, other_via).await;
    let second = project(mcp, target, response).await;
    let c = submit(fixture, mcp, target, fetch("C"), 2).await;
    site.held(1).await.remove("/C").unwrap().send(()).unwrap();
    let response = finish(mcp, target, c, 2).await;
    let next = project(mcp, target, response).await;
    let cookies = operation(
        mcp,
        target,
        json!({"operation":"http_context_cookies_read","httpContext":client}),
    )
    .await;
    let mut names = cookies["cookies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cookie| cookie["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    names.sort_unstable();
    let count = site.count.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(count, 3);
    json!({"order":order,"first":first,"second":second,"next":next,"cookies":names,"requests":count})
}
async fn cancel_one(fixture: &Fixture, mcp: &mut Mcp, target: &str, client: &Value) {
    let mut site = wire::Site::start().await;
    let fetch = |name: &str| {
        json!({"operation":"http_fetch","httpContext":client,
        "request":{"url":format!("{}/{}",site.url,name),"readBody":true}})
    };
    let a = submit(fixture, mcp, target, fetch("A"), 1).await;
    let b = submit(fixture, mcp, target, fetch("B"), 0).await;
    let mut held = site.held(2).await;
    assert_eq!(active(mcp, target, client).await["activeRequests"], 2);
    mcp.tool(
        "browser_cancel",
        json!({"targetId":target,"operationId":a["operation"]["id"]}),
    )
    .await;
    let cancelled = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":a["operation"]["id"],"waitMs":2000}),
        )
        .await;
    assert_eq!(cancelled["operation"]["settled"], true, "{cancelled}");
    assert_eq!(cancelled["operation"]["code"], "CANCELLED", "{cancelled}");
    let request_id = cancelled["operation"]["result"]["requestId"].clone();
    let until = std::time::Instant::now() + Duration::from_secs(2);
    while active(mcp, target, client).await["activeRequests"] != 1 {
        assert!(
            std::time::Instant::now() < until,
            "Cancelled worker must actually release its lease"
        );
        tokio::task::yield_now().await;
    }
    let pending = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":b["operation"]["id"],"waitMs":0}),
        )
        .await;
    assert_eq!(pending["operation"]["settled"], false, "{pending}");
    let remove = submit(
        fixture,
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":client}),
        1,
    )
    .await;
    let refused = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":remove["operation"]["id"],"waitMs":2000}),
        )
        .await;
    assert_eq!(
        refused["operation"]["code"], "HTTP_CONTEXT_BUSY",
        "{refused}"
    );
    held.remove("/B").unwrap().send(()).unwrap();
    let response = finish(mcp, target, b, 0).await;
    assert_eq!(project(mcp, target, response).await["status"], 202);
    let until = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let finalize = submit(
            fixture,
            mcp,
            target,
            json!({"operation":"http_response_finalize","requestId":request_id}),
            1,
        )
        .await;
        let result = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,"operationId":finalize["operation"]["id"],"waitMs":2000}),
            )
            .await;
        assert_eq!(result["operation"]["settled"], true, "{result}");
        if result["operation"]["code"] == "HTTP_RESPONSE_PENDING" {
            assert!(
                std::time::Instant::now() < until,
                "Cancelled response must settle"
            );
            tokio::task::yield_now().await;
            continue;
        }
        assert_eq!(
            result["operation"]["code"], "HTTP_REQUEST_OUTCOME_UNKNOWN",
            "{result}"
        );
        break;
    }
    let _ = held.remove("/A").unwrap().send(());
    assert_eq!(site.count.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(active(mcp, target, client).await["activeRequests"], 0);
}
async fn running_filter(fixture: &Fixture, mcp: &mut Mcp, target: &str, client: &Value) {
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
    let submitted = submit(
        fixture,
        mcp,
        target,
        json!({"operation":"http_context_cookies_clear","httpContext":client,
        "options":{"name":{"regex":"^(a+)+$"}}}),
        1,
    )
    .await;
    let id = &submitted["operation"]["id"];
    let until = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let status = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,"operationId":id,"waitMs":25}),
            )
            .await;
        assert_eq!(status["operation"]["settled"], false, "{status}");
        if active(mcp, target, client).await["active"] == true {
            let record = status["browser"]["operations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|operation| operation["id"] == *id)
                .unwrap();
            assert_eq!(
                record["outcome"], "running",
                "Responsive HTTP must publish actual running state: {record}"
            );
            break;
        }
        assert!(
            std::time::Instant::now() < until,
            "Filter must actually acquire its worker lease"
        );
    }
    mcp.tool(
        "browser_cancel",
        json!({"targetId":target,"operationId":id}),
    )
    .await;
    let result = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":id,"waitMs":2000}),
        )
        .await;
    assert_eq!(result["operation"]["code"], "CANCELLED", "{result}");
    let until = std::time::Instant::now() + Duration::from_secs(2);
    while active(mcp, target, client).await["active"] == true {
        assert!(
            std::time::Instant::now() < until,
            "Filter worker must actually exit"
        );
        tokio::task::yield_now().await;
    }
    let after = operation(
        mcp,
        target,
        json!({"operation":"http_context_cookies_read","httpContext":client}),
    )
    .await;
    assert_eq!(after["cookies"], before["cookies"]);
}
#[tokio::test]
async fn installed_step_four_http_concurrency_checkpoint() {
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
            let result=std::panic::AssertUnwindSafe(async {
                let doctor=mcp.tool("browser_doctor",json!({"engine":engine})).await;
                assert_eq!(doctor["capabilities"]["httpContexts"]["requestOwnership"]["sameContextConcurrency"],true);
                let mut reports=json!({});
                for associated in [false,true] {
                    let mut options=json!({});
                    if associated {options["browserContext"]=context.clone();options["allowUnknownCookiePartitions"]=json!(engine=="firefox");}
                    let created=operation(&mut mcp,target,json!({"operation":"http_context_create","options":options})).await;
                    let client=&created["httpContext"];
                    let report=positive(&fixture,&mut mcp,target,client,mode).await;
                    let name=if associated {"associated"} else {"isolated"};
                    assert_eq!(report,expected[name],"{engine}/{mode}/{name}");
                    cancel_one(&fixture,&mut mcp,target,client).await;
                    if !associated {running_filter(&fixture,&mut mcp,target,client).await;}
                    operation(&mut mcp,target,json!({"operation":"http_context_cookies_clear","httpContext":client})).await;
                    operation(&mut mcp,target,json!({"operation":"http_context_remove","httpContext":client})).await;
                    reports[name]=report;
                }
                assert_eq!(operation(&mut mcp,target,json!({"operation":"http_context_list"})).await["contexts"],json!([]));
                reports
            }).catch_unwind().await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("concurrency-installed-{engine}-{mode}.json")),
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
                "HTTP concurrency {engine}/{mode}: isolated/associated independent completion, exact cancellation and running publication"
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
async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-http-concurrency-baseline.cjs"
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
                .join(format!("concurrency-sdk-{engine}-{mode}.{suffix}")),
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
async fn pinned_http_concurrency_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            reference(&fixture, engine, mode).await;
        }
    }
}
