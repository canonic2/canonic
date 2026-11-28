//! Evaluation and events closure against the unchanged pinned SDK in every
//! engine/mode: a retained frame across child navigation, page/context
//! lifecycle event waits and extra-header precedence on page and worker
//! requests, plus `web_socket` diagnostics waits.
use super::*;

const SITE: &str = include_str!("../../../fixtures/browser-evaluation-closure-site.json");
const MARKER: &str = "()=>document.getElementById('marker').textContent";

/// The same paths as the SDK reference's server; `/headers` echoes `X-Acme`.
async fn servers() -> (String, String, Vec<tokio::task::JoinHandle<()>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let site: Value = serde_json::from_str(SITE).unwrap();
    let http = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let origin = format!("http://{}", http.local_addr().unwrap());
    let pages = tokio::spawn(async move {
        loop {
            let (mut stream, _) = http.accept().await.unwrap();
            let site = site.clone();
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
                let (status, kind, body) = if path == "/headers" {
                    let value = text
                        .lines()
                        .find_map(|line| {
                            line.split_once(':')
                                .filter(|(name, _)| name.eq_ignore_ascii_case("x-acme"))
                                .map(|(_, value)| value.trim().to_owned())
                        })
                        .unwrap_or_else(|| "none".into());
                    ("200 OK", "text/plain", value)
                } else if let Some(body) = site["paths"][path.as_str()].as_str() {
                    let kind = if path.ends_with(".js") {
                        "text/javascript"
                    } else {
                        "text/html"
                    };
                    ("200 OK", kind, body.to_owned())
                } else {
                    ("404 Not Found", "text/plain", String::new())
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let socket = format!("ws://{}/acme-socket", listener.local_addr().unwrap());
    let sockets = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                use futures_util::StreamExt;
                if let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await {
                    while let Some(Ok(_)) = socket.next().await {}
                }
            });
        }
    });
    (origin, socket, vec![pages, sockets])
}

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = tokio::process::Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-evaluation-closure-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(fixture.cache.join("browsers"))
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

async fn evaluate(mcp: &mut Mcp, target: &str, scope: &Value, function: &str) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":scope,"expression":function,"isFunction":true}),
    )
    .await["value"]["value"]
        .clone()
}
/// A request's settled operation, failed or not.
async fn settled(mcp: &mut Mcp, target: &str, request: Value) -> Value {
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":request}))
        .await;
    mcp.tool(
        "browser_status",
        json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
    )
    .await["operation"]
        .clone()
}

async fn frames(mcp: &mut Mcp, target: &str, page: &Value, origin: &str) -> Value {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{origin}/frames"),"waitUntil":"load"}})).await;
    let frame = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    let owner = operation(
        mcp,
        target,
        json!({"operation":"frame_retain","frame":frame}),
    )
    .await["frame"]
        .clone();
    let one = evaluate(mcp, target, &owner, MARKER).await;
    evaluate(
        mcp,
        target,
        page,
        "()=>{document.getElementById('child').src='/child-two';}",
    )
    .await;
    // The owner follows the child into its new document once it commits.
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let two = loop {
        let read = settled(
            mcp,
            target,
            json!({"operation":"evaluate","scope":owner,"expression":MARKER,"isFunction":true}),
        )
        .await;
        if read["outcome"] == "succeeded" && read["result"]["value"]["value"] == "two" {
            break json!("two");
        }
        assert!(tokio::time::Instant::now() < until, "{read}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let stale = settled(
        mcp,
        target,
        json!({"operation":"evaluate","scope":frame,"expression":MARKER,"isFunction":true}),
    )
    .await;
    assert_eq!(stale["code"], "STALE_REFERENCE", "{stale}");
    let selector = operation(mcp,target,json!({"operation":"selector_evaluate","scope":owner,"options":{"locator":{"kind":"css","selector":"#marker"},"expression":"element=>element.textContent","isFunction":true}})).await["value"]["value"].clone();
    let element = operation(
        mcp,
        target,
        json!({"operation":"frame_element","frame":owner}),
    )
    .await;
    let embedding = evaluate(mcp, target, &element["handle"], "element=>element.id").await;
    let released = operation(
        mcp,
        target,
        json!({"operation":"frame_owner_release","frame":owner}),
    )
    .await;
    assert_eq!(released["nativeScopeClosed"], false);
    json!({"one":one,"two":two,"same":released["released"],"selector":selector,"embedding":embedding})
}

async fn events(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    origin: &str,
    chrome: bool,
) -> Value {
    let site: Value = serde_json::from_str(SITE).unwrap();
    let after = operation(
        mcp,
        target,
        json!({"operation":"events_read","context":context,"limit":1}),
    )
    .await["latestSequence"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{origin}/page"),"waitUntil":"load"}})).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":site["lifecycle"],"isFunction":true,"argument":{"kind":"json","value":chrome}})).await;
    let mut expected = vec!["worker_created"];
    if chrome {
        expected.push("service_worker_created");
    }
    // An expired wait still reads history, so a mismatch reports every event.
    for event in expected {
        settled(mcp,target,json!({"operation":"events_wait","context":context,"after":after,"conditions":[{"event":event}]})).await;
    }
    // Page through retained history and keep the events after the trigger.
    let mut events = Vec::new();
    let mut request = json!({"operation":"events_read","context":context,"limit":100});
    loop {
        let read = operation(mcp, target, request).await;
        events.extend(read["events"].as_array().unwrap().iter().cloned());
        if read["nextRequest"].is_null() {
            break;
        }
        request = read["nextRequest"].clone();
    }
    let after = after.as_u64().unwrap();
    let mut names = events
        .iter()
        .filter(|event| {
            event["sequence"]
                .as_u64()
                .is_some_and(|sequence| sequence > after)
        })
        .filter(|event| event["page"] == *page || event["page"].is_null())
        .filter_map(|event| match event["event"].as_str() {
            Some("frame_attached") => Some("frameattached"),
            Some("frame_detached") => Some("framedetached"),
            Some("load") => Some("load"),
            Some("dom_content_loaded") => Some("domcontentloaded"),
            Some("worker_created") => Some("worker"),
            Some("service_worker_created") => Some("serviceworker"),
            _ => None,
        })
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    json!(names)
}

