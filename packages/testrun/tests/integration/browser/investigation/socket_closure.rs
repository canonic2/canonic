//! WebSocket closure against the unchanged pinned SDK in every engine/mode:
//! default relay and mock, a lookbehind regex with an unmatched sibling,
//! unrouted worker sockets and 512 KiB relayed messages. Testrun's 512 KiB send
//! bound and reason-only server close are checked separately.
use super::*;
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const SCENARIOS: &str = include_str!("../../../fixtures/browser-socket-closure-scenarios.json");
const REASON: &str = "({base,path})=>new Promise(resolve=>{const s=new WebSocket(base+'/'+path);window.AcmeReason=s;s.onmessage=()=>resolve(JSON.stringify('open'))})";

#[derive(Default)]
struct Wire {
    connections: BTreeMap<String, usize>,
    closes: BTreeMap<String, Value>,
}
struct Servers {
    page: String,
    socket: String,
    wire: Arc<Mutex<Wire>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl Drop for Servers {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
/// The same page, worker and echo behavior as the SDK reference's server.
async fn servers(fixture: &Value) -> Servers {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let http = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let page = format!("http://{}", http.local_addr().unwrap());
    let (html, worker) = (
        fixture["page"].as_str().unwrap().to_owned(),
        fixture["worker"].as_str().unwrap().to_owned(),
    );
    let pages = tokio::spawn(async move {
        loop {
            let (mut stream, _) = http.accept().await.unwrap();
            let (html, worker) = (html.clone(), worker.clone());
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
                let script = String::from_utf8_lossy(&head).contains("GET /socket-worker.js ");
                let (kind, body) = if script {
                    ("text/javascript", worker)
                } else {
                    ("text/html", html)
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let socket = format!("ws://{}", listener.local_addr().unwrap());
    let wire = Arc::new(Mutex::new(Wire::default()));
    let records = wire.clone();
    let sockets = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let wire = records.clone();
            tokio::spawn(async move {
                use tokio_tungstenite::tungstenite::{
                    Message,
                    handshake::server::{Request, Response},
                };
                let path = Arc::new(Mutex::new(String::new()));
                let seen = path.clone();
                let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(
                    stream,
                    move |request: &Request, response: Response| {
                        *seen.lock().unwrap() =
                            request.uri().path().trim_start_matches('/').to_owned();
                        Ok(response)
                    },
                )
                .await
                else {
                    return;
                };
                let name = path.lock().unwrap().clone();
                *wire
                    .lock()
                    .unwrap()
                    .connections
                    .entry(name.clone())
                    .or_default() += 1;
                if socket
                    .send(Message::Text("Acme-open".into()))
                    .await
                    .is_err()
                {
                    return;
                }
                while let Some(Ok(message)) = socket.next().await {
                    match message {
                        Message::Text(_) | Message::Binary(_) => {
                            if socket.send(message).await.is_err() {
                                break;
                            }
                        }
                        Message::Close(frame) => {
                            wire.lock().unwrap().closes.insert(
                                name.clone(),
                                frame.map_or(Value::Null, |frame| {
                                    json!({"code":u16::from(frame.code),"reason":frame.reason.to_string()})
                                }),
                            );
                            let _ = socket.flush().await;
                            break;
                        }
                        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
                    }
                }
            });
        }
    });
    Servers {
        page,
        socket,
        wire,
        tasks: vec![pages, sockets],
    }
}

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = tokio::process::Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-socket-closure-baseline.cjs"
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

async fn evaluate(
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    function: &str,
    argument: Value,
) -> Value {
    mcp.tool("browser_send", json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":function,"isFunction":true,"argument":{"kind":"json","value":argument}}})).await
}
fn parsed(result: &Value) -> Value {
    serde_json::from_str(
        result["value"]["value"]
            .as_str()
            .unwrap_or_else(|| panic!("{result}")),
    )
    .unwrap()
}
fn no_binding_callback(status: &Value) -> bool {
    let browser = &status["browser"];
    !browser["callbacks"]
        .as_array()
        .or_else(|| browser["resources"].as_array())
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "binding")
}

async fn cell(mcp: &mut Mcp, target: &str, ready: &Value, servers: &Servers, sdk: &Value) -> Value {
    let fixture: Value = serde_json::from_str(SCENARIOS).unwrap();
    let context = &ready["browser"]["selectedContext"];
    let page = &ready["browser"]["selectedPage"];
    let base = &servers.socket;
    let mut report = json!({});
    operation(
        mcp,
        target,
        json!({"operation":"diagnostics_start","context":context}),
    )
    .await;
    for scenario in fixture["scenarios"].as_array().unwrap() {
        let name = scenario["name"].as_str().unwrap();
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":servers.page,"waitUntil":"load"}})).await;
        // SDK defaults apply without `sdkDefaults`.
        let request = if scenario["regex"].is_object() {
            json!({"operation":"web_socket_route_add_matcher","scope":page,"matcher":{"kind":"regex","pattern":scenario["regex"]["source"],"flags":scenario["regex"]["flags"]}})
        } else {
            json!({"operation":"web_socket_route_add","scope":page,"url":format!("{base}/{}",scenario["routed"].as_str().unwrap())})
        };
        let route = operation(mcp, target, request).await;
        assert_eq!(route["sdkDefaults"], true, "{route}");
        let mut argument = scenario["argument"].clone();
        argument["base"] = json!(base);
        let pending = evaluate(
            mcp,
            target,
            page,
            scenario["page"].as_str().unwrap(),
            argument,
        )
        .await;
        if name == "worker" {
            // A worker's socket connects normally, without a route callback.
            let result = parsed(&completed(mcp, target, pending).await);
            let status = mcp.detailed_browser_status(target).await;
            assert!(no_binding_callback(&status), "{status}");
            report[name] = result;
        } else {
            let (callback, read) =
                socket_protocols::callback(mcp, target, &route, "onCreate").await;
            let commands = if scenario["relay"] == true {
                json!([{"type":"connect"}])
            } else {
                json!([])
            };
            socket_protocols::reply(mcp, target, callback, commands).await;
            assert!(
                read["frame"].is_object() || read["frame"].is_string(),
                "{read}"
            );
            report[name] = parsed(&completed(mcp, target, pending).await);
        }
        operation(
            mcp,
            target,
            json!({"operation":"web_socket_route_remove","scope":page,"routeId":route["routeId"]}),
        )
        .await;
    }
    // Testrun refuses one byte over 512 KiB; the SDK sends it.
    assert_eq!(report["large"]["oversized"], "RangeError", "{report}");
    assert_eq!(sdk["cases"]["large"]["oversized"], "none", "{sdk}");
    let mut compared = report.clone();
    compared["large"]["oversized"] = json!("none");
    assert_eq!(compared, sdk["cases"], "Testrun {report} SDK {sdk}");
    // The SDK's page WebSocket object maps to `web_socket` diagnostics: the
    // unrouted socket's creation, received frame and close are observable.
    let mut events = Vec::new();
    let mut request = json!({"operation":"diagnostics_read","context":context,"query":{"kind":"web_socket","limit":100}});
    loop {
        let read = operation(mcp, target, request).await;
        events.extend(read["events"].as_array().unwrap().iter().cloned());
        if read["nextRequest"].is_null() {
            break;
        }
        request = read["nextRequest"].clone();
    }
    let observed = json!(events);
    let plain = events
        .iter()
        .filter(|event| {
            event["data"]["url"]
                .as_str()
                .is_some_and(|url| url.contains("/regex-plain"))
        })
        .map(|event| {
            (
                event["data"]["phase"].clone(),
                event["data"]["direction"].clone(),
            )
        })
        .collect::<Vec<_>>();
    for expected in [
        (json!("created"), Value::Null),
        (json!("frame"), json!("received")),
        (json!("closed"), Value::Null),
    ] {
        assert!(plain.contains(&expected), "{expected:?} in {observed}");
    }
    // A reason without a code reaches the server with status 1000 on every engine.
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":servers.page,"waitUntil":"load"}})).await;
    let route = operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_add","scope":page,"url":format!("{base}/reason")}),
    )
    .await;
    let pending = evaluate(
        mcp,
        target,
        page,
        REASON,
        json!({"base":base,"path":"reason"}),
    )
    .await;
    let (callback, read) = socket_protocols::callback(mcp, target, &route, "onCreate").await;
    socket_protocols::reply(mcp, target, callback, json!([{"type":"connect"}])).await;
    assert_eq!(parsed(&completed(mcp, target, pending).await), "open");
    operation(mcp,target,json!({"operation":"web_socket_route_close","scope":page,"frame":read["frame"],"routeId":route["routeId"],"socketId":socket_field(&read["arguments"][0],"id")["value"],"to":"server","options":{"reason":"Acme reason"}})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let closed = loop {
        if let Some(close) = servers.wire.lock().unwrap().closes.get("reason").cloned() {
            break close;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Reason-only close never reached the server"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!(closed, json!({"code":1000,"reason":"Acme reason"}));
    operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_remove","scope":page,"routeId":route["routeId"]}),
    )
    .await;
    report
}

#[tokio::test]
async fn installed_step_four_socket_closure_checkpoint() {
    let fixture = installed_fixture();
    let scenarios: Value = serde_json::from_str(SCENARIOS).unwrap();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let sdk = reference(&fixture, engine, mode).await;
            assert_eq!(sdk["connections"], scenarios["connections"], "{sdk}");
            let servers = servers(&scenarios).await;
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap().to_owned();
            let ready = fixture.ready().await;
            let result =
                std::panic::AssertUnwindSafe(cell(&mut mcp, &target, &ready, &servers, &sdk))
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
                Ok(report) => {
                    let mut connections = json!(servers.wire.lock().unwrap().connections.clone());
                    assert_eq!(connections["reason"], 1, "{connections}");
                    connections.as_object_mut().unwrap().remove("reason");
                    assert_eq!(connections, scenarios["connections"], "{connections}");
                    eprintln!("Socket closure {engine} {mode}: {report}");
                }
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
        failures.is_empty(),
        "Original disconnect failures: {failures:?}"
    );
}
