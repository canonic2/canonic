//! One installed checkpoint for host regex admission and socket URL matching.
use super::*;

async fn http(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    base: &str,
) -> Value {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(
        mcp,
        target,
        json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":base}}),
    )
    .await;
    // JavaScript-only syntax: a named group, an inline modifier and the `d` flag.
    let matcher = json!({"kind":"regex","pattern":format!(r"^{}/(?<kind>(?i:regex))-(?:one|two)\?token=Acme_\d{{1,}}$",regex::escape(base)),"flags":"d"});
    let mut handlers = Vec::new();
    let mut registration_ms = Vec::new();
    for scope in [context, context, &page] {
        let request=json!({"operation":"network_route","scope":scope,"action":{"kind":"add_matcher","matcher":matcher,"times":if scope==&page{json!(1)}else{Value::Null}}}).to_string();
        let started = std::time::Instant::now();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request])
            .await;
        let added = completed(mcp, target, submitted).await;
        registration_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        assert!(!added.to_string().contains("(?<kind>"));
        handlers.push(added["handlerId"].clone());
    }
    let url = format!("{base}/REGEX-one?token=Acme_42");
    let mut timings = Vec::new();
    for iteration in 0..2 {
        let started = std::time::Instant::now();
        let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":format!("async()=>await (await fetch({})).text()",json!(url)),"isFunction":true}})).await;
        if iteration == 0 {
            let callback = installed_handler_callback(mcp, target, &handlers[2]).await;
            operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fallback_with","overrides":{"url":format!("{base}/rewritten-original")}}})).await;
        }
        let callback = installed_handler_callback(mcp, target, &handlers[1]).await;
        operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","status":201,"body":"Acme regex result","contentType":"text/plain"}})).await;
        assert_eq!(
            completed(mcp, target, pending).await["value"]["value"],
            "Acme regex result"
        );
        timings.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    // Broad driver/CDP candidates must privately continue without a manual callback.
    let passed=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":format!("async()=>(await fetch({})).status",json!(format!("{base}/regex-one?token=other"))),"isFunction":true})).await;
    assert_eq!(passed["value"]["value"], 200);
    let removed=operation(mcp,target,json!({"operation":"network_route","scope":context,"action":{"kind":"remove_matcher","matcher":matcher}})).await;
    assert!(removed["handlers"].as_array().unwrap().is_empty());
    operation(
        mcp,
        target,
        json!({"operation":"network_route","scope":page,"action":{"kind":"clear"}}),
    )
    .await;
    let passed=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":format!("async()=>(await fetch({})).status",json!(url)),"isFunction":true})).await;
    assert_eq!(passed["value"]["value"], 200);
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    json!({"freshCliRegexRegistrationMs":registration_ms,"firstRegexFallbackWorkflowMs":timings[0],"retainedRegexWorkflowMs":timings[1]})
}

#[tokio::test]
async fn installed_step_four_url_matching_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready().await;
            let context = &ready["browser"]["selectedContext"];
            let result = std::panic::AssertUnwindSafe(async {
                let latency = http(&fixture, &mut mcp, target, context, &site.url).await;
                har_clear::run(&fixture, &mut mcp, &site, target, context).await;
                let sockets =
                    socket_protocols::matched_urls(&fixture, &mut mcp, target, context, "glob")
                        .await;
                eprintln!(
                    "URL matching latency {engine} {mode}: {}",
                    json!({"http":latency,"sockets":sockets})
                );
            })
            .catch_unwind()
            .await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
}
