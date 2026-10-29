//! A synchronous MCP server over newline-delimited JSON-RPC.
//!
//! [`serve`] reads frames on one thread, answers protocol requests on the
//! calling thread and runs tool calls and extension methods on worker threads
//! grouped into [`Lane`]s. Every request has a deadline that starts when it is
//! admitted. Cancellation, deadlines and disconnection stop work through its
//! [`Context`]; they never interrupt code that does not check it.

mod reader;
mod scheduler;

use crate::{
    control::{Control, Interrupted},
    json,
    rpc::{self, RpcError},
    schema::ArgumentError,
    session::{Client, Decision, Inbound, ServerInfo, Session, SessionOptions},
    tool::{Catalog, Content, ToolCall, ToolResult},
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

/// The default request deadline.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// The default time a host's user has to answer a confirmation.
pub const DEFAULT_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(300);
/// The default time running mutations keep after the input closes.
pub const DEFAULT_DISCONNECT_GRACE: Duration = Duration::from_millis(250);
/// The default limit on one encoded output frame, in bytes.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
/// Progress notifications buffered before newer ones are dropped.
pub const PROGRESS_BUFFER: usize = 16;
/// Frames buffered between the reader and the dispatcher.
pub const INPUT_BUFFER: usize = 8;

/// A product's tools and extension methods.
///
/// The driver calls [`Handler::initialize`] and [`Handler::admit`] on the
/// dispatching thread, so they must return quickly. [`Handler::call`] runs
/// on a worker thread.
pub trait Handler: Send + Sync + 'static {
    /// Accepts or refuses the client and returns the server identity.
    ///
    /// # Errors
    ///
    /// The returned error is sent as the initialization reply; the client may
    /// initialize again.
    fn initialize(&self, client: &Client) -> Result<ServerInfo, RpcError>;

    /// Chooses the lane, serialization key and deadline for a request.
    fn admit(&self, request: &Request) -> Admission {
        let _ = request;
        Admission::default()
    }

    /// Admission with the immutable modern request context, absent for legacy work.
    fn admit_context(
        &self,
        request: &Request,
        context: Option<&crate::RequestContext>,
    ) -> Admission {
        let _ = context;
        self.admit(request)
    }

    /// Runs one request.
    fn call(&self, request: Request, context: &Context) -> Outcome;

    /// The tool result for arguments that do not match the tool's input schema.
    fn invalid_arguments(&self, call: &ToolCall, error: &ArgumentError) -> ToolResult {
        let _ = call;
        ToolResult::error(vec![Content::text(error.message.clone())])
    }
}

/// Work a [`Handler`] runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Immutable library-owned resource bytes, exposed to admission only.
    Resource(crate::BundledRead),
    /// A tool call whose name and arguments the session has checked.
    Tool(ToolCall),
    /// A validated resource read or product request from [`SessionOptions::extension_methods`].
    Method {
        /// The method.
        method: String,
        /// Its parameters.
        params: Value,
    },
    /// A product extension notification from
    /// [`SessionOptions::extension_notifications`]. Its outcome is discarded.
    Notification {
        /// The method.
        method: String,
        /// Its parameters.
        params: Value,
    },
}

/// What a request produced.
pub enum Outcome {
    /// A prepared bundled read, normally produced internally by the driver.
    Bundled(crate::BundledRead),
    /// Unknown/inaccessible resource, encoded for the admitted protocol revision.
    ResourceNotFound(String),
    /// A resource read with explicitly selected freshness and authorization scope.
    ResourceWithCache(crate::ReadResourceResult, crate::CachePolicy),
    /// Modern consumer-owned confirmation; no in-memory closure is serialized.
    InputRequired(crate::InputRequiredResult),
    /// Validated resource read contents.
    Resource(crate::resource::ReadResourceResult),
    /// A tool result.
    Tool(ToolResult),
    /// An extension request's result.
    Method(Result<Value, RpcError>),
    /// Nothing to reply, for notifications.
    Done,
    /// Ask the host's user, then continue.
    Confirm(Confirm),
}

