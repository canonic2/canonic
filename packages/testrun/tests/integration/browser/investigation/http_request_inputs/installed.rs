//! Request bytes and logical URL ownership, compared with the pinned SDK.
use super::*;

async fn refused(mcp: &mut Mcp, target: &str, client: &Value, request: Value, code: &str) {
    let response = mcp.request("tools/call", json!({"name":"browser_send","arguments":{
        "targetId":target,"request":{"operation":"http_fetch","httpContext":client,"request":request}
    }})).await;
    let result = &response["result"]["structuredContent"];
    if result["ok"] == false {
        assert_eq!(result["code"], code, "{response}");
    } else {
        assert_eq!(result["ok"], true, "{response}");
        let status = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,
            "operationId":result["data"]["operation"]["id"],"waitMs":3000}),
            )
            .await;
        assert_eq!(status["operation"]["settled"], true, "{status}");
        assert_eq!(status["operation"]["code"], code, "{status}");
    }
}
fn logical_url(value: &str) -> String {
    let mut url = url::Url::parse(value).unwrap();
    url.set_port(Some(12345)).unwrap();
    url.to_string()
}
async fn fetch(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    client: &Value,
    request: Value,
    via: usize,
) -> (Value, Value) {
    let mut request = request;
    request["readBody"] = json!(true);
    let input = json!({"operation":"http_fetch","httpContext":client,"request":request});
    let response = match via {
        0 => {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &input.to_string()])
                .await;
            completed(mcp, target, submitted).await
        }
        1 => operation(mcp, target, input).await,
        _ => {
            let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_fetch","httpContext":client,"request":request}]})).await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
    };
    assert_eq!(response["status"], 200, "{response}");
    assert_eq!(response["retryCount"], 0);
    let details = operation(
        mcp,
        target,
        json!({"operation":"http_response_read","requestId":response["requestId"]}),
    )
    .await;
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
    (body["value"].clone(), details)
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
    let client = operation(
        mcp,
        target,
        json!({"operation":"http_context_create","options":options}),
    )
    .await["httpContext"]
        .clone();
    let credentials = site.url.replace("http://", "http://Acme:known@");
    let cases = [
        (
            "fragment",
            json!({"url":format!("{}/url-input#Acme",site.url)}),
        ),
        (
            "credentials",
            json!({"url":format!("{credentials}/url-input")}),
        ),
        (
            "outerWhitespace",
            json!({"url":format!("\t {}/url-input \r\n",site.url)}),
        ),
        (
            "spaces",
            json!({"url":format!("{}/Acme path?q=one two",site.url)}),
        ),
        (
            "explicitAuthorization",
            json!({"url":format!("{credentials}/url-input"),"headers":[{"name":"Authorization","value":"Acme explicit"}]}),
        ),
        (
            "redirectCredentials",
            json!({"url":format!("{credentials}/redirect-credentials")}),
        ),
    ];
    for (index, (name, input)) in cases.into_iter().enumerate() {
        let (body, details) = fetch(fixture, mcp, target, &client, input, index % 3).await;
        let sdk = &expected["urlInputs"][name];
        assert_eq!(sdk["ok"], true, "{name}: {sdk}");
        assert_eq!(body["url"], sdk["value"]["url"], "{name}");
        assert_eq!(
            body["headers"]["authorization"], sdk["value"]["headers"]["authorization"],
            "{name}"
        );
        assert_eq!(
            logical_url(details["url"].as_str().unwrap()),
            logical_url(sdk["url"].as_str().unwrap()),
            "{name}"
        );
    }
    let (body,_)=fetch(fixture,mcp,target,&client,json!({"url":format!("{}/echo",site.url),"headers":[{"name":"X-Latin","value":"ÿ"},{"name":"Host","value":"Acme-host.invalid"},{"name":"Cookie","value":""}]}),1).await;
    assert_eq!(
        body["headers"]["x-latin"],
        expected["cases"]["latin1Header"]["value"]["headers"]["x-latin"]
    );
    assert_eq!(
        body["headers"]["host"],
        expected["initialHeaders"]["explicit"]["headers"]["host"]
    );
    assert_eq!(
        body["headers"]["cookie"],
        expected["initialHeaders"]["emptyCookie"]["headers"]["cookie"]
    );
    let before = site.records.lock().unwrap().len();
    for value in ["💩", "Acme\r\nInjected: yes"] {
        refused(
            mcp,
            target,
            &client,
            json!({"url":format!("{}/echo",site.url),"headers":[{"name":"X-Acme","value":value}]}),
            "HTTP_HEADER_INVALID",
        )
        .await;
    }
    refused(
        mcp,
        target,
        &client,
        json!({"url":format!("{}/echo",site.url.replace("http://","http://Acme:%ff@"))}),
        "HTTP_URL_INVALID",
    )
    .await;
    assert_eq!(site.records.lock().unwrap().len(), before);
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":client}),
    )
    .await;
    options["httpCredentials"] = json!({"username":"AcmeConfigured","password":"correct"});
    let client = operation(
        mcp,
        target,
        json!({"operation":"http_context_create","options":options}),
    )
    .await["httpContext"]
        .clone();
    let (body, details) = fetch(
        fixture,
        mcp,
        target,
        &client,
        json!({"url":format!("{credentials}/auth-fallback")}),
        1,
    )
    .await;
    assert_eq!(
        body["headers"]["authorization"],
        expected["urlInputs"]["challengeFallback"]["value"]["headers"]["authorization"]
    );
    assert_eq!(details["authenticationRetries"], 1);
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":client}),
    )
    .await;
}
#[tokio::test]
async fn installed_step_four_http_request_wire_inputs_checkpoint() {
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
            let result =
                std::panic::AssertUnwindSafe(async {
                    exercise(
                        &fixture, &mut mcp, target, browser, engine, false, &site, &expected,
                    )
                    .await;
                    exercise(
                        &fixture, &mut mcp, target, browser, engine, true, &site, &expected,
                    )
                    .await;
                    assert_eq!(site.records.lock().unwrap().len(), 20);
                    assert_eq!(
                        operation(&mut mcp, target, json!({"operation":"http_context_list"})).await
                            ["contexts"],
                        json!([])
                    );
                })
                .catch_unwind()
                .await;
            std::fs::write(fixture.workspace.path().join(format!("http-wire-installed-{engine}-{mode}.json")),serde_json::to_vec_pretty(&json!({"exerciseSucceeded":result.is_ok(),"records":*site.records.lock().unwrap()})).unwrap()).unwrap();
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
                "HTTP wire inputs {engine}/{mode}: isolated/associated exact bytes, normalized URLs, credentials, preflight refusals; original disconnect observed"
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
