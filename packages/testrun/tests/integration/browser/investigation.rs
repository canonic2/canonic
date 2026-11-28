//! Owned loopback investigation through public CLI/MCP in every initial-host cell.
use super::*;
use futures_util::FutureExt;
use tokio::io::AsyncReadExt;
mod array_buffer;
mod base_url;
mod binding_values;
mod browser_socks;
#[path = "../../fixtures/browser_http_client_certificates.rs"]
mod certificate_fixture;
mod client_certificate_paths;
mod client_certificate_selection;
mod client_certificates;
mod clock;
mod closed_call_capacity;
mod collected_callers;
mod collected_debugger;
mod collectors;
mod context_emulation;
mod context_proxy;
mod coverage_results;
mod credential_lifecycle;
mod credentials;
mod debugger_order;
mod evaluation_adoption;
mod evaluation_closure;
mod evaluation_variants;
mod event_waits;
mod failure_evidence;
#[cfg(target_os = "macos")]
mod feature_checkpoint;
mod frame_element;
mod frame_relationships;
#[path = "../../fixtures/browser_react.rs"]
mod framework_fixture;
mod framework_profiles;
mod har_clear;
mod har_fallback;
mod har_filter;
mod har_full_capture;
mod har_interleave;
mod har_page_capture;
mod har_record;
mod har_replay_filter;
mod har_sockets;
mod header_defaults;
mod http_associated_state;
mod http_concurrency;
mod http_connection_details;
mod http_cookie_admin;
mod http_cookie_filters;
mod http_credentials;
mod http_decoding;
mod http_defaults;
mod http_disposal;
mod http_projections;
mod http_proxy;
mod http_proxy_refusal;
mod http_request_fallback;
mod http_request_input_ownership;
mod http_request_inputs;
mod http_request_json_strings;
mod http_request_semantics;
mod http_socks;
mod http_storage_state;
mod http_user_agent;
mod launch_proxy;
mod lifecycle_evidence;
mod native_client_certificates;
mod native_tracing;
mod network_response;
mod object_projection;
mod observation_closure;
mod owner_getters;
mod page_audit;
mod page_tags;
mod pause_evidence;
mod performance_lifecycle;
mod property_callers;
mod redirect_headers;
mod regular_regex;
mod retained_allocation;
mod route_closure;
mod route_fetch;
mod sdk_closed_context;
mod selector_evaluation;
mod service_worker_capture;
mod service_worker_defaults;
mod service_worker_har;
mod service_worker_routes;
mod session_checkpoint;
mod socket_close_contracts;
mod socket_closure;
mod socket_layers;
mod socket_ownership;
mod socket_protocols;
mod socket_regex;
mod socket_sdk;
#[path = "../../fixtures/browser_http_socks.rs"]
mod socks_fixture;
mod source_input_diagnostic;
mod storage_closure;
mod trace_analysis;
mod trace_analysis_page;
mod trace_export;
mod trace_filenames;
mod url_matching;
mod url_wait;
#[path = "../../fixtures/browser_http_wire_inputs.rs"]
mod wire_fixture;
mod worker_credentials;
mod worker_lifetime;
type ResetCounts = std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, usize>>>;

struct Site {
    url: String,
    task: tokio::task::JoinHandle<()>,
    counts: ResetCounts,
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Site {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let resets = ResetCounts::default();
        let counts = resets.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        requests.spawn(respond(stream,resets.clone()));
                    }
                    _ = requests.join_next(), if !requests.is_empty() => {}
                }
            }
        });
        Self { url, task, counts }
    }
}
async fn respond(mut stream: tokio::net::TcpStream, _resets: ResetCounts) {
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    while bytes.len() < 8192 && !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).await.unwrap();
        if count == 0 {
            return;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    if bytes.starts_with(b"GET /mock-socket ") {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::{
            Message, handshake::derive_accept_key, protocol::Role,
        };
        let header = String::from_utf8_lossy(&bytes);
        let key = header
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("sec-websocket-key"))
                    .map(|(_, value)| value.trim())
            })
            .expect("Owned WebSocket handshake");
        let accept = derive_accept_key(key.as_bytes());
        stream.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").as_bytes()).await.unwrap();
        let mut socket =
            tokio_tungstenite::WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
        let incoming = socket.next().await.unwrap().unwrap();
        assert_eq!(incoming.to_text().unwrap(), "Acme automatic relay");
        socket
            .send(Message::Binary(vec![0, 255, 128].into()))
            .await
            .unwrap();
        while let Some(message) = socket.next().await {
            let message = message.unwrap();
            if let Message::Close(frame) = message {
                let frame = frame.expect("Owned page close frame");
                assert_eq!(u16::from(frame.code), 3007);
                assert_eq!(frame.reason, "Acme installed close");
                socket.flush().await.unwrap();
                break;
            }
        }
        return;
    }
    if bytes.starts_with(b"GET /http-encoded?") {
        http_decoding::respond(&mut stream, &bytes).await;
        return;
    }
    if bytes.starts_with(b"POST /http-payload ") {
        let end = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        let header = String::from_utf8_lossy(&bytes[..end]).into_owned();
        let length = header
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            })
            .unwrap();
        assert!(length <= 1048576);
        while bytes.len() < end + length {
            let count = stream.read(&mut chunk).await.unwrap();
            if count == 0 {
                return;
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        let content_type = header
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                    .map(|(_, value)| value.trim())
            })
            .unwrap_or("");
        let body = json!({"body":String::from_utf8_lossy(&bytes[end..end+length]),"contentType":content_type}).to_string();
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(reply.as_bytes()).await;
        return;
    }
    if bytes.starts_with(b"POST /har-filter/") {
        // Read the whole body so closing the connection cannot reset it.
        let end = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        let header = String::from_utf8_lossy(&bytes[..end]).into_owned();
        let length = header
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        while bytes.len() < end + length {
            let count = stream.read(&mut chunk).await.unwrap();
            if count == 0 {
                return;
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        let target = header.split_whitespace().nth(1).unwrap();
        let body = String::from_utf8_lossy(&bytes[end..end + length]);
        *_resets
            .lock()
            .unwrap()
            .entry(format!("POST {target} {body}"))
            .or_default() += 1;
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 12\r\nConnection: close\r\n\r\nAcme network").await.unwrap();
        return;
    }
    let request = String::from_utf8_lossy(&bytes);
    if request.starts_with("GET /har-filter/") {
        let target = request.split_whitespace().nth(1).unwrap();
        *_resets.lock().unwrap().entry(target.into()).or_default() += 1;
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 12\r\nConnection: close\r\n\r\nAcme network").await.unwrap();
        return;
    }
    #[cfg(unix)]
    if request.starts_with("GET /http-reset?") {
        let target = request.split_whitespace().nth(1).unwrap();
        let attempt = {
            let mut counts = _resets.lock().unwrap();
            assert!(counts.len() < 128);
            let count = counts.entry(target.into()).or_default();
            *count += 1;
            *count
        };
        if attempt == 1 {
            use std::os::fd::AsRawFd;
            let linger = libc::linger {
                l_onoff: 1,
                l_linger: 0,
            };
            // The fixture owns this socket; a zero linger deliberately sends RST.
            let result = unsafe {
                libc::setsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_LINGER,
                    (&linger as *const libc::linger).cast(),
                    std::mem::size_of::<libc::linger>() as libc::socklen_t,
                )
            };
            assert_eq!(result, 0);
            return;
        }
        let body = json!({"attempts":attempt}).to_string();
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(reply.as_bytes()).await;
        return;
    }
    if request.starts_with("GET /http-query?") {
        let body = json!({"target":request.split_whitespace().nth(1).unwrap()}).to_string();
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(reply.as_bytes()).await;
        return;
    }
    if request.starts_with("GET /trace-live-source.js?") {
        let body = "globalThis.AcmeTraceScript=(globalThis.AcmeTraceScript||0)+1; // Acme live private script";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        return;
    }
    if request.starts_with("GET /react-fixture.js ")
        || request.starts_with("GET /react-dom-fixture.js ")
    {
        let body = framework_fixture::asset(request.starts_with("GET /react-dom-fixture.js "));
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.write_all(&body).await;
        return;
    }
    let (status, extra, mime, body) = if request.starts_with("GET /cookie-exchange ") {
        (
            "200 OK",
            "Set-Cookie: FromHttp=Acme-http-private; Path=/; HttpOnly; SameSite=Lax\r\n",
            "text/plain",
            request
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("cookie"))
                        .map(|(_, value)| value.trim())
                })
                .unwrap_or(""),
        )
    } else if request.starts_with("GET /http-agent ") {
        (
            "200 OK",
            "",
            "text/plain",
            request
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
                        .map(|(_, value)| value.trim())
                })
                .unwrap_or(""),
        )
    } else if request.starts_with("GET /framework ") {
        ("200 OK", "", "text/html", framework_fixture::HTML)
    } else if request.starts_with("GET /tools ") {
        (
            "200 OK",
            "Content-Security-Policy: script-src 'unsafe-inline'; object-src 'none'\r\n",
            "text/html",
            "<!doctype html><title>Acme tools</title><button id=probe>Acme</button><script>globalThis.acmeCalls=0;globalThis.acmeElementCalls=0;globalThis.acmeNestedCalls=0;addEventListener('devtoolstooldiscovery',event=>event.respondWith({name:'Acme',tools:[{name:'increment',description:'Increment the fixture counter',inputSchema:{type:'object',properties:{amount:{type:'integer',minimum:1}},required:['amount'],additionalProperties:false},execute:({amount})=>{globalThis.acmeCalls++;return amount+1}},{name:'element',description:'Inspect the application element',inputSchema:{type:'object',properties:{element:{'x-mcp-type':'HTMLElement'}},required:['element'],additionalProperties:false},execute:({element})=>{acmeElementCalls++;return element.tagName}},{name:'nested',description:'Inspect nested application elements',inputSchema:{type:'object',definitions:{node:{'x-mcp-type':'HTMLElement'}},properties:{payload:{type:'array',items:{$ref:'#/definitions/node'}}},required:['payload'],additionalProperties:false},execute:({payload})=>{acmeNestedCalls++;return payload.map(node=>node.tagName).join(',')}}]}))</script>",
        )
    } else if request.starts_with("GET /performance ") {
        (
            "200 OK",
            "",
            "text/html",
            "<!doctype html><title>Acme performance</title><h1>Acme performance</h1><button id=work style='--acme:1' onclick=\"const end=performance.now()+25;while(performance.now()<end){};document.body.dataset.clicked='yes'\">Work</button>",
        )
    } else if request.starts_with("GET /auth ") {
        if request.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.eq_ignore_ascii_case("authorization")
                    && value.trim() == "Basic QWNtZTpBY21lLXBhc3N3b3Jk"
            })
        }) {
            (
                "200 OK",
                "",
                "text/html",
                "<!doctype html><title>Acme authenticated</title>",
            )
        } else {
            (
                "401 Unauthorized",
                "WWW-Authenticate: Basic realm=\"Acme\"\r\n",
                "text/html",
                "<!doctype html><title>Acme denied</title>",
            )
        }
    } else if request.starts_with("GET /redirect?") {
        (
            "302 Found",
            "Location: /failure?token=Acme-private-token\r\n",
            "text/plain",
            "redirect",
        )
    } else if request.starts_with("GET /failure?")
        || request.starts_with("GET /other?")
        || request.starts_with("POST /failure?")
    {
        (
            "409 Conflict",
            "",
            "application/json",
            "{\"error\":\"Acme-private-body\"}",
        )
    } else if request.starts_with("GET /favicon.ico ") {
        ("204 No Content", "", "text/plain", "")
    } else {
        (
            "200 OK",
            "",
            "text/html",
            "<!doctype html><title>Acme investigation</title><button id=count onclick='globalThis.acmeCounterClicks++;console.count(\"Acme counter\")'>Count</button><button id=dialog onclick='setTimeout(()=>{alert(\"Acme dialog\");throw new Error(\"Acme callback error\")},100)'>Dialog</button><script>globalThis.acmeCounterClicks=0;console.error('Acme-private-console'); function acmeDiagnosticFailure(){throw new Error('Acme-private-error')}setTimeout(acmeDiagnosticFailure,0);fetch('/other?token=Acme-private-token');fetch('/redirect?token=Acme-private-token');</script>",
        )
    };
    let response = format!(
        "HTTP/1.1 {status}\r\n{extra}Content-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

async fn operation(mcp: &mut Mcp, target: &str, request: Value) -> Value {
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":request}))
        .await;
    completed(mcp, target, submitted).await
}
async fn completed(mcp: &mut Mcp, target: &str, submitted: Value) -> Value {
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(status["operation"]["settled"], true, "{status}");
    if status["operation"]["outcome"] != "succeeded" {
        let mut calls = Value::Null;
        if let Some(context) = status["browser"]["automationDebuggers"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("context"))
        {
            let read=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"context_debugger","context":context,"action":{"kind":"calls","limit":32,"includeDetails":true}}})).await;
            calls = mcp
                .tool(
                    "browser_status",
                    json!({"targetId":target,"operationId":read["operation"]["id"],"waitMs":1000}),
                )
                .await["operation"]
                .clone();
        }
        panic!(
            "Installed operation failed: {}; engine: {}; mode: {}; input: {}; automation calls: {calls}",
            status["operation"],
            status["browser"]["engine"],
            status["browser"]["identity"]["mode"],
            status["browser"]["input"]
        );
    }
    status["operation"]["result"].clone()
}

fn installed_fixture() -> Fixture {
    use std::os::unix::fs::PermissionsExt;
    let binary =
        std::env::var_os("TESTRUN_TEST_INSTALLED_BINARY").expect("Select an installed binary");
    assert!(std::path::Path::new(&binary).is_absolute());
    let workspace = crate::Workspace::new();
    std::fs::set_permissions(workspace.path(), std::fs::Permissions::from_mode(0o700))
        .expect("Private installed browser fixture");
    Fixture {
        workspace,
        cache: std::env::var_os("TESTRUN_BROWSER_ROOT")
            .expect("Select a private browser cache")
            .into(),
    }
}

