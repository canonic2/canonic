//! The HTTP driver over loopback sockets: replies and their statuses, the
//! HTTP subset, SSE progress, cancellation by closure, independent IDs across
//! POSTs, listen streams and shutdown.
use super::*;
use crate::{
    ModernOptions, ServerInfo, SessionOptions, SubscriptionOptions, Tool,
    stdio::{ChangeSender, Context, Lane, Outcome, Request},
    tool::ToolResult,
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    sync::mpsc::Sender,
};

const WAIT: Duration = Duration::from_secs(10);

/// Echoes, pauses, reports progress or waits for cancellation.
struct Acme {
    changes: Mutex<Option<ChangeSender>>,
    events: Mutex<Sender<&'static str>>,
    started: AtomicUsize,
    keyed: AtomicUsize,
    keyed_peak: AtomicUsize,
}

impl Handler for Acme {
    fn admit(&self, request: &Request) -> crate::stdio::Admission {
        let keyed = matches!(request, Request::Tool(call) if call.name == "acme_keyed");
        crate::stdio::Admission {
            key: keyed.then(|| "acme".to_owned()),
            ..crate::stdio::Admission::default()
        }
    }

    fn connected(&self, changes: ChangeSender) {
        *self.changes.lock().unwrap() = Some(changes);
    }

    fn initialize(&self, _: &crate::Client) -> Result<ServerInfo, RpcError> {
        Err(RpcError::new(rpc::INTERNAL_ERROR, "never called over HTTP"))
    }

    fn call(&self, request: Request, context: &Context) -> Outcome {
        let call = match request {
            Request::Tool(call) => call,
            Request::Notification { method, .. } => {
                if method == "notifications/acme/changed" {
                    self.send("notified");
                }
                return Outcome::Done;
            }
            _ => return Outcome::Done,
        };
        let text =
            |text: &str| Outcome::Tool(ToolResult::success(vec![crate::Content::text(text)]));
        match call.name.as_str() {
            "acme_echo" => text(call.arguments["text"].as_str().unwrap_or_default()),
            "acme_keyed" => {
                let running = self.keyed.fetch_add(1, Ordering::SeqCst) + 1;
                self.keyed_peak.fetch_max(running, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(50));
                self.keyed.fetch_sub(1, Ordering::SeqCst);
                text("keyed")
            }
            "acme_pause" => {
                // Returns only once three calls run at the same time.
                self.started.fetch_add(1, Ordering::SeqCst);
                let deadline = Instant::now() + WAIT;
                while self.started.load(Ordering::SeqCst) < 3 && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(5));
                }
                text(&self.started.load(Ordering::SeqCst).to_string())
            }
            "acme_progress" => {
                assert!(context.progress(1.0, Some(2.0), Some("half")));
                assert!(context.progress(2.0, Some(2.0), None));
                text("done")
            }
            _ => {
                self.send("started");
                let deadline = Instant::now() + WAIT;
                while context.check().is_ok() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(5));
                }
                if context.check().is_err() {
                    self.send("cancelled");
                }
                text("late")
            }
        }
    }
}

impl Acme {
    fn send(&self, event: &'static str) {
        let _ = self.events.lock().unwrap().send(event);
    }
}

fn tool(name: &str) -> Tool {
    Tool::new(
        name,
        "Run an Acme test step.",
        json!({"type":"object","properties":{
            "text":{"type":"string","x-mcp-header":"Text"}
        }}),
    )
}

fn catalog() -> Arc<Catalog> {
    Arc::new(
        Catalog::new(
            [
                "acme_echo",
                "acme_pause",
                "acme_progress",
                "acme_block",
                "acme_keyed",
            ]
            .into_iter()
            .map(tool)
            .collect(),
        )
        .unwrap(),
    )
}

struct Server {
    address: SocketAddr,
    shutdown: Shutdown,
    serving: Option<thread::JoinHandle<Result<(), ServeError>>>,
    handler: Arc<Acme>,
    events: Mutex<mpsc::Receiver<&'static str>>,
}

