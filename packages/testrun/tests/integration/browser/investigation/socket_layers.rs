//! Sibling socket registration, fixed socket ownership and selective removal through installed clients.
use super::*;

async fn add(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    scope: &Value,
    matcher: Value,
    cli: bool,
) -> (Value, f64) {
    let request = json!({"operation":"web_socket_route_add_matcher","scope":scope,"matcher":matcher,"sdkDefaults":false});
    let started = std::time::Instant::now();
    let value = if cli {
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request.to_string()])
            .await;
        completed(mcp, target, submitted).await
    } else {
        operation(mcp, target, request).await
    };
    (value, started.elapsed().as_secs_f64() * 1000.0)
}
async fn open(
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    route: &Value,
    url: &str,
    key: &str,
) -> Value {
    let expression = format!(
        "()=>new Promise(resolve=>{{globalThis.AcmeLayerSockets??={{}};const socket=new WebSocket({});const entry=AcmeLayerSockets[{}]={{socket}};entry.message=new Promise(done=>socket.onmessage=event=>done(event.data));entry.closed=new Promise(done=>socket.onclose=event=>done(event.code));socket.onopen=()=>resolve(socket.readyState);}})",
        json!(url),
        json!(key)
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
    let (reference, read) = socket_protocols::callback(mcp, target, route, "onCreate").await;
    socket_protocols::reply(mcp, target, reference, json!([{"type":"ensureOpened"}])).await;
    assert_eq!(completed(mcp, target, pending).await["value"]["value"], 1);
    read
}
async fn closed(mcp: &mut Mcp, target: &str, page: &Value, key: &str) {
    let value=operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":format!("async()=>await AcmeLayerSockets[{}].closed",json!(key))})).await;
    assert_eq!(value["value"]["value"], 3001);
}
async fn remove(mcp: &mut Mcp, target: &str, scope: &Value, route: &Value) -> f64 {
    let started = std::time::Instant::now();
    operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_remove","scope":scope,"routeId":route["routeId"]}),
    )
    .await;
    started.elapsed().as_secs_f64() * 1000.0
}
async fn listed(mcp: &mut Mcp, target: &str, scope: &Value, routes: &[&Value]) {
    let value = operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_list","scope":scope}),
    )
    .await;
    assert_eq!(value["order"], "newest-first");
    let actual = value["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|route| route["routeId"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        routes
            .iter()
            .map(|route| route["routeId"].clone())
            .collect::<Vec<_>>()
    );
    assert!(!value.to_string().contains("acme-layer"));
}
async fn cell(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value, url: &str) -> Value {
    let exact = json!({"kind":"exact","url":url});
    let glob = json!({"kind":"glob","pattern":format!("{url}*")});
    let (old_context, fresh_ms) = add(fixture, mcp, target, context, glob.clone(), true).await;
    let (context_route, context_ms) =
        add(fixture, mcp, target, context, exact.clone(), false).await;
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    let started = std::time::Instant::now();
    let original = open(mcp, target, &page, &context_route, url, "original").await;
    let mock_ms = started.elapsed().as_secs_f64() * 1000.0;
    let (old_page, old_page_ms) = add(fixture, mcp, target, &page, exact, false).await;
    let (page_route, page_ms) = add(fixture, mcp, target, &page, glob.clone(), false).await;
    // A Context registered after both Page routes must still lose to the Page.
    let (new_context, new_context_ms) = add(fixture, mcp, target, context, glob, false).await;
    listed(
        mcp,
        target,
        context,
        &[&new_context, &context_route, &old_context],
    )
    .await;
    listed(mcp, target, &page, &[&page_route, &old_page]).await;
    open(mcp, target, &page, &page_route, url, "page-new").await;
    let future = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    open(mcp, target, &future, &new_context, url, "future").await;
    let mut cleanup_ms = vec![remove(mcp, target, &page, &page_route).await];
    closed(mcp, target, &page, "page-new").await;
    listed(mcp, target, &page, &[&old_page]).await;
    // The pre-existing Context socket keeps its selected owner even after newer
    // Context and Page registrations, and removal of a different live owner.
    let send = |route: &Value, data: &str| json!({"operation":"web_socket_route_send","scope":context,"frame":original["frame"],"routeId":route["routeId"],"socketId":socket_field(&original["arguments"][0],"id")["value"],"to":"page","message":{"data":data,"isBase64":false}});
    let wrong = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":send(&new_context,"Acme wrong owner")}),
        )
        .await;
    let refused = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":wrong["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(
        refused["operation"]["code"], "WEBSOCKET_SOCKET_EXPIRED",
        "{refused}"
    );
    operation(mcp, target, send(&context_route, "Acme retained owner")).await;
    let received=operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"async()=>await AcmeLayerSockets.original.message"})).await;
    assert_eq!(received["value"]["value"], "Acme retained owner");
    open(mcp, target, &page, &old_page, url, "page-old").await;
    cleanup_ms.push(remove(mcp, target, &page, &old_page).await);
    closed(mcp, target, &page, "page-old").await;
    open(mcp, target, &page, &new_context, url, "context-new").await;
    cleanup_ms.push(remove(mcp, target, context, &new_context).await);
    closed(mcp, target, &page, "context-new").await;
    closed(mcp, target, &future, "future").await;
    open(mcp, target, &page, &context_route, url, "context-middle").await;
    cleanup_ms.push(remove(mcp, target, context, &context_route).await);
    closed(mcp, target, &page, "original").await;
    closed(mcp, target, &page, "context-middle").await;
    open(mcp, target, &page, &old_context, url, "context-old").await;
    cleanup_ms.push(remove(mcp, target, context, &old_context).await);
    closed(mcp, target, &page, "context-old").await;
    listed(mcp, target, context, &[]).await;
    listed(mcp, target, &page, &[]).await;
    for page in [&page, &future] {
        operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    }
    json!({"freshCliRegistrationMs":fresh_ms,"retainedRegistrationMs":[context_ms,old_page_ms,page_ms,new_context_ms],"firstMockWorkflowMs":mock_ms,"retainedRemovalMs":cleanup_ms})
}
#[tokio::test]
async fn installed_step_four_socket_layers_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let url = format!("{}/acme-layer", site.url.replacen("http:", "ws:", 1));
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture,
                &mut mcp,
                target,
                &ready["browser"]["selectedContext"],
                &url,
            ))
            .catch_unwind()
            .await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                cleanup_failures.push(format!("{engine}/{mode}: {closed}"));
            }
            match result {
                Ok(latency) => eprintln!("Socket layer latency {engine} {mode}: {latency}"),
                Err(panic) => {
                    fixture.cli(&["disconnect"]).await;
                    mcp.tool("disconnect", json!({})).await;
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        cleanup_failures.is_empty(),
        "Original cleanup failures: {cleanup_failures:?}"
    );
}