#[tokio::test]
async fn installed_step_four_socket_close_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(installed_socket_route(
                &fixture,
                &mut mcp,
                &site.url,
                target,
                &ready["browser"]["selectedContext"],
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
    assert!(
        cleanup_failures.is_empty(),
        "Original cleanup failures: {cleanup_failures:?}"
    );
}

#[tokio::test]
async fn installed_step_four_automation_debugger_firefox_headed() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let connected = fixture
        .cli(&[
            "browser", "connect", "--engine", "firefox", "--mode", "headed",
        ])
        .await;
    let target = connected["targetId"].as_str().unwrap();
    let ready = fixture.ready().await;
    let result = std::panic::AssertUnwindSafe(installed_automation_debugger(
        &fixture,
        &mut mcp,
        target,
        &ready["browser"]["selectedPage"],
    ))
    .catch_unwind()
    .await;
    if result.is_err() {
        let status = mcp.detailed_browser_status(target).await;
        let mut observation = json!({"input":status["browser"]["input"],"debuggers":status["browser"]["automationDebuggers"],"trace":status["browser"]["trace"]});
        if let Some(page) = status["browser"]["input"]["pages"]
            .as_array()
            .and_then(|pages| pages.first())
            .and_then(|page| page.get("page"))
        {
            let submitted=mcp.request("tools/call",json!({"name":"browser_send","arguments":{"targetId":target,"timeoutMs":3000,"request":{"operation":"page_read","page":page,"locator":{"kind":"css","selector":"#Acme-private-step"},"read":"state","maxBytes":4096}}})).await;
            observation["renderer"] = if let Some(id) =
                submitted["result"]["structuredContent"]["data"]["operation"]["id"].as_str()
            {
                mcp.request("tools/call",json!({"name":"browser_status","arguments":{"targetId":target,"operationId":id,"waitMs":3000}})).await
            } else {
                submitted
            };
        }
        std::fs::write(
            fixture
                .workspace
                .path()
                .join("debugger-failure-observations.json"),
            serde_json::to_vec_pretty(&observation).unwrap(),
        )
        .unwrap();
        eprintln!("Debugger failure observations: {observation}");
    }
    mcp.tool("disconnect", json!({"targetId":target})).await;
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn installed_step_four_framework_registry_owner_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(installed_framework(
                &fixture,
                &mut mcp,
                &site.url,
                target,
                &ready["browser"]["selectedPage"],
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
    assert!(
        cleanup_failures.is_empty(),
        "Original cleanup failures: {cleanup_failures:?}"
    );
}

#[tokio::test]
async fn installed_step_four_diagnostic_history_cli_mcp_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap().to_owned();
            let ready = fixture.ready().await;
            let result = std::panic::AssertUnwindSafe(exercise(
                &fixture,
                &mut mcp,
                &site.url,
                &target,
                ready["browser"].clone(),
            ))
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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let result = fixture.cli(&["manager", "stop"]).await;
        if result["retired"] == true {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{result}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Every diagnostic event matching `query` after sequence `after`. Reads take no
/// numeric start, so page retained history with `nextRequest` and filter.
async fn diagnostics_after(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    after: &Value,
    mut query: Value,
) -> Value {
    let after = after.as_u64().unwrap_or_else(|| panic!("Sequence {after}"));
    query["limit"] = json!(100);
    let mut request = json!({"operation":"diagnostics_read","context":context,"query":query});
    let mut events = Vec::new();
    loop {
        let mut read = operation(mcp, target, request).await;
        events.extend(
            read["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| {
                    event["sequence"]
                        .as_u64()
                        .is_some_and(|sequence| sequence > after)
                })
                .cloned(),
        );
        if read["nextRequest"].is_null() {
            read["events"] = json!(events);
            return read;
        }
        request = read["nextRequest"].clone();
    }
}
async fn installed_console_arguments(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
) {
    let request = json!({"operation":"diagnostics_arguments_set","context":context,"enabled":true})
        .to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    completed(mcp, target, submitted).await;
    let before = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{globalThis.acmeArgumentReads=0;console.log({name:'Acme private argument',get counted(){return ++globalThis.acmeArgumentReads}})}"})).await;
    let history = diagnostics_after(
        mcp,
        target,
        context,
        &before["latestSequence"],
        json!({"kind":"console"}),
    )
    .await;
    assert!(!history.to_string().contains("Acme private argument"));
    let event = &history["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["data"]["arguments"]["retained"] == true)
        .unwrap();
    let exact=operation(mcp,target,json!({"operation":"diagnostics_read","context":context,"query":{"sequence":event["sequence"],"kind":"console","levels":["LOG"]}})).await;
    assert_eq!(exact["events"].as_array().unwrap().len(), 1);
    assert_eq!(exact["events"][0], **event);
    let group = &exact["events"][0]["data"]["arguments"]["groupId"];
    let request=json!({"operation":"diagnostics_arguments_read","context":context,"groupId":group,"query":{"index":0,"path":["name"]}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    assert_eq!(
        completed(mcp, target, submitted).await["value"]["value"],
        "Acme private argument"
    );
    let read=operation(mcp,target,json!({"operation":"diagnostics_arguments_read","context":context,"groupId":group,"query":{"index":0,"path":["counted"]}})).await;
    assert_eq!(read["value"]["value"], 1);
    assert_eq!(read["potentiallyMutating"], true);
    let count=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"acmeArgumentReads","isFunction":false})).await;
    assert_eq!(count["value"]["value"], 1);
    operation(
        mcp,
        target,
        json!({"operation":"diagnostics_arguments_remove","context":context,"groupId":group}),
    )
    .await;
    let disabled = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_arguments_set","context":context,"enabled":false}),
    )
    .await;
    assert!(
        disabled["groups"].as_array().unwrap().is_empty(),
        "{disabled}"
    );
}

async fn exercise(fixture: &Fixture, mcp: &mut Mcp, url: &str, target: &str, ready: Value) {
    let context = &ready["selectedContext"];
    let page = &ready["selectedPage"];
    installed_context_scripts(fixture, mcp, url, target, context, page).await;
    installed_socket_route(fixture, mcp, url, target, context).await;
    startup_scripts_and_interception(
        fixture,
        mcp,
        url,
        target,
        page,
        ready["backend"] == "chrome_cdp",
    )
    .await;
    installed_route_handlers(fixture, mcp, url, target, page, context).await;
    let start = json!({"operation":"diagnostics_start","context":context}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &start])
        .await;
    let started = completed(mcp, target, submitted).await;
    assert_eq!(started["active"], true);
    installed_console_arguments(fixture, mcp, target, context, page).await;
    let after = &started["latestSequence"];
    let wait = json!({"operation":"diagnostics_wait","context":context,"after":after,"conditions":[{"event":"response","page":page,"method":"GET","status":409,"url":format!("{url}/failure")}]}).to_string();
    let waiting = fixture
        .cli(&["browser", "send", "--target", target, &wait])
        .await;
    operation(
        mcp,
        target,
        json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":url}}),
    )
    .await;
    let removed=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"typeof acmeInvestigation","isFunction":false})).await;
    assert_eq!(removed["value"]["value"], "undefined");
    let matched = completed(mcp, target, waiting).await;
    let request = &matched["event"]["data"]["requestId"];
    operation(mcp, target, json!({"operation":"diagnostics_wait","context":context,"after":after,"conditions":[{"event":"request_finished","requestId":request}]})).await;
    operation(mcp, target, json!({"operation":"diagnostics_wait","context":context,"after":after,"conditions":[{"event":"page_error","page":page}]})).await;
    let metadata =
        diagnostics_after(mcp, target, context, after, json!({"kind":"page_error"})).await;
    assert!(!metadata.to_string().contains("acmeDiagnosticFailure"));
    // The CLI submits the private stack read; it starts at retained history.
    let stack_read=json!({"operation":"diagnostics_read","context":context,"query":{"kind":"page_error","includeStack":true,"limit":100}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &stack_read])
        .await;
    let stacks = completed(mcp, target, submitted).await;
    let first = after.as_u64().unwrap();
    let errors = stacks["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| {
            event["sequence"]
                .as_u64()
                .is_some_and(|sequence| sequence > first)
        })
        .cloned()
        .collect::<Vec<_>>();
    assert!(!errors.is_empty(), "{stacks}");
    assert!(
        errors
            .iter()
            .any(|event| event["data"]["stackIncluded"] == true
                && event["data"]["stack"]
                    .to_string()
                    .contains("acmeDiagnosticFailure")),
        "{stacks}"
    );
    assert!(
        errors
            .iter()
            .all(|event| event["data"].get("text").is_none())
    );
    let read = diagnostics_after(
        mcp,
        target,
        context,
        after,
        json!({"requestId":request,"resourceTypes":["FETCH"]}),
    )
    .await;
    assert!(!read.to_string().contains("Acme-private"), "{read}");
    let body=operation(mcp,target,json!({"operation":"network_read","context":context,"requestId":request,"options":{"body":"response"}})).await;
    installed_evidence_route(fixture, mcp, url, target, page, &body["body"]["evidenceId"]).await;
    operation(
        mcp,
        target,
        json!({"operation":"network_evidence_remove","evidenceId":body["body"]["evidenceId"]}),
    )
    .await;
    installed_har_route(fixture, mcp, url, target, context, page).await;
    installed_har_capture(fixture, mcp, url, target, page, context, false).await;
    let events = read["events"].as_array().unwrap();
    assert!(events.iter().any(|event| event["kind"] == "request"));
    assert!(
        events
            .iter()
            .any(|event| event["kind"] == "response" && event["data"]["status"] == 409)
    );
    let predecessor = events
        .iter()
        .find(|event| event["kind"] == "request")
        .unwrap()["data"]["redirectedFrom"]
        .clone();
    assert!(predecessor.is_string(), "{read}");
    assert_ne!(&predecessor, request);
    let redirect = diagnostics_after(
        mcp,
        target,
        context,
        after,
        json!({"requestId":predecessor,"kind":"response"}),
    )
    .await;
    assert_eq!(redirect["events"][0]["data"]["status"], 302);
    let console = diagnostics_after(
        mcp,
        target,
        context,
        after,
        json!({"kind":"console","includeText":true}),
    )
    .await;
    assert!(console["events"].as_array().unwrap().iter().any(|event| {
        event["data"]["text"]
            .as_str()
            .is_some_and(|text| text.contains("Acme-private-console"))
    }));
    installed_request_correlation(
        fixture,
        mcp,
        url,
        target,
        context,
        page,
        ready["backend"] == "chrome_cdp",
    )
    .await;
    let clicks=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"acmeCounterClicks","isFunction":false})).await;
    assert_eq!(
        clicks["value"]["value"], 0,
        "Unexpected counter execution before requested clicks: {clicks}"
    );
    let counter_start = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    for _ in 0..2 {
        operation(mcp, target, json!({"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#count"},"action":{"kind":"click"}})).await;
    }
    let clicks=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"acmeCounterClicks","isFunction":false})).await;
    assert_eq!(
        clicks["value"]["value"], 2,
        "Requested clicks executed an unexpected number of application handlers: {clicks}"
    );
    // Page from retained history; the paging checks below cover every page.
    let mut counts = operation(mcp, target, json!({"operation":"diagnostics_read","context":context,"query":{"kind":"console","includeText":true}})).await;
    let mut counter_events = counts["events"].as_array().unwrap().clone();
    let mut pages = 1;
    while !counts["nextRequest"].is_null() {
        assert!(
            counts["nextCursor"].is_string(),
            "Diagnostic page offered a next request without a cursor: {counts}"
        );
        pages += 1;
        assert!(pages <= 512, "Console history did not terminate: {counts}");
        counts = operation(mcp, target, counts["nextRequest"].clone()).await;
        counter_events.extend(counts["events"].as_array().unwrap().iter().cloned());
    }
    assert!(counts["nextCursor"].is_null(), "{counts}");
    assert!(
        counter_events.windows(2).all(|pair| {
            pair[0]["sequence"].as_u64().unwrap() < pair[1]["sequence"].as_u64().unwrap()
        }),
        "Diagnostic pages did not advance: {counter_events:?}"
    );
    let first = counter_start["latestSequence"].as_u64().unwrap();
    counter_events.retain(|event| {
        event["sequence"]
            .as_u64()
            .is_some_and(|sequence| sequence > first)
    });
    counts["events"] = json!(counter_events);
    let counter_messages = counts["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["data"]["text"].as_str())
        .filter(|text| text.starts_with("Acme counter"))
        .collect::<Vec<_>>();
    assert_eq!(counter_messages.len(), 2, "{counts}");
    // The pinned Firefox diagnostic path returns only the count label. Numeric
    // counter preservation tests Chrome's new object-group cleanup specifically.
    if ready["backend"] == "chrome_cdp" {
        for expected in ["1", "2"] {
            assert!(
                counts["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|event| event["data"]["text"]
                        .as_str()
                        .and_then(|text| text.strip_prefix("Acme counter:"))
                        .is_some_and(|count| count.trim() == expected)),
                "{counts}"
            );
        }
    }
    let before = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    let batch = mcp.tool("browser_send", json!({"targetId":target,"request":{"operation":"batch","requests":[
        {"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#dialog"},"action":{"kind":"click"}},
        {"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"page_error","page":page}]}
    ]}})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let callback = browser["callbacks"]
            .as_array()
            .and_then(|callbacks| callbacks.first())
            .or_else(|| {
                browser["resources"].as_array().and_then(|resources| {
                    resources
                        .iter()
                        .find(|resource| resource["kind"] == "dialog")
                })
            });
        if let Some(callback) = callback {
            break callback["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Dialog unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    operation(mcp, target, json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"dialog","accept":true}})).await;
    let batch = completed(mcp, target, batch).await;
    assert_eq!(batch["completedActions"], 2);
    let stopped = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_stop","context":context}),
    )
    .await;
    assert_eq!(stopped["active"], false);
    assert_eq!(stopped["subscriptionPending"], false);
    if ready["backend"] != "chrome_cdp" {
        installed_trace_archive(fixture, mcp, target, context, page).await;
        installed_automation_debugger(fixture, mcp, target, page).await;
    }
}

async fn installed_automation_pause(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    old: Option<&Value>,
    caller: (&str, &Value, &Value),
) -> Value {
    let evidence = pause_evidence::attempt(fixture, target, context, old, caller);
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = operation(
            mcp,
            target,
            json!({"operation":"context_debugger","context":context,"action":{"kind":"status"}}),
        )
        .await;
        if status["paused"] == true && old.is_none_or(|old| status["pauseId"] != *old) {
            assert!(status.get("details").is_none());
            return status["pauseId"].clone();
        }
        if tokio::time::Instant::now() >= until {
            pause_evidence::capture(mcp, target, context, &evidence, &status, caller).await;
            panic!("Automation pause unavailable: {status}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn installed_source_run_to(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
) {
    let source_file = "src/Acme-journey.rs";
    let location = json!({"file":"Acme-journey.rs","line":30,"column":3});
    let click = |file: &str, line, column| json!({"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#Acme-private-step"},"action":{"kind":"at_source","source":{"file":file,"line":line,"column":column},"action":{"kind":"click"}}});
    operation(
        mcp,
        target,
        json!({"operation":"context_debugger","context":context,"action":{"kind":"pause"}}),
    )
    .await;
    let first = mcp
        .tool(
            "browser_send",
            json!({"targetId":target,"request":click(source_file,10,3)}),
        )
        .await;
    let first_pause = installed_automation_pause(
        fixture,
        mcp,
        target,
        context,
        None,
        ("source-first", &first, page),
    )
    .await;
    let run=json!({"operation":"context_debugger","context":context,"action":{"kind":"run_to","pauseId":first_pause,"location":location}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &run])
        .await;
    assert_eq!(
        completed(mcp, target, submitted).await["lastControl"]["action"],
        "run_to"
    );
    completed(mcp, target, first).await;
    for (file, line, column) in [
        (source_file, 20, 3),
        ("src/Acme-other.rs", 30, 3),
        (source_file, 30, 4),
    ] {
        operation(mcp, target, click(file, line, column)).await;
        let status = operation(
            mcp,
            target,
            json!({"operation":"context_debugger","context":context,"action":{"kind":"status"}}),
        )
        .await;
        assert_ne!(status["paused"], true);
    }
    let count=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"acmeDebuggerClicks","isFunction":false})).await;
    assert_eq!(count["value"]["value"], 6);
    let matching = mcp
        .tool(
            "act",
            json!({"targetId":target,"actions":[{"action":"page_action","page":page,"locator":{"kind":"css","selector":"#Acme-private-step"},"input":{"action":"at_source","source":{"file":source_file,"line":30,"column":3},"input":{"action":"click"}}}]}),
        )
        .await;
    let matching_pause = installed_automation_pause(
        fixture,
        mcp,
        target,
        context,
        Some(&first_pause),
        ("source-matching", &matching, page),
    )
    .await;
    let metadata = operation(
        mcp,
        target,
        json!({"operation":"context_debugger","context":context,"action":{"kind":"status"}}),
    )
    .await;
    assert!(!metadata.to_string().contains("Acme-journey"));
    let details=operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"status","includeDetails":true}})).await;
    assert_eq!(
        details["details"]["location"],
        json!({"file":source_file,"line":30,"column":3})
    );
    let stale=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"context_debugger","context":context,"action":{"kind":"run_to","pauseId":first_pause,"location":location}}})).await;
    let stale = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":stale["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(stale["operation"]["code"], "STALE_AUTOMATION_PAUSE");
    operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"resume","pauseId":matching_pause}})).await;
    completed(mcp, target, matching).await;
    let count=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"acmeDebuggerClicks","isFunction":false})).await;
    assert_eq!(count["value"]["value"], 7);
    let calls=operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"calls","limit":32,"includeDetails":true}})).await;
    assert!(
        calls["calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| call["status"] == "success"
                && call["details"]["location"] == json!({"file":source_file,"line":30,"column":3})),
        "{calls}"
    );
    let metadata=operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"calls","limit":32}})).await;
    assert!(!metadata.to_string().contains("Acme-journey"));
}

