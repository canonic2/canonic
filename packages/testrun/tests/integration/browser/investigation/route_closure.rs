//! Interception closure against the unchanged pinned SDK in every engine/mode:
//! abort codes, JSON/file fulfill, unroute without waiting, worker routing,
//! worker loss while paused and worker HAR capture.
use super::*;

const WORKER: &str = "self.onmessage=async e=>{try{const r=await fetch(e.data);postMessage({status:r.status,type:r.headers.get('content-type'),acme:r.headers.get('x-acme'),text:await r.text()})}catch(error){postMessage({error:String(error.message||error)})}}";
const SHARED: &str = "onconnect=e=>{const port=e.ports[0];port.onmessage=async m=>{try{const r=await fetch(m.data);port.postMessage({status:r.status,text:await r.text()})}catch(error){port.postMessage({error:String(error.message||error)})}}}";
// Page helpers return JSON text so results compare as plain JSON with the SDK.
const FETCH: &str = "async url=>{try{const r=await fetch(url);return JSON.stringify({status:r.status,type:r.headers.get('content-type'),acme:r.headers.get('x-acme'),text:await r.text()})}catch(error){return JSON.stringify({error:String(error.message||error)})}}";
const VIA_WORKER: &str = "url=>new Promise(resolve=>{const w=new Worker('/worker.js');w.onmessage=e=>{resolve(JSON.stringify(e.data));w.terminate()};w.postMessage(url)})";
fn parsed(result: &Value) -> Value {
    serde_json::from_str(
        result["value"]["value"]
            .as_str()
            .unwrap_or_else(|| panic!("{result}")),
    )
    .unwrap()
}

/// Same paths and bodies as the SDK reference's server.
async fn server() -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buffer = [0; 1024];
                while !head.windows(4).any(|part| part == b"\r\n\r\n") {
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    head.extend_from_slice(&buffer[..read]);
                }
                let text = String::from_utf8_lossy(&head);
                let path = text
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/")
                    .to_owned();
                let (kind, body) = match path.as_str() {
                    "/page" => (
                        "text/html",
                        "<!doctype html><title>Acme route closure</title>".to_owned(),
                    ),
                    "/worker.js" => ("text/javascript", WORKER.to_owned()),
                    "/shared.js" => ("text/javascript", SHARED.to_owned()),
                    other => ("text/plain", format!("network {other}")),
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    (url, task)
}

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = tokio::process::Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-route-closure-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(fixture.cache.join("browsers"))
    .arg(fixture.workspace.path())
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .stdin(std::process::Stdio::null())
    .output()
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