/// A confirmation step: a message for the host's user and the work to run
/// with their answer.
///
/// The worker is released while the user decides. The continuation runs on
/// the same lane and key with a fresh deadline. It runs with
/// [`Decision::Unavailable`] when the client cannot show a form or another
/// confirmation is pending, and does not run when the request is cancelled,
/// times out or the connection closes. A continuation that asks again is
/// answered with an internal error.
pub struct Confirm {
    message: String,
    resume: Resume,
}

type Resume = Box<dyn FnOnce(Decision, &Context) -> Outcome + Send>;

impl Confirm {
    /// A confirmation showing `message`, continued by `resume`.
    pub fn new(
        message: impl Into<String>,
        resume: impl FnOnce(Decision, &Context) -> Outcome + Send + 'static,
    ) -> Self {
        Self {
            message: message.into(),
            resume: Box::new(resume),
        }
    }
}

/// Where and how long a request runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Admission {
    /// An index into [`Options::lanes`].
    pub lane: usize,
    /// Requests with the same key in one lane run one at a time, in admission order.
    pub key: Option<String>,
    /// The deadline from admission; [`Options::timeout`] when `None`.
    pub timeout: Option<Duration>,
}

/// One group of worker threads with its own queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lane {
    /// A name for diagnostics.
    pub name: String,
    /// Requests that run at the same time.
    pub workers: usize,
    /// Requests queued or running before new ones are refused.
    pub capacity: usize,
}

impl Lane {
    /// A lane with `workers` threads accepting up to `capacity` requests.
    #[must_use]
    pub fn new(name: impl Into<String>, workers: usize, capacity: usize) -> Self {
        Self {
            name: name.into(),
            workers,
            capacity,
        }
    }
}

/// Which way a frame travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Received from the client.
    Input,
    /// Sent to the client.
    Output,
}

/// Receives every frame, without its line break, for diagnostics.
pub type Observer = Arc<dyn Fn(Direction, &[u8]) + Send + Sync>;

/// Server limits and scheduling.
#[derive(Clone)]
pub struct Options {
    /// Protocol limits and extension methods.
    pub session: SessionOptions,
    /// Worker lanes; [`Admission::lane`] indexes this list.
    pub lanes: Vec<Lane>,
    /// The default request deadline.
    pub timeout: Duration,
    /// How long a confirmation may wait for the user.
    pub confirmation_timeout: Duration,
    /// How long running mutations continue after the input closes before
    /// they are cancelled.
    pub disconnect_grace: Duration,
    /// The longest encoded output frame, in bytes. Oversized replies become
    /// errors, progress is dropped, and confirmations resume as unavailable.
    pub max_output_bytes: usize,
    /// An optional frame observer.
    pub observer: Option<Observer>,
}

impl Default for Options {
    /// One worker with up to eight outstanding requests.
    fn default() -> Self {
        Self {
            session: SessionOptions::default(),
            lanes: vec![Lane::new("tools", 1, 8)],
            timeout: DEFAULT_TIMEOUT,
            confirmation_timeout: DEFAULT_CONFIRMATION_TIMEOUT,
            disconnect_grace: DEFAULT_DISCONNECT_GRACE,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            observer: None,
        }
    }
}

