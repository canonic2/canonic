//! A synchronous MCP server over newline-delimited JSON-RPC.
//!
//! [`serve`] reads frames on one thread, answers protocol requests on the
//! calling thread and runs tool calls and extension methods on worker threads
//! grouped into [`Lane`]s. Every request has a deadline that starts when it is
//! admitted. Cancellation, deadlines and disconnection stop work through its
//! [`Context`]; they never interrupt code that does not check it.

mod changes;
mod observation;
mod protect;
mod reader;
pub(crate) mod scheduler;
#[cfg(feature = "http-server")]
pub(crate) use changes::channel as change_channel;
pub use changes::{CHANGE_BUFFER, ChangeSender, SendChangeError};
pub use observation::{
    FrameKind, MAX_OBSERVATION_BUFFER, Observation, ObservationError, ObservationKind,
    ObservationSender, Settlement, Stage, observation_channel,
};
pub(crate) use observation::{Span, Trace};
pub use protect::{Protocol, protect};

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
    collections::HashMap,
    io::{self, BufRead, Write},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

/// The default request deadline.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// The default time a host's user has to answer a confirmation.
pub const DEFAULT_CONFIRMATION_TIMEOUT: Duration = Duration::from_mins(5);
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
    /// Receives this connection's producer when subscriptions are enabled.
    /// Called once on the dispatching thread; retain the sender and return quickly.
    fn connected(&self, changes: ChangeSender) {
        let _ = changes;
    }
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
    /// A catalog-checked argument completion request.
    Completion(crate::CompleteRequest),
    /// A catalog prompt with validated string arguments.
    Prompt(crate::PromptCall),
    /// A validated management operation; the consumer owns record access and effects.
    Task(crate::task::TaskRequest),
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
    /// Validated ordered suggestions.
    Completion(crate::CompleteResult),
    /// Validated prompt messages.
    Prompt(crate::GetPromptResult),
    /// Ask a rich legacy question. Modern callers use typed input-required rounds instead.
    Elicit(Elicit),
    /// Typed task creation, snapshot, acknowledgement or management failure.
    Task(Result<crate::task::TaskResult, RpcError>),
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

type ElicitResume = Box<dyn FnOnce(crate::elicitation::Answer, &Context) -> Outcome + Send>;

/// A legacy form or URL wait with a typed continuation. No worker is held while waiting.
pub struct Elicit {
    question: crate::elicitation::Request,
    resume: ElicitResume,
}

impl Elicit {
    /// Creates a wait. The driver enforces capacity, round count and total lifetime.
    pub fn new(
        question: crate::elicitation::Request,
        resume: impl FnOnce(crate::elicitation::Answer, &Context) -> Outcome + Send + 'static,
    ) -> Self {
        Self {
            question,
            resume: Box::new(resume),
        }
    }
}

/// Bounds for a rich legacy workflow, including all its waits and execution rounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitationPolicy {
    /// Maximum rich questions issued for one original RPC, within 1..=64.
    pub max_rounds: u8,
    /// Total time from admission, positive and at most 24 hours.
    pub timeout: Duration,
}

impl Default for ElicitationPolicy {
    fn default() -> Self {
        Self {
            max_rounds: 8,
            timeout: Duration::from_mins(5),
        }
    }
}

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
    /// Round and total-time bounds for the separate rich legacy API.
    pub elicitation: ElicitationPolicy,
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
    /// Optional bounded lifecycle records; full/closed receivers never delay serving.
    pub observations: Option<ObservationSender>,
}

impl Default for Options {
    /// One worker with up to eight outstanding requests.
    fn default() -> Self {
        Self {
            elicitation: ElicitationPolicy::default(),
            session: SessionOptions::default(),
            lanes: vec![Lane::new("tools", 1, 8)],
            timeout: DEFAULT_TIMEOUT,
            confirmation_timeout: DEFAULT_CONFIRMATION_TIMEOUT,
            disconnect_grace: DEFAULT_DISCONNECT_GRACE,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            observer: None,
            observations: None,
        }
    }
}

