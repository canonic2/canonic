//! An in-memory host connection and a scriptable handler.

use canonic_mcp::{
    Catalog, Client, Content, RpcError, ServerInfo, Tool, ToolResult,
    stdio::{self, Admission, Context, Handler, Options, Outcome, Request, ServeError},
    tool::Annotations,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufReader, Read, Write},
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// Bounds every wait so a broken server fails the test instead of hanging it.
pub(crate) const WAIT: Duration = Duration::from_secs(10);

type Call = dyn Fn(Request, &Context) -> Outcome + Send + Sync;

/// A handler whose behavior each test supplies. Arguments may name a
/// serialization `key` and a `timeoutMs` deadline.
pub(crate) struct Script {
    call: Box<Call>,
}

impl Script {
    pub(crate) fn new(call: impl Fn(Request, &Context) -> Outcome + Send + Sync + 'static) -> Self {
        Self {
            call: Box::new(call),
        }
    }
}

impl Handler for Script {
    fn initialize(&self, client: &Client) -> Result<ServerInfo, RpcError> {
        if client.name == "refused-host" {
            return Err(RpcError::new(-32000, "Acme refuses this host"));
        }
        Ok(ServerInfo::new("acme", "1.0.0").with_instructions("Use acme_echo."))
    }

    fn admit(&self, request: &Request) -> Admission {
        Admission {
            key: arguments(request)["key"].as_str().map(str::to_owned),
            timeout: arguments(request)["timeoutMs"]
                .as_u64()
                .map(Duration::from_millis),
            ..Admission::default()
        }
    }

    fn call(&self, request: Request, context: &Context) -> Outcome {
        (self.call)(request, context)
    }
}

pub(crate) fn arguments(request: &Request) -> &Value {
    match request {
        Request::Resource(_) => &Value::Null,
        Request::Tool(call) => &call.arguments,
        Request::Method { params, .. } | Request::Notification { params, .. } => params,
    }
}

pub(crate) fn tool_name(request: &Request) -> &str {
    match request {
        Request::Resource(_) => "resources/read",
        Request::Tool(call) => &call.name,
        Request::Method { method, .. } | Request::Notification { method, .. } => method,
    }
}

/// Echo, a read-only probe and a mutation, each accepting `key`, `timeoutMs` and `message`.
pub(crate) fn catalog() -> Arc<Catalog> {
    let schema = json!({"type":"object","properties":{
        "message":{"type":"string"},"key":{"type":"string"},"timeoutMs":{"type":"integer","minimum":1}
    },"additionalProperties":false});
    let read_only = Annotations {
        read_only_hint: Some(true),
        ..Annotations::default()
    };
    Arc::new(
        Catalog::new(vec![
            Tool::new("acme_echo", "Return the Acme message.", schema.clone())
                .with_annotations(read_only.clone()),
            Tool::new("acme_probe", "Wait for an Acme release.", schema.clone())
                .with_annotations(read_only),
            Tool::new("acme_change", "Change Acme state.", schema),
        ])
        .expect("valid catalog"),
    )
}

pub(crate) fn text(message: &str) -> Outcome {
    Outcome::Tool(ToolResult::success(vec![Content::text(message)]))
}

/// Records events from worker threads in order.
#[derive(Clone)]
pub(crate) struct Events {
    sender: Sender<String>,
    receiver: Arc<Mutex<Receiver<String>>>,
}

impl Events {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    pub(crate) fn record(&self, event: impl Into<String>) {
        let _ = self.sender.send(event.into());
    }

    pub(crate) fn next(&self) -> String {
        self.receiver
            .lock()
            .expect("event receiver")
            .recv_timeout(WAIT)
            .expect("an event before the wait bound")
    }

    /// The next event if one arrives within `window`; used to show that work did not run.
    pub(crate) fn none_within(&self, window: Duration) -> Option<String> {
        self.receiver
            .lock()
            .expect("event receiver")
            .recv_timeout(window)
            .ok()
    }
}

/// A one-way release that blocked work waits on.
#[derive(Clone, Default)]
pub(crate) struct Latch(Arc<(Mutex<bool>, Condvar)>);

impl Latch {
    pub(crate) fn release(&self) {
        *self.0.0.lock().expect("latch state") = true;
        self.0.1.notify_all();
    }

    /// Waits for release; returns `false` once the request's context stops it.
    pub(crate) fn wait(&self, context: &Context) -> bool {
        let (released, signal) = &*self.0;
        let mut released = released.lock().expect("latch state");
        loop {
            if *released {
                return true;
            }
            if context.check().is_err() {
                return false;
            }
            // Cancellation has no signal of its own; wake briefly to observe it.
            released = signal
                .wait_timeout(released, Duration::from_millis(5))
                .expect("latch state")
                .0;
        }
    }
}