async fn installed_automation_debugger(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    sibling: &Value,
) {
    let (context, page, click) = installed_debugger_boundaries(fixture, mcp, target).await;
    installed_source_run_to(fixture, mcp, target, &context, &page).await;
    installed_debugger_paused_close(fixture, mcp, target, sibling, &context, &page, click).await;
}

async fn installed_debugger_boundaries(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
) -> (Value, Value, Value) {
    let context =
        operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    let html = pause_evidence::observed_html(
        "<script>globalThis.acmeDebuggerClicks=0</script><button id=Acme-private-step onclick='acmeDebuggerClicks++'>Acme</button>",
        "#Acme-private-step",
    );
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":html,"waitUntil":"load"}})).await;
    let enable =
        json!({"operation":"context_debugger","context":context,"action":{"kind":"enable"}})
            .to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &enable])
        .await;
    assert_eq!(
        completed(mcp, target, submitted).await["callCollectionEnabled"],
        true
    );
    operation(
        mcp,
        target,
        json!({"operation":"trace_start","context":context,"options":{"snapshots":true}}),
    )
    .await;
    let arm = json!({"operation":"context_debugger","context":context,"action":{"kind":"pause"}})
        .to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &arm])
        .await;
    completed(mcp, target, submitted).await;
    let click = json!({"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#Acme-private-step"},"action":{"kind":"click"}});
    let first = mcp
        .tool("browser_send", json!({"targetId":target,"request":click}))
        .await;
    let first_pause = installed_automation_pause(
        fixture,
        mcp,
        target,
        &context,
        None,
        ("boundary-first", &first, &page),
    )
    .await;
    let details=operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"status","includeDetails":true}})).await;
    assert!(details["details"].is_object());
    assert!(
        details["details"]["title"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("click"),
        "Internal preflight consumed the caller pause: {details}"
    );
    assert_eq!(details["javascriptDebugger"], false);
    for read in ["count", "text", "geometry"] {
        operation(mcp,target,json!({"operation":"page_read","page":page,"locator":{"kind":"css","selector":"#Acme-private-step"},"read":read,"maxBytes":8192})).await;
    }
    let stopped = operation(mcp, target, json!({"operation":"trace_stop"})).await;
    let report = &stopped["reports"][0];
    assert_eq!(report["format"], "playwright-trace-zip");
    let mut archive = zip::ZipArchive::new(
        std::fs::File::open(report["attachment"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert!(
        archive
            .file_names()
            .any(|name| name.is_ok_and(|name| name == "trace.trace"))
    );
    let mut trace = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("trace.trace").unwrap(), &mut trace)
        .unwrap();
    for line in trace.lines() {
        let event: Value = serde_json::from_str(line).unwrap();
        assert!(
            !matches!(
                event["method"].as_str(),
                Some(
                    "evaluateExpression"
                        | "queryCount"
                        | "evalOnSelector"
                        | "frameElement"
                        | "isVisible"
                )
            ),
            "Host probe entered the SDK trace: {event}"
        );
    }
    let paused = operation(
        mcp,
        target,
        json!({"operation":"context_debugger","context":context,"action":{"kind":"status"}}),
    )
    .await;
    assert_eq!(paused["pauseId"], first_pause);
    operation(
        mcp,
        target,
        json!({"operation":"developer_evidence_remove","evidenceId":report["evidenceId"]}),
    )
    .await;
    let next=json!({"operation":"context_debugger","context":context,"action":{"kind":"next","pauseId":first_pause}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &next])
        .await;
    completed(mcp, target, submitted).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let progress = mcp
            .tool(
                "browser_status",
                json!({"targetId":target,"operationId":first["operation"]["id"]}),
            )
            .await;
        if progress["operation"]["settled"] == true {
            break;
        }
        let status = operation(
            mcp,
            target,
            json!({"operation":"context_debugger","context":context,"action":{"kind":"status"}}),
        )
        .await;
        if status["paused"] == true && status["pauseId"] != first_pause {
            operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"resume","pauseId":status["pauseId"]}})).await;
            break;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Stepped installed action did not settle: {progress}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    completed(mcp, target, first).await;
    let observed=operation(mcp,target,json!({"operation":"page_read","page":page,"locator":{"kind":"css","selector":"#Acme-private-step"},"read":"text","maxBytes":4096})).await;
    assert!(observed["result"]["value"].is_string(), "{observed}");
    let status = operation(
        mcp,
        target,
        json!({"operation":"context_debugger","context":context,"action":{"kind":"status"}}),
    )
    .await;
    assert_ne!(
        status["paused"], true,
        "Read consumed the pending step: {status}"
    );
    if status["pauseRequested"] != true {
        operation(
            mcp,
            target,
            json!({"operation":"context_debugger","context":context,"action":{"kind":"pause"}}),
        )
        .await;
    }
    let second = mcp
        .tool("browser_send", json!({"targetId":target,"request":click}))
        .await;
    let second_pause = installed_automation_pause(
        fixture,
        mcp,
        target,
        &context,
        Some(&first_pause),
        ("boundary-second", &second, &page),
    )
    .await;
    let stale=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"context_debugger","context":context,"action":{"kind":"resume","pauseId":first_pause}}})).await;
    let stale = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":stale["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(stale["operation"]["code"], "STALE_AUTOMATION_PAUSE");
    operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"resume","pauseId":second_pause}})).await;
    completed(mcp, target, second).await;
    let count=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"acmeDebuggerClicks","isFunction":false})).await;
    assert_eq!(count["value"]["value"], 2);
    let read=json!({"operation":"context_debugger","context":context,"action":{"kind":"calls","limit":32}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &read])
        .await;
    let metadata = completed(mcp, target, submitted).await;
    assert!(!metadata.to_string().contains("Acme-private"));
    assert!(
        metadata["calls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|call| call.get("details").is_none() && call.get("logs").is_none())
    );
    let calls=operation(mcp,target,json!({"operation":"context_debugger","context":context,"action":{"kind":"calls","limit":32,"includeDetails":true}})).await;
    assert_eq!(
        calls["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["details"]["title"]
                .as_str()
                .is_some_and(|title| title.to_ascii_lowercase().contains("evaluate")))
            .count(),
        1,
        "Only the caller evaluation belongs in call collection: {calls}"
    );
    assert!(
        calls["calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| call["status"] == "success"
                && call["details"]["title"]
                    .as_str()
                    .is_some_and(|title| title.to_ascii_lowercase().contains("click"))),
        "{calls}"
    );
    assert!(
        calls["calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| !call["logs"].as_array().unwrap().is_empty()),
        "{calls}"
    );
    (context, page, click)
}

async fn installed_debugger_paused_close(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    sibling: &Value,
    context: &Value,
    page: &Value,
    click: Value,
) {
    operation(
        mcp,
        target,
        json!({"operation":"context_debugger","context":context,"action":{"kind":"pause"}}),
    )
    .await;
    let pending = mcp
        .tool("browser_send", json!({"targetId":target,"request":click}))
        .await;
    installed_automation_pause(
        fixture,
        mcp,
        target,
        &context,
        None,
        ("paused-close", &pending, page),
    )
    .await;
    let close = json!({"operation":"context_close","context":context}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &close])
        .await;
    completed(mcp, target, submitted).await;
    let closed = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":pending["operation"]["id"],"waitMs":5000}),
        )
        .await;
    assert_eq!(closed["operation"]["settled"], true);
    assert_ne!(closed["operation"]["outcome"], "succeeded");
    let sibling = operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":sibling,"expression":"6*7","isFunction":false}),
    )
    .await;
    assert_eq!(sibling["value"]["value"], 42);
}

async fn installed_debugger_boundaries_cell(engine: &str, mode: &str) {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let connected = fixture
        .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
        .await;
    let target = connected["targetId"].as_str().unwrap();
    let ready = fixture.ready().await;
    let result=std::panic::AssertUnwindSafe(async {
        let (context,_,_)=installed_debugger_boundaries(&fixture,&mut mcp,target).await;
        operation(&mut mcp,target,json!({"operation":"context_close","context":context})).await;
        let sibling=operation(&mut mcp,target,json!({"operation":"evaluate","scope":ready["browser"]["selectedPage"],"expression":"6*7","isFunction":false})).await;
        assert_eq!(sibling["value"]["value"],42);
    }).catch_unwind().await;
    mcp.tool("disconnect", json!({"targetId":target})).await;
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn installed_step_four_debugger_boundaries_firefox_headless() {
    installed_debugger_boundaries_cell("firefox", "headless").await;
}
#[tokio::test]
async fn installed_step_four_debugger_boundaries_firefox_headed() {
    installed_debugger_boundaries_cell("firefox", "headed").await;
}
#[tokio::test]
async fn installed_step_four_debugger_boundaries_webkit_headless() {
    installed_debugger_boundaries_cell("webkit", "headless").await;
}
#[tokio::test]
async fn installed_step_four_debugger_boundaries_webkit_headed() {
    installed_debugger_boundaries_cell("webkit", "headed").await;
}

async fn installed_trace_archive(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
) {
    let start=json!({"operation":"trace_start","context":context,"options":{"snapshots":true,"maxDurationMs":30000}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &start])
        .await;
    assert_eq!(
        completed(mcp, target, submitted).await["format"],
        "playwright-trace-zip"
    );
    operation(
        mcp,
        target,
        json!({"operation":"trace_group_start","name":"Acme installed trace group"}),
    )
    .await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"console.log('Acme installed private trace');42","isFunction":false})).await;
    operation(mcp, target, json!({"operation":"trace_group_end"})).await;
    let first = operation(mcp, target, json!({"operation":"trace_chunk_stop"})).await;
    assert_eq!(first["reports"][0]["format"], "playwright-trace-zip");
    let chunk =
        json!({"operation":"trace_chunk_start","title":"Acme installed second chunk"}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &chunk])
        .await;
    completed(mcp, target, submitted).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"document.title","isFunction":false})).await;
    let stop = json!({"operation":"trace_stop"}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &stop])
        .await;
    let stopped = completed(mcp, target, submitted).await;
    assert_eq!(stopped["active"], false);
    assert_eq!(stopped["reports"].as_array().unwrap().len(), 2);
    assert_eq!(
        operation(mcp, target, json!({"operation":"trace_stop"})).await,
        stopped
    );
    for (index, report) in stopped["reports"].as_array().unwrap().iter().enumerate() {
        use std::io::Read;
        assert_eq!(report["complete"], true);
        assert_eq!(report["format"], "playwright-trace-zip");
        let bytes = std::fs::read(report["attachment"]["path"].as_str().unwrap()).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut trace = String::new();
        zip.by_name("trace.trace")
            .unwrap()
            .read_to_string(&mut trace)
            .unwrap();
        assert!(trace.contains(if index == 0 {
            "Acme installed trace group"
        } else {
            "Acme installed second chunk"
        }));
        operation(
            mcp,
            target,
            json!({"operation":"developer_evidence_remove","evidenceId":report["evidenceId"]}),
        )
        .await;
    }
}