/// Why [`serve`] ended other than at the end of input.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// Invalid modern identity or conflicting catalog ownership, before threads start.
    #[error(transparent)]
    Configuration(#[from] crate::ConfigurationError),
    /// The options cannot run a server.
    #[error("invalid MCP server options: {0}")]
    Options(&'static str),
    /// A reader or worker thread could not start.
    #[error("cannot start an MCP server thread")]
    Spawn(#[source] io::Error),
    /// Reading the input failed.
    #[error("cannot read MCP input")]
    Input(#[source] io::Error),
    /// Writing the output failed.
    #[error("cannot write MCP output")]
    Output(#[source] io::Error),
    /// The client broke the protocol in a way that ends the connection.
    #[error("MCP connection ended: {0}")]
    Protocol(&'static str),
}

/// The context of one running request.
pub struct Context {
    protocol: Option<Arc<crate::RequestContext>>,
    id: Option<Value>,
    control: Arc<Control>,
    progress: Option<Progress>,
}

struct Progress {
    token: Value,
    sequence: u64,
    sender: mpsc::SyncSender<(u64, Value)>,
    dispatcher: thread::Thread,
}

impl Context {
    /// This call's immutable modern metadata and retry inputs, or None for legacy work.
    #[must_use]
    pub fn request_context(&self) -> Option<&crate::RequestContext> {
        self.protocol.as_deref()
    }
    /// The request ID, or `None` for a notification.
    #[must_use]
    pub fn request_id(&self) -> Option<&Value> {
        self.id.as_ref()
    }

    /// The request's cancellation and deadline.
    #[must_use]
    pub fn control(&self) -> &Control {
        &self.control
    }

    /// Succeeds while the request may continue.
    ///
    /// # Errors
    ///
    /// Returns [`Interrupted`] after cancellation, the deadline or the
    /// disconnection grace period.
    pub fn check(&self) -> Result<(), Interrupted> {
        self.control.check()
    }

    /// Time left before the deadline.
    #[must_use]
    pub fn remaining(&self) -> Duration {
        self.control.remaining()
    }

    /// Sends a progress notification when the caller supplied a progress token.
    ///
    /// `progress` must increase with each call. Returns `false` when there is
    /// no token, the buffer is full or the request has ended; progress is never
    /// sent after the request's reply.
    #[expect(
        clippy::must_use_candidate,
        reason = "progress is best effort; most callers ignore whether it was buffered"
    )]
    pub fn progress(&self, progress: f64, total: Option<f64>, message: Option<&str>) -> bool {
        if self.control.check().is_err() {
            return false;
        }
        let Some(sink) = &self.progress else {
            return false;
        };
        let mut params = json!({"progressToken": sink.token, "progress": progress});
        if let Some(total) = total {
            params["total"] = json!(total);
        }
        if let Some(message) = message {
            params["message"] = json!(message);
        }
        let notification = rpc::notification("notifications/progress", params);
        let sent = sink.sender.try_send((sink.sequence, notification)).is_ok();
        sink.dispatcher.unpark();
        sent
    }
}

/// Serves one connection until its input ends and admitted work settles.
///
/// After the input ends, queued requests for tools not annotated read-only are
/// refused with [`rpc::REQUEST_CANCELLED`], a pending confirmation is withdrawn,
/// running mutations are cancelled after [`Options::disconnect_grace`], and
/// read-only work continues to its deadline. Replies are still written while
/// the output accepts them.
/// Resource reads follow the same lifetime as read-only tools.
///
/// # Errors
///
/// Returns [`ServeError`] for invalid options, input or output failure, or a
/// protocol violation that ends the connection: an oversized or unterminated
/// frame, a duplicate outstanding request ID or the session message limit.
/// Worker threads are joined before returning; work that never checks its
/// [`Context`] delays the return until it finishes.
pub fn serve<H: Handler>(
    handler: Arc<H>,
    catalog: Arc<Catalog>,
    input: impl BufRead + Send + 'static,
    output: impl Write,
    options: &Options,
) -> Result<(), ServeError> {
    validate(options)?;
    let session = Session::try_new(catalog, options.session.clone())?;
    let dispatcher = thread::current();
    let scheduler = scheduler::Scheduler::start(&handler, options, dispatcher.clone())?;
    let (frames, incoming) = mpsc::sync_channel(INPUT_BUFFER);
    let limit = options.session.max_message_bytes;
    let reading = thread::Builder::new()
        .name("mcp-input".into())
        .spawn(move || reader::read(input, &frames, &dispatcher, limit))
        .map_err(ServeError::Spawn)?;
    let mut connection = Connection {
        session,
        scheduler,
        handler,
        output: Output {
            writer: output,
            observer: options.observer.clone(),
            limit: options.max_output_bytes,
        },
        eof: false,
    };
    let result = connection.run(&incoming);
    drop(incoming);
    connection.scheduler.shutdown();
    if connection.eof || reading.is_finished() {
        let _ = reading.join();
    }
    // A blocking read cannot be interrupted portably; after an early failure
    // the reader ends with its input or the process.
    result
}

fn validate(options: &Options) -> Result<(), ServeError> {
    if options.lanes.is_empty() {
        return Err(ServeError::Options("at least one lane is required"));
    }
    if options
        .lanes
        .iter()
        .any(|lane| lane.workers == 0 || lane.capacity < lane.workers)
    {
        return Err(ServeError::Options(
            "each lane needs a worker and a capacity of at least its workers",
        ));
    }
    if options.session.max_message_bytes == 0 || options.max_output_bytes < 1024 {
        return Err(ServeError::Options(
            "message limits must allow protocol replies",
        ));
    }
    Ok(())
}

struct Connection<H, W> {
    session: Session,
    scheduler: scheduler::Scheduler,
    handler: Arc<H>,
    output: Output<W>,
    eof: bool,
}

impl<H: Handler, W: Write> Connection<H, W> {
    fn run(&mut self, incoming: &mpsc::Receiver<reader::Input>) -> Result<(), ServeError> {
        let mut input_error = None;
        loop {
            let outgoing = self.scheduler.settle(&mut self.session);
            self.output.send(outgoing)?;
            if self.eof && self.scheduler.idle() {
                return input_error.map_or(Ok(()), |error| Err(ServeError::Input(error)));
            }
            match incoming.try_recv() {
                Ok(reader::Input::Frame(frame)) => {
                    self.output.observe(Direction::Input, &frame);
                    let inbound = self.session.receive(&frame);
                    self.inbound(inbound)?;
                }
                Ok(reader::Input::Invalid) => {
                    self.output.value(&rpc::failure(
                        Value::Null,
                        &RpcError::new(rpc::PARSE_ERROR, "Invalid or oversized message frame"),
                    ))?;
                    return Err(ServeError::Protocol("invalid or oversized message frame"));
                }
                Ok(reader::Input::End(result)) => {
                    input_error = result.err();
                    self.disconnect()?;
                }
                Err(mpsc::TryRecvError::Disconnected) if !self.eof => self.disconnect()?,
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => {
                    match self.scheduler.next_wake() {
                        Some(wait) => thread::park_timeout(wait),
                        None => thread::park(),
                    }
                }
            }
        }
    }

    fn schedule(
        &mut self,
        id: Option<Value>,
        request: Request,
        read_only: bool,
        progress_token: Option<Value>,
    ) -> Result<(), ServeError> {
        let context = id.as_ref().and_then(|id| self.session.request_context(id));
        let admission = self.handler.admit_context(&request, context.as_deref());
        let outgoing = self.scheduler.admit(
            &mut self.session,
            id,
            request,
            admission,
            read_only,
            progress_token,
        );
        self.output.send(outgoing)
    }

    fn disconnect(&mut self) -> Result<(), ServeError> {
        self.eof = true;
        let outgoing = self.scheduler.disconnect(&mut self.session);
        self.output.send(outgoing)
    }

    fn inbound(&mut self, inbound: Inbound) -> Result<(), ServeError> {
        match inbound {
            Inbound::BundledResource {
                id,
                read,
                progress_token,
            } => self.schedule(Some(id), Request::Resource(read), true, progress_token),
            Inbound::Reply(reply) => self.output.value(&reply),
            Inbound::Fatal { reply, reason } => {
                self.output.value(&reply)?;
                Err(ServeError::Protocol(reason))
            }
            Inbound::Initialize { id, client } => {
                let reply = match self.handler.initialize(&client) {
                    Ok(info) => self.session.accept_initialize(id, &info),
                    Err(error) => self.session.reject_initialize(id, &error),
                };
                self.output.value(&reply)
            }
            Inbound::InvalidArguments { id, call, error } => {
                let result = self.handler.invalid_arguments(&call, &error);
                let context = self.session.request_context(&id);
                self.session.finish(&id);
                if let Some(context) = context {
                    match context.encode_success(&id, &result, self.output.limit, &|| Ok(())) {
                        Ok(bytes) => self.output.bytes(&bytes),
                        Err(_) => self.output.value(&rpc::failure(
                            id,
                            &RpcError::new(rpc::INTERNAL_ERROR, "Response exceeds output limit"),
                        )),
                    }
                } else {
                    self.output.tool_result(&id, &result)
                }
            }
            Inbound::Tool { id, call } => {
                let read_only = self
                    .session
                    .catalog()
                    .get(&call.name)
                    .is_some_and(crate::tool::Tool::read_only);
                let token = call.progress_token.clone();
                self.schedule(Some(id), Request::Tool(call), read_only, token)
            }
            Inbound::Method { id, method, params } => {
                let read_only = method == "resources/read";
                let token = read_only
                    .then(|| params["_meta"].get("progressToken").cloned())
                    .flatten();
                self.schedule(
                    Some(id),
                    Request::Method { method, params },
                    read_only,
                    token,
                )
            }
            Inbound::Notification { method, params } => {
                self.schedule(None, Request::Notification { method, params }, false, None)
            }
            Inbound::Cancel { id } => {
                let outgoing = self.scheduler.cancel(&mut self.session, &id);
                self.output.send(outgoing)
            }
            Inbound::Confirmation { id, decision } => {
                let outgoing = self.scheduler.confirmed(&mut self.session, &id, decision);
                self.output.send(outgoing)
            }
            Inbound::Ignored => Ok(()),
        }
    }
}

/// A frame for the output, already encoded or still a value.
enum Outgoing {
    Value(Value),
    Bytes(Vec<u8>),
}

struct Output<W> {
    writer: W,
    observer: Option<Observer>,
    limit: usize,
}

impl<W: Write> Output<W> {
    fn send(&mut self, outgoing: Vec<Outgoing>) -> Result<(), ServeError> {
        for frame in outgoing {
            match frame {
                Outgoing::Value(value) => self.value(&value)?,
                Outgoing::Bytes(bytes) => self.bytes(&bytes)?,
            }
        }
        Ok(())
    }

    fn value(&mut self, value: &Value) -> Result<(), ServeError> {
        let bytes = if let Ok(bytes) = json::encode(value, self.limit) {
            bytes
        } else {
            // Only replies can be replaced with an error response. A request's
            // ID belongs to the server, and notifications have no reply ID.
            let Some(id) = value.get("id").filter(|_| value.get("method").is_none()) else {
                return Ok(());
            };
            let error = RpcError::new(rpc::INTERNAL_ERROR, "Response exceeds output limit");
            let fallback = rpc::failure(id.clone(), &error);
            // An ID is at most 128 bytes, so this reply fits any valid limit.
            serde_json::to_vec(&fallback).unwrap_or_default()
        };
        self.bytes(&bytes)
    }

    fn tool_result(&mut self, id: &Value, result: &ToolResult) -> Result<(), ServeError> {
        match rpc::encode_success(id, result, self.limit, &|| Ok(())) {
            Ok(bytes) => self.bytes(&bytes),
            Err(_) => self.value(&rpc::failure(
                id.clone(),
                &RpcError::new(rpc::INTERNAL_ERROR, "Response exceeds output limit"),
            )),
        }
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<(), ServeError> {
        self.writer
            .write_all(bytes)
            .and_then(|()| self.writer.write_all(b"\n"))
            .and_then(|()| self.writer.flush())
            .map_err(ServeError::Output)?;
        self.observe(Direction::Output, bytes);
        Ok(())
    }

    fn observe(&self, direction: Direction, frame: &[u8]) {
        if let Some(observer) = &self.observer {
            observer(direction, frame.strip_suffix(b"\n").unwrap_or(frame));
        }
    }
}