/// Why [`serve`] ended other than at the end of input.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// Invalid protocol options or catalog composition, including a rejected change.
    #[error(transparent)]
    Configuration(#[from] crate::ConfigurationError),
    /// The options cannot run a server.
    #[error("invalid MCP server options: {0}")]
    Options(&'static str),
    /// The HTTP endpoint path, origins or body limit are invalid.
    #[error(transparent)]
    Endpoint(#[from] crate::http::EndpointError),
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
    grant: Option<Arc<crate::http::Grant>>,
    id: Option<Value>,
    control: Arc<Control>,
    progress: Option<Progress>,
    result_schema: Option<crate::ResultSchema>,
}

struct Progress {
    token: Value,
    sequence: u64,
    sender: mpsc::SyncSender<(u64, Value)>,
    dispatcher: thread::Thread,
}

impl Context {
    /// Captured tool output contract; retain it with the producer of durable tasks.
    #[must_use]
    pub fn result_schema(&self) -> Option<&crate::ResultSchema> {
        self.result_schema.as_ref()
    }
    /// This call's immutable modern metadata and retry inputs, or None for legacy work.
    #[must_use]
    pub fn request_context(&self) -> Option<&crate::RequestContext> {
        self.protocol.as_deref()
    }
    /// What the request's verified bearer token grants, on an authorized HTTP
    /// request; `None` on stdio and unprotected HTTP. The server still decides
    /// what the grant permits.
    #[must_use]
    pub fn authorization(&self) -> Option<&crate::http::Grant> {
        self.grant.as_deref()
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
    let trace = Trace::connection(options.observations.as_ref());
    let _connection = trace.span(Stage::Connection);
    let dispatcher = thread::current();
    let scheduler = scheduler::Scheduler::start(&handler, options, dispatcher.clone())?;
    let (frames, incoming) = mpsc::sync_channel(INPUT_BUFFER);
    let limit = options.session.max_message_bytes;
    let reader_trace = trace.clone();
    let reading = thread::Builder::new()
        .name("mcp-input".into())
        .spawn(move || reader::read(input, &frames, &dispatcher, limit, &reader_trace))
        .map_err(ServeError::Spawn)?;
    let mut connection = Connection {
        trace: trace.clone(),
        streams: HashMap::new(),
        changes: None,
        session,
        scheduler,
        handler,
        output: Output {
            trace: Trace::default(),
            kind: FrameKind::Reply,
            writer: output,
            observer: options.observer.clone(),
            limit: options.max_output_bytes,
        },
        eof: false,
    };
    if options.session.subscriptions.is_some() {
        let (sender, receiver) = changes::channel(&options.session);
        connection.changes = Some(receiver);
        let _callback = trace.span(Stage::Callback);
        connection.handler.connected(sender);
    }
    let result = connection.run(&incoming);
    drop(incoming);
    {
        let _drain = trace.span_with_parent(Stage::Drain, Stage::Connection);
        connection.scheduler.shutdown();
        for (_, stream) in connection.streams.drain() {
            stream.event(ObservationKind::StreamClosed);
        }
    }
    if connection.eof || reading.is_finished() {
        let _ = reading.join();
    } else {
        trace.event(ObservationKind::ReaderDetached);
    }
    // A blocking read cannot be interrupted portably; after an early failure
    // the reader ends with its input or the process.
    result
}

/// Inbound work for the scheduler: its ID, request, read-only rule and progress token.
pub(crate) struct Scheduled {
    pub(crate) id: Option<Value>,
    pub(crate) request: Request,
    pub(crate) read_only: bool,
    pub(crate) progress_token: Option<Value>,
}

/// The scheduled work for `inbound`, or `inbound` back when it is not scheduled.
pub(crate) fn scheduled(session: &Session, inbound: Inbound) -> Result<Scheduled, Box<Inbound>> {
    let work = |id, request, read_only, progress_token| Scheduled {
        id,
        request,
        read_only,
        progress_token,
    };
    Ok(match inbound {
        Inbound::Completion { id, request } => {
            let token = request.progress_token.clone();
            work(Some(id), Request::Completion(request), true, token)
        }
        Inbound::Prompt { id, call } => {
            let token = call.progress_token.clone();
            work(Some(id), Request::Prompt(call), true, token)
        }
        Inbound::Task { id, request } => {
            let read_only = matches!(request, crate::task::TaskRequest::Get { .. });
            work(Some(id), Request::Task(request), read_only, None)
        }
        Inbound::BundledResource {
            id,
            read,
            progress_token,
        } => work(Some(id), Request::Resource(read), true, progress_token),
        Inbound::Tool { id, call } => {
            let read_only = session
                .catalog()
                .get(&call.name)
                .is_some_and(crate::tool::Tool::read_only);
            let token = call.progress_token.clone();
            work(Some(id), Request::Tool(call), read_only, token)
        }
        Inbound::Method { id, method, params } => {
            let read_only = method == "resources/read";
            let token = read_only
                .then(|| params["_meta"].get("progressToken").cloned())
                .flatten();
            work(
                Some(id),
                Request::Method { method, params },
                read_only,
                token,
            )
        }
        Inbound::Notification { method, params } => {
            work(None, Request::Notification { method, params }, false, None)
        }
        other => return Err(Box::new(other)),
    })
}

/// Encodes `value` within `limit`. An oversized reply becomes an internal
/// error for its ID; an oversized request or notification is dropped (`None`).
pub(crate) fn encode_frame(value: &Value, limit: usize) -> Result<Vec<u8>, Option<Vec<u8>>> {
    if let Ok(bytes) = json::encode(value, limit) {
        return Ok(bytes);
    }
    // Only replies can be replaced with an error response. A request's
    // ID belongs to the server, and notifications have no reply ID.
    let Some(id) = value.get("id").filter(|_| value.get("method").is_none()) else {
        return Err(None);
    };
    let error = RpcError::new(rpc::INTERNAL_ERROR, "Response exceeds output limit");
    // An ID is at most 128 bytes, so this reply fits any valid limit.
    Err(Some(
        serde_json::to_vec(&rpc::failure(id.clone(), &error)).unwrap_or_default(),
    ))
}

pub(crate) fn validate(options: &Options) -> Result<(), ServeError> {
    if !(1..=64).contains(&options.elicitation.max_rounds)
        || options.elicitation.timeout.is_zero()
        || options.elicitation.timeout > crate::control::MAX_TIMEOUT
    {
        return Err(ServeError::Options(
            "rich elicitation requires finite round and time limits",
        ));
    }
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
    trace: Trace,
    streams: HashMap<String, Trace>,
    changes: Option<mpsc::Receiver<crate::Change>>,
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
            if !self.eof {
                let expired = self.session.expire_subscriptions(std::time::Instant::now());
                self.subscription_frames(expired)?;
                self.publish_changes()?;
            }
            if self.eof && self.scheduler.idle() {
                return input_error.map_or(Ok(()), |error| Err(ServeError::Input(error)));
            }
            match incoming.try_recv() {
                Ok(reader::Input::Frame(frame, trace)) => {
                    trace.event(ObservationKind::End(Stage::InputQueue));
                    self.output.trace = trace.clone();
                    self.output.observe(Direction::Input, &frame);
                    let inbound = {
                        let _decode = trace.span(Stage::Decode);
                        self.session
                            .receive_inspected(&frame, |value| trace.bind(value))
                    };
                    let settlement = match &inbound {
                        Inbound::Ignored
                        | Inbound::Cancel { .. }
                        | Inbound::Confirmation { .. }
                        | Inbound::Elicitation { .. } => Some(Settlement::NoReply),
                        Inbound::Subscription { .. }
                        | Inbound::Completion { .. }
                        | Inbound::Prompt { .. }
                        | Inbound::Task { .. }
                        | Inbound::BundledResource { .. }
                        | Inbound::Tool { .. }
                        | Inbound::Method { .. }
                        | Inbound::Notification { .. }
                        | Inbound::Reply(_)
                        | Inbound::Fatal { .. }
                        | Inbound::Initialize { .. }
                        | Inbound::InvalidArguments { .. } => None,
                    };
                    let result = self.inbound(inbound);
                    if let Some(why) = settlement {
                        trace.settle(why);
                    }
                    self.output.trace = Trace::default();
                    result?;
                }
                Ok(reader::Input::Invalid(trace)) => {
                    self.output.trace = trace.clone();
                    trace.settle(Settlement::Refused);
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
                    match [self.scheduler.next_wake(), self.session.subscription_wait()]
                        .into_iter()
                        .flatten()
                        .min()
                    {
                        Some(wait) => thread::park_timeout(wait),
                        None => thread::park(),
                    }
                }
            }
        }
    }

    fn schedule(&mut self, work: Scheduled) -> Result<(), ServeError> {
        let Scheduled {
            id,
            request,
            read_only,
            progress_token,
        } = work;
        let context = id.as_ref().and_then(|id| self.session.request_context(id));
        let admission = {
            let _callback = self.output.trace.span(Stage::Callback);
            self.handler.admit_context(&request, context.as_deref())
        };
        let outgoing = self.scheduler.admit(
            &mut self.session,
            scheduler::Submission {
                trace: self.output.trace.clone(),
                id,
                grant: None,
                request,
                admission,
                read_only,
                progress_token,
            },
        );
        self.output.send(outgoing)
    }

    fn disconnect(&mut self) -> Result<(), ServeError> {
        self.eof = true;
        self.changes.take();
        let closed = self.session.end_subscriptions();
        self.subscription_frames(closed)?;
        let outgoing = self.scheduler.disconnect(&mut self.session);
        self.output.send(outgoing)
    }

    fn invalid_arguments(
        &mut self,
        id: Value,
        call: &ToolCall,
        error: &ArgumentError,
    ) -> Result<(), ServeError> {
        let result = {
            let _callback = self.output.trace.span(Stage::Callback);
            self.handler.invalid_arguments(call, error)
        };
        let context = self.session.request_context(&id);
        let encoding = self.output.trace.span(Stage::Encode);
        let encoded = context.map_or_else(
            || {
                self.session
                    .catalog()
                    .result_schema(&call.name)
                    .expect("admitted catalog tool")
                    .encode_legacy(&id, &result, self.output.limit, &|| Ok(()))
            },
            |context| context.encode_success(&id, &result, self.output.limit, &|| Ok(())),
        );
        drop(encoding);
        self.session.finish(&id);
        self.output.trace.settle(Settlement::Reply);
        match encoded {
            Ok(bytes) => self.output.bytes(&bytes),
            Err(error) => {
                self.output.trace.event(ObservationKind::EncodeFailed);
                self.output.value(&rpc::failure(
                    id,
                    &RpcError::new(
                        rpc::INTERNAL_ERROR,
                        match error {
                            crate::json::EncodeError::Invalid => {
                                "Tool result does not match its output schema"
                            }
                            crate::json::EncodeError::ValidationLimit => {
                                "Tool result exceeds schema validation limits"
                            }
                            _ => "Response exceeds output limit",
                        },
                    ),
                ))
            }
        }
    }

    fn inbound(&mut self, inbound: Inbound) -> Result<(), ServeError> {
        let inbound = match scheduled(&self.session, inbound) {
            Ok(work) => return self.schedule(work),
            Err(inbound) => *inbound,
        };
        match inbound {
            Inbound::Subscription { id, acknowledged } => {
                self.acknowledge_subscription(id, &acknowledged)
            }
            Inbound::Reply(reply) => {
                self.output.trace.settle(Settlement::Reply);
                self.output.value(&reply)
            }
            Inbound::Fatal { reply, reason } => {
                self.output.trace.settle(Settlement::Reply);
                self.output.value(&reply)?;
                Err(ServeError::Protocol(reason))
            }
            Inbound::Initialize { id, client } => {
                let initialized = {
                    let _callback = self.output.trace.span(Stage::Callback);
                    self.handler.initialize(&client)
                };
                let reply = match initialized {
                    Ok(info) => self.session.accept_initialize(id, &info),
                    Err(error) => self.session.reject_initialize(id, &error),
                };
                self.output.trace.settle(Settlement::Reply);
                self.output.value(&reply)
            }
            Inbound::InvalidArguments { id, call, error } => {
                self.invalid_arguments(id, &call, &error)
            }
            Inbound::Cancel { id } => {
                if self.trace.enabled()
                    && let Some(stream) = self.streams.remove(&id.to_string())
                {
                    self.output.trace.relate(&stream);
                    stream.event(ObservationKind::StreamClosed);
                    stream.settle(Settlement::Cancelled);
                }
                let outgoing = self
                    .scheduler
                    .cancel(&mut self.session, &id, &self.output.trace);
                self.output.send(outgoing)
            }
            Inbound::Confirmation { id, decision } => {
                let outgoing =
                    self.scheduler
                        .confirmed(&mut self.session, &id, decision, &self.output.trace);
                self.output.send(outgoing)
            }
            Inbound::Elicitation { id, answer } => {
                let outgoing =
                    self.scheduler
                        .elicited(&mut self.session, &id, answer, &self.output.trace);
                self.output.send(outgoing)
            }
            Inbound::Ignored
            | Inbound::Completion { .. }
            | Inbound::Prompt { .. }
            | Inbound::Task { .. }
            | Inbound::BundledResource { .. }
            | Inbound::Tool { .. }
            | Inbound::Method { .. }
            | Inbound::Notification { .. } => Ok(()),
        }
    }
}

impl<H: Handler, W: Write> Connection<H, W> {
    fn acknowledge_subscription(
        &mut self,
        id: Value,
        acknowledged: &Value,
    ) -> Result<(), ServeError> {
        let encoded = {
            let _encode = self.output.trace.span(Stage::Encode);
            json::encode(acknowledged, self.output.limit)
        };
        if let Ok(bytes) = encoded {
            if self.output.trace.enabled() {
                self.output.trace.event(ObservationKind::StreamOpened);
                self.streams
                    .insert(id.to_string(), self.output.trace.clone());
            }
            self.output.kind = FrameKind::Subscription;
            let result = self.output.bytes(&bytes);
            self.output.kind = FrameKind::Reply;
            result
        } else {
            self.output.trace.event(ObservationKind::EncodeFailed);
            self.output.trace.settle(Settlement::Refused);
            self.session.finish(&id);
            self.output.value(&rpc::failure(
                id,
                &RpcError::new(
                    rpc::INTERNAL_ERROR,
                    "Subscription acknowledgement exceeds output limit",
                ),
            ))
        }
    }

    fn publish_changes(&mut self) -> Result<(), ServeError> {
        for _ in 0..CHANGE_BUFFER {
            let Some(change) = self
                .changes
                .as_ref()
                .and_then(|changes| changes.try_recv().ok())
            else {
                break;
            };
            let frames = self.session.publish(change)?;
            self.subscription_frames(frames)?;
        }
        Ok(())
    }

    fn subscription_frames(&mut self, frames: Vec<Value>) -> Result<(), ServeError> {
        for frame in frames {
            // Results carry id; notifications carry the originating subscription in params.
            let id = frame
                .get("id")
                .unwrap_or(&frame["params"]["_meta"][crate::subscription::SUBSCRIPTION_KEY]);
            let trace = if self.trace.enabled() {
                self.streams
                    .get(&id.to_string())
                    .cloned()
                    .unwrap_or_default()
            } else {
                Trace::default()
            };
            let terminal = frame.get("id").is_some();
            let encoded = {
                let _encode = trace.span(Stage::Encode);
                json::encode(&frame, self.output.limit)
            };
            if let Ok(bytes) = encoded {
                // A prior oversized frame in this batch may already have ended this stream.
                if frame.get("id").is_some() || self.session.request_context(id).is_some() {
                    self.output
                        .send([Outgoing::Bytes(bytes).observed(&trace, FrameKind::Subscription)])?;
                }
            } else {
                trace.event(ObservationKind::EncodeFailed);
                let _ = self.session.end_subscription(id);
                self.output.send([Outgoing::Value(rpc::failure(
                    id.clone(),
                    &RpcError::new(
                        rpc::INTERNAL_ERROR,
                        if frame.get("id").is_some() {
                            "Subscription result exceeds output limit"
                        } else {
                            "Subscription notification exceeds output limit"
                        },
                    ),
                ))
                .observed(&trace, FrameKind::Subscription)])?;
            }
            if (terminal || self.session.request_context(id).is_none()) && trace.enabled() {
                self.streams.remove(&id.to_string());
                trace.event(ObservationKind::StreamClosed);
                trace.settle(Settlement::Reply);
            }
        }
        Ok(())
    }
}

/// A frame for the output, already encoded or still a value.
pub(crate) enum Outgoing {
    Value(Value),
    Bytes(Vec<u8>),
    Observed(Box<Self>, Span, FrameKind),
    /// A frame without its own ID, such as progress, for this request ID.
    Routed(
        #[cfg_attr(
            not(feature = "http-server"),
            expect(dead_code, reason = "only the HTTP driver routes frames by request ID")
        )]
        Value,
        Box<Self>,
    ),
}

