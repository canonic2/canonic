//! Installed Request copies follow caller routing intent and retain old copies.
use super::super::har_interleave::callback;
use super::*;
async fn read(mcp: &mut Mcp, target: &str, response: Value) -> Value {
    assert_eq!(response["status"], 200, "{response}");
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
async fn copy(mcp: &mut Mcp, target: &str, client: &Value, source: &Value) -> Value {
    let response=operation(mcp,target,json!({"operation":"http_fetch_request","httpContext":client,"sourceRequestId":source,"options":{"readBody":true}})).await;
    read(mcp, target, response).await
}
async fn resolve(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    paused: &Value,
    response: Value,
    via: usize,
) -> Value {
    let input = json!({"operation":"callback_resolve","callback":paused,"response":response});
    match via {
        0 => {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &input.to_string()])
                .await;
            completed(mcp, target, submitted).await
        }
        1 => operation(mcp, target, input).await,
        _ => {
            let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"callback_resolve","callback":paused,"response":response}]})).await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
    }
}
async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    site: &wire::Site,
    expected: &Value,
    client: &Value,
    kind: &str,
    index: usize,
) {
    let context =
        operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context}),
    )
    .await["page"]
        .clone();
    operation(
        mcp,
        target,
        json!({"operation":"diagnostics_start","context":context}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"http_request_inputs_set","context":context,"enabled":true}),
    )
    .await;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/fixture",site.url),"waitUntil":"load"}})).await;
    let url = format!("{}/source-{kind}", site.url);
    let added=operation(mcp,target,json!({"operation":"network_route","scope":page,"action":{"kind":"add_matcher","matcher":{"kind":"exact","url":url}}})).await;
    let mut handler = added["handlers"][0]["handlerId"].clone();
    if kind == "layered" {
        let added=operation(mcp,target,json!({"operation":"network_route","scope":page,"action":{"kind":"add_matcher","matcher":{"kind":"exact","url":url}}})).await;
        handler = added["handlers"][0]["handlerId"].clone();
    }
    let before = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"url=>{globalThis.AcmeRequestFallback=fetch(url,{method:'POST',headers:{'X-Acme':'source'},body:new Uint8Array([65,99,109,101,0,255])}).then(async response=>{if(response.status!==200)throw Error('Acme source status');return response.text();});return true;}","argument":{"kind":"json","value":url}})).await;
    let paused = callback(mcp, target, &page, &handler).await;
    let event=operation(mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"request","page":page,"url":url}]})).await;
    assert_eq!(
        event["event"]["data"]["requestInputAvailable"], true,
        "{event}"
    );
    let source = &event["event"]["data"]["requestId"];
    let held = if kind == "layered" {
        site.hold("/source-layered");
        let before = site
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|record| record["url"] == "/source-layered")
            .count();
        let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_fetch_request","httpContext":client,"sourceRequestId":source,"options":{"readBody":true}}})).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let count = site
                    .records
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|record| record["url"] == "/source-layered")
                    .count();
                if count == before + 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        Some(pending)
    } else {
        None
    };
    let overrides = if kind == "empty" {
        json!({"headers":[],"bodyBase64":""})
    } else {
        json!({"url":format!("{}/changed-{kind}",site.url),"method":"PUT","headers":[{"name":"X-Acme","value":"routed"},{"name":"Cookie","value":"AcmeFallback=private"}],"bodyBase64":"AP+A"})
    };
    let result=resolve(fixture,mcp,target,&paused,json!({"kind":if kind=="continue"{"route_continue_with"}else{"route_fallback_with"},"overrides":overrides}),index%3).await;
    if kind == "layered" {
        assert!(result["nextCallback"].is_object(), "{result}");
        let middle = copy(mcp, target, client, source).await;
        assert_eq!(
            middle["bodyBase64"],
            expected["cases"][kind]["intermediate"]["bodyBase64"]
        );
        assert_eq!(
            middle["headers"]["cookie"],
            expected["cases"][kind]["intermediate"]["headers"]["cookie"]
        );
        resolve(fixture,mcp,target,&result["nextCallback"],json!({"kind":"route_fallback_with","overrides":{"headers":[{"name":"X-Acme","value":"first"},{"name":"x-acme","value":"last"},{"name":"Cookie","value":"AcmeLayer=private"},{"name":"__proto__","value":"Acme literal"}]}}),1).await;
    }
    let ended=operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>globalThis.AcmeRequestFallback"})).await;
    let browser: Value = serde_json::from_str(ended["value"]["value"].as_str().unwrap()).unwrap();
    let sdk = &expected["cases"][kind];
    for key in ["url", "method", "bodyBase64"] {
        assert_eq!(
            browser[key], sdk["browserValue"][key],
            "browser {kind}/{key}"
        );
    }
    assert_eq!(
        browser["headers"]["cookie"],
        sdk["browserValue"]["headers"]["cookie"]
    );
    if let Some(pending) = held {
        site.release();
        let response = completed(mcp, target, pending).await;
        let original = read(mcp, target, response).await;
        assert_eq!(original["url"], "/source-layered");
        assert_eq!(original["method"], "POST");
        assert_eq!(original["bodyBase64"], "QWNtZQD/");
    }
    operation(mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"request_finished","page":page,"url":url}]})).await;
    let body = operation(mcp,target,json!({"operation":"network_read","context":context,"requestId":source,"options":{"body":"request"}})).await;
    assert_eq!(
        body["requestBodyFidelity"], "binary-caller-route-override",
        "{body}"
    );
    let path = body["body"]["attachment"]["path"].as_str().unwrap();
    assert!(std::path::Path::new(path).starts_with(fixture.workspace.path()));
    let expected_body: &[u8] = if kind == "empty" { &[] } else { &[0, 255, 128] };
    assert_eq!(std::fs::read(path).unwrap(), expected_body);
    operation(
        mcp,
        target,
        json!({"operation":"network_evidence_remove","evidenceId":body["body"]["evidenceId"]}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":context}),
    )
    .await;
    let actual = copy(mcp, target, client, source).await;
    for key in ["url", "method", "bodyBase64"] {
        assert_eq!(actual[key], sdk["copy"][key], "copy {kind}/{key}");
    }
    for name in ["x-acme", "cookie", "content-length"] {
        assert_eq!(
            actual["headers"][name], sdk["copy"]["headers"][name],
            "copy {kind}/{name}"
        );
    }
    assert!(actual["headers"].get("__proto__").is_none());
    operation(
        mcp,
        target,
        json!({"operation":"http_request_input_remove","sourceRequestId":source}),
    )
    .await;
}
#[tokio::test]
async fn installed_step_four_http_request_fallback_checkpoint() {
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
            let result = std::panic::AssertUnwindSafe(async {
                let doctor = mcp.tool("browser_doctor", json!({"engine":engine})).await;
                let inputs = &doctor["capabilities"]["httpContexts"]["requestInputs"];
                assert_eq!(inputs["fallbackOverrides"], true, "{doctor}");
                assert_eq!(inputs["continueOverrides"], true, "{doctor}");
                assert_eq!(
                    inputs["routeOverrideFields"],
                    json!(["url", "method", "headers", "bodyBase64"])
                );
                assert_eq!(
                    inputs["bodySource"],
                    "binary-initializer-or-caller-route-override"
                );
                assert_eq!(inputs["fullSdkParity"], false);
                let client = operation(
                    &mut mcp,
                    target,
                    json!({"operation":"http_context_create","options":{}}),
                )
                .await["httpContext"]
                    .clone();
                for (index, kind) in ["fallback", "continue", "empty", "layered"]
                    .into_iter()
                    .enumerate()
                {
                    exercise(
                        &fixture, &mut mcp, target, &site, &expected, &client, kind, index,
                    )
                    .await;
                }
                operation(
                    &mut mcp,
                    target,
                    json!({"operation":"http_context_remove","httpContext":client}),
                )
                .await;
            })
            .catch_unwind()
            .await;
            std::fs::write(fixture.workspace.path().join(format!("http-request-fallback-installed-{engine}-{mode}.json")),serde_json::to_vec_pretty(&json!({"exerciseSucceeded":result.is_ok(),"records":*site.records.lock().unwrap()})).unwrap()).unwrap();
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
                "Request override copies {engine}/{mode}: four cases, closed sources and old in-flight copy; original disconnect observed"
            );
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(failures.is_empty(), "{failures:?}");
}