async fn installed_request_correlation(
    fixture: &Fixture,
    mcp: &mut Mcp,
    base: &str,
    target: &str,
    context: &Value,
    page: &Value,
    native: bool,
) {
    let before = operation(
        mcp,
        target,
        json!({"operation":"diagnostics_read","context":context}),
    )
    .await;
    let expression = format!(
        "async()=>{{await Promise.all([fetch({},{{method:'POST'}}),fetch({},{{method:'POST'}})]);console.error('Acme ambiguous 409')}}",
        json!(format!("{base}/failure?token=Acme-correlation-private-a")),
        json!(format!("{base}/failure?token=Acme-correlation-private-b"))
    );
    operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}),
    )
    .await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let (console, network, responses) = loop {
        let read = diagnostics_after(
            mcp,
            target,
            context,
            &before["latestSequence"],
            json!({"page":page,"kind":"console","levels":["ERROR"],"includeText":true}),
        )
        .await;
        let events = read["events"].as_array().unwrap();
        let console = events
            .iter()
            .find(|event| event["data"]["text"] == "Acme ambiguous 409")
            .cloned();
        let network = events
            .iter()
            .find(|event| {
                event["data"]["source"] == "network"
                    && event["data"]["location"]["url"] == format!("{base}/failure")
            })
            .cloned();
        let responses = diagnostics_after(mcp,target,context,&before["latestSequence"],json!({"page":page,"kind":"response","method":"POST","status":409,"url":format!("{base}/failure")})).await;
        if let Some(console) = console
            && (!native || network.is_some())
            && responses["events"].as_array().unwrap().len() == 2
        {
            break (console, network, responses);
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Console correlation source unavailable: {read}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let latest=operation(mcp,target,json!({"operation":"diagnostics_read","context":context,"query":{"through":responses["latestSequence"],"latest":true,"page":page,"kind":"response","method":"POST","status":409,"url":format!("{base}/failure")}})).await;
    assert_eq!(latest["events"].as_array().unwrap().len(), 1);
    assert_eq!(
        &latest["events"][0],
        responses["events"].as_array().unwrap().last().unwrap()
    );
    assert_eq!(
        latest["observedRange"]["throughSequence"],
        responses["latestSequence"]
    );
    assert!(latest["nextCursor"].is_null(), "{latest}");
    assert!(latest["nextRequest"].is_null(), "{latest}");
    assert!(!latest.to_string().contains("Acme-correlation-private"));
    let annotated=operation(mcp,target,json!({"operation":"diagnostics_read","context":context,"query":{"sequence":latest["events"][0]["sequence"],"ignoreRules":[{"kind":"response","requestId":latest["events"][0]["data"]["requestId"],"page":page,"status":409,"reason":"Expected fixture conflict"}]}})).await;
    assert_eq!(annotated["events"][0]["data"]["status"], 409);
    assert_eq!(annotated["events"][0]["data"]["ignored"], true);
    assert_eq!(
        annotated["events"][0]["data"]["ignoreRules"][0]["reason"],
        "Expected fixture conflict"
    );
    assert_eq!(
        annotated["events"][0]["data"]["ignoreRules"][0]["scope"]["context"],
        *context
    );
    assert!(latest["events"][0]["data"].get("ignored").is_none());
    let request=json!({"operation":"diagnostics_read","context":context,"query":{"sequence":console["sequence"],"correlation":{"windowMs":60000,"limit":4}}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let read = completed(mcp, target, submitted).await;
    let correlation = &read["events"][0]["data"]["requestCorrelation"];
    assert_eq!(correlation["ambiguous"], true);
    assert_eq!(correlation["confidence"], "candidate-only");
    assert_eq!(correlation["causalProof"], false);
    assert!(correlation["candidateCount"].as_u64().unwrap() >= 2);
    assert!(correlation["exactRequestId"].is_null());
    assert!(!read.to_string().contains("Acme-correlation-private"));
    assert!(!read.to_string().contains("Acme ambiguous 409"));
    if let Some(network) = network {
        let read=operation(mcp,target,json!({"operation":"diagnostics_read","context":context,"query":{"sequence":network["sequence"],"correlation":{}}})).await;
        let correlation = &read["events"][0]["data"]["requestCorrelation"];
        if network["data"]["requestId"].is_string() {
            assert_eq!(correlation["exactRequestId"], network["data"]["requestId"]);
            assert_eq!(correlation["confidence"], "source-identity");
            assert_eq!(correlation["ambiguous"], false);
        } else {
            assert_eq!(correlation["confidence"], "candidate-only");
            assert_eq!(correlation["ambiguous"], true);
        }
        assert!(!read.to_string().contains("Acme-correlation-private"));
    }
}

async fn installed_route_handlers(
    fixture: &Fixture,
    mcp: &mut Mcp,
    base: &str,
    target: &str,
    page: &Value,
    context: &Value,
) {
    let url = format!("{base}/handler-chain?token=Acme-route-token");
    let other_url = format!("{base}/handler-other?token=Acme-route-token");
    let mut other_order = Vec::new();
    let mut order = Vec::new();
    for scope in [page, context] {
        let added=operation(mcp,target,json!({"operation":"network_route","scope":scope,"action":{"kind":"add","url":other_url}})).await;
        other_order.push(added["handlerId"].clone());
        let mut ids = Vec::new();
        for _ in 0..2 {
            let request = json!({"operation":"network_route","scope":scope,"action":{"kind":"add","url":url}}).to_string();
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &request])
                .await;
            ids.push(completed(mcp, target, submitted).await["handlerId"].clone());
        }
        order.extend(ids.into_iter().rev());
    }
    let expression = format!("async()=>await (await fetch({})).text()", json!(url));
    let pending = mcp.tool("browser_send", json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
    for (index, expected) in order.iter().enumerate() {
        let callback = installed_handler_callback(mcp, target, expected).await;
        let response = if index + 1 == order.len() {
            json!({"kind":"route_fulfill","status":200,"body":"Acme public handler chain","contentType":"text/plain"})
        } else if index == 0 {
            json!({"kind":"route_fallback_with","overrides":{"url":format!("{base}/rewritten-chain")}})
        } else {
            json!({"kind":"route_fallback"})
        };
        operation(
            mcp,
            target,
            json!({"operation":"callback_resolve","callback":callback,"response":response}),
        )
        .await;
    }
    assert_eq!(
        completed(mcp, target, pending).await["value"]["value"],
        "Acme public handler chain"
    );
    let expression = format!("async()=>await (await fetch({})).text()", json!(other_url));
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
    for (index, expected) in other_order.iter().enumerate() {
        let callback = installed_handler_callback(mcp, target, expected).await;
        let response = if index == 0 {
            json!({"kind":"route_fallback_with","overrides":{"url":format!("{base}/rewritten-other")}})
        } else {
            json!({"kind":"route_fulfill","status":200,"body":"Acme other handler chain","contentType":"text/plain"})
        };
        operation(
            mcp,
            target,
            json!({"operation":"callback_resolve","callback":callback,"response":response}),
        )
        .await;
    }
    assert_eq!(
        completed(mcp, target, pending).await["value"]["value"],
        "Acme other handler chain"
    );
    for scope in [page, context] {
        let listed = operation(
            mcp,
            target,
            json!({"operation":"network_route","scope":scope,"action":{"kind":"list"}}),
        )
        .await;
        assert_eq!(listed["handlers"].as_array().unwrap().len(), 3);
        let distinct=operation(mcp,target,json!({"operation":"network_route","scope":scope,"action":{"kind":"remove","handlerId":listed["handlers"][2]["handlerId"]}})).await;
        assert_eq!(distinct["handlers"].as_array().unwrap().len(), 2);
        let removed = operation(mcp,target,json!({"operation":"network_route","scope":scope,"action":{"kind":"remove","handlerId":listed["handlers"][0]["handlerId"]}})).await;
        assert_eq!(removed["handlers"].as_array().unwrap().len(), 1);
    }
    let expression = format!("async()=>(await fetch({})).status", json!(other_url));
    let passed = operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}),
    )
    .await;
    assert_eq!(passed["value"]["value"], 200);
    for scope in [page, context] {
        operation(
            mcp,
            target,
            json!({"operation":"network_route","scope":scope,"action":{"kind":"clear"}}),
        )
        .await;
    }
    installed_glob_routes(fixture, mcp, base, target, page, context).await;
    installed_route_limits(fixture, mcp, base, target, page, context).await;
}

async fn installed_route_limits(
    fixture: &Fixture,
    mcp: &mut Mcp,
    base: &str,
    target: &str,
    page: &Value,
    context: &Value,
) {
    let mut ids = Vec::new();
    for (scope, times) in [(page, 1), (page, 1), (context, 2)] {
        let request=json!({"operation":"network_route","scope":scope,"action":{"kind":"add_matcher","matcher":{"kind":"glob","pattern":format!("{base}/limited?request=*")},"times":times}}).to_string();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request])
            .await;
        ids.push(completed(mcp, target, submitted).await["handlerId"].clone());
    }
    let request = |name: &str| json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":format!("async()=>await (await fetch({})).text()",json!(format!("{base}/limited?request={name}"))),"isFunction":true}});
    let first = mcp.tool("browser_send", request("first")).await;
    let callback = installed_handler_callback(mcp, target, &ids[1]).await;
    let resolved=operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","status":200,"body":"Acme newest limited","contentType":"text/plain"}})).await;
    assert!(
        resolved.get("handlerCleanupPending").is_none(),
        "{resolved}"
    );
    assert_eq!(
        completed(mcp, target, first).await["value"]["value"],
        "Acme newest limited"
    );
    let listed = operation(
        mcp,
        target,
        json!({"operation":"network_route","scope":page,"action":{"kind":"list"}}),
    )
    .await;
    assert_eq!(listed["handlers"].as_array().unwrap().len(), 1);
    assert_eq!(listed["handlers"][0]["handlerId"], ids[0]);
    assert_eq!(listed["handlers"][0]["remainingUses"], 1);

    let second = mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":format!("async()=>JSON.stringify(await Promise.all([{},{}].map(async url=>await (await fetch(url)).text())))",json!(format!("{base}/limited?request=second")),json!(format!("{base}/limited?request=third"))),"isFunction":true}})).await;
    let page_callback = installed_handler_callback(mcp, target, &ids[0]).await;
    let context_callback = installed_handler_callback(mcp, target, &ids[2]).await;
    let fallback=operation(mcp,target,json!({"operation":"callback_resolve","callback":page_callback,"response":{"kind":"route_fallback"}})).await;
    let listed = operation(
        mcp,
        target,
        json!({"operation":"network_route","scope":context,"action":{"kind":"list"}}),
    )
    .await;
    assert_eq!(listed["handlers"][0]["remainingUses"], 0);
    assert_eq!(listed["handlers"][0]["exhausted"], true);
    for callback in [context_callback, fallback["nextCallback"].clone()] {
        let reply=json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","status":200,"body":"Acme bounded installed","contentType":"text/plain"}}).to_string();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &reply])
            .await;
        let result = completed(mcp, target, submitted).await;
        assert!(result.get("handlerCleanupPending").is_none(), "{result}");
    }
    assert_eq!(
        completed(mcp, target, second).await["value"]["value"],
        "[\"Acme bounded installed\",\"Acme bounded installed\"]"
    );
    for scope in [page, context] {
        let listed = operation(
            mcp,
            target,
            json!({"operation":"network_route","scope":scope,"action":{"kind":"list"}}),
        )
        .await;
        assert!(
            listed["handlers"].as_array().unwrap().is_empty(),
            "{listed}"
        );
    }
    let passed=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":format!("async()=>(await fetch({})).status",json!(format!("{base}/limited?request=after"))),"isFunction":true})).await;
    assert_eq!(passed["value"]["value"], 200);
}

async fn installed_glob_routes(
    fixture: &Fixture,
    mcp: &mut Mcp,
    base: &str,
    target: &str,
    page: &Value,
    context: &Value,
) {
    let mut handlers = Vec::new();
    for (scope, pattern) in [
        (
            page,
            format!("{base}/Acme folder/**/glob-{{one,two}}?token=*"),
        ),
        (
            context,
            format!("{base}/Acme%20folder/glob-{{one,two}}?token=*"),
        ),
    ] {
        let request=json!({"operation":"network_route","scope":scope,"action":{"kind":"add_matcher","matcher":{"kind":"glob","pattern":pattern}}}).to_string();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request])
            .await;
        handlers.push(completed(mcp, target, submitted).await["handlerId"].clone());
    }
    for path in [
        "Acme%20folder/glob-one/deeper?token=Acme-private-glob",
        "Acme%20folder/glob-oneXtoken=Acme-private-glob",
        "Acme%20folder/glob-one?token=Acme/private-glob",
    ] {
        let passed=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":format!("async()=>(await fetch({})).status",json!(format!("{base}/{path}"))),"isFunction":true})).await;
        assert_eq!(passed["value"]["value"], 200);
    }
    let url = format!("{base}/Acme%20folder/glob-one?token=Acme-private-glob");
    let expression = format!("async()=>await (await fetch({})).text()", json!(url));
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
    for (index, handler) in handlers.iter().enumerate() {
        let callback = installed_handler_callback(mcp, target, handler).await;
        let response = if index == 0 {
            json!({"kind":"route_fallback_with","overrides":{"url":format!("{base}/glob-rewritten")}})
        } else {
            json!({"kind":"route_fulfill","status":201,"body":"Acme glob fallback","contentType":"text/plain"})
        };
        operation(
            mcp,
            target,
            json!({"operation":"callback_resolve","callback":callback,"response":response}),
        )
        .await;
    }
    assert_eq!(
        completed(mcp, target, pending).await["value"]["value"],
        "Acme glob fallback"
    );
    let deep = format!("{base}/Acme%20folder/branch/deeper/glob-two?token=Acme-private-glob");
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":format!("async()=>await (await fetch({})).text()",json!(deep)),"isFunction":true}})).await;
    let callback = installed_handler_callback(mcp, target, &handlers[0]).await;
    operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","status":202,"body":"Acme deep glob","contentType":"text/plain"}})).await;
    assert_eq!(
        completed(mcp, target, pending).await["value"]["value"],
        "Acme deep glob"
    );
    let matcher =
        json!({"kind":"glob","pattern":format!("{base}/Acme folder/**/glob-{{one,two}}?token=*")});
    let duplicate = operation(mcp,target,json!({"operation":"network_route","scope":page,"action":{"kind":"add_matcher","matcher":matcher}})).await;
    assert_eq!(duplicate["handlers"].as_array().unwrap().len(), 2);
    let request = json!({"operation":"network_route","scope":page,"action":{"kind":"remove_matcher","matcher":matcher}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    assert!(
        completed(mcp, target, submitted).await["handlers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":format!("async()=>await (await fetch({})).text()",json!(url)),"isFunction":true}})).await;
    let callback = installed_handler_callback(mcp, target, &handlers[1]).await;
    operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","status":203,"body":"Acme context glob","contentType":"text/plain"}})).await;
    assert_eq!(
        completed(mcp, target, pending).await["value"]["value"],
        "Acme context glob"
    );
    let removed = operation(mcp,target,json!({"operation":"network_route","scope":context,"action":{"kind":"remove_matcher","matcher":{"kind":"glob","pattern":format!("{base}/Acme%20folder/glob-{{one,two}}?token=*")}}})).await;
    assert!(removed["handlers"].as_array().unwrap().is_empty());
    let passed=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":format!("async()=>(await fetch({})).status",json!(url)),"isFunction":true})).await;
    assert_eq!(passed["value"]["value"], 200);
}

