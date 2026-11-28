//! Installed `url_match` waits with `waitUntil`, holding the load event on a paused image.
use super::har_interleave::callback;
use super::har_replay_filter::refuses;
use super::*;

/// Submit `request`, wait for it to settle and return its operation record.
async fn settled(mcp: &mut Mcp, target: &str, request: Value, timeout_ms: u64) -> Value {
    let submitted = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":request,"timeoutMs":timeout_ms}),
        )
        .await;
    mcp.tool(
        "browser_status",
        json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
    )
    .await["operation"]
        .clone()
}

async fn run(mcp: &mut Mcp, site: &Site, target: &str, page: &Value) {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url}})).await;
    let prefix = format!("/har-filter/{}", uuid::Uuid::new_v4());
    let document = format!("{}{prefix}/page", site.url);
    let image = format!("{}{prefix}/image", site.url);
    // A URLPattern route selects the image: a named segment, matched case-insensitively.
    let image_pattern =
        json!({"kind":"url_pattern","pathname":"/HAR-FILTER/:id/image","search":""});
    let mut handlers = Vec::new();
    for matcher in [json!({"kind":"exact","url":document}), image_pattern] {
        let added = operation(mcp,target,json!({"operation":"network_route","scope":page,"action":{"kind":"add_matcher","matcher":matcher}})).await;
        handlers.push(added["handlers"][0]["handlerId"].clone());
    }
    let navigation = mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":document,"waitUntil":"commit"}}})).await;
    let paused = callback(mcp, target, page, &handlers[0]).await;
    let html = format!(
        "<!doctype html><title>Acme</title><img src={}>",
        json!(image)
    );
    operation(mcp,target,json!({"operation":"callback_resolve","callback":paused,"response":{"kind":"route_fulfill","status":200,"body":html,"contentType":"text/html"}})).await;
    completed(mcp, target, navigation).await;
    // The image request holds the document's load event.
    let held = callback(mcp, target, page, &handlers[1]).await;
    let matcher = json!({"kind":"glob","pattern":format!("**{prefix}/page")});
    for wait_until in ["commit", "domcontentloaded"] {
        let waited = operation(mcp,target,json!({"operation":"page_wait","page":page,"condition":{"kind":"url_match","matcher":matcher,"waitUntil":wait_until}})).await;
        assert_eq!(waited["satisfied"], true, "{wait_until}: {waited}");
    }
    let started = std::time::Instant::now();
    let timed_out = settled(mcp,target,json!({"operation":"page_wait","page":page,"condition":{"kind":"url_match","matcher":matcher}}),1000).await;
    assert_eq!(timed_out["code"], "CONDITION_TIMEOUT", "{timed_out}");
    eprintln!("URL wait load timeout: {:?}", started.elapsed());
    let busy = settled(mcp,target,json!({"operation":"page_wait","page":page,"condition":{"kind":"url_match","matcher":matcher,"waitUntil":"networkidle"}}),1000).await;
    assert_eq!(busy["code"], "CONDITION_TIMEOUT", "{busy}");
    // Pinned `unrouteAll({behavior: "wait"})`: clearing waits for the paused
    // image callback, which no operation is waiting on; plain clear refuses.
    refuses(
        mcp,
        target,
        json!({"operation":"network_route","scope":page,"action":{"kind":"clear"}}),
        "ROUTE_CALLBACKS_PENDING",
    )
    .await;
    let clearing = mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"network_route","scope":page,"action":{"kind":"clear","behavior":"wait"}}})).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let waiting = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":clearing["operation"]["id"]}),
        )
        .await;
    assert_eq!(waiting["operation"]["settled"], false, "{waiting}");
    operation(mcp,target,json!({"operation":"callback_resolve","callback":held,"response":{"kind":"route_fulfill","status":404,"body":"","contentType":"image/png"}})).await;
    let cleared = completed(mcp, target, clearing).await;
    assert!(
        cleared["handlers"].as_array().unwrap().is_empty(),
        "{cleared}"
    );
    // URLPattern waits: the page's own path, and a different path that never matches.
    let matcher =
        json!({"kind":"url_pattern","protocol":"http","pathname":format!("{prefix}/:name")});
    let other = json!({"kind":"url_pattern","pathname":format!("{prefix}/other")});
    let missed = settled(mcp,target,json!({"operation":"page_wait","page":page,"condition":{"kind":"url_match","matcher":other,"waitUntil":"commit"}}),500).await;
    assert_eq!(missed["code"], "CONDITION_TIMEOUT", "{missed}");
    let loaded = operation(mcp,target,json!({"operation":"page_wait","page":page,"condition":{"kind":"url_match","matcher":matcher,"waitUntil":"load"}})).await;
    assert_eq!(loaded["satisfied"], true, "{loaded}");
    let started = std::time::Instant::now();
    let idle = operation(mcp,target,json!({"operation":"page_wait","page":page,"condition":{"kind":"url_match","matcher":matcher,"waitUntil":"networkidle"}})).await;
    assert_eq!(idle["satisfied"], true, "{idle}");
    eprintln!("URL wait networkidle after load: {:?}", started.elapsed());
    operation(
        mcp,
        target,
        json!({"operation":"network_route","scope":page,"action":{"kind":"clear"}}),
    )
    .await;
}

#[tokio::test]
async fn installed_step_four_url_wait_checkpoint() {
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
            let started = std::time::Instant::now();
            let page = &ready["browser"]["selectedPage"];
            let result = std::panic::AssertUnwindSafe(run(&mut mcp, &site, target, page))
                .catch_unwind()
                .await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            eprintln!("URL wait finished {engine} {mode}: {:?}", started.elapsed());
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
