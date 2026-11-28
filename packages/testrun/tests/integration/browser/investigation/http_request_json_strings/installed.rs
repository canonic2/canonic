//! Compare all measured caller-header/string/binary cases through installed clients.
use super::*;
async fn dispatch(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    input: Value,
    via: usize,
) -> Value {
    match via % 3 {
        0 => {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &input.to_string()])
                .await;
            completed(mcp, target, submitted).await
        }
        1 => operation(mcp, target, input).await,
        _ => {
            let mut action = input;
            let name = action.as_object_mut().unwrap().remove("operation").unwrap();
            action["action"] = name;
            let submitted = mcp
                .tool("act", json!({"targetId":target,"actions":[action]}))
                .await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
    }
}
async fn echo(mcp: &mut Mcp, target: &str, response: Value) -> Value {
    assert_eq!(response["status"], 200, "{response}");
    let read=operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":response["body"]["evidenceId"],"format":"json"})).await;
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
    read["value"].clone()
}
async fn source(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    site: &wire::Site,
    kind: &str,
) -> Value {
    let before = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    let url = format!("{}/source-{kind}", site.url);
    let mime = if kind == "json" {
        "application/json"
    } else {
        "text/plain"
    };
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"async({url,contentType})=>{const r=await fetch(url,{method:'POST',headers:{'Content-Type':contentType,'X-Acme':'source'},body:new Uint8Array([65,99,109,101,0,255])});if(r.status!==200)throw Error('Acme source status');await r.text();return true;}","argument":{"kind":"json","value":{"url":url,"contentType":mime}}})).await;
    let event=operation(mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"request","page":page,"url":url}]})).await;
    assert_eq!(
        event["event"]["data"]["requestInputAvailable"], true,
        "{event}"
    );
    event["event"]["data"]["requestId"].clone()
}
#[tokio::test]
async fn installed_step_four_http_json_string_request_checkpoint() {
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
            let initial = &ready["browser"]["selectedContext"];
            let result=std::panic::AssertUnwindSafe(async {
            let doctor=mcp.tool("browser_doctor",json!({"engine":engine})).await;
            let capabilities=&doctor["capabilities"]["httpContexts"]["requestStringBodies"];
            assert_eq!(capabilities["jsonSelection"],"first-caller-content-type-exact-application/json","{doctor}");
            assert_eq!(capabilities["contextDefaultsSelectJson"],false);assert_eq!(capabilities["fullSdkParity"],false);
            let plain=operation(&mut mcp,target,json!({"operation":"http_context_create","options":{}})).await["httpContext"].clone();
            let defaults=operation(&mut mcp,target,json!({"operation":"http_context_create","options":{"headers":[{"name":"Content-Type","value":"application/json"}]}})).await["httpContext"].clone();
            let context=operation(&mut mcp,target,json!({"operation":"context_create"})).await["context"].clone();
            let page=operation(&mut mcp,target,json!({"operation":"page_create","context":context})).await["page"].clone();
            operation(&mut mcp,target,json!({"operation":"diagnostics_start","context":context})).await;
            operation(&mut mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/fixture",site.url),"waitUntil":"load"}})).await;
            let sources=json!({"json":source(&mut mcp,target,&context,&page,&site,"json").await,"plain":source(&mut mcp,target,&context,&page,&site,"plain").await});
            let mut reports=json!({});
            for (index,(name,case)) in expected["cases"].as_object().unwrap().iter().enumerate() {
                let recipe=&case["input"];let client=if recipe["client"]=="defaults"{&defaults}else{&plain};
                let mut options=json!({"method":"POST","body":recipe["body"],"readBody":true});
                if let Some(headers)=recipe.get("headers") {options["headers"]=headers.clone();}
                let input=if let Some(kind)=recipe["source"].as_str() {assert!(sources[kind].is_string());json!({"operation":"http_fetch_request","httpContext":client,"sourceRequestId":sources[kind],"options":options})}
                    else {options["url"]=json!(format!("{}/case-{name}",site.url));json!({"operation":"http_fetch","httpContext":client,"request":options})};
                let response=dispatch(&fixture,&mut mcp,target,input,index).await;let actual=echo(&mut mcp,target,response).await;
                for key in ["url","method","bodyBase64"] {assert_eq!(actual[key],case["value"][key],"{engine}/{mode}/{name}/{key}: {actual}");}
                for name in ["content-type","content-length"] {assert_eq!(actual["headers"][name],case["value"]["headers"][name],"{engine}/{mode}/{name}: {actual}");}
                reports[name]=actual;
            }
            assert_eq!(reports.as_object().unwrap().len(),28);
            std::fs::write(fixture.workspace.path().join(format!("http-json-strings-installed-{engine}-{mode}.json")),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
            for kind in ["json","plain"] {operation(&mut mcp,target,json!({"operation":"http_request_input_remove","sourceRequestId":sources[kind]})).await;}
            operation(&mut mcp,target,json!({"operation":"context_close","context":context})).await;
            for client in [&plain,&defaults] {operation(&mut mcp,target,json!({"operation":"http_context_remove","httpContext":client})).await;}
        }).catch_unwind().await;
            std::fs::write(fixture.workspace.path().join(format!("http-json-strings-records-{engine}-{mode}.json")),serde_json::to_vec_pretty(&json!({"exerciseSucceeded":result.is_ok(),"records":*site.records.lock().unwrap()})).unwrap()).unwrap();
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
                "JSON request strings {engine}/{mode}: 28 SDK-compared cases through CLI/MCP/shared actions; original disconnect observed"
            );
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(failures.is_empty(), "{failures:?}");
}