async fn installed_handler_callback(mcp: &mut Mcp, target: &str, expected: &Value) -> Value {
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(callback) = items
            .iter()
            .find(|item| item["kind"] == "route" && item["handlerId"] == *expected)
        {
            assert!(!callback.to_string().contains("Acme-route-token"));
            return callback["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Handler callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn startup_scripts_and_interception(
    fixture: &Fixture,
    mcp: &mut Mcp,
    url: &str,
    target: &str,
    page: &Value,
    native: bool,
) {
    let added=operation(mcp,target,json!({"operation":"page_init_script_add","page":page,"source":"globalThis.acmeInvestigation=42"})).await;
    let script = &added["scriptId"];
    let route =
        json!({"operation":"page_route_set","page":page,"url":format!("{url}/intercepted")})
            .to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &route])
        .await;
    completed(mcp, target, submitted).await;
    let navigation=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{url}/intercepted")}}})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(route) = items.iter().find(|item| item["kind"] == "route") {
            break route["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Interception unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let fulfill=json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","status":200,"body":"<!doctype html><title>Acme intercepted</title>","contentType":"text/html"}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &fulfill])
        .await;
    completed(mcp, target, submitted).await;
    completed(mcp, target, navigation).await;
    let value=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"document.title+':'+acmeInvestigation","isFunction":false})).await;
    assert_eq!(value["value"]["value"], "Acme intercepted:42");
    installed_binary_route(fixture, mcp, url, target, page).await;
    let binding = operation(
        mcp,
        target,
        json!({"operation":"binding_add","scope":page,"name":"acmeBridge"}),
    )
    .await;
    let evaluation=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":"acmeBridge('Acme private binding')","isFunction":false}})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = mcp.detailed_browser_status(target).await;
        assert!(!status.to_string().contains("Acme private binding"));
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(binding) = items.iter().find(|item| item["kind"] == "binding") {
            break binding["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Binding unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let read = json!({"operation":"binding_read","callback":callback}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &read])
        .await;
    let read = completed(mcp, target, submitted).await;
    assert!(read.to_string().contains("Acme private binding"));
    operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"binding_resolve","value":42}})).await;
    assert_eq!(
        completed(mcp, target, evaluation).await["value"]["value"],
        42
    );
    operation(
        mcp,
        target,
        json!({"operation":"binding_remove","scope":page,"bindingId":binding["bindingId"]}),
    )
    .await;
    let list = json!({"operation":"page_init_script_list","page":page}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &list])
        .await;
    let listed = completed(mcp, target, submitted).await;
    assert_eq!(listed["scripts"].as_array().unwrap().len(), 1);
    assert!(!listed.to_string().contains("acmeInvestigation"));
    operation(
        mcp,
        target,
        json!({"operation":"page_init_script_remove","page":page,"scriptId":script}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"page_route_set","page":page,"url":null}),
    )
    .await;
    if native {
        operation(mcp,target,json!({"operation":"page_route_set","page":page,"url":format!("{url}/auth"),"authenticationChallenges":true})).await;
        let navigation=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{url}/auth")}}})).await;
        for phase in ["request", "authentication"] {
            let until = tokio::time::Instant::now() + Duration::from_secs(10);
            let callback = loop {
                let status = mcp.detailed_browser_status(target).await;
                assert!(!status.to_string().contains("Acme-password"));
                if let Some(callback) = status["browser"]["callbacks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item["kind"] == "route" && item["details"]["phase"] == phase)
                {
                    break callback["reference"].clone();
                }
                assert!(
                    tokio::time::Instant::now() < until,
                    "Missing auth workflow callback: {status}"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            };
            let response = if phase == "request" {
                json!({"kind":"route_continue"})
            } else {
                json!({"kind":"auth_credentials","username":"Acme","password":"Acme-password"})
            };
            let resolve =
                json!({"operation":"callback_resolve","callback":callback,"response":response})
                    .to_string();
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &resolve])
                .await;
            completed(mcp, target, submitted).await;
        }
        completed(mcp, target, navigation).await;
        let title=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"document.title","isFunction":false})).await;
        assert_eq!(title["value"]["value"], "Acme authenticated");
        operation(
            mcp,
            target,
            json!({"operation":"page_route_set","page":page,"url":null}),
        )
        .await;
    }
    let create = json!({"operation":"http_context_create","options":{"baseUrl":url}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &create])
        .await;
    let context = completed(mcp, target, submitted).await["httpContext"].clone();
    installed_http_payloads(fixture, mcp, target, &context).await;
    let response=operation(mcp,target,json!({"operation":"http_fetch","httpContext":context,"request":{"url":"/failure?token=Acme-private-token","readBody":true,"maxBytes":128}})).await;
    assert_eq!(response["status"], 409);
    assert!(!response.to_string().contains("Acme-private-body"));
    assert_eq!(
        std::fs::read(response["body"]["attachment"]["path"].as_str().unwrap()).unwrap(),
        b"{\"error\":\"Acme-private-body\"}"
    );
    let finalize =
        json!({"operation":"http_response_finalize","requestId":response["requestId"]}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &finalize])
        .await;
    assert_eq!(completed(mcp, target, submitted).await, response);
    operation(
        mcp,
        target,
        json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]}),
    )
    .await;
    let remove = json!({"operation":"http_context_remove","httpContext":context}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &remove])
        .await;
    completed(mcp, target, submitted).await;
    installed_http_association(fixture, mcp, url, target, page).await;
    installed_http_browser_defaults(mcp, url, target).await;
    installed_developer_tools(fixture, mcp, url, target, page, native).await;
    installed_framework(fixture, mcp, url, target, page).await;
    if native {
        installed_webmcp_debug(fixture, mcp, target, page).await;
        installed_heap(fixture, mcp, target, page).await;
        installed_performance(fixture, mcp, url, target, page).await;
    }
}

async fn installed_webmcp_debug(fixture: &Fixture, mcp: &mut Mcp, target: &str, page: &Value) {
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"(()=>{globalThis.acmeDebugCalls=0;document.modelContext.registerTool({name:'acme-installed-debug',description:'Acme private diagnostic',inputSchema:{type:'object'},annotations:{debugging:true},execute:async()=>({calls:++acmeDebugCalls,text:'Acme-private-debug-output'})});return true})()","isFunction":false})).await;
    let enable =
        json!({"operation":"web_mcp_debug","scope":page,"action":{"kind":"enable"}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &enable])
        .await;
    let enabled = completed(mcp, target, submitted).await;
    assert!(
        enabled["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "acme-installed-debug"),
        "{enabled}"
    );
    let invocation=operation(mcp,target,json!({"operation":"web_mcp_debug","scope":page,"action":{"kind":"invoke","name":"acme-installed-debug","parameters":{}}})).await;
    let read=json!({"operation":"web_mcp_debug","scope":page,"action":{"kind":"read","invocationId":invocation["invocationId"]}}).to_string();
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let result = loop {
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &read])
            .await;
        let result = completed(mcp, target, submitted).await;
        if result["result"].is_object() {
            break result;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Debug invocation pending: {result}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert_eq!(result["state"], "completed", "{result}");
    assert_eq!(result["result"]["output"]["calls"], 1);
    assert_eq!(
        result["result"]["output"]["text"],
        "Acme-private-debug-output"
    );
    let status = operation(
        mcp,
        target,
        json!({"operation":"web_mcp_debug","scope":page,"action":{"kind":"status"}}),
    )
    .await;
    assert!(!status.to_string().contains("Acme-private-debug-output"));
    operation(
        mcp,
        target,
        json!({"operation":"web_mcp_debug","scope":page,"action":{"kind":"disable"}}),
    )
    .await;
}

async fn installed_evidence_route(
    fixture: &Fixture,
    mcp: &mut Mcp,
    url: &str,
    target: &str,
    page: &Value,
    evidence_id: &Value,
) {
    let replay = format!("{url}/evidence-replay");
    operation(
        mcp,
        target,
        json!({"operation":"page_route_set","page":page,"url":replay}),
    )
    .await;
    let expression = format!(
        "()=>fetch({}).then(response=>response.text())",
        json!(replay)
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"isFunction":true,"expression":expression}})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(route) = items.iter().find(|item| item["kind"] == "route") {
            break route["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Evidence interception unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let response=json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill_evidence","status":201,"networkEvidenceId":evidence_id}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &response])
        .await;
    completed(mcp, target, submitted).await;
    let result = completed(mcp, target, pending).await;
    assert_eq!(
        result["value"]["value"],
        "{\"error\":\"Acme-private-body\"}"
    );
    operation(
        mcp,
        target,
        json!({"operation":"page_route_set","page":page,"url":null}),
    )
    .await;
}

async fn installed_har_capture(
    fixture: &Fixture,
    mcp: &mut Mcp,
    url: &str,
    target: &str,
    page: &Value,
    context: &Value,
    filtered: bool,
) {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let capture_url = format!("{url}/failure?token=Acme-capture-private");
    let expression = format!(
        "async()=>{{const response=await fetch({});await response.arrayBuffer();return response.status}}",
        json!(capture_url)
    );
    let mut captures: Vec<(Value, String, Vec<u8>)> = Vec::new();
    for index in 0..if filtered { 3 } else { 2 } {
        let replaces = captures
            .first()
            .map(|(capture, _, _)| capture["archiveId"].clone());
        let filter = if filtered {
            match index {
                0 => json!({"kind":"exact","url":capture_url}),
                1 => json!({"kind":"glob","pattern":format!("{url}/failure?token=Acme-capture-*")}),
                _ => {
                    json!({"kind":"regex","pattern":r"/FAILURE\?token=Acme-capture-private$","flags":"i"})
                }
            }
        } else {
            Value::Null
        };
        let request=json!({"operation":"har_capture_start","context":context,"content":"embed","replacesArchiveId":replaces,"urlFilter":filter}).to_string();
        let registration = std::time::Instant::now();
        let retained = filtered && index == 1;
        let started = if retained {
            operation(mcp, target, serde_json::from_str(&request).unwrap()).await
        } else {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &request])
                .await;
            completed(mcp, target, submitted).await
        };
        if filtered {
            eprintln!(
                "HAR filter registration {index}, retained MCP {retained}: {:?}",
                registration.elapsed()
            );
        }
        assert_eq!(started["content"], "embed");
        assert_eq!(started["urlFilterActive"], filtered);
        assert_eq!(
            started["replacesArchiveId"],
            replaces.unwrap_or(Value::Null)
        );
        if filtered {
            let before = operation(
                mcp,
                target,
                json!({"operation":"diagnostics_read","context":context,"query":{}}),
            )
            .await;
            let excluded = format!("{url}/failure?token=excluded-{index}");
            operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":format!("async()=>{{const r=await fetch({});await r.arrayBuffer();return r.status}}",json!(excluded))})).await;
            operation(mcp,target,json!({"operation":"diagnostics_wait","context":context,"after":before["latestSequence"],"conditions":[{"event":"response","url":format!("{url}/failure"),"status":409}]})).await;
        }
        assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":expression})
        )
        .await["value"]["value"],
        409
    );
        let until = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let status = operation(
                mcp,
                target,
                json!({"operation":"har_capture_status","context":context}),
            )
            .await;
            if status["entries"].as_u64().unwrap() > 0
                && status["entries"] == status["completedEntries"]
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "Public capture did not settle: {status}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let request = json!({"operation":"har_capture_stop","context":context}).to_string();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request])
            .await;
        let captured = completed(mcp, target, submitted).await;
        assert!(!captured.to_string().contains("Acme-private-body"));
        assert_eq!(captured["reports"][0]["format"], "har-1.2-capture");
        let path = captured["reports"][0]["attachment"]["path"]
            .as_str()
            .unwrap();
        let bytes = std::fs::read(path).unwrap();
        let archive: Value = serde_json::from_slice(&bytes).unwrap();
        if filtered {
            assert_eq!(
                archive["log"]["entries"].as_array().unwrap().len(),
                1,
                "{archive}"
            );
            assert_eq!(archive["log"]["_testrunCapture"]["droppedEntries"], 0);
        }
        let entry = archive["log"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["request"]["url"] == capture_url)
            .unwrap();
        let body = STANDARD
            .decode(entry["response"]["content"]["text"].as_str().unwrap())
            .unwrap();
        assert_eq!(body, b"{\"error\":\"Acme-private-body\"}");
        if let Some((original, path, bytes)) = captures.first() {
            assert_ne!(captured["archiveId"], original["archiveId"]);
            assert_eq!(captured["replacesArchiveId"], original["archiveId"]);
            assert_eq!(std::fs::read(path).unwrap().as_slice(), bytes.as_slice());
        }
        captures.push((captured.clone(), path.into(), bytes));
    }
    let cleanup = std::time::Instant::now();
    for (captured, path, _) in captures {
        operation(
            mcp,
            target,
            json!({"operation":"har_remove","archiveId":captured["archiveId"]}),
        )
        .await;
        assert!(!std::path::Path::new(&path).exists());
    }
    if filtered {
        eprintln!(
            "HAR filter archive/artifact cleanup: {:?}",
            cleanup.elapsed()
        );
    }
}

fn socket_field<'a>(value: &'a Value, key: &str) -> &'a Value {
    &value["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key"] == key)
        .unwrap()["value"]
}
#[expect(
    clippy::too_many_lines,
    reason = "One installed-client exchange verifies future-document routing, private binary callbacks and scoped removal"
)]
async fn installed_socket_route(
    fixture: &Fixture,
    mcp: &mut Mcp,
    url: &str,
    target: &str,
    context: &Value,
) {
    let add=json!({"operation":"web_socket_route_add","scope":context,"url":format!("{}/mock-socket",url.replacen("http:","ws:",1)),"sdkDefaults":false}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &add])
        .await;
    let route = completed(mcp, target, submitted).await;
    let created = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await;
    let page = &created["page"];
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{url}/framework"),"waitUntil":"load"}})).await;
    let expression = format!(
        "()=>new Promise(resolve=>{{const socket=globalThis.AcmeInstalledSocket=new WebSocket({});socket.binaryType='arraybuffer';socket.onopen=()=>socket.send(new Uint8Array([9,0,255,128,9]).subarray(1,4));socket.onmessage=event=>resolve(JSON.stringify(Array.from(new Uint8Array(event.data))));}})",
        json!(format!("{}/mock-socket", url.replacen("http:", "ws:", 1)))
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
    for expected in ["onCreate", "onMessageFromPage"] {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let callback = loop {
            let status = mcp.detailed_browser_status(target).await;
            let browser = &status["browser"];
            let items = browser["callbacks"]
                .as_array()
                .or_else(|| browser["resources"].as_array())
                .unwrap();
            if let Some(callback) = items.iter().find(|item| item["kind"] == "binding") {
                assert!(!callback.to_string().contains("AP+A"));
                break callback["reference"].clone();
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Socket callback unavailable: {status}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        let read = json!({"operation":"binding_read","callback":callback}).to_string();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &read])
            .await;
        let read = completed(mcp, target, submitted).await;
        assert_eq!(read["bindingId"], route["bindingId"]);
        assert_eq!(
            socket_field(&read["arguments"][0], "type")["value"],
            expected
        );
        let commands = if expected == "onCreate" {
            json!([{"type":"ensureOpened"}])
        } else {
            let payload = socket_field(socket_field(&read["arguments"][0], "data"), "data");
            assert_eq!(payload["value"], "AP+A");
            json!([])
        };
        operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"binding_resolve","value":commands}})).await;
        if expected == "onMessageFromPage" {
            let send=json!({"operation":"web_socket_route_send","scope":context,"frame":read["frame"],"routeId":route["routeId"],"socketId":socket_field(&read["arguments"][0],"id")["value"],"to":"page","message":{"data":"AP+A","isBase64":true}}).to_string();
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &send])
                .await;
            let sent = completed(mcp, target, submitted).await;
            assert_eq!(sent["payloadIncluded"], false);
            assert!(!sent.to_string().contains("AP+A"));
        }
    }
    assert_eq!(
        completed(mcp, target, pending).await["value"]["value"],
        "[0,255,128]"
    );
    let expression = format!(
        "()=>new Promise(resolve=>{{const socket=globalThis.AcmeInstalledAutomaticSocket=new WebSocket({});let bytes;socket.binaryType='arraybuffer';socket.onopen=()=>socket.send('Acme automatic relay');socket.onmessage=event=>{{bytes=Array.from(new Uint8Array(event.data));socket.close(3007,'Acme installed close');}};socket.onclose=event=>resolve(JSON.stringify({{bytes,code:event.code,reason:event.reason,wasClean:event.wasClean,readyState:socket.readyState}}));}})",
        json!(format!("{}/mock-socket", url.replacen("http:", "ws:", 1)))
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(callback) = items.iter().find(|item| item["kind"] == "binding") {
            break callback["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Automatic relay callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let read = operation(
        mcp,
        target,
        json!({"operation":"binding_read","callback":callback}),
    )
    .await;
    assert_eq!(read["bindingId"], route["bindingId"]);
    assert_eq!(
        socket_field(&read["arguments"][0], "type")["value"],
        "onCreate"
    );
    let reply=json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"binding_resolve","value":[
        {"type":"setForwarding","from":"page","enabled":true},
        {"type":"setForwarding","from":"server","enabled":true},
        {"type":"setCloseForwarding","from":"page","enabled":true},
        {"type":"setCloseForwarding","from":"server","enabled":true},
        {"type":"connect"}
    ]}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &reply])
        .await;
    completed(mcp, target, submitted).await;
    let result = completed(mcp, target, pending).await;
    assert_eq!(
        serde_json::from_str::<Value>(result["value"]["value"].as_str().unwrap()).unwrap(),
        json!({"bytes":[0,255,128],"code":3007,"reason":"Acme installed close","wasClean":true,"readyState":3})
    );
    let remove =
        json!({"operation":"web_socket_route_remove","scope":context,"routeId":route["routeId"]})
            .to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &remove])
        .await;
    assert_eq!(completed(mcp, target, submitted).await["removed"], true);
    assert_eq!(operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"AcmeInstalledSocket.readyState","isFunction":false})).await["value"]["value"],3);
    assert_eq!(operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"AcmeInstalledAutomaticSocket.readyState","isFunction":false})).await["value"]["value"],3);
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
}