impl Server {
    fn start(http: HttpOptions, subscriptions: Option<SubscriptionOptions>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, events) = mpsc::channel();
        let handler = Arc::new(Acme {
            changes: Mutex::new(None),
            events: Mutex::new(sender),
            started: AtomicUsize::new(0),
            keyed: AtomicUsize::new(0),
            keyed_peak: AtomicUsize::new(0),
        });
        let options = Options {
            session: SessionOptions {
                modern: Some(ModernOptions::new(ServerInfo::new("acme", "1.0.0"))),
                subscriptions,
                extension_notifications: vec!["notifications/acme/changed".into()],
                ..SessionOptions::default()
            },
            lanes: vec![Lane::new("tools", 4, 16)],
            ..Options::default()
        };
        let shutdown = Shutdown::new();
        let serving = {
            let (handler, shutdown) = (Arc::clone(&handler), shutdown.clone());
            thread::spawn(move || serve(handler, catalog(), listener, &http, &options, &shutdown))
        };
        Self {
            address,
            shutdown,
            serving: Some(serving),
            handler,
            events: Mutex::new(events),
        }
    }

    fn next_event(&self) -> Result<&'static str, mpsc::RecvTimeoutError> {
        self.events.lock().unwrap().recv_timeout(WAIT)
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect(self.address).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream
    }

    /// Sends raw request bytes and reads the whole response.
    fn exchange(&self, request: &[u8]) -> Reply {
        let mut stream = self.connect();
        stream.write_all(request).unwrap();
        Reply::read(&mut BufReader::new(stream))
    }

    fn stop(&mut self) -> Result<(), ServeError> {
        self.shutdown.shutdown();
        self.serving.take().unwrap().join().unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.serving.is_some() {
            let _ = self.stop();
        }
    }
}