impl Outgoing {
    fn observed(self, trace: &Trace, kind: FrameKind) -> Self {
        if trace.enabled() {
            Self::Observed(Box::new(self), trace.span(Stage::OutputQueue), kind)
        } else {
            self
        }
    }
}

struct Output<W> {
    trace: Trace,
    kind: FrameKind,
    writer: W,
    observer: Option<Observer>,
    limit: usize,
}

impl<W: Write> Output<W> {
    fn send(&mut self, outgoing: impl IntoIterator<Item = Outgoing>) -> Result<(), ServeError> {
        for frame in outgoing {
            match frame {
                Outgoing::Value(value) => self.value(&value)?,
                Outgoing::Bytes(bytes) => self.bytes(&bytes)?,
                Outgoing::Observed(frame, queued, kind) => {
                    let trace = queued.trace.clone();
                    drop(queued);
                    let previous = std::mem::replace(&mut self.trace, trace);
                    let previous_kind = std::mem::replace(&mut self.kind, kind);
                    let result = self.send([*frame]);
                    self.trace = previous;
                    self.kind = previous_kind;
                    result?;
                }
                Outgoing::Routed(_, frame) => self.send([*frame])?,
            }
        }
        Ok(())
    }

    fn value(&mut self, value: &Value) -> Result<(), ServeError> {
        let encode = self.trace.span(Stage::Encode);
        let bytes = match encode_frame(value, self.limit) {
            Ok(bytes) => bytes,
            Err(fallback) => {
                self.trace.event(ObservationKind::EncodeFailed);
                let Some(bytes) = fallback else {
                    return Ok(());
                };
                bytes
            }
        };
        drop(encode);
        self.bytes(&bytes)
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<(), ServeError> {
        self.trace.event(ObservationKind::Frame {
            kind: self.kind,
            bytes: bytes.len(),
        });
        let publication = if self.kind == FrameKind::Interaction {
            self.trace
                .span_with_parent(Stage::Publication, Stage::Interaction)
        } else {
            self.trace.span(Stage::Publication)
        };
        let result = self
            .writer
            .write_all(bytes)
            .and_then(|()| self.writer.write_all(b"\n"))
            .and_then(|()| self.writer.flush())
            .map_err(ServeError::Output);
        if result.is_err() {
            self.trace.event(ObservationKind::PublicationFailed);
        }
        drop(publication);
        result?;
        self.observe(Direction::Output, bytes);
        Ok(())
    }

    fn observe(&self, direction: Direction, frame: &[u8]) {
        if let Some(observer) = &self.observer {
            let _callback = self.trace.span(Stage::Callback);
            observer(direction, frame.strip_suffix(b"\n").unwrap_or(frame));
        }
    }
}