async fn installed_har_fallback(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    replay: &str,
) {
    let source = format!("{replay}/fallback-source?item=Acme&token=Acme-private-route");
    operation(
        mcp,
        target,
        json!({"operation":"page_route_set","page":page,"url":source}),
    )
    .await;
    let near_miss = source.replacen('?', "X", 1);
    let probe = format!(
        "async()=>fetch({}).then(()=>true,()=>true)",
        json!(near_miss)
    );
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"evaluate","scope":page,"expression":probe,"isFunction":true})
        )
        .await["value"]["value"],
        true
    );
    let expression = format!(
        "async()=>JSON.stringify(Array.from(new Uint8Array(await (await fetch({})).arrayBuffer())))",
        json!(source)
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"isFunction":true,"expression":expression}})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(callback) = items
            .iter()
            .find(|item| item["kind"] == "route" && item["policyScope"] == *page)
        {
            assert!(!callback.to_string().contains("Acme-private-route"));
            break callback["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Page fallback callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let reply=json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fallback_with","overrides":{"url":replay}}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &reply])
        .await;
    assert_eq!(completed(mcp, target, submitted).await["resolved"], true);
    assert_eq!(
        completed(mcp, target, pending).await["value"]["value"],
        "[0,255,128]"
    );
    operation(
        mcp,
        target,
        json!({"operation":"page_route_set","page":page,"url":null}),
    )
    .await;
}

#[expect(
    clippy::too_many_lines,
    reason = "One public-client workflow verifies page/context manual replies, fallback, automatic replay, future-page installation and archive retirement"
)]
async fn installed_har_route(
    fixture: &Fixture,
    mcp: &mut Mcp,
    url: &str,
    target: &str,
    context: &Value,
    page: &Value,
) {
    let directory = fixture
        .workspace
        .path()
        .join(format!("har-{}", uuid::Uuid::new_v4()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory).unwrap();
    let path = directory.join("source.har");
    let replay = format!("{url}/har-replay");
    let app = format!("{url}/context-har-app");
    let archive = json!({"log":{"version":"1.2","creator":{"name":"Acme","version":"1"},"entries":[{"request":{"url":replay,"method":"GET","headers":[]},"response":{"status":201,"headers":[{"name":"Content-Type","value":"application/octet-stream"},{"name":"Content-Encoding","value":"gzip"},{"name":"Content-Length","value":"123"}],"content":{"size":3,"encoding":"base64","text":"AP+A"}}},{"request":{"url":app,"method":"GET","headers":[]},"response":{"status":200,"headers":[{"name":"Content-Type","value":"text/html"}],"content":{"text":"<!doctype html><script>globalThis.AcmeInstalledHar=fetch('/har-replay').then(async response=>JSON.stringify(Array.from(new Uint8Array(await response.arrayBuffer()))))</script>"}}}]}});
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    serde_json::to_writer(options.open(&path).unwrap(), &archive).unwrap();
    let request = json!({"operation":"har_register","path":path}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let registered = completed(mcp, target, submitted).await;
    let archive_id = &registered["archiveId"];
    for (operation_name, field, scope) in [
        ("page_route_set", "page", page),
        ("context_route_set", "context", context),
    ] {
        let request = json!({"operation":operation_name,(field):scope,"url":replay}).to_string();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request])
            .await;
        completed(mcp, target, submitted).await;
        for method in ["GET", "POST"] {
            let expression = format!(
                "async()=>{{try{{const r=await fetch({},{{method:{}}});return JSON.stringify(Array.from(new Uint8Array(await r.arrayBuffer())))}}catch{{return JSON.stringify({{blocked:true}})}}}}",
                json!(replay),
                json!(method)
            );
            let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"isFunction":true,"expression":expression}})).await;
            let until = tokio::time::Instant::now() + Duration::from_secs(10);
            let callback = loop {
                let status = mcp.detailed_browser_status(target).await;
                let browser = &status["browser"];
                let items = browser["callbacks"]
                    .as_array()
                    .or_else(|| browser["resources"].as_array())
                    .unwrap();
                if let Some(route) = items.iter().find(|item| item["kind"] == "route") {
                    assert_eq!(route["policyScope"], *scope, "{status}");
                    break route["reference"].clone();
                }
                assert!(
                    tokio::time::Instant::now() < until,
                    "HAR interception unavailable: {status}"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            };
            let request=json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_har","archiveId":archive_id}}).to_string();
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &request])
                .await;
            let resolved = completed(mcp, target, submitted).await;
            assert_eq!(resolved["har"]["matched"], method == "GET");
            let result = completed(mcp, target, pending).await;
            let result: Value =
                serde_json::from_str(result["value"]["value"].as_str().unwrap()).unwrap();
            assert_eq!(
                result,
                if method == "GET" {
                    json!([0, 255, 128])
                } else {
                    json!({"blocked":true})
                }
            );
        }
        operation(
            mcp,
            target,
            json!({"operation":operation_name,(field):scope,"url":null}),
        )
        .await;
    }
    for (operation_name, field, scope) in [
        ("page_har_set", "page", page),
        ("context_har_set", "context", context),
    ] {
        let request =
            json!({"operation":operation_name,(field):scope,"archiveId":archive_id}).to_string();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request])
            .await;
        assert_eq!(
            completed(mcp, target, submitted).await["rule"]["phase"],
            "active"
        );
        let expression = format!(
            "async()=>{{const replies=await Promise.all(Array.from({{length:8}},()=>fetch({}).then(async r=>Array.from(new Uint8Array(await r.arrayBuffer())))));const blocked=await fetch({},{{method:'POST'}}).then(()=>false,()=>true);return JSON.stringify({{replies,blocked}})}}",
            json!(replay),
            json!(replay)
        );
        let result = operation(
            mcp,
            target,
            json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":expression}),
        )
        .await;
        let result: Value =
            serde_json::from_str(result["value"]["value"].as_str().unwrap()).unwrap();
        assert_eq!(result["blocked"], true);
        assert_eq!(result["replies"], json!(vec![vec![0, 255, 128]; 8]));
        let rules = operation(mcp, target, json!({"operation":"har_list"})).await;
        assert_eq!(rules["routes"][0]["failures"], 0, "{rules}");
        assert_eq!(rules["routes"][0]["scopeKind"], field);
        if field == "context" {
            installed_har_fallback(fixture, mcp, target, page, &replay).await;
            let created = operation(
                mcp,
                target,
                json!({"operation":"page_create","context":context,"select":false}),
            )
            .await;
            let fresh = &created["page"];
            operation(mcp,target,json!({"operation":"page_navigate","page":fresh,"navigation":{"kind":"goto","url":app,"waitUntil":"load"}})).await;
            let value=operation(mcp,target,json!({"operation":"evaluate","scope":fresh,"expression":"AcmeInstalledHar","isFunction":false})).await;
            assert_eq!(value["value"]["value"], "[0,255,128]");
            operation(mcp, target, json!({"operation":"page_close","page":fresh})).await;
        }
        operation(
            mcp,
            target,
            json!({"operation":operation_name,(field):scope,"archiveId":null}),
        )
        .await;
    }
    let request = json!({"operation":"har_remove","archiveId":archive_id}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    completed(mcp, target, submitted).await;
    assert!(
        operation(mcp, target, json!({"operation":"har_list"})).await["archives"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(path.exists());
}

async fn installed_binary_route(
    fixture: &Fixture,
    mcp: &mut Mcp,
    url: &str,
    target: &str,
    page: &Value,
) {
    let expression = format!(
        "async()=>{{const response=await fetch({});return JSON.stringify({{status:response.status,bytes:Array.from(new Uint8Array(await response.arrayBuffer())),header:response.headers.get('x-acme')}});}}",
        json!(format!("{url}/intercepted"))
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"isFunction":true,"expression":expression}})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(route) = items.iter().find(|item| item["kind"] == "route") {
            break route["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Binary interception unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let response=json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill_binary","status":201,"bodyBase64":"AP+A","headers":[{"name":"content-type","value":"application/octet-stream"},{"name":"x-acme","value":"first"},{"name":"x-acme","value":"second"}]}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &response])
        .await;
    completed(mcp, target, submitted).await;
    let result = completed(mcp, target, pending).await;
    let result: Value = serde_json::from_str(result["value"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(
        result,
        json!({"status":201,"bytes":[0,255,128],"header":"first, second"})
    );
    let expression = format!(
        "async()=>{{const r=await fetch({});return JSON.stringify({{status:r.status,body:await r.json()}})}}",
        json!(format!("{url}/intercepted"))
    );
    let pending=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"isFunction":true,"expression":expression}})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(route) = items.iter().find(|item| item["kind"] == "route") {
            break route["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Rewrite callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let request=json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"route_continue_with","overrides":{"url":format!("{url}/failure?token=Acme-private-token"),"method":"GET"}}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    completed(mcp, target, submitted).await;
    let result = completed(mcp, target, pending).await;
    let result: Value = serde_json::from_str(result["value"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(
        result,
        json!({"status":409,"body":{"error":"Acme-private-body"}})
    );
}

async fn installed_http_payloads(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value) {
    for ignore in [Some(true), Some(false), None] {
        let request = json!({"operation":"http_fetch","httpContext":context,"request":{"url":"/failure?case=Acme-tls-policy","ignoreHttpsErrors":ignore}});
        let response = if ignore == Some(true) {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &request.to_string()])
                .await;
            completed(mcp, target, submitted).await
        } else {
            operation(mcp, target, request).await
        };
        assert_eq!(response["status"], 409);
        assert_eq!(response["tlsVerification"], ignore != Some(true));
        assert_eq!(response["tlsPolicyOverridden"], ignore.is_some());
    }
    #[cfg(unix)]
    {
        let response=operation(mcp,target,json!({"operation":"http_fetch","httpContext":context,"request":{"url":format!("/http-reset?case={}",uuid::Uuid::new_v4()),"maxRetries":1,"readBody":true}})).await;
        assert_eq!(response["status"], 200);
        assert_eq!(response["retryCount"], 1);
        assert_eq!(response["replayed"], true);
        let bytes =
            std::fs::read(response["body"]["attachment"]["path"].as_str().unwrap()).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["attempts"],
            2
        );
        operation(
            mcp,
            target,
            json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]}),
        )
        .await;
    }
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_fetch","httpContext":context,"request":{"url":"/failure?token=Acme-private-token","failOnStatusCode":true,"readBody":true}}})).await;
    let failed = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(failed["operation"]["settled"], true);
    assert_eq!(failed["operation"]["code"], "HTTP_STATUS");
    assert_eq!(failed["operation"]["result"]["status"], 409);
    assert!(failed["operation"]["result"]["body"].is_null());
    assert!(
        !failed["operation"]
            .to_string()
            .contains("Acme-private-body")
    );
    for (params, expected) in [
        (
            json!([{"name":"label","value":"Acme query private +&"},{"name":"label","value":"東京"}]),
            "/http-query?old=one+two&label=Acme+query+private+%2B%26&label=%E6%9D%B1%E4%BA%AC",
        ),
        (
            json!("label=Acme%20query%20private"),
            "/http-query?label=Acme%20query%20private",
        ),
    ] {
        let response=operation(mcp,target,json!({"operation":"http_fetch","httpContext":context,"request":{"url":"/http-query?old=one%20two","method":"gEt","params":params,"timeoutMs":10000,"maxRedirects":0,"readBody":true}})).await;
        assert_eq!(response["status"], 200);
        assert!(!response.to_string().contains("query private"));
        let bytes =
            std::fs::read(response["body"]["attachment"]["path"].as_str().unwrap()).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["target"],
            expected
        );
        operation(
            mcp,
            target,
            json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]}),
        )
        .await;
    }
    let value = json!({"name":"Acme HTTP JSON private","unicode":"東京","record":{"connectionId":"11111111-1111-4111-8111-111111111111","id":"22222222-2222-4222-8222-222222222222","generation":"33333333-3333-4333-8333-333333333333"}});
    let selected_file = fixture.workspace.path().join("Acme-selected.JSON");
    std::fs::write(&selected_file, b"Acme selected upload private").unwrap();
    for (payload, content_type) in [
        (json!({"kind":"json","value":value}), "application/json"),
        (
            json!({"kind":"form","fields":[{"name":"label","value":"Acme +&"},{"name":"label","value":"two"}]}),
            "application/x-www-form-urlencoded",
        ),
        (
            json!({"kind":"multipart","fields":[{"kind":"text","name":"label","value":"Acme multipart private 東京"},{"kind":"file","name":"upload","filename":"Acme.txt","data":"QWNtZSBmaWxl"},{"kind":"file_path","name":"selected","path":selected_file}]}),
            "multipart/form-data",
        ),
    ] {
        let request = json!({"operation":"http_fetch","httpContext":context,"request":{"url":"/http-payload","method":"POST","body":payload,"readBody":true,"maxBytes":4096}}).to_string();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request])
            .await;
        let response = completed(mcp, target, submitted).await;
        assert_eq!(response["status"], 200);
        assert!(!response.to_string().contains("Acme HTTP JSON private"));
        let bytes =
            std::fs::read(response["body"]["attachment"]["path"].as_str().unwrap()).unwrap();
        let echoed: Value = serde_json::from_slice(&bytes).unwrap();
        if payload["kind"] == "multipart" {
            let boundary = echoed["contentType"]
                .as_str()
                .unwrap()
                .strip_prefix("multipart/form-data; boundary=")
                .unwrap();
            let body = echoed["body"].as_str().unwrap();
            assert!(body.starts_with(&format!("--{boundary}\r\n")));
            assert!(body.ends_with(&format!("--{boundary}--\r\n")));
            assert!(body.contains("Acme multipart private 東京"));
            assert!(body.contains(
                "filename=\"Acme.txt\"\r\nContent-Type: text/plain\r\n\r\nAcme file\r\n"
            ));
            assert!(!response.to_string().contains("Acme multipart private"));
            assert!(body.contains("filename=\"Acme-selected.JSON\"\r\nContent-Type: application/json\r\n\r\nAcme selected upload private\r\n"));
            assert!(
                !response
                    .to_string()
                    .contains("Acme selected upload private")
            );
        } else {
            assert_eq!(echoed["contentType"], content_type);
        }
        if payload["kind"] == "json" {
            assert_eq!(
                serde_json::from_str::<Value>(echoed["body"].as_str().unwrap()).unwrap(),
                value
            );
        } else if payload["kind"] == "form" {
            assert_eq!(echoed["body"], "label=Acme+%2B%26&label=two");
        }
        operation(
            mcp,
            target,
            json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]}),
        )
        .await;
    }
}
async fn installed_http_association(
    fixture: &Fixture,
    mcp: &mut Mcp,
    url: &str,
    target: &str,
    page: &Value,
) {
    let status = mcp.tool("browser_status", json!({"targetId":target})).await;
    let browser_context = &status["browser"]["selectedContext"];
    let unknown_partitions = status["browser"]["engine"] == "firefox";
    let partition_request = json!({"operation":"http_context_create","options":{"baseUrl":url,"browserContext":browser_context,"cookiePartition":{"topLevelSite":"https://example.com"}}});
    if unknown_partitions {
        let submitted = mcp
            .tool(
                "browser_send",
                json!({"targetId":target,"request":partition_request}),
            )
            .await;
        let refused = mcp.tool("browser_status", json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000})).await;
        assert_eq!(
            refused["operation"]["code"], "HTTP_COOKIE_PARTITION_UNSUPPORTED",
            "{refused}"
        );
    } else {
        let partitioned = operation(mcp, target, partition_request).await;
        assert_eq!(partitioned["cookiePartitionConfigured"], true);
        assert_eq!(partitioned["cookiePartitionIdentityAvailable"], true);
        assert_eq!(partitioned["unknownCookiePartitionsAllowed"], false);
        assert!(!partitioned.to_string().contains("https://example.com"));
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":partitioned["httpContext"]}),
        )
        .await;
    }
    if unknown_partitions {
        let submitted = mcp.tool("browser_send", json!({"targetId":target,"request":{"operation":"http_context_create","options":{"baseUrl":url,"browserContext":browser_context}}})).await;
        let refused = mcp.tool("browser_status", json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000})).await;
        assert_eq!(
            refused["operation"]["code"], "HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE",
            "{refused}"
        );
        assert_eq!(refused["operation"]["result"]["created"], false);
    }
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"document.cookie='FromPage=Acme-page-private; Path=/'","isFunction":false})).await;
    let create=json!({"operation":"http_context_create","options":{"baseUrl":url,"browserContext":browser_context,"allowUnknownCookiePartitions":true}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &create])
        .await;
    let created = completed(mcp, target, submitted).await;
    assert_eq!(created["browserAssociated"], true);
    assert_eq!(
        created["cookiePartitionIdentityAvailable"], !unknown_partitions,
        "{created}; {status}"
    );
    assert_eq!(created["unknownCookiePartitionsAllowed"], true);
    let context = &created["httpContext"];
    installed_http_payloads(fixture, mcp, target, context).await;
    let response=operation(mcp,target,json!({"operation":"http_fetch","httpContext":context,"request":{"url":"/cookie-exchange","readBody":true,"maxBytes":1024}})).await;
    assert_eq!(
        response["browserCookieUpdate"]["state"], "acknowledged",
        "{response}"
    );
    let bytes =
        std::fs::read_to_string(response["body"]["attachment"]["path"].as_str().unwrap()).unwrap();
    assert!(bytes.contains("FromPage=Acme-page-private"), "{bytes}");
    let browser=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>fetch('/cookie-exchange').then(response=>response.text())","isFunction":true})).await;
    assert!(
        browser["value"]["value"]
            .as_str()
            .unwrap()
            .contains("FromHttp=Acme-http-private"),
        "{browser}"
    );
    let clear = json!({"operation":"http_context_cookies_clear","httpContext":context}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &clear])
        .await;
    completed(mcp, target, submitted).await;
    let empty=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"document.cookie","isFunction":false})).await;
    assert_eq!(empty["value"]["value"], "");
    operation(
        mcp,
        target,
        json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]}),
    )
    .await;
    let remove = json!({"operation":"http_context_remove","httpContext":context}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &remove])
        .await;
    completed(mcp, target, submitted).await;
}