async fn headers(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    origin: &str,
) -> Value {
    let site: Value = serde_json::from_str(SITE).unwrap();
    operation(mcp,target,json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"context"}]}})).await;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{origin}/page"),"waitUntil":"load"}})).await;
    operation(mcp,target,json!({"operation":"page_set_extra_http_headers","page":page,"headers":[{"name":"X-Acme","value":"page"}]})).await;
    let result = evaluate(mcp, target, page, site["headers"].as_str().unwrap()).await;
    serde_json::from_str(result.as_str().unwrap()).unwrap()
}

/// `diagnostics_wait` matches a `web_socket` event submitted before the socket.
async fn socket_wait(mcp: &mut Mcp, target: &str, context: &Value, page: &Value, url: &str) {
    let started = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_start","context":context}),
    )
    .await;
    let waiting = mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"diagnostics_wait","context":context,"after":started["latestSequence"],"conditions":[{"event":"web_socket","page":page}]}})).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"url=>{globalThis.acmeSocket=new WebSocket(url);}","isFunction":true,"argument":{"kind":"json","value":url}})).await;
    let matched = completed(mcp, target, waiting).await;
    assert_eq!(matched["event"]["kind"], "web_socket", "{matched}");
    assert!(
        matched["event"]["data"]["url"]
            .as_str()
            .is_some_and(|value| value.ends_with("/acme-socket")),
        "{matched}"
    );
}

async fn cell(
    mcp: &mut Mcp,
    target: &str,
    ready: &Value,
    origin: &str,
    socket: &str,
    sdk: &Value,
    chrome: bool,
) -> Value {
    let context = &ready["browser"]["selectedContext"];
    let page = &ready["browser"]["selectedPage"];
    let frames = frames(mcp, target, page, origin).await;
    let fresh = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    let events = events(mcp, target, context, &fresh, origin, chrome).await;
    let headers = headers(mcp, target, context, &fresh, origin).await;
    socket_wait(mcp, target, context, &fresh, socket).await;
    let report = json!({"frames":frames,"events":events,"headers":headers});
    assert_eq!(report, *sdk, "Testrun {report} SDK {sdk}");
    report
}

#[tokio::test]
async fn installed_step_four_evaluation_closure_checkpoint() {
    let fixture = installed_fixture();
    let (origin, socket, servers) = servers().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap().to_owned();
            let ready = fixture.ready().await;
            // The reference uses the cache's pinned Node and driver, which a
            // Firefox or WebKit connection prepares; Chrome prepares neither.
            let sdk = reference(&fixture, engine, mode).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &mut mcp,
                &target,
                &ready,
                &origin,
                &socket,
                &sdk,
                engine == "chrome",
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
                Ok(report) => eprintln!("Evaluation closure {engine} {mode}: {report}"),
                Err(panic) => {
                    fixture.cli(&["disconnect"]).await;
                    mcp.tool("disconnect", json!({})).await;
                    mcp.stop().await;
                    for server in &servers {
                        server.abort();
                    }
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    for server in &servers {
        server.abort();
    }
    assert!(
        failures.is_empty(),
        "Original disconnect failures: {failures:?}"
    );
}