/// The host side of one connection.
pub(crate) struct Host {
    input: Option<Sender<Vec<u8>>>,
    output: Receiver<Value>,
    done: Receiver<Result<(), ServeError>>,
    server: Option<JoinHandle<()>>,
}

impl Host {
    pub(crate) fn start(handler: Script, options: Options) -> Self {
        Self::start_with(Arc::new(handler), catalog(), options)
    }

    pub(crate) fn start_with(
        handler: Arc<Script>,
        catalog: Arc<Catalog>,
        options: Options,
    ) -> Self {
        let (input, frames) = mpsc::channel();
        let (lines, output) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let server = thread::spawn(move || {
            let result = stdio::serve(
                handler,
                catalog,
                BufReader::new(Pipe {
                    frames,
                    buffer: Vec::new(),
                    offset: 0,
                }),
                Lines {
                    lines,
                    buffer: Vec::new(),
                },
                &options,
            );
            let _ = finished.send(result);
        });
        Self {
            input: Some(input),
            output,
            done,
            server: Some(server),
        }
    }

    /// Starts a server and completes initialization with `capabilities`.
    pub(crate) fn ready(handler: Script, options: Options, capabilities: Value) -> Self {
        let mut host = Self::start(handler, options);
        host.initialize(capabilities);
        host
    }

    pub(crate) fn initialize(&mut self, capabilities: Value) -> Value {
        let mut request = json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{
            "protocolVersion":"2025-11-25","clientInfo":{"name":"acme-host","version":"2.1.0"}}});
        request["params"]["capabilities"] = capabilities;
        self.send(&request);
        let reply = self.recv();
        self.send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        reply
    }

    pub(crate) fn send(&self, message: &Value) {
        let mut bytes = serde_json::to_vec(message).expect("encodable message");
        bytes.push(b'\n');
        self.send_bytes(bytes);
    }

    pub(crate) fn send_bytes(&self, bytes: Vec<u8>) {
        self.input
            .as_ref()
            .expect("input is open")
            .send(bytes)
            .expect("server is reading");
    }

    pub(crate) fn call(&self, id: u64, name: &str, arguments: Value) {
        let mut request =
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name}});
        request["params"]["arguments"] = arguments;
        self.send(&request);
    }

    pub(crate) fn cancel(&self, id: u64) {
        self.send(
            &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}),
        );
    }

    pub(crate) fn recv(&self) -> Value {
        self.output
            .recv_timeout(WAIT)
            .expect("a server message before the wait bound")
    }

    /// Asserts that the next reply answers a ping, so nothing else was sent first.
    pub(crate) fn assert_quiet(&self) {
        self.send(&json!({"jsonrpc":"2.0","id":"quiet","method":"ping"}));
        assert_eq!(
            self.recv(),
            json!({"jsonrpc":"2.0","id":"quiet","result":{}})
        );
    }

    /// Ends the input.
    pub(crate) fn close(&mut self) {
        self.input = None;
    }

    /// Ends the input, waits for the server to return and collects the rest of its output.
    pub(crate) fn finish(mut self) -> (Result<(), ServeError>, Vec<Value>) {
        self.close();
        let result = self
            .done
            .recv_timeout(WAIT)
            .expect("server returns before the wait bound");
        self.server
            .take()
            .expect("server thread")
            .join()
            .expect("server thread does not panic");
        (result, self.output.try_iter().collect())
    }
}

pub(crate) fn result_text(reply: &Value) -> &str {
    reply["result"]["content"][0]["text"]
        .as_str()
        .expect("text result")
}

/// Blocking input fed by the host; ends when the host drops its sender.
struct Pipe {
    frames: Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    offset: usize,
}

impl Read for Pipe {
    fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
        if self.offset == self.buffer.len() {
            match self.frames.recv() {
                Ok(frame) => {
                    self.buffer = frame;
                    self.offset = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let count = target.len().min(self.buffer.len() - self.offset);
        target[..count].copy_from_slice(&self.buffer[self.offset..self.offset + count]);
        self.offset += count;
        Ok(count)
    }
}

/// Output that hands each complete line to the host as JSON.
struct Lines {
    lines: Sender<Value>,
    buffer: Vec<u8>,
}

impl Write for Lines {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        while let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let value = serde_json::from_slice(&line[..line.len() - 1])
                .map_err(|_| io::Error::other("server wrote invalid JSON"))?;
            let _ = self.lines.send(value);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