async fn installed_http_browser_defaults(mcp: &mut Mcp, url: &str, target: &str) {
    let native = mcp.tool("browser_status", json!({"targetId":target})).await["browser"]["backend"]
        == "chrome_cdp";
    let browser_options = if native {
        json!({"viewport":{"width":1280,"height":720},"deviceScaleFactor":1})
    } else {
        json!({"viewport":{"width":1280,"height":720},"deviceScaleFactor":1,"userAgent":"Acme inherited agent","ignoreHTTPSErrors":true})
    };
    let browser = operation(
        mcp,
        target,
        json!({"operation":"context_create","options":browser_options}),
    )
    .await;
    let browser = &browser["context"];
    for (extra, expected, verification) in [
        (json!({}), "Acme inherited agent", false),
        (
            json!({"userAgent":"Acme explicit agent","ignoreHttpsErrors":false}),
            "Acme explicit agent",
            true,
        ),
    ] {
        let mut options =
            json!({"baseUrl":url,"browserContext":browser,"allowUnknownCookiePartitions":true});
        if native {
            options["userAgent"] = json!("Acme inherited agent");
            options["ignoreHttpsErrors"] = json!(true);
        }
        options
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let created = operation(
            mcp,
            target,
            json!({"operation":"http_context_create","options":options}),
        )
        .await;
        let context = &created["httpContext"];
        let listed = operation(mcp, target, json!({"operation":"http_context_list"})).await;
        let client = listed["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|client| client["httpContext"] == *context)
            .unwrap();
        assert_eq!(client["tlsVerification"], verification);
        assert_eq!(client["userAgentConfigured"], true);
        for request_agent in [None, Some("Acme request agent"), None] {
            let headers = request_agent
                .map(|agent| json!([{"name":"User-Agent","value":agent}]))
                .unwrap_or(json!([]));
            let response = operation(mcp,target,json!({"operation":"http_fetch","httpContext":context,"request":{"url":"/http-agent","headers":headers,"readBody":true}})).await;
            assert_eq!(
                std::fs::read_to_string(response["body"]["attachment"]["path"].as_str().unwrap())
                    .unwrap(),
                request_agent.unwrap_or(expected)
            );
            assert!(!response.to_string().contains(expected));
            operation(mcp,target,json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]})).await;
        }
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":context}),
        )
        .await;
    }
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":browser}),
    )
    .await;
}

async fn installed_developer_tools(
    fixture: &Fixture,
    mcp: &mut Mcp,
    url: &str,
    target: &str,
    page: &Value,
    native: bool,
) {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{url}/tools"),"waitUntil":"load"}})).await;
    let discovered = operation(
        mcp,
        target,
        json!({"operation":"developer_tools_discover","scope":page}),
    )
    .await;
    let registry = &discovered["registry"];
    let tool = &discovered["metadata"]["groups"][0]["tools"][0]["toolId"];
    let invalid=json!({"operation":"developer_tool_call","registry":registry,"toolId":tool,"parameters":{"amount":"two"}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &invalid])
        .await;
    let rejected = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(
        rejected["operation"]["code"], "DEVELOPER_TOOL_ARGUMENTS_INVALID",
        "{rejected}"
    );
    let call=json!({"operation":"developer_tool_call","registry":registry,"toolId":tool,"parameters":{"amount":2}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &call])
        .await;
    let called = completed(mcp, target, submitted).await;
    let handle = &called["handle"];
    let value = operation(
        mcp,
        target,
        json!({"operation":"handle_read","handle":handle}),
    )
    .await;
    assert_eq!(value["value"]["value"], 3);
    let calls = operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":"acmeCalls","isFunction":false}),
    )
    .await;
    assert_eq!(calls["value"]["value"], 1);
    let element = operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"document.querySelector('#probe')","isFunction":false,"returnHandle":true})).await;
    let read =
        json!({"operation":"developer_tool_result_read","handle":element["handle"]}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &read])
        .await;
    let projected = completed(mcp, target, submitted).await;
    assert_eq!(projected["annotations"].as_array().unwrap().len(), 1);
    assert_eq!(projected["originalToolReplayed"], false);
    let snapshot=operation(mcp,target,json!({"operation":"page_snapshot","page":projected["page"],"frame":projected["frame"],"locator":projected["annotations"][0]["locator"]})).await;
    assert!(
        snapshot["value"].as_str().unwrap().contains("Acme"),
        "{snapshot}"
    );
    let element_tool = &discovered["metadata"]["groups"][0]["tools"][1]["toolId"];
    let call = json!({"operation":"developer_tool_call","registry":registry,"toolId":element_tool,"parameters":{"element":{"elementHandle":element["handle"]}},"returnHandle":false}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &call])
        .await;
    assert_eq!(
        completed(mcp, target, submitted).await["value"]["value"],
        "BUTTON"
    );
    let nested_tool = &discovered["metadata"]["groups"][0]["tools"][2]["toolId"];
    let nested_call=json!({"operation":"developer_tool_call","registry":registry,"toolId":nested_tool,"parameters":{"payload":[{"elementHandle":element["handle"]},{"elementHandle":element["handle"]}]},"returnHandle":false}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &nested_call])
        .await;
    assert_eq!(
        completed(mcp, target, submitted).await["value"]["value"],
        "BUTTON,BUTTON"
    );
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"document.querySelector('#probe').remove()","isFunction":false})).await;
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &call])
        .await;
    let rejected = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(
        rejected["operation"]["code"],
        if native {
            "JAVASCRIPT_EXCEPTION"
        } else {
            "BROWSER_METHOD_FAILED"
        },
        "{rejected}"
    );
    let calls=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"acmeElementCalls","isFunction":false})).await;
    assert_eq!(calls["value"]["value"], 1);
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &nested_call])
        .await;
    let rejected = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(
        rejected["operation"]["code"],
        if native {
            "JAVASCRIPT_EXCEPTION"
        } else {
            "BROWSER_METHOD_FAILED"
        },
        "{rejected}"
    );
    let calls=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"acmeNestedCalls","isFunction":false})).await;
    assert_eq!(calls["value"]["value"], 1);
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":element["handle"]}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":handle}),
    )
    .await;
    let release = json!({"operation":"developer_tools_release","registry":registry}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &release])
        .await;
    completed(mcp, target, submitted).await;
}

#[expect(
    clippy::too_many_lines,
    reason = "One public-client workflow verifies ordered context bindings, startup code, framework preparation and independent removal"
)]
async fn installed_context_scripts(
    fixture: &Fixture,
    mcp: &mut Mcp,
    url: &str,
    target: &str,
    context: &Value,
    existing_page: &Value,
) {
    let add = json!({"operation":"context_init_script_add","context":context,"source":"globalThis.AcmeInstalledContext='Acme context';globalThis.AcmeInstalledBindingAtInit=typeof AcmeInstalledBridge"}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &add])
        .await;
    let added = completed(mcp, target, submitted).await;
    let binding_add =
        json!({"operation":"binding_add","scope":context,"name":"AcmeInstalledBridge"}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &binding_add])
        .await;
    let binding = completed(mcp, target, submitted).await;
    let prepared = operation(
        mcp,
        target,
        json!({"operation":"context_framework_prepare","context":context}),
    )
    .await;
    let existing = operation(mcp,target,json!({"operation":"evaluate","scope":existing_page,"expression":"typeof AcmeInstalledContext","isFunction":false})).await;
    assert_eq!(existing["value"]["value"], "undefined");
    let created = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await;
    let page = &created["page"];
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{url}/framework"),"waitUntil":"load"}})).await;
    let mounted = operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"async()=>{const until=Date.now()+10000;while(!document.querySelector('#count')){if(Date.now()>until)throw Error('Acme context React did not mount');await new Promise(resolve=>setTimeout(resolve,25));}return AcmeInstalledContext;}"})).await;
    assert_eq!(mounted["value"]["value"], "Acme context");
    let binding_at_init = operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"AcmeInstalledBindingAtInit","isFunction":false})).await;
    assert_eq!(binding_at_init["value"]["value"], "function");
    let evaluation = mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":"AcmeInstalledBridge('Acme context private')","isFunction":false}})).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = mcp.detailed_browser_status(target).await;
        let browser = &status["browser"];
        let items = browser["callbacks"]
            .as_array()
            .or_else(|| browser["resources"].as_array())
            .unwrap();
        if let Some(callback) = items.iter().find(|item| item["kind"] == "binding") {
            assert!(!callback.to_string().contains("Acme context private"));
            break callback["reference"].clone();
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Context binding unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let read = json!({"operation":"binding_read","callback":callback}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &read])
        .await;
    let read = completed(mcp, target, submitted).await;
    assert_eq!(read["bindingId"], binding["bindingId"]);
    assert_eq!(read["arguments"][0]["value"], "Acme context private");
    operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"binding_resolve","value":42}})).await;
    assert_eq!(
        completed(mcp, target, evaluation).await["value"]["value"],
        42
    );
    let opened = operation(
        mcp,
        target,
        json!({"operation":"framework_open","scope":page}),
    )
    .await;
    let tree = operation(mcp,target,json!({"operation":"framework_query","registry":opened["registry"],"query":{"kind":"tree","limit":100}})).await;
    assert!(
        tree["report"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "AcmeCounter"),
        "{tree}"
    );
    operation(
        mcp,
        target,
        json!({"operation":"framework_release","registry":opened["registry"]}),
    )
    .await;
    for script in [&added["scriptId"], &prepared["scriptId"]] {
        operation(
            mcp,
            target,
            json!({"operation":"context_init_script_remove","context":context,"scriptId":script}),
        )
        .await;
    }
    let list = json!({"operation":"context_init_script_list","context":context}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &list])
        .await;
    assert!(
        completed(mcp, target, submitted).await["scripts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let retained = operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"AcmeInstalledContext","isFunction":false})).await;
    assert_eq!(retained["value"]["value"], "Acme context");
    operation(
        mcp,
        target,
        json!({"operation":"binding_remove","scope":context,"bindingId":binding["bindingId"]}),
    )
    .await;
    let removed = operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"typeof AcmeInstalledBridge","isFunction":false})).await;
    assert_eq!(removed["value"]["value"], "undefined");
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{url}/framework"),"waitUntil":"load"}})).await;
    let absent = operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"JSON.stringify([typeof AcmeInstalledContext,typeof __REACT_DEVTOOLS_GLOBAL_HOOK__])","isFunction":false})).await;
    assert_eq!(absent["value"]["value"], "[\"undefined\",\"undefined\"]");
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
}

