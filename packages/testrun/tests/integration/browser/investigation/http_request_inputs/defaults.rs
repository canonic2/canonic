//! Automatic diagnostic Request retention, persistent opt-out and binary reads.
use super::*;

async fn control(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    request: Value,
    via: usize,
) -> Value {
    match via % 3 {
        0 => {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &request.to_string()])
                .await;
            completed(mcp, target, submitted).await
        }
        1 => operation(mcp, target, request).await,
        _ => {
            let mut action = request;
            let name = action.as_object_mut().unwrap().remove("operation").unwrap();
            action["action"] = name;
            let submitted = mcp
                .tool("act", json!({"targetId":target,"actions":[action]}))
                .await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
    }
}
async fn send(mcp: &mut Mcp, target: &str, page: &Value, url: &str) {
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"async url=>{const r=await fetch(url,{method:'POST',headers:{'X-Acme':'source','Content-Type':'application/Acme'},body:new Uint8Array([65,99,109,101,0,255])});if(r.status!==200)throw Error('Acme source status');await r.text();return true;}","argument":{"kind":"json","value":url}})).await;
}
async fn capture(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    url: &str,
    enabled: bool,
) -> Value {
    let before = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    send(mcp, target, page, url).await;
    let event=operation(mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"request","page":page,"url":url}]})).await;
    if enabled {
        assert_eq!(
            event["event"]["data"]["requestInputAvailable"], true,
            "{event}"
        );
    } else {
        assert!(
            event["event"]["data"]
                .get("requestInputAvailable")
                .is_none(),
            "{event}"
        );
    }
    operation(mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"request_finished","page":page,"url":url}]})).await;
    event["event"]["data"]["requestId"].clone()
}
async fn body(mcp: &mut Mcp, target: &str, context: &Value, source: &Value, fixture: &Fixture) {
    let read=operation(mcp,target,json!({"operation":"network_read","context":context,"requestId":source,"options":{"body":"request"}})).await;
    assert_eq!(
        read["requestBodyFidelity"], "binary-request-initializer",
        "{read}"
    );
    let path = read["body"]["attachment"]["path"].as_str().unwrap();
    assert!(std::path::Path::new(path).starts_with(fixture.workspace.path()));
    assert_eq!(std::fs::read(path).unwrap(), [65, 99, 109, 101, 0, 255]);
    operation(
        mcp,
        target,
        json!({"operation":"network_evidence_remove","evidenceId":read["body"]["evidenceId"]}),
    )
    .await;
}
async fn copy(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    client: &Value,
    source: &Value,
    expected: &Value,
    via: usize,
) {
    let actual = owned::fetch(mcp, target, client, source, json!({}), via % 3, fixture).await;
    for field in ["method", "bodyBase64"] {
        assert_eq!(actual[field], expected[field], "{field}");
    }
    for name in ["x-acme", "content-type"] {
        assert_eq!(actual["headers"][name], expected["headers"][name], "{name}");
    }
}
async fn fresh(mcp: &mut Mcp, target: &str, site: &wire::Site) -> (Value, Value) {
    let context =
        operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/fixture",site.url),"waitUntil":"load"}})).await;
    (context, page)
}
#[tokio::test]
async fn installed_step_four_http_request_capture_defaults_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let sdk = reference(&fixture, engine, mode).await;
            let expected = &sdk["cases"]["defaults"]["value"];
            assert_eq!(expected["bodyBase64"], "QWNtZQD/");
            let site = wire::Site::start().await;
            let connected = mcp
                .tool("browser_connect", json!({"engine":engine,"mode":mode}))
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let initial = &ready["browser"]["selectedContext"];
            let result=std::panic::AssertUnwindSafe(async {
                let doctor=mcp.tool("browser_doctor",json!({"engine":engine})).await;
                let inputs=&doctor["capabilities"]["httpContexts"]["requestInputs"];
                assert_eq!(inputs["defaultEnabled"],true,"{doctor}");
                assert_eq!(inputs["fallbackOverrides"],true);
                assert_eq!(inputs["fullSdkParity"],false);
                let client=operation(&mut mcp,target,json!({"operation":"http_context_create","options":{}})).await["httpContext"].clone();
                let (context,page)=fresh(&mut mcp,target,&site).await;
                control(&fixture,&mut mcp,target,json!({"operation":"diagnostics_start","context":context}),0).await;
                let first=capture(&mut mcp,target,&context,&page,&format!("{}/source-default",site.url),true).await;
                body(&mut mcp,target,&context,&first,&fixture).await;
                copy(&fixture,&mut mcp,target,&client,&first,expected,0).await;
                control(&fixture,&mut mcp,target,json!({"operation":"diagnostics_stop","context":context}),1).await;
                copy(&fixture,&mut mcp,target,&client,&first,expected,1).await;
                let ignored=format!("{}/source-stopped",site.url);
                send(&mut mcp,target,&page,&ignored).await;
                let stopped=operation(&mut mcp,target,json!({"operation":"diagnostics_read","context":context})).await;
                assert_eq!(stopped["active"],false);
                assert!(stopped["events"].as_array().unwrap().iter().all(|event|event["data"]["url"]!=ignored));
                control(&fixture,&mut mcp,target,json!({"operation":"diagnostics_start","context":context}),2).await;
                let restarted=capture(&mut mcp,target,&context,&page,&format!("{}/source-restarted",site.url),true).await;
                body(&mut mcp,target,&context,&restarted,&fixture).await;
                control(&fixture,&mut mcp,target,json!({"operation":"diagnostics_stop","context":context}),0).await;
                control(&fixture,&mut mcp,target,json!({"operation":"http_request_inputs_set","context":context,"enabled":false}),2).await;
                control(&fixture,&mut mcp,target,json!({"operation":"diagnostics_start","context":context}),1).await;
                let opted_out=capture(&mut mcp,target,&context,&page,&format!("{}/source-opted-out",site.url),false).await;
                let before=site.records.lock().unwrap().len();
                let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_fetch_request","httpContext":client,"sourceRequestId":opted_out}})).await;
                let rejected=mcp.tool("browser_status",json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000})).await;
                assert_eq!(rejected["operation"]["settled"],true,"{rejected}");
                assert_eq!(rejected["operation"]["outcome"],"failed","{rejected}");
                assert_eq!(rejected["operation"]["code"],"HTTP_REQUEST_INPUT_UNKNOWN","{rejected}");
                assert_eq!(site.records.lock().unwrap().len(),before,"No origin request for a nonretained source");
                copy(&fixture,&mut mcp,target,&client,&first,expected,2).await;
                control(&fixture,&mut mcp,target,json!({"operation":"http_request_inputs_set","context":context,"enabled":true}),1).await;
                let resumed=capture(&mut mcp,target,&context,&page,&format!("{}/source-resumed",site.url),true).await;
                body(&mut mcp,target,&context,&resumed,&fixture).await;
                control(&fixture,&mut mcp,target,json!({"operation":"http_request_inputs_set","context":context,"enabled":false}),0).await;
                let (neighbor,neighbor_page)=fresh(&mut mcp,target,&site).await;
                control(&fixture,&mut mcp,target,json!({"operation":"diagnostics_start","context":neighbor}),2).await;
                let independent=capture(&mut mcp,target,&neighbor,&neighbor_page,&format!("{}/source-neighbor",site.url),true).await;
                body(&mut mcp,target,&neighbor,&independent,&fixture).await;
                for scope in [&context,&neighbor] {operation(&mut mcp,target,json!({"operation":"context_close","context":scope})).await;}
                for (index,source) in [&first,&restarted,&resumed,&independent].into_iter().enumerate() {
                    copy(&fixture,&mut mcp,target,&client,source,expected,index).await;
                    assert_eq!(operation(&mut mcp,target,json!({"operation":"http_request_input_remove","sourceRequestId":source})).await["removed"],true);
                }
                operation(&mut mcp,target,json!({"operation":"http_context_remove","httpContext":client})).await;
            }).catch_unwind().await;
            std::fs::write(fixture.workspace.path().join(format!("http-request-defaults-{engine}-{mode}.json")),serde_json::to_vec_pretty(&json!({"exerciseSucceeded":result.is_ok(),"records":*site.records.lock().unwrap()})).unwrap()).unwrap();
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, initial,
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
                "Automatic Request retention {engine}/{mode}: exact binary defaults, stop/restart, persistent opt-out, neighbor and closed-source copies; original disconnect observed"
            );
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(failures.is_empty(), "{failures:?}");
}
