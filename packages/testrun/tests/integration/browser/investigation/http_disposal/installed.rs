//! Installed public disposal preserves exact SDK lifetime distinctions and cleanup.
use super::*;
pub(super) async fn submit(
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
async fn successful(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    request: Value,
    via: usize,
) -> Value {
    let submitted = submit(fixture, mcp, target, request, via).await;
    let value = completed(mcp, target, submitted).await;
    if via == 2 {
        value["results"][0].clone()
    } else {
        value
    }
}
pub(super) async fn terminal(mcp: &mut Mcp, target: &str, submitted: &Value) -> Value {
    let value = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":3000}),
        )
        .await;
    assert_eq!(value["operation"]["settled"], true, "{value}");
    value["operation"].clone()
}
async fn refused(mcp: &mut Mcp, target: &str, request: Value, code: &str) {
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":request}))
        .await;
    let result = terminal(mcp, target, &submitted).await;
    assert_eq!(result["code"], code, "{result}");
}
async fn body(mcp: &mut Mcp, target: &str, response: &Value) -> Value {
    operation(mcp,target,json!({"operation":"http_response_body_read","requestId":response["requestId"],"format":"text"})).await["value"].clone()
}
async fn private_body(mcp: &mut Mcp, target: &str, response: &Value) -> Value {
    operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":response["body"]["evidenceId"],"format":"text"})).await["value"].clone()
}
async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    browser: &Value,
    engine: &str,
    associated: bool,
    site: &mut wire::Site,
    expected: &Value,
) -> Value {
    let mut options = json!({});
    if associated {
        options["browserContext"] = browser.clone();
        options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
    }
    let start = std::time::Instant::now();
    let created = successful(
        fixture,
        mcp,
        target,
        json!({"operation":"http_context_create","options":options}),
        0,
    )
    .await;
    let create_ms = start.elapsed().as_secs_f64() * 1000.;
    let client = &created["httpContext"];
    let url = site.url.clone();
    let fetch = |path: &str| json!({"operation":"http_fetch","httpContext":client,"request":{"url":format!("{url}{path}"),"readBody":true}});
    let start = std::time::Instant::now();
    let complete = successful(fixture, mcp, target, fetch("/complete"), 0).await;
    let fetch_ms = start.elapsed().as_secs_f64() * 1000.;
    assert_eq!(body(mcp, target, &complete).await, "Acme");
    let retained = std::time::Instant::now();
    let separate = successful(fixture, mcp, target, fetch("/response-dispose"), 1).await;
    let before = operation(
        mcp,
        target,
        json!({"operation":"http_response_read","requestId":separate["requestId"]}),
    )
    .await;
    successful(
        fixture,
        mcp,
        target,
        json!({"operation":"http_response_dispose","requestId":separate["requestId"]}),
        2,
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"http_response_dispose","requestId":separate["requestId"]}),
    )
    .await;
    refused(mcp,target,json!({"operation":"http_response_body_read","requestId":separate["requestId"],"format":"text"}),"HTTP_RESPONSE_DISPOSED").await;
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"http_response_read","requestId":separate["requestId"]})
        )
        .await,
        before
    );
    assert_eq!(private_body(mcp, target, &separate).await, "Acme");
    assert_eq!(
        expected["responseDisposal"],
        json!({"bodyClosed":true,"metadataSurvived":true})
    );
    let a = submit(fixture, mcp, target, fetch("/hold-first"), 0).await;
    let b = submit(fixture, mcp, target, fetch("/hold-second"), 1).await;
    let held = site.held(2).await;
    let clients = operation(mcp, target, json!({"operation":"http_context_list"})).await;
    let active = clients["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["httpContext"] == *client)
        .unwrap();
    assert_eq!(active["activeRequests"], 2);
    let disposition=successful(fixture,mcp,target,json!({"operation":"http_context_dispose","httpContext":client,"options":{"reason":"Acme first disposal"}}),2).await;
    assert_eq!(disposition["disposed"], true);
    assert_eq!(
        disposition["inflightPolicy"],
        if associated {
            "continue-dispatched"
        } else {
            "cancel-owned"
        }
    );
    let mut responses = vec![complete.clone(), separate.clone()];
    if associated {
        for submitted in [&a, &b] {
            let status=mcp.tool("browser_status",json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":0})).await;
            assert_eq!(status["operation"]["settled"], false, "{status}");
        }
        assert!(
            expected["pending"]
                .as_array()
                .unwrap()
                .iter()
                .all(|entry| entry["ok"] == true
                    && entry["value"]["status"] == 202
                    && entry["value"]["body"]["ok"] == false)
        );
        for (_, grant) in held {
            grant.send(()).unwrap();
        }
        for submitted in [a, b] {
            let response = completed(mcp, target, submitted).await;
            assert_eq!(response["status"], 202);
            refused(mcp,target,json!({"operation":"http_response_body_read","requestId":response["requestId"],"format":"text"}),"HTTP_RESPONSE_DISPOSED").await;
            assert_eq!(private_body(mcp, target, &response).await, "Acme");
            responses.push(response);
        }
    } else {
        assert!(expected["pending"].as_array().unwrap().iter().all(|entry| {
            entry["ok"] == false
                && entry["message"]
                    .as_str()
                    .unwrap()
                    .contains("Acme first disposal")
        }));
        assert_eq!(site.closed(2).await, ["/hold-first", "/hold-second"]);
        for submitted in [&a, &b] {
            let result = terminal(mcp, target, submitted).await;
            assert_eq!(result["code"], "HTTP_CONTEXT_DISPOSED", "{result}");
            let request_id = &result["result"]["requestId"];
            assert!(request_id.as_str().is_some(), "{result}");
            let until = std::time::Instant::now() + Duration::from_secs(2);
            loop {
                let finalized=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_response_finalize","requestId":request_id}})).await;
                let value = terminal(mcp, target, &finalized).await;
                if value["code"] == "HTTP_RESPONSE_PENDING" {
                    assert!(std::time::Instant::now() < until);
                    tokio::task::yield_now().await;
                    continue;
                }
                assert_eq!(value["code"], "HTTP_REQUEST_OUTCOME_UNKNOWN", "{value}");
                break;
            }
        }
        drop(held);
    }
    for request in [
        fetch("/never"),
        json!({"operation":"http_context_cookies_read","httpContext":client}),
        json!({"operation":"http_context_cookies_add","httpContext":client,"cookies":[]}),
        json!({"operation":"http_context_cookies_clear","httpContext":client}),
        json!({"operation":"http_context_storage_state","httpContext":client}),
        json!({"operation":"http_response_dispose","requestId":complete["requestId"]}),
    ] {
        refused(mcp, target, request, "HTTP_CONTEXT_DISPOSED").await;
    }
    refused(mcp,target,json!({"operation":"http_response_body_read","requestId":complete["requestId"],"format":"text"}),"HTTP_RESPONSE_DISPOSED").await;
    assert_eq!(private_body(mcp, target, &complete).await, "Acme");
    let ordinary = operation(
        mcp,
        target,
        json!({"operation":"http_context_read","httpContext":client}),
    )
    .await;
    assert!(!ordinary.to_string().contains("Acme first disposal"));
    let explicit = operation(
        mcp,
        target,
        json!({"operation":"http_context_read","httpContext":client,"includeReason":true}),
    )
    .await;
    assert_eq!(explicit["initialReason"], "Acme first disposal");
    assert_eq!(explicit["fetchReason"], "Acme first disposal");
    let repeat=successful(fixture,mcp,target,json!({"operation":"http_context_dispose","httpContext":client,"options":{"reason":"Acme second disposal"}}),0).await;
    assert_eq!(repeat["repeated"], true);
    assert_eq!(expected["repeated"]["ok"], true);
    let explicit = operation(
        mcp,
        target,
        json!({"operation":"http_context_read","httpContext":client,"includeReason":true}),
    )
    .await;
    assert_eq!(explicit["initialReason"], "Acme first disposal");
    assert_eq!(explicit["fetchReason"], "Acme second disposal");
    assert!(
        expected["afterRepeated"]["message"]
            .as_str()
            .unwrap()
            .contains("Acme second disposal")
    );
    for response in responses {
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
    let removed = operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":client}),
    )
    .await;
    assert_eq!(removed["removed"], true);
    json!({"freshCliCreateAndCompletionMs":create_ms,"freshCliFetchAndPublicationMs":fetch_ms,"retainedDisposalWorkflowMs":retained.elapsed().as_secs_f64()*1000.})
}
#[tokio::test]
async fn installed_step_four_http_disposal_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let expected = reference(&fixture, engine, mode).await;
            let mut site = wire::Site::start().await;
            let start = std::time::Instant::now();
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let browser = &ready["browser"]["selectedContext"];
            let connect_ms = start.elapsed().as_secs_f64() * 1000.;
            let result=std::panic::AssertUnwindSafe(async {
                let isolated=exercise(&fixture,&mut mcp,target,browser,engine,false,&mut site,&expected["isolated"]).await;
                let associated=exercise(&fixture,&mut mcp,target,browser,engine,true,&mut site,&expected["associated"]).await;
                let page=operation(&mut mcp,target,json!({"operation":"page_create","context":browser,"select":false})).await["page"].clone();
                operation(&mut mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/parent",site.url),"waitUntil":"load"}})).await;
                let read=operation(&mut mcp,target,json!({"operation":"evaluate","scope":page,"expression":"document.body.textContent"})).await;
                assert_eq!(read["value"]["value"],"Acme");
                assert_eq!(expected["parentContextSurvived"],true);
                assert_eq!(site.count.load(std::sync::atomic::Ordering::SeqCst),9);
                assert_eq!(operation(&mut mcp,target,json!({"operation":"http_context_list"})).await["contexts"],json!([]));
                json!({"isolated":isolated,"associated":associated,"connectMs":connect_ms})
            }).catch_unwind().await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("http-disposal-installed-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(
                    &json!({"reference":expected,"result":result.as_ref().ok()}),
                )
                .unwrap(),
            )
            .unwrap();
            let cleanup = std::time::Instant::now();
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, browser,
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
                "HTTP disposal {engine}/{mode}: {}; cleanupMs={}",
                result.unwrap(),
                cleanup.elapsed().as_secs_f64() * 1000.
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