async fn installed_framework(
    fixture: &Fixture,
    mcp: &mut Mcp,
    url: &str,
    target: &str,
    page: &Value,
) {
    let prepare = json!({"operation":"framework_prepare","page":page}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &prepare])
        .await;
    let prepared = completed(mcp, target, submitted).await;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{url}/framework"),"waitUntil":"load"}})).await;
    let mounted=operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"async()=>{const until=Date.now()+10000;while(!document.querySelector('#count')){if(Date.now()>until)throw Error('Acme React did not mount');await new Promise(resolve=>setTimeout(resolve,25));}return true;}"})).await;
    assert_eq!(mounted["value"]["value"], true);
    let opened = operation(
        mcp,
        target,
        json!({"operation":"framework_open","scope":page}),
    )
    .await;
    let registry = &opened["registry"];
    let dispose = json!({"operation":"handle_dispose","handle":registry}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &dispose])
        .await;
    let refused = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(
        refused["operation"]["code"], "FRAMEWORK_RELEASE_REQUIRED",
        "{refused}"
    );
    let tree=json!({"operation":"framework_query","registry":registry,"query":{"kind":"tree","limit":100}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &tree])
        .await;
    let tree = completed(mcp, target, submitted).await;
    assert!(
        tree["report"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "AcmeCounter"),
        "{tree}"
    );
    operation(mcp,target,json!({"operation":"framework_query","registry":registry,"query":{"kind":"renders_start","durationMs":30000}})).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"document.querySelector('#count').click();true","isFunction":false})).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"async()=>{const until=Date.now()+10000;while(document.querySelector('#count').textContent!=='Count 1'){if(Date.now()>until)throw Error('Acme React update did not commit');await new Promise(resolve=>setTimeout(resolve,25));}return true;}"})).await;
    let profile =
        json!({"operation":"framework_query","registry":registry,"query":{"kind":"renders_stop"}})
            .to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &profile])
        .await;
    let profile = completed(mcp, target, submitted).await;
    assert!(
        profile["report"]["data"]["dataForRoots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|root| root["commitData"]
                .as_array()
                .is_some_and(|commits| !commits.is_empty())),
        "{profile}"
    );
    let released = operation(
        mcp,
        target,
        json!({"operation":"framework_release","registry":registry}),
    )
    .await;
    assert_eq!(released["released"], true);
    assert_eq!(released["hookInstallationPreserved"], true);
    let repeated = operation(
        mcp,
        target,
        json!({"operation":"framework_release","registry":registry}),
    )
    .await;
    assert_eq!(repeated["released"], true);
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &dispose])
        .await;
    let stale = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(stale["operation"]["code"], "STALE_REFERENCE", "{stale}");
    operation(
        mcp,
        target,
        json!({"operation":"page_init_script_remove","page":page,"scriptId":prepared["scriptId"]}),
    )
    .await;
}

async fn installed_heap(fixture: &Fixture, mcp: &mut Mcp, target: &str, page: &Value) {
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>{globalThis.AcmeInstalledHeapRecord=class AcmeInstalledHeapRecord{constructor(){this.AcmeMarker={value:42}}};globalThis.acmeInstalledHeapRecords=Array.from({length:2},()=>new AcmeInstalledHeapRecord());return true;}"})).await;
    let opened = operation(
        mcp,
        target,
        json!({"operation":"developer_open","page":page,"panel":"heap-profiler"}),
    )
    .await;
    let developer = &opened["developer"];
    let capture = json!({"operation":"heap_capture","page":page,"maxBytes":33554432}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &capture])
        .await;
    let capture = completed(mcp, target, submitted).await;
    let report = &capture["reports"][0];
    assert_eq!(report["format"], "v8-heap-snapshot-json");
    assert_eq!(report["complete"], true);
    let snapshot: Value = serde_json::from_slice(
        &std::fs::read(report["attachment"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert!(snapshot["snapshot"]["node_count"].as_u64().unwrap() > 0);
    let loaded = operation(
        mcp,
        target,
        json!({"operation":"heap_load","developer":developer,"evidenceId":report["evidenceId"]}),
    )
    .await;
    let snapshot = &loaded["snapshotId"];
    let query=json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot,"query":{"kind":"summary"}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &query])
        .await;
    let summary = completed(mcp, target, submitted).await;
    assert!(
        summary["result"]["data"]["statistics"]["total"]
            .as_u64()
            .unwrap()
            > 0
    );
    let query=json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot,"maxNameLength":64,"query":{"kind":"objects","className":"AcmeInstalledHeapRecord","propertyName":"AcmeMarker","nodeType":"object","retainedSize":{"min":1},"selfSize":{"min":1},"sortBy":"id","limit":10}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &query])
        .await;
    let objects = completed(mcp, target, submitted).await;
    let items = objects["result"]["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{objects}");
    assert_eq!(objects["result"]["data"]["endPosition"], 2);
    assert_eq!(
        objects["result"]["data"].get("nextOffset"),
        Some(&Value::Null)
    );
    let node = items[0]["id"].clone();
    for kind in ["retainers", "dominators", "duplicate_strings"] {
        let query = match kind {
            "duplicate_strings" => json!({"kind":kind,"limit":10}),
            "dominators" => json!({"kind":kind,"nodeId":node}),
            _ => json!({"kind":kind,"nodeId":node,"limit":10}),
        };
        let read=operation(mcp,target,json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot,"query":query})).await;
        assert!(!read["result"]["data"].is_null(), "{read}");
    }
    let edges=operation(mcp,target,json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot,"query":{"kind":"edges","nodeId":node,"sortBy":"name","minRetainedSize":9007199254740991u64,"excludePrimitives":false}})).await;
    assert!(
        edges["result"]["data"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let short=operation(mcp,target,json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot,"maxNameLength":2,"query":{"kind":"object","nodeId":node}})).await;
    assert_eq!(short["result"]["data"]["name"], "Ac");
    assert_eq!(short["result"]["truncated"], true);
    let paths=operation(mcp,target,json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot,"maxNameLength":64,"query":{"kind":"retaining_paths","nodeId":node,"maxDepth":3,"maxNodes":20,"maxSiblings":2}})).await;
    assert!(!paths["result"]["data"].is_null(), "{paths}");
    let default_paths=operation(mcp,target,json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot,"query":{"kind":"retaining_paths","nodeId":node}})).await;
    assert!(
        !default_paths["result"]["data"].is_null(),
        "{default_paths}"
    );
    assert!(serde_json::to_vec(&default_paths).unwrap().len() <= 65536);
    let classes=operation(mcp,target,json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot,"query":{"kind":"classes","limit":1}})).await;
    let class_id = &classes["result"]["data"]["items"][0]["classId"];
    assert!(classes["result"]["data"]["objectCount"].as_u64().unwrap() > 0);
    assert!(classes["result"]["data"]["totalSelfSize"].as_u64().unwrap() > 0);
    let filtered=operation(mcp,target,json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot,"query":{"kind":"classes","filterName":"noNativeContext","limit":1}})).await;
    assert!(filtered["result"]["data"]["items"].is_array(), "{filtered}");
    let instances=operation(mcp,target,json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot,"query":{"kind":"class_nodes","classId":class_id,"limit":1}})).await;
    assert_eq!(
        instances["result"]["data"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "{instances}"
    );
    let contexts=operation(mcp,target,json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot,"query":{"kind":"contexts","retainedSize":{"min":0},"offset":0,"limit":1}})).await;
    assert_eq!(contexts["result"]["data"]["scoreIsReclaimableBytes"], false);
    assert_eq!(
        contexts["result"]["data"]["scoreMetric"],
        "deadFieldsRetainedSizeSum"
    );
    assert!(
        contexts["result"]["data"]["scopes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|scope| scope["contexts"].as_array().unwrap().len())
            .sum::<usize>()
            <= 1
    );
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"acmeInstalledHeapRecords.push(new AcmeInstalledHeapRecord());true","isFunction":false})).await;
    let second = operation(
        mcp,
        target,
        json!({"operation":"heap_capture","page":page,"maxBytes":33554432}),
    )
    .await;
    let second_report = &second["reports"][0];
    let loaded_second=operation(mcp,target,json!({"operation":"heap_load","developer":developer,"evidenceId":second_report["evidenceId"]})).await;
    let second_snapshot = &loaded_second["snapshotId"];
    let comparison=operation(mcp,target,json!({"operation":"heap_compare","developer":developer,"snapshotId":second_snapshot,"baseSnapshotId":snapshot,"limit":100})).await;
    let difference = comparison["result"]["data"]["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|class| class["name"] == "AcmeInstalledHeapRecord")
        .unwrap();
    assert_eq!(difference["addedCount"], 1, "{difference}");
    assert!(
        comparison["result"]["data"]["totals"]["addedCount"]
            .as_u64()
            .unwrap()
            > 0
    );
    let detailed=json!({"operation":"heap_compare","developer":developer,"snapshotId":second_snapshot,"baseSnapshotId":snapshot,"classIndex":difference["classIndex"],"maxNameLength":2,"limit":10}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &detailed])
        .await;
    let detailed = completed(mcp, target, submitted).await;
    assert_eq!(detailed["result"]["data"]["classDiff"]["name"], "Ac");
    assert_eq!(detailed["result"]["data"]["objects"][0]["change"], "added");
    assert_eq!(
        detailed["result"]["data"]["addedObjectSnapshot"],
        *second_snapshot
    );
    operation(
        mcp,
        target,
        json!({"operation":"heap_release","developer":developer,"snapshotId":second_snapshot}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"developer_evidence_remove","evidenceId":second_report["evidenceId"]}),
    )
    .await;
    let import=json!({"operation":"heap_import","developer":developer,"path":report["attachment"]["path"],"maxBytes":33554432}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &import])
        .await;
    let imported = completed(mcp, target, submitted).await;
    let imported_report = &imported["reports"][0];
    let imported_model=operation(mcp,target,json!({"operation":"heap_load","developer":developer,"evidenceId":imported_report["evidenceId"]})).await;
    assert_eq!(imported_model["identity"]["source"], "caller-import");
    assert_eq!(imported_model["identity"]["verified"], false);
    let imported_snapshot = &imported_model["snapshotId"];
    let imported_summary=operation(mcp,target,json!({"operation":"heap_query","developer":developer,"snapshotId":imported_snapshot,"query":{"kind":"summary"}})).await;
    assert!(
        imported_summary["result"]["data"]["statistics"]["total"]
            .as_u64()
            .unwrap()
            > 0
    );
    operation(
        mcp,
        target,
        json!({"operation":"heap_release","developer":developer,"snapshotId":imported_snapshot}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"developer_evidence_remove","evidenceId":imported_report["evidenceId"]}),
    )
    .await;
    operation(mcp, target, json!({"operation":"heap_stop","page":page})).await;
    let release =
        json!({"operation":"heap_release","developer":developer,"snapshotId":snapshot}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &release])
        .await;
    completed(mcp, target, submitted).await;
    operation(
        mcp,
        target,
        json!({"operation":"developer_close","developer":developer}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"developer_evidence_remove","evidenceId":report["evidenceId"]}),
    )
    .await;
}

async fn installed_performance(
    fixture: &Fixture,
    mcp: &mut Mcp,
    url: &str,
    target: &str,
    page: &Value,
) {
    let status = mcp.tool("browser_status", json!({"targetId":target})).await;
    let context = &status["browser"]["selectedContext"];
    operation(mcp, target, json!({"operation":"page_select","page":page})).await;
    let start=json!({"operation":"trace_start","context":context,"options":{"screenshots":false,"cpuSampling":false}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &start])
        .await;
    completed(mcp, target, submitted).await;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{url}/performance"),"waitUntil":"load"}})).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>new Promise(resolve=>{const finish=()=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true)));if(performance.getEntriesByType('paint').some(entry=>entry.name==='first-contentful-paint'))return finish();const observer=new PerformanceObserver(list=>{if(list.getEntries().some(entry=>entry.name==='first-contentful-paint')){observer.disconnect();finish();}});observer.observe({type:'paint',buffered:true});})"})).await;
    operation(mcp,target,json!({"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#work"},"action":{"kind":"click"}})).await;
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true))))"})).await;
    let stop = json!({"operation":"trace_stop"}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &stop])
        .await;
    let stopped = completed(mcp, target, submitted).await;
    let report = &stopped["report"];
    assert_eq!(report["complete"], true);
    let opened = operation(
        mcp,
        target,
        json!({"operation":"developer_open","page":page,"panel":"timeline"}),
    )
    .await;
    let developer = &opened["developer"];
    let css=json!({"operation":"page_css","developer":developer,"page":page,"locator":{"kind":"css","selector":"#work"}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &css])
        .await;
    let styles = completed(mcp, target, submitted).await;
    assert!(
        styles["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["declarations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|property| property["name"] == "--acme")),
        "{styles}"
    );
    let loaded=operation(mcp,target,json!({"operation":"performance_load","developer":developer,"evidenceId":report["evidenceId"]})).await;
    let analysis = &loaded["analysisId"];
    let insight_set = loaded["summary"]["data"]["insightSets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|set| set["url"] == format!("{url}/performance") && set["navigationObserved"] == true)
        .unwrap();
    let query=json!({"operation":"performance_query","developer":developer,"analysisId":analysis,"query":{"kind":"vitals","insightSetId":insight_set["insightSetId"]}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &query])
        .await;
    let vitals = completed(mcp, target, submitted).await;
    for metric in ["LCP", "INP"] {
        assert_eq!(
            vitals["result"]["data"][metric]["available"], true,
            "{vitals}"
        );
        assert!(
            vitals["result"]["data"][metric]["value"].as_f64().unwrap() > 0.0,
            "{vitals}"
        );
    }
    operation(
        mcp,
        target,
        json!({"operation":"performance_release","developer":developer,"analysisId":analysis}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"developer_close","developer":developer}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"developer_evidence_remove","evidenceId":report["evidenceId"]}),
    )
    .await;
}
