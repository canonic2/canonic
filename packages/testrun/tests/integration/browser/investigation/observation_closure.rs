//! Request/Response observation closure against the unchanged pinned SDK in
//! every engine/mode: a fetch redirect chain with navigation, frame, status
//! text, HTTP version, private raw headers and response sizes, plus
//! `page_request_gc` clearing a page `WeakRef`.
use super::*;

const PAGE: &str = "<!doctype html><title>Acme observation</title><p>Acme</p>";
const FINAL: &str = "Acme final body";

/// `/page`, `/redirect` (302 to `/final`) and `/final`, which the SDK
/// reference also requests.
async fn server() -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let http = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let origin = format!("http://{}", http.local_addr().unwrap());
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = http.accept().await.unwrap();
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
                let text = String::from_utf8_lossy(&head).into_owned();
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let response = match path.as_str() {
                    "/page" => format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{PAGE}",
                        PAGE.len()
                    ),
                    "/redirect" => "HTTP/1.1 302 Found\r\nlocation: /final\r\ncontent-length: 0\r\ncache-control: no-store\r\nconnection: close\r\n\r\n".to_owned(),
                    "/final" => format!(
                        "HTTP/1.1 200 Acme OK\r\ncontent-type: text/plain\r\nx-acme-response: final\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{FINAL}",
                        FINAL.len()
                    ),
                    _ => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        .to_owned(),
                };
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    (origin, task)
}

async fn reference(fixture: &Fixture, engine: &str, mode: &str, origin: &str) -> Value {
    let output = tokio::process::Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-observation-closure-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(fixture.cache.join("browsers"))
    .arg(origin)
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

/// The SDK reference's lowercased `{name,value}` rows for the selected names.
fn pick(raw: &Value, names: &[&str]) -> Value {
    let mut rows = raw["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| names.contains(&row["name"].as_str().unwrap().to_lowercase().as_str()))
        .map(
            |row| json!({"name":row["name"].as_str().unwrap().to_lowercase(),"value":row["value"]}),
        )
        .collect::<Vec<_>>();
    // Native raw headers come back in name order; compare as a set.
    rows.sort_by_key(|row| row["name"].as_str().unwrap().to_owned());
    json!(rows)
}

fn sorted(mut report: Value) -> Value {
    for side in ["last", "first"] {
        for key in ["requestHeaders", "responseHeaders"] {
            if let Some(rows) = report[side][key].as_array_mut() {
                rows.sort_by_key(|row| row["name"].as_str().unwrap().to_owned());
            }
        }
    }
    report
}

async fn read(mcp: &mut Mcp, target: &str, context: &Value, request: &Value) -> Value {
    operation(mcp,target,json!({"operation":"network_read","context":context,"requestId":request,"options":{"responseDetails":true,"rawHeaders":true}})).await
}

async fn cell(mcp: &mut Mcp, target: &str, ready: &Value, origin: &str, sdk: &Value) -> Value {
    let context = &ready["browser"]["selectedContext"];
    let page = &ready["browser"]["selectedPage"];
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{origin}/page"),"waitUntil":"load"}})).await;
    let document = operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>document","isFunction":true,"returnHandle":true})).await["handle"].clone();
    let main = operation(
        mcp,
        target,
        json!({"operation":"handle_owner_frame","handle":document}),
    )
    .await["frame"]
        .clone();
    let started = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_start","context":context}),
    )
    .await;
    let waiting = mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"diagnostics_wait","context":context,"after":started["latestSequence"],"conditions":[{"event":"request_finished","page":page,"url":format!("{origin}/final")}]}})).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>fetch('/redirect',{headers:{'X-Acme':'request'}}).then(response=>response.text())","isFunction":true})).await;
    let finished = completed(mcp, target, waiting).await;
    let last = read(
        mcp,
        target,
        context,
        &finished["event"]["data"]["requestId"],
    )
    .await;
    assert_eq!(last["rawHeadersIncluded"], true, "{last}");
    assert_eq!(last["rawRequestHeaders"]["private"], true, "{last}");
    assert_eq!(last["responseDetails"]["sizesPending"], false, "{last}");
    let first = read(mcp, target, context, &last["redirectedFrom"]).await;
    let sizes = &last["responseDetails"]["sizes"];
    let report = json!({
        "last":{"method":last["method"],"resourceType":last["resourceType"],"navigation":last["navigation"],
            "mainFrame":last["frame"]==main,"redirectedTo":last["redirectedTo"].is_null(),"status":last["status"],
            "statusText":last["statusText"],"httpVersion":last["responseDetails"]["httpVersion"],
            "fromServiceWorker":last["fromServiceWorker"],"requestHeaders":pick(&last["rawRequestHeaders"],&["x-acme"]),
            "responseHeaders":pick(&last["rawResponseHeaders"],&["x-acme-response","content-length"]),
            "sizes":{"responseBodySize":sizes["responseBodySize"],"responseHeadersSize":sizes["responseHeadersSize"],"requestBodySize":sizes["requestBodySize"]}},
        "first":{"redirectedTo":first["redirectedTo"]==finished["event"]["data"]["requestId"],"status":first["status"],
            "statusText":first["statusText"],"responseHeaders":pick(&first["rawResponseHeaders"],&["location"])},
    });
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{globalThis.acmeWeak=new WeakRef({acme:new Array(1000).fill('Acme')});}","isFunction":true})).await;
    let gc = operation(
        mcp,
        target,
        json!({"operation":"page_request_gc","page":page}),
    )
    .await;
    assert_eq!(gc["requested"], true, "{gc}");
    let collected = operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>globalThis.acmeWeak.deref()===undefined","isFunction":true})).await["value"]["value"].clone();
    let mut report = report;
    report["collected"] = collected;
    let report = sorted(report);
    let sdk = sorted(sdk.clone());
    assert_eq!(report, sdk, "Testrun {report} SDK {sdk}");
    report
}

#[tokio::test]
async fn installed_step_four_observation_closure_checkpoint() {
    let fixture = installed_fixture();
    let (origin, server) = server().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap().to_owned();
            let ready = fixture.ready().await;
            // Firefox and WebKit connections prepare the cache's pinned Node
            // and driver for the reference; Chrome prepares neither.
            let sdk = reference(&fixture, engine, mode, &origin).await;
            let result =
                std::panic::AssertUnwindSafe(cell(&mut mcp, &target, &ready, &origin, &sdk))
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
                Ok(report) => eprintln!("Observation closure {engine} {mode}: {report}"),
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