/// A modern POST with matching MCP headers, plus `extra` header lines.
fn post(method: &str, id: &Value, mut params: Value, extra: &str) -> Vec<u8> {
    params["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("2026-07-28");
    let capabilities = "io.modelcontextprotocol/clientCapabilities";
    if params["_meta"].get(capabilities).is_none() {
        params["_meta"][capabilities] = json!({});
    }
    let body = serde_json::to_vec(&rpc::request(id.clone(), method, params.clone())).unwrap();
    let name = params["name"]
        .as_str()
        .map(|name| format!("Mcp-Name: {name}\r\n"))
        .unwrap_or_default();
    let mut request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\nMCP-Protocol-Version: 2026-07-28\r\n\
         Mcp-Method: {method}\r\n{name}{extra}Content-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(&body);
    request
}

fn call(id: &Value, name: &str, arguments: &Value, extra: &str) -> Vec<u8> {
    post(
        "tools/call",
        id,
        json!({"name": name, "arguments": arguments}),
        extra,
    )
}

#[derive(Debug)]
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn read(reader: &mut impl BufRead) -> Self {
        let mut status = 0;
        let mut headers = Vec::new();
        loop {
            let text = line(reader);
            if status == 0 {
                status = text.split(' ').nth(1).unwrap().parse().unwrap();
                if status == 100 {
                    assert_eq!(line(reader), "");
                    status = 0;
                }
                continue;
            }
            if text.is_empty() {
                break;
            }
            let (name, value) = text.split_once(": ").unwrap();
            headers.push((name.to_ascii_lowercase(), value.to_owned()));
        }
        let mut reply = Self {
            status,
            headers,
            body: Vec::new(),
        };
        if reply.header("content-type") != Some("text/event-stream") {
            reader.read_to_end(&mut reply.body).unwrap();
        }
        reply
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

fn line(reader: &mut impl BufRead) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    line.trim_end_matches(['\r', '\n']).to_owned()
}

/// The next SSE message, or `None` at a keep-alive comment or the end.
fn event(reader: &mut impl BufRead) -> Option<Value> {
    let first = line(reader);
    if first.starts_with(':') || first.is_empty() {
        return None;
    }
    assert_eq!(first, "event: message");
    let data = line(reader);
    assert_eq!(line(reader), "");
    Some(serde_json::from_str(data.strip_prefix("data: ").unwrap()).unwrap())
}

fn http() -> HttpOptions {
    HttpOptions::default()
}

#[test]
fn json_requests_reply_with_their_own_ids_and_statuses() {
    let server = Server::start(http(), None);
    let listed = server.exchange(&post("tools/list", &json!("list-1"), json!({}), ""));
    assert_eq!(listed.status, 200);
    assert_eq!(listed.header("content-type"), Some("application/json"));
    assert_eq!(listed.header("connection"), Some("close"));
    assert_eq!(listed.json()["id"], "list-1");
    assert_eq!(
        listed.json()["result"]["tools"].as_array().unwrap().len(),
        5
    );

    let echoed = server.exchange(&call(
        &json!(7),
        "acme_echo",
        &json!({"text":"hi"}),
        "Mcp-Param-Text: hi\r\n",
    ));
    assert_eq!(echoed.status, 200);
    assert_eq!(echoed.json()["id"], 7);
    assert_eq!(echoed.json()["result"]["content"][0]["text"], "hi");

    let mismatch = server.exchange(&call(&json!(8), "acme_echo", &json!({"text":"hi"}), ""));
    assert_eq!(
        (mismatch.status, mismatch.json()["error"]["code"].clone()),
        (400, json!(-32020))
    );
    // Invalid body metadata (step 9) is refused before parameter headers (step 13).
    let invalid = server.exchange(&post(
        "tools/call",
        &json!(10),
        json!({"name":"acme_echo","arguments":{"text":"hi"},
            "_meta":{"io.modelcontextprotocol/clientCapabilities":"invalid"}}),
        "",
    ));
    assert_eq!(
        (invalid.status, invalid.json()["error"]["code"].clone()),
        (200, json!(-32602))
    );
    let missing = server.exchange(&post("acme/missing", &json!(9), json!({}), ""));
    assert_eq!(missing.status, 404);
    assert_eq!(missing.json()["id"], 9);

    let get = server.exchange(b"GET /mcp HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert_eq!((get.status, get.header("allow")), (405, Some("POST")));
    let notification =
        br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#;
    let mut request = format!(
        "POST /mcp HTTP/1.1\r\nContent-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\nContent-Length: {}\r\n\r\n",
        notification.len()
    )
    .into_bytes();
    request.extend_from_slice(notification);
    let accepted = server.exchange(&request);
    assert_eq!((accepted.status, accepted.body.len()), (202, 0));
}

#[test]
fn http_subset_refuses_what_it_does_not_serve() {
    let server = Server::start(http(), None);
    let status = |request: &[u8]| server.exchange(request).status;
    assert_eq!(status(b"POST /mcp HTTP/1.0\r\n\r\n"), 505);
    assert_eq!(
        status(b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n"),
        501
    );
    assert_eq!(
        status(b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 2\r\n\r\n"),
        400
    );
    let head = "POST /mcp HTTP/1.1\r\nContent-Type: application/json\r\n\
                Accept: application/json, text/event-stream\r\n";
    assert_eq!(status(format!("{head}\r\n").as_bytes()), 411);
    assert_eq!(
        status(format!("{head}Content-Length: 99999999\r\n\r\n").as_bytes()),
        413
    );
    assert_eq!(
        status(format!("{head}Expect: acme\r\nContent-Length: 2\r\n\r\n{{}}").as_bytes()),
        417
    );
    let long = format!(
        "POST /{} HTTP/1.1\r\n\r\n",
        "a".repeat(MAX_REQUEST_LINE_BYTES)
    );
    assert_eq!(status(long.as_bytes()), 431);
    let many = "X-Acme: a\r\n".repeat(MAX_HEADER_FIELDS + 1);
    assert_eq!(
        status(format!("POST /mcp HTTP/1.1\r\n{many}\r\n").as_bytes()),
        431
    );

    let mut stream = server.connect();
    let request = post(
        "tools/list",
        &json!(1),
        json!({}),
        "Expect: 100-continue\r\n",
    );
    let split = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap()
        + 4;
    stream.write_all(&request[..split]).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    assert_eq!(line(&mut reader), "HTTP/1.1 100 Continue");
    assert_eq!(line(&mut reader), "");
    stream.write_all(&request[split..]).unwrap();
    assert_eq!(Reply::read(&mut reader).status, 200);
}

#[test]
fn progress_requests_stream_progress_then_the_reply() {
    let server = Server::start(http(), None);
    let mut stream = server.connect();
    stream
        .write_all(&post(
            "tools/call",
            &json!("p-1"),
            json!({"name":"acme_progress","arguments":{},"_meta":{"progressToken":"t"}}),
            "",
        ))
        .unwrap();
    let mut reader = BufReader::new(stream);
    let head = Reply::read(&mut reader);
    assert_eq!(head.status, 200);
    assert_eq!(head.header("content-type"), Some("text/event-stream"));
    let mut progress = Vec::new();
    let reply = loop {
        let message = event(&mut reader).expect("an event before the reply");
        if message.get("id").is_some() {
            break message;
        }
        progress.push(message["params"]["progress"].clone());
    };
    assert_eq!(progress, [json!(1.0), json!(2.0)]);
    assert_eq!(reply["id"], "p-1");
    assert_eq!(reply["result"]["content"][0]["text"], "done");
}

#[test]
fn restoring_a_long_client_id_keeps_replies_within_the_output_limit() {
    let limit = 1024;
    let client = json!("c".repeat(rpc::MAX_ID_BYTES));
    // A reply that fit with server ID 7 and has exactly the room a client ID needs.
    let room = limit - REPLY_PREFIX.len() - serde_json::to_vec(&client).unwrap().len();
    let text = |length: usize| {
        let filler = "x".repeat(length - r#","result":{"text":""}}"#.len());
        format!(r#","result":{{"text":"{filler}"}}}}"#).into_bytes()
    };
    let (fits, code) = with_client_id(&client, &text(room), limit);
    assert_eq!((fits.len(), code), (limit, None));
    let reply: Value = serde_json::from_slice(&fits).unwrap();
    assert_eq!(reply["id"], client);

    let (refused, code) = with_client_id(&client, &text(room + 1), limit);
    assert!(refused.len() <= limit);
    assert_eq!(code, Some(rpc::INTERNAL_ERROR));
    let reply: Value = serde_json::from_slice(&refused).unwrap();
    assert_eq!(reply["id"], client);
    assert_eq!(reply["error"]["message"], "Response exceeds output limit");

    let error = br#","error":{"code":-32602,"message":"Invalid params"}}"#;
    assert_eq!(with_client_id(&json!(1), error, limit).1, Some(-32602));
}

#[test]
fn listed_modern_notifications_reach_the_handler() {
    let server = Server::start(http(), None);
    let notification = |method: &str| {
        let body = serde_json::to_vec(&rpc::notification(
            method,
            json!({"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}),
        ))
        .unwrap();
        let mut request = format!(
            "POST /mcp HTTP/1.1\r\nContent-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        request.extend_from_slice(&body);
        request
    };
    let unlisted = server.exchange(&notification("notifications/acme/other"));
    assert_eq!(unlisted.status, 202);
    let listed = server.exchange(&notification("notifications/acme/changed"));
    assert_eq!(listed.status, 202);
    assert_eq!(server.next_event(), Ok("notified"));
}

#[test]
fn closing_a_response_cancels_its_request() {
    let server = Server::start(http(), None);
    // Trailing input is sent once the call runs, so the request reader cannot
    // have buffered it: a byte then a graceful close, or input over the bound.
    let over_bound = vec![b'x'; 70_000];
    let trailing: [(&[u8], bool); 3] = [(b"", true), (b"x", true), (&over_bound, false)];
    for (bytes, close) in trailing {
        let mut stream = server.connect();
        stream
            .write_all(&call(&json!(1), "acme_block", &json!({}), ""))
            .unwrap();
        assert_eq!(server.next_event(), Ok("started"));
        stream.write_all(bytes).unwrap();
        if close {
            stream.shutdown(std::net::Shutdown::Write).unwrap();
        }
        assert_eq!(
            server.next_event(),
            Ok("cancelled"),
            "{} bytes",
            bytes.len()
        );
        drop(stream);
    }
}

#[test]
fn a_key_orders_requests_across_posts() {
    let server = Arc::new(Server::start(http(), None));
    let clients: Vec<_> = (0..4)
        .map(|index| {
            let server = Arc::clone(&server);
            thread::spawn(move || {
                server.exchange(&call(&json!(index), "acme_keyed", &json!({}), ""))
            })
        })
        .collect();
    for client in clients {
        assert_eq!(client.join().unwrap().status, 200);
    }
    // Four workers are free, but one key admits one call at a time.
    assert_eq!(server.handler.keyed_peak.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_posts_with_the_same_id_run_independently() {
    let server = Arc::new(Server::start(http(), None));
    let clients: Vec<_> = (0..3)
        .map(|_| {
            let server = Arc::clone(&server);
            thread::spawn(move || server.exchange(&call(&json!(1), "acme_pause", &json!({}), "")))
        })
        .collect();
    for client in clients {
        let reply = client.join().unwrap();
        let reply = reply.json();
        assert_eq!(reply["id"], 1);
        assert_eq!(
            reply["result"]["content"][0]["text"], "3",
            "each call saw all three running"
        );
    }
}

#[test]
fn connections_above_the_limit_get_503() {
    let server = Server::start(
        HttpOptions {
            max_connections: 1,
            read_timeout: Duration::from_secs(1),
            ..http()
        },
        None,
    );
    let idle = server.connect();
    // The idle connection holds the only slot until its read timeout. The
    // second sends nothing, so the refusal is not raced by unread input.
    let busy = Reply::read(&mut BufReader::new(server.connect()));
    assert_eq!((busy.status, busy.body.len()), (503, 0));
    let mut closed = Vec::new();
    BufReader::new(idle).read_to_end(&mut closed).unwrap();
    assert!(
        closed.is_empty(),
        "a slow client is closed without a response"
    );
}

#[test]
fn listen_streams_acknowledge_keep_alive_publish_and_end_at_shutdown() {
    let mut server = Server::start(
        HttpOptions {
            keep_alive: Duration::from_secs(1),
            ..http()
        },
        Some(SubscriptionOptions::default()),
    );
    let mut stream = server.connect();
    stream
        .write_all(&post(
            "subscriptions/listen",
            &json!("listen-1"),
            json!({"notifications":{"toolsListChanged":true}}),
            "",
        ))
        .unwrap();
    let mut reader = BufReader::new(stream);
    assert_eq!(Reply::read(&mut reader).status, 200);
    let ack = event(&mut reader).unwrap();
    assert_eq!(ack["method"], "notifications/subscriptions/acknowledged");
    assert_eq!(ack["params"]["_meta"][SUBSCRIPTION_KEY], "listen-1");
    assert_eq!(event(&mut reader), None, "a keep-alive comment while quiet");

    let sender = server.handler.changes.lock().unwrap().clone().unwrap();
    // The replacement drops the `x-mcp-header` annotation from `acme_echo`.
    let plain = Tool::new(
        "acme_echo",
        "Run an Acme test step.",
        json!({"type":"object","properties":{"text":{"type":"string"}}}),
    );
    sender
        .send(Change::Tools(Arc::new(Catalog::new(vec![plain]).unwrap())))
        .unwrap();
    let changed = loop {
        if let Some(message) = event(&mut reader) {
            break message;
        }
    };
    assert_eq!(changed["method"], "notifications/tools/list_changed");
    assert_eq!(changed["params"]["_meta"][SUBSCRIPTION_KEY], "listen-1");
    let listed = server.exchange(&post("tools/list", &json!(2), json!({}), ""));
    assert_eq!(
        listed.json()["result"]["tools"].as_array().unwrap().len(),
        1
    );
    // Parameter headers follow the replaced catalog: no header is needed now.
    let echoed = server.exchange(&call(&json!(3), "acme_echo", &json!({"text":"hi"}), ""));
    assert_eq!(echoed.status, 200);
    assert_eq!(echoed.json()["result"]["content"][0]["text"], "hi");

    let stopping = thread::spawn(move || server.stop());
    let last = loop {
        if let Some(message) = event(&mut reader) {
            break message;
        }
    };
    assert_eq!(last["id"], "listen-1");
    assert_eq!(last["result"]["_meta"][SUBSCRIPTION_KEY], "listen-1");
    drop(reader);
    stopping.join().unwrap().unwrap();
}

#[test]
fn listen_streams_end_at_their_lifetime_and_release_on_closure() {
    let server = Server::start(
        http(),
        Some(SubscriptionOptions {
            max_streams: 1,
            timeout: Duration::from_millis(300),
            ..SubscriptionOptions::default()
        }),
    );
    let listen = |id: &str| {
        post(
            "subscriptions/listen",
            &json!(id),
            json!({"notifications":{}}),
            "",
        )
    };
    let mut stream = server.connect();
    stream.write_all(&listen("a")).unwrap();
    let mut reader = BufReader::new(stream);
    assert_eq!(Reply::read(&mut reader).status, 200);
    assert!(event(&mut reader).is_some());
    let expired = event(&mut reader).unwrap();
    assert_eq!(
        (
            expired["id"].clone(),
            expired["result"]["resultType"].clone()
        ),
        (json!("a"), json!("complete"))
    );

    let mut stream = server.connect();
    stream.write_all(&listen("b")).unwrap();
    let mut reader = BufReader::new(stream);
    assert_eq!(Reply::read(&mut reader).status, 200);
    assert!(event(&mut reader).is_some());
    drop(reader);
    // The closed stream releases the only slot for the next listener.
    let deadline = Instant::now() + WAIT;
    loop {
        let mut stream = server.connect();
        stream.write_all(&listen("c")).unwrap();
        let mut reader = BufReader::new(stream);
        Reply::read(&mut reader);
        if event(&mut reader).is_some_and(|ack| ack.get("method").is_some()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the closed stream was never released"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// Retains the change sender so a test can publish a refused change.
struct Conflict(Mutex<Option<ChangeSender>>);

impl Handler for Conflict {
    fn connected(&self, sender: ChangeSender) {
        *self.0.lock().unwrap() = Some(sender);
    }

    fn initialize(&self, _: &crate::Client) -> Result<ServerInfo, RpcError> {
        Err(RpcError::new(rpc::INTERNAL_ERROR, "never called over HTTP"))
    }

    fn call(&self, _: Request, _: &Context) -> Outcome {
        Outcome::Done
    }
}

#[test]
fn a_refused_change_stops_serving_despite_steady_traffic() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let mut options = Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    options.session.subscriptions = Some(SubscriptionOptions::default());
    options.session.resources = Some(Arc::new(
        crate::ResourceCatalog::new(
            vec![crate::Resource::new("acme://report", "report")],
            vec![],
        )
        .unwrap(),
    ));
    options.session.skills = Some(Arc::new(
        crate::SkillCatalog::new(vec![crate::SkillDefinition::new(
            "skill://acme/status",
            vec![crate::SkillFile::text(
                "SKILL.md",
                "---\nname: status\ndescription: Inspect Acme status.\n---\nRead status.\n",
            )],
        )])
        .unwrap(),
    ));
    let handler = Arc::new(Conflict(Mutex::new(None)));
    let (done, finished) = mpsc::channel();
    {
        let handler = Arc::clone(&handler);
        let http = HttpOptions {
            read_timeout: Duration::from_secs(1),
            ..http()
        };
        thread::spawn(move || {
            let result = serve(
                handler,
                catalog(),
                listener,
                &http,
                &options,
                &Shutdown::new(),
            );
            let _ = done.send(result);
        });
    }
    // Idle connections arrive faster than the read timeout closes them, so
    // some connection is always live while the server keeps accepting.
    let (opened, connections) = mpsc::channel();
    let traffic = Arc::new(AtomicBool::new(true));
    let client = {
        let traffic = Arc::clone(&traffic);
        thread::spawn(move || {
            let mut idle = Vec::new();
            while traffic.load(Ordering::SeqCst) {
                if let Ok(stream) = TcpStream::connect(address) {
                    idle.push(stream);
                    let _ = opened.send(());
                }
                thread::sleep(Duration::from_millis(50));
            }
        })
    };
    for _ in 0..3 {
        connections.recv_timeout(WAIT).unwrap();
    }
    let sender = handler.0.lock().unwrap().clone().unwrap();
    let collision = crate::ResourceCatalog::new(
        vec![crate::Resource::new(
            "skill://acme/status/SKILL.md",
            "collision",
        )],
        vec![],
    )
    .unwrap();
    sender.send(Change::Resources(Arc::new(collision))).unwrap();
    let result = finished.recv_timeout(WAIT);
    traffic.store(false, Ordering::SeqCst);
    client.join().unwrap();
    let Ok(Err(ServeError::Configuration(error))) = result else {
        panic!("serve must return the refused change, got {result:?}");
    };
    assert!(error.0.contains("skill://acme/status/SKILL.md"));
}

#[test]
fn serve_requires_modern_options_and_valid_limits() {
    let refused = |http: &HttpOptions, session: SessionOptions| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let handler = Arc::new(Acme {
            changes: Mutex::new(None),
            events: Mutex::new(mpsc::channel().0),
            started: AtomicUsize::new(0),
            keyed: AtomicUsize::new(0),
            keyed_peak: AtomicUsize::new(0),
        });
        let options = Options {
            session,
            ..Options::default()
        };
        serve(
            handler,
            catalog(),
            listener,
            http,
            &options,
            &Shutdown::new(),
        )
        .unwrap_err()
    };
    let modern = || SessionOptions {
        modern: Some(ModernOptions::new(ServerInfo::new("acme", "1.0.0"))),
        ..SessionOptions::default()
    };
    assert!(matches!(
        refused(&http(), SessionOptions::default()),
        ServeError::Options(_)
    ));
    assert!(matches!(
        refused(
            &HttpOptions {
                max_connections: 0,
                ..http()
            },
            modern()
        ),
        ServeError::Options(_)
    ));
    assert!(matches!(
        refused(
            &HttpOptions {
                path: "mcp".into(),
                ..http()
            },
            modern()
        ),
        ServeError::Endpoint(crate::http::EndpointError::Path)
    ));
}
