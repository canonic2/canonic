//! Public reads agree with independently observed pinned APIResponse projections.
use super::*;

async fn read(mcp: &mut Mcp, target: &str, request: Value) -> Value {
    operation(mcp, target, request).await
}
async fn refused(mcp: &mut Mcp, target: &str, request: Value, code: &str) {
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":request}))
        .await;
    let value = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":3000}),
        )
        .await;
    assert_eq!(value["operation"]["settled"], true, "{value}");
    assert_eq!(value["operation"]["code"], code, "{value}");
}
async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    browser: &Value,
    engine: &str,
    associated: bool,
    site: &wire::Site,
    expected: &Value,
) {
    let mut options = json!({});
    if associated {
        options["browserContext"] = browser.clone();
        options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
    }
    let client = read(
        mcp,
        target,
        json!({"operation":"http_context_create","options":options}),
    )
    .await["httpContext"]
        .clone();
    let cases = wire::cases();
    assert_eq!(expected.as_array().unwrap().len(), cases.len());
    for (index, (name, bytes)) in cases.iter().enumerate() {
        let row = &expected[index];
        assert_eq!(row["name"], *name);
        let fetch = json!({"operation":"http_fetch","httpContext":client,"request":{"url":format!("{}/{index}",site.url),"readBody":true}});
        let response = if index == 0 {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &fetch.to_string()])
                .await;
            completed(mcp, target, submitted).await
        } else {
            read(mcp, target, fetch).await
        };
        assert_eq!(response["status"], 200, "{name}: {response}");
        let id = &response["requestId"];
        let metadata = read(
            mcp,
            target,
            json!({"operation":"http_response_read","requestId":id}),
        )
        .await;
        assert!(metadata.get("headersObject").is_none());
        assert_eq!(metadata["wireMetadataAvailable"], true);
        let headers = metadata["headers"].as_array().unwrap();
        let sdk_headers = headers
            .iter()
            .map(|header| json!({"name":header["name"],"value":header["value"]}))
            .collect::<Vec<_>>();
        assert_eq!(json!(sdk_headers), row["headersArray"], "{name}");
        assert_eq!(
            headers
                .iter()
                .find(|header| header["name"] == "X-Latin")
                .unwrap()["valueBase64"],
            "/w=="
        );
        let projection =
            json!({"operation":"http_response_read","requestId":id,"includeHeadersObject":true});
        let projected = if index == 0 {
            let submitted = fixture
                .cli(&[
                    "browser",
                    "send",
                    "--target",
                    target,
                    &projection.to_string(),
                ])
                .await;
            completed(mcp, target, submitted).await
        } else {
            read(mcp, target, projection).await
        };
        assert_eq!(projected["headersObject"], row["headers"], "{name}");
        assert_eq!(projected["headerObjectKeys"], row["headerKeys"], "{name}");
        assert_eq!(projected["headers"], metadata["headers"]);
        refused(mcp,target,json!({"operation":"http_response_read","requestId":id,"includeHeadersObject":true,"maxBytes":1}),"HTTP_RESPONSE_RESULT_BOUNDS").await;
        assert_eq!(
            read(
                mcp,
                target,
                json!({"operation":"http_response_read","requestId":id})
            )
            .await,
            metadata
        );
        let evidence = &response["body"]["evidenceId"];
        let body_read = |format: &str| json!({"operation":"http_response_body_read","requestId":id,"format":format});
        let actual = read(mcp, target, body_read("text")).await;
        assert_eq!(actual["value"], row["text"], "{name}");
        assert_eq!(actual["bodyBytes"], bytes.len());
        if row["json"]["ok"] == true {
            let actual = read(mcp, target, body_read("json_value")).await;
            assert_eq!(actual["value"], row["json"]["value"], "{name}");
            assert_eq!(actual["replayed"], false);
            assert_eq!(actual["evidenceId"], *evidence);
            let private=read(mcp,target,json!({"operation":"http_evidence_read","evidenceId":evidence,"format":"json_value"})).await;
            assert!(private.get("requestId").is_none());
            let mut public = actual.clone();
            assert_eq!(
                public.as_object_mut().unwrap().remove("requestId"),
                Some(id.clone())
            );
            assert_eq!(private, public);
            refused(mcp,target,json!({"operation":"http_response_body_read","requestId":id,"format":"json_value","maxBytes":1}),"HTTP_RESPONSE_RESULT_BOUNDS").await;
            assert_eq!(read(mcp, target, body_read("json_value")).await, actual);
            if index == 0 {
                let submitted=mcp.tool("act",json!({"targetId":target,"actions":[
                    {"action":"http_response_read","requestId":id,"includeHeadersObject":true},
                    {"action":"http_response_body_read","requestId":id,"format":"json_value"}
                ]})).await;
                let batch = completed(mcp, target, submitted).await;
                assert_eq!(batch["results"], json!([projected, actual]));
            }
        } else {
            refused(mcp, target, body_read("json_value"), "HTTP_RESPONSE_JSON").await;
        }
        let encoded = read(mcp, target, body_read("base64")).await;
        assert_eq!(
            encoded["value"],
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
        );
        read(
            mcp,
            target,
            json!({"operation":"http_response_dispose","requestId":id}),
        )
        .await;
        refused(
            mcp,
            target,
            body_read("json_value"),
            "HTTP_RESPONSE_DISPOSED",
        )
        .await;
        assert_eq!(
            read(
                mcp,
                target,
                json!({"operation":"http_response_read","requestId":id,"includeHeadersObject":true})
            )
            .await,
            projected
        );
        read(
            mcp,
            target,
            json!({"operation":"http_evidence_remove","evidenceId":evidence}),
        )
        .await;
        read(
            mcp,
            target,
            json!({"operation":"http_response_remove","requestId":id}),
        )
        .await;
    }
    read(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":client}),
    )
    .await;
}
#[tokio::test]
async fn installed_step_four_http_projections_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let expected = reference(&fixture, engine, mode).await;
            let site = wire::Site::start().await;
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let browser = &ready["browser"]["selectedContext"];
            let result=std::panic::AssertUnwindSafe(async {
                exercise(&fixture,&mut mcp,target,browser,engine,false,&site,&expected["isolated"]).await;
                exercise(&fixture,&mut mcp,target,browser,engine,true,&site,&expected["associated"]).await;
                assert_eq!(site.count.load(std::sync::atomic::Ordering::SeqCst),36);
                assert_eq!(read(&mut mcp,target,json!({"operation":"http_context_list"})).await["contexts"],json!([]));
            }).catch_unwind().await;
            std::fs::write(fixture.workspace.path().join(format!("http-projections-installed-{engine}-{mode}.json")),serde_json::to_vec_pretty(&json!({"reference":expected,"requests":site.count.load(std::sync::atomic::Ordering::SeqCst),"exerciseSucceeded":result.is_ok()})).unwrap()).unwrap();
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
                "HTTP projections {engine}/{mode}: 36 requests; exact retained header/text/JSON/body reads; original cleanup observed"
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