async fn submit(mcp: &mut Mcp, target: &str, request: Value) -> Value {
    mcp.tool("browser_send", json!({"targetId":target,"request":request}))
        .await
}
async fn add(mcp: &mut Mcp, target: &str, scope: &Value, pattern: &str) -> Value {
    operation(mcp, target, json!({"operation":"network_route","scope":scope,"action":{"kind":"add_matcher","matcher":{"kind":"glob","pattern":pattern}}})).await["handlerId"].clone()
}
async fn evaluate(
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    function: &str,
    argument: Value,
) -> Value {
    submit(mcp, target, json!({"operation":"evaluate","scope":page,"expression":function,"isFunction":true,"argument":{"kind":"json","value":argument}})).await
}
/// A reply to a callback that interception already released must refuse.
async fn refused_reply(mcp: &mut Mcp, target: &str, callback: &Value) -> Value {
    let submitted = submit(mcp, target, json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","body":"late reply"}})).await;
    if submitted["operation"].is_null() {
        return submitted["code"].clone();
    }
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_ne!(status["operation"]["outcome"], "succeeded", "{status}");
    status["operation"]["code"].clone()
}

#[expect(
    clippy::too_many_lines,
    reason = "One cell mirrors each SDK reference case in order"
)]
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    ready: &Value,
    base: &str,
    sdk: &Value,
) -> Value {
    let context = &ready["browser"]["selectedContext"];
    let page = &ready["browser"]["selectedPage"];
    operation(
        mcp,
        target,
        json!({"operation":"diagnostics_start","context":context}),
    )
    .await;
    operation(mcp, target, json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{base}/page")}})).await;
    let mut report = json!({"abort":{}});
    let abort = add(mcp, target, page, "**/abort/*").await;
    for code in ["failed", "aborted", "namenotresolved"] {
        let before = operation(
            mcp,
            target,
            json!({"operation":"diagnostics_read","context":context}),
        )
        .await;
        let url = format!("{base}/abort/{code}");
        let pending = evaluate(mcp, target, page, FETCH, json!(url)).await;
        let callback = installed_handler_callback(mcp, target, &abort).await;
        operation(mcp, target, json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_abort","errorCode":code}})).await;
        assert!(parsed(&completed(mcp, target, pending).await)["error"].is_string());
        let event = operation(mcp, target, json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"request_failed","page":page,"url":url}]})).await;
        // Both backends retain the browser's failure text as private event text.
        let read = operation(mcp, target, json!({"operation":"diagnostics_read","context":context,"query":{"kind":"request_failed","requestId":event["event"]["data"]["requestId"],"includeText":true}})).await;
        report["abort"][code] = read["events"][0]["data"]["text"].clone();
    }
    let json_handler = add(mcp, target, page, "**/json").await;
    let pending = evaluate(mcp, target, page, FETCH, json!(format!("{base}/json"))).await;
    let callback = installed_handler_callback(mcp, target, &json_handler).await;
    operation(mcp, target, json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","json":{"acme":[1,true]},"headers":[{"name":"X-Acme","value":"one"}]}})).await;
    report["json"] = parsed(&completed(mcp, target, pending).await);
    let file = fixture.workspace.path().join("route-closure-testrun.html");
    std::fs::write(&file, "<b>Acme file</b>").unwrap();
    let file_handler = add(mcp, target, page, "**/file").await;
    let pending = evaluate(mcp, target, page, FETCH, json!(format!("{base}/file"))).await;
    let callback = installed_handler_callback(mcp, target, &file_handler).await;
    operation(mcp, target, json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","path":file}})).await;
    report["file"] = parsed(&completed(mcp, target, pending).await);
    // unrouteAll({behavior:"default"}) while a page request is paused.
    let held = add(mcp, target, page, "**/held-fulfill").await;
    let pending = evaluate(
        mcp,
        target,
        page,
        FETCH,
        json!(format!("{base}/held-fulfill")),
    )
    .await;
    let callback = installed_handler_callback(mcp, target, &held).await;
    operation(mcp, target, json!({"operation":"network_route","scope":page,"action":{"kind":"clear","behavior":"default"}})).await;
    report["unrouteDefaultFulfill"] = json!({"held":parsed(&completed(mcp, target, pending).await),"refused":refused_reply(mcp, target, &callback).await});
    let dedicated = add(mcp, target, page, "**/worker-data").await;
    let pending = evaluate(
        mcp,
        target,
        page,
        VIA_WORKER,
        json!(format!("{base}/worker-data")),
    )
    .await;
    let callback = installed_handler_callback(mcp, target, &dedicated).await;
    operation(mcp, target, json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","body":"routed dedicated"}})).await;
    report["dedicatedWorker"] = parsed(&completed(mcp, target, pending).await);
    operation(mcp, target, json!({"operation":"network_route","scope":page,"action":{"kind":"clear","behavior":"wait"}})).await;
    add(mcp, target, context, "**/shared-data").await;
    let shared = operation(mcp, target, json!({"operation":"evaluate","scope":page,"isFunction":true,"argument":{"kind":"json","value":format!("{base}/shared-data")},
        "expression":"url=>new Promise(done=>{const resolve=value=>done(JSON.stringify(value));if(typeof SharedWorker!=='function')return resolve({error:'SharedWorker unavailable'});const w=new SharedWorker('/shared.js');w.port.onmessage=e=>resolve(e.data);w.port.postMessage(url);setTimeout(()=>resolve({error:'timeout'}),5000)})"})).await;
    report["sharedWorker"] = parsed(&shared);
    operation(mcp, target, json!({"operation":"network_route","scope":context,"action":{"kind":"clear","behavior":"wait"}})).await;
    // Worker loss while its request is paused: continue still succeeds.
    let loss = add(mcp, target, page, "**/worker-held").await;
    operation(mcp, target, json!({"operation":"evaluate","scope":page,"isFunction":true,"argument":{"kind":"json","value":format!("{base}/worker-held")},"expression":"url=>{globalThis.acmeWorker=new Worker('/worker.js');acmeWorker.postMessage(url);return true}"})).await;
    let callback = installed_handler_callback(mcp, target, &loss).await;
    operation(mcp, target, json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>{acmeWorker.terminate();return true}"})).await;
    let submitted = submit(mcp, target, json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_continue"}})).await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    report["workerLoss"] = json!({"routed":true,"continued":if status["operation"]["outcome"]=="succeeded"{json!("ok")}else{status["operation"]["code"].clone()}});
    operation(mcp, target, json!({"operation":"network_route","scope":page,"action":{"kind":"clear","behavior":"ignoreErrors"}})).await;
    // unroute while a worker request is paused.
    let late = add(mcp, target, page, "**/worker-late").await;
    let pending = evaluate(
        mcp,
        target,
        page,
        VIA_WORKER,
        json!(format!("{base}/worker-late")),
    )
    .await;
    let callback = installed_handler_callback(mcp, target, &late).await;
    operation(mcp, target, json!({"operation":"network_route","scope":page,"action":{"kind":"remove","handlerId":late}})).await;
    report["removalWhilePaused"] = json!({"result":parsed(&completed(mcp, target, pending).await),"refused":refused_reply(mcp, target, &callback).await});
    operation(mcp, target, json!({"operation":"network_route","scope":page,"action":{"kind":"clear","behavior":"wait"}})).await;
    // Worker requests in recordHar.
    let har = fixture.workspace.path().join(format!(
        "route-closure-testrun-worker-{}.har",
        uuid::Uuid::new_v4()
    ));
    let recorded = operation(mcp, target, json!({"operation":"context_create","options":{"viewport":{"width":800,"height":600},"deviceScaleFactor":1,"recordHar":{"path":har,"content":"embed"}}})).await["context"].clone();
    let recorded_page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":recorded,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp, target, json!({"operation":"page_navigate","page":recorded_page,"navigation":{"kind":"goto","url":format!("{base}/page")}})).await;
    report["workerHarFetch"] = parsed(&operation(mcp, target, json!({"operation":"evaluate","scope":recorded_page,"isFunction":true,"expression":VIA_WORKER,"argument":{"kind":"json","value":format!("{base}/net/worker-har")}})).await);
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":recorded}),
    )
    .await;
    let archive: Value = serde_json::from_slice(&std::fs::read(&har).unwrap()).unwrap();
    let mut urls: Vec<String> = archive["log"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            url::Url::parse(entry["request"]["url"].as_str().unwrap())
                .unwrap()
                .path()
                .to_owned()
        })
        .filter(|path| path != "/favicon.ico")
        .collect();
    urls.sort();
    urls.dedup();
    report["workerHarUrls"] = json!(urls);
    // Compare every observable outcome with the unchanged SDK in this cell.
    assert_eq!(report["abort"], sdk["abort"], "{report}");
    for key in [
        "json",
        "file",
        "dedicatedWorker",
        "sharedWorker",
        "workerLoss",
        "workerHarFetch",
        "workerHarUrls",
    ] {
        assert_eq!(report[key], sdk[key], "{key}: {report} vs {sdk}");
    }
    assert_eq!(
        report["unrouteDefaultFulfill"]["held"], sdk["unrouteDefaultFulfill"]["held"],
        "{report}"
    );
    assert!(
        sdk["unrouteDefaultFulfill"]["fulfilled"]
            .as_str()
            .unwrap()
            .contains("already handled")
    );
    assert!(
        report["unrouteDefaultFulfill"]["refused"].is_string(),
        "{report}"
    );
    assert_eq!(
        report["removalWhilePaused"]["result"], sdk["removalWhilePaused"]["result"],
        "{report}"
    );
    assert!(
        report["removalWhilePaused"]["refused"].is_string(),
        "{report}"
    );
    report
}

#[tokio::test]
async fn installed_step_four_route_closure_checkpoint() {
    let fixture = installed_fixture();
    let (base, server) = server().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let sdk = reference(&fixture, engine, mode).await;
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap().to_owned();
            let ready = fixture.ready().await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture, &mut mcp, &target, &ready, &base, &sdk,
            ))
            .catch_unwind()
            .await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                &target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(closed);
            }
            match result {
                Ok(report) => eprintln!("Route closure {engine} {mode}: {report}"),
                Err(panic) => {
                    fixture.cli(&["disconnect"]).await;
                    mcp.tool("disconnect", json!({})).await;
                    mcp.stop().await;
                    server.abort();
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    server.abort();
    assert!(
        failures.is_empty(),
        "Original disconnect failures: {failures:?}"
    );
}
