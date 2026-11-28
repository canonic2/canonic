//! Installed Request initializer defaults, binary copies and source lifetime.
use super::*;
async fn capture(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    site: &wire::Site,
    name: &str,
) -> Value {
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
    let before = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    let url = format!("{}/source-{name}", site.url);
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"async url=>{const response=await fetch(url,{method:'POST',headers:{'X-Acme':'source','Content-Type':'application/Acme'},body:new Uint8Array([65,99,109,101,0,255])});if(response.status!==200)throw Error('Acme source status');await response.text();return true;}","argument":{"kind":"json","value":url}})).await;
    let event=operation(mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"request","page":page,"url":url}]})).await;
    assert_eq!(
        event["event"]["data"]["requestInputAvailable"], true,
        "{event}"
    );
    operation(mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"request_finished","page":page,"url":url}]})).await;
    event["event"]["data"]["requestId"].clone()
}
pub(super) async fn fetch(
    mcp: &mut Mcp,
    target: &str,
    client: &Value,
    source: &Value,
    options: Value,
    via: usize,
    fixture: &Fixture,
) -> Value {
    let mut options = options;
    options["readBody"] = json!(true);
    let input = json!({"operation":"http_fetch_request","httpContext":client,"sourceRequestId":source,"options":options});
    let response = match via {
        0 => {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &input.to_string()])
                .await;
            completed(mcp, target, submitted).await
        }
        1 => operation(mcp, target, input).await,
        _ => {
            let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_fetch_request","httpContext":client,"sourceRequestId":source,"options":options}]})).await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
    };
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
#[tokio::test]
async fn installed_step_four_http_owned_request_inputs_checkpoint() {
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
            let context=operation(&mut mcp,target,json!({"operation":"context_create"})).await["context"].clone();
            let page=operation(&mut mcp,target,json!({"operation":"page_create","context":context})).await["page"].clone();
            let client=operation(&mut mcp,target,json!({"operation":"http_context_create","options":{}})).await["httpContext"].clone();
            let source=capture(&mut mcp,target,&context,&page,&site,"defaults").await;
            if engine=="chrome" {
                let read=operation(&mut mcp,target,json!({"operation":"network_read","context":context,"requestId":source,"options":{"body":"request"}})).await;
                assert_eq!(read["requestBodyFidelity"],"binary-request-initializer");
                let path=read["body"]["attachment"]["path"].as_str().unwrap();
                assert!(std::path::Path::new(path).starts_with(fixture.workspace.path()));
                assert_eq!(std::fs::read(path).unwrap(),[65,99,109,101,0,255]);
                operation(&mut mcp,target,json!({"operation":"network_evidence_remove","evidenceId":read["body"]["evidenceId"]})).await;
            }
            let cases=[("defaults",json!({})),("method",json!({"method":"GET"})),("emptyMethod",json!({"method":""})),("headers",json!({"headers":[{"name":"X-Acme","value":"override"}]})),("emptyHeaders",json!({"headers":[]})),("body",json!({"body":{"kind":"text","text":"Acme replacement"}})),("emptyBody",json!({"body":{"kind":"text","text":""}})),("params",json!({"params":"Acme=one&Acme=two"}))];
            for (index,(name,options)) in cases.into_iter().enumerate() {
                let body=fetch(&mut mcp,target,&client,&source,options,index%3,&fixture).await;
                let sdk=&expected["cases"][name]["value"];
                assert_eq!(body["method"],sdk["method"],"{engine}/{mode}/{name}");
                assert_eq!(body["bodyBase64"],sdk["bodyBase64"],"{engine}/{mode}/{name}");
                assert_eq!(body["headers"]["x-acme"],sdk["headers"]["x-acme"],"{engine}/{mode}/{name}");
                if name=="params" {assert_eq!(body["url"],"/source-defaults?Acme=one&Acme=two");}
            }
            operation(&mut mcp,target,json!({"operation":"http_request_inputs_set","context":context,"enabled":false})).await;
            operation(&mut mcp,target,json!({"operation":"page_close","page":page})).await;
            assert_eq!(fetch(&mut mcp,target,&client,&source,json!({}),1,&fixture).await["bodyBase64"],"QWNtZQD/");
            let fresh_page=operation(&mut mcp,target,json!({"operation":"page_create","context":context})).await["page"].clone();
            let fresh=capture(&mut mcp,target,&context,&fresh_page,&site,"unwarmed-context").await;
            operation(&mut mcp,target,json!({"operation":"context_close","context":context})).await;
            for id in [&source,&fresh] {assert_eq!(fetch(&mut mcp,target,&client,id,json!({}),1,&fixture).await["bodyBase64"],"QWNtZQD/");}
            site.hold("/source-unwarmed-context");
            let before=site.records.lock().unwrap().iter().filter(|record|record["url"]=="/source-unwarmed-context").count();
            let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_fetch_request","httpContext":client,"sourceRequestId":fresh,"options":{"readBody":true}}})).await;
            tokio::time::timeout(std::time::Duration::from_secs(5),async {loop {let count=site.records.lock().unwrap().iter().filter(|record|record["url"]=="/source-unwarmed-context").count();if count==before+1 {break;}tokio::time::sleep(std::time::Duration::from_millis(10)).await;}}).await.unwrap();
            assert_eq!(operation(&mut mcp,target,json!({"operation":"http_request_input_remove","sourceRequestId":fresh})).await["removed"],true);
            site.release();
            let response=completed(&mut mcp,target,pending).await;
            assert_eq!(response["status"],200);
            let body=operation(&mut mcp,target,json!({"operation":"http_evidence_read","evidenceId":response["body"]["evidenceId"],"format":"json"})).await;
            assert_eq!(body["value"]["bodyBase64"],"QWNtZQD/");
            operation(&mut mcp,target,json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]})).await;
            operation(&mut mcp,target,json!({"operation":"http_response_remove","requestId":response["requestId"]})).await;
            for (id,removed) in [(&source,true),(&fresh,false)] {
                assert_eq!(operation(&mut mcp,target,json!({"operation":"http_request_input_remove","sourceRequestId":id})).await["removed"],removed);
                assert_eq!(operation(&mut mcp,target,json!({"operation":"http_request_input_remove","sourceRequestId":id})).await["removed"],false);
            }
            operation(&mut mcp,target,json!({"operation":"http_context_remove","httpContext":client})).await;
            assert_eq!(operation(&mut mcp,target,json!({"operation":"http_context_list"})).await["contexts"],json!([]));
        }).catch_unwind().await;
            std::fs::write(fixture.workspace.path().join(format!("http-owned-request-{engine}-{mode}.json")),serde_json::to_vec_pretty(&json!({"exerciseSucceeded":result.is_ok(),"records":*site.records.lock().unwrap()})).unwrap()).unwrap();
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
                "Owned Request inputs {engine}/{mode}: overrides, binary bodies, page/context lifetime, explicit release; original disconnect observed"
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
