//! Independent CDP reader/writer lanes with retained dispatch and late outcomes.

use super::{
    CONTROL_RESERVE, MAX_MESSAGE_BYTES, MAX_PENDING, MAX_RETAINED_BYTES, RemoteError,
    events::{Buffer, Events},
    wire::{self, Incoming},
};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    net::IpAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{Notify, OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch},
    time::{Instant, timeout_at},
};
use tokio_tungstenite::{
    WebSocketStream, client_async_with_config,
    tungstenite::{self, Message, client::IntoClientRequest, http::Uri, protocol::WebSocketConfig},
};

const QUEUED: u8 = 0;
const DISPATCHED: u8 = 1;
const CANCELLED: u8 = 2;
const WRITE_BUDGET: Duration = Duration::from_secs(5);
pub(super) mod pipe;
mod retirement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dispatch {
    NotDispatched,
    PossiblyDispatched,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loss {
    Closed,
    Eof,
    Read,
    Write,
    Protocol,
    EventCapacity,
    ReplyCapacity,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("CDP endpoint must name an explicitly selected loopback browser WebSocket")]
    Endpoint,
    #[error("CDP WebSocket handshake failed; endpoint details are private")]
    Handshake,
    #[error("CDP command requires an object, method name and valid optional session")]
    InvalidCommand,
    #[error("CDP command exceeds its byte budget")]
    CommandBounds,
    #[error("CDP control lane does not admit this method")]
    ControlMethod,
    #[error("CDP call capacity is exhausted; drain retained replies before retrying")]
    Busy,
    #[error("CDP scoped owner is closing; this command was not dispatched")]
    ScopeClosing,
    #[error("CDP scoped owner closed ({dispatch:?}); caller effects are not confirmed")]
    ScopeClosed { dispatch: Dispatch },
    #[error(
        "CDP retired-call ownership is full; close the connection before admitting more closed calls"
    )]
    RetirementCapacity,
    #[error("CDP request ID space is exhausted")]
    Exhausted,
    #[error("CDP browser identity differs from the selected build or protocol")]
    Identity,
    #[error(transparent)]
    Schema(#[from] super::schema::Error),
    #[error("CDP deadline expired ({dispatch:?})")]
    Timeout { dispatch: Dispatch },
    #[error("CDP connection was lost ({reason:?}, {dispatch:?})")]
    Lost { reason: Loss, dispatch: Dispatch },
    #[error(transparent)]
    Remote(#[from] RemoteError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Ordinary,
    Control,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub open: bool,
    pub pending: usize,
    pub retained_replies: usize,
    pub retired_calls: usize,
    pub loss: Option<Loss>,
}

/// A late response remains private, including any allocated browser resources.
/// The workflow owner must reconcile it; it is never a request to repeat input.
#[derive(Debug)]
pub struct SettledReply {
    pub id: u64,
    pub method: String,
    pub session_id: Option<String>,
    pub result: Result<Value, RemoteError>,
}

type Reply = Result<Value, Error>;
struct Pending {
    session: Option<String>,
    method: String,
    state: Arc<AtomicU8>,
    reply: oneshot::Sender<Result<(), Error>>,
    waiting: Arc<AtomicBool>,
    permit: OwnedSemaphorePermit,
}
struct Retained {
    reply: SettledReply,
    bytes: usize,
    waiting: Arc<AtomicBool>,
    _permit: OwnedSemaphorePermit,
}
struct State {
    next_id: u64,
    pending: HashMap<u64, Pending>,
    retained: VecDeque<Retained>,
    retained_bytes: usize,
    retired: HashMap<u64, retirement::Retired>,
    retiring: BTreeSet<String>,
    retirement_reserved: usize,
    loss: Option<Loss>,
}
struct Shared {
    react_native: bool,
    edge_target_metadata: AtomicBool,
    mac_keyboard: AtomicBool,
    external_browser: AtomicBool,
    state: Mutex<State>,
    closed: Notify,
    events: Buffer,
    frames: crate::browser::frames::Lane<String>,
    ordinal: AtomicU64,
    frame_acks: mpsc::Sender<(String, Value)>,
}
struct Client {
    ordinary: mpsc::Sender<Outbound>,
    control: mpsc::Sender<Outbound>,
    ordinary_slots: Arc<Semaphore>,
    control_slots: Arc<Semaphore>,
    shutdown: watch::Sender<bool>,
    shared: Arc<Shared>,
}

#[derive(Clone)]
pub struct Connection(Arc<Client>);

struct Outbound {
    id: u64,
    text: String,
    deadline: Instant,
    state: Arc<AtomicU8>,
}

struct CancelQueued(Arc<AtomicU8>, Arc<AtomicBool>);
impl Drop for CancelQueued {
    fn drop(&mut self) {
        let _ = self
            .0
            .compare_exchange(QUEUED, CANCELLED, Ordering::SeqCst, Ordering::SeqCst);
        self.1.store(false, Ordering::SeqCst);
    }
}

fn dispatch(state: &AtomicU8) -> Dispatch {
    if state.load(Ordering::SeqCst) == DISPATCHED {
        Dispatch::PossiblyDispatched
    } else {
        Dispatch::NotDispatched
    }
}

fn control_method(method: &str, params: &Value) -> bool {
    if method == "Network.setCacheDisabled" && params == &serde_json::json!({"cacheDisabled":false})
    {
        return true;
    }
    if method == "Emulation.setDefaultBackgroundColorOverride"
        && params.as_object().is_some_and(serde_json::Map::is_empty)
    {
        return true;
    }
    if method == "Input.setInterceptDrags" && params["enabled"] == false {
        return true;
    }
    if matches!(
        (method, params["type"].as_str()),
        ("Input.dispatchKeyEvent", Some("keyUp"))
            | ("Input.dispatchMouseEvent", Some("mouseReleased"))
            | ("Input.dispatchTouchEvent", Some("touchEnd" | "touchCancel"))
    ) {
        return true;
    }
    matches!(
        method,
        "Browser.close"
            | "IO.close"
            | "IO.read"
            | "Tracing.end"
            | "WebMCP.cancelInvocation"
            | "WebMCP.disable"
            | "Browser.cancelDownload"
            | "Target.closeTarget"
            | "Target.detachFromTarget"
            | "Target.disposeBrowserContext"
            | "Runtime.runIfWaitingForDebugger"
            | "Runtime.releaseObject"
            | "Runtime.releaseObjectGroup"
            | "Page.screencastFrameAck"
            | "Page.stopScreencast"
            | "Page.handleJavaScriptDialog"
            | "Fetch.continueRequest"
            | "Fetch.continueResponse"
            | "Fetch.continueWithAuth"
            | "Fetch.fulfillRequest"
            | "Fetch.failRequest"
            | "Debugger.resume"
            | "Debugger.pause"
            | "Debugger.disable"
            | "Debugger.stepInto"
            | "Debugger.stepOver"
            | "Debugger.stepOut"
            | "Debugger.evaluateOnCallFrame"
            | "Runtime.getProperties"
            | "Debugger.setBreakpointByUrl"
            | "Debugger.removeBreakpoint"
            | "Profiler.stop"
            | "Profiler.takePreciseCoverage"
            | "Profiler.stopPreciseCoverage"
            | "Profiler.disable"
            | "HeapProfiler.disable"
            | "CSS.stopRuleUsageTracking"
            | "CSS.disable"
            | "DOM.disable"
            | "Emulation.setDeviceMetricsOverride"
    )
}

pub(super) fn endpoint_valid(endpoint: &str) -> bool {
    if endpoint.len() > 2048 {
        return false;
    }
    let Ok(uri) = endpoint.parse::<Uri>() else {
        return false;
    };
    let Some(host) = uri.host() else { return false };
    let Ok(address) = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    else {
        return false;
    };
    let Some(identity) = uri.path().strip_prefix("/devtools/browser/") else {
        return false;
    };
    uri.scheme_str() == Some("ws")
        && address.is_loopback()
        && !endpoint.contains('#')
        && uri
            .authority()
            .is_some_and(|authority| !authority.as_str().contains('@'))
        && uri.port_u16().is_some_and(|port| port != 0)
        && uri.query().is_none()
        && !identity.is_empty()
        && identity.len() <= 1024
        && identity
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(16 * 1024)
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_MESSAGE_BYTES + 4096)
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES))
}

impl Connection {
    pub(super) fn attached(socket: crate::browser::attachment::Socket) -> (Self, Events) {
        Self::start_profile(socket, false)
    }
    /// Explicit borrowed embedded page; discovery/attachment authority stays with its adapter.
    pub(crate) async fn open_embedded<S>(
        stream: S,
        endpoint: &str,
        deadline: Instant,
    ) -> Result<(Self, Events), Error>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let uri = endpoint.parse::<Uri>().map_err(|_| Error::Endpoint)?;
        let id = uri
            .path()
            .strip_prefix("/devtools/page/")
            .ok_or(Error::Endpoint)?;
        // Reuse the exact loopback/identity boundary of the browser transport.
        let browser = format!(
            "ws://{}/devtools/browser/{id}",
            uri.authority().ok_or(Error::Endpoint)?
        );
        if !endpoint_valid(&browser)
            || uri.scheme_str() != Some("ws")
            || uri.query().is_some()
            || endpoint.contains('#')
            || endpoint.len() > 2048
        {
            return Err(Error::Endpoint);
        }
        Self::upgrade(stream, endpoint, deadline, false).await
    }
    /// Only the RN adapter admits this inspector-proxy transport; no Chrome identity is implied.
    pub(crate) async fn open_react_native<S>(
        stream: S,
        endpoint: &str,
        deadline: Instant,
    ) -> Result<(Self, Events), Error>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        if !super::react_native::endpoint(endpoint) {
            return Err(Error::Endpoint);
        }
        Self::upgrade(stream, endpoint, deadline, true).await
    }
    async fn upgrade<S>(
        stream: S,
        endpoint: &str,
        deadline: Instant,
        react_native: bool,
    ) -> Result<(Self, Events), Error>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        if deadline <= Instant::now() {
            return Err(Error::Timeout {
                dispatch: Dispatch::NotDispatched,
            });
        }
        let mut request = endpoint
            .into_client_request()
            .map_err(|_| Error::Endpoint)?;
        if react_native {
            // Metro checks the inspector's origin. Derive it only from the
            // admitted loopback socket; callers cannot supply arbitrary headers.
            let uri = endpoint.parse::<Uri>().map_err(|_| Error::Endpoint)?;
            let origin = format!("http://{}", uri.authority().ok_or(Error::Endpoint)?);
            request.headers_mut().insert(
                tungstenite::http::header::ORIGIN,
                origin.parse().map_err(|_| Error::Endpoint)?,
            );
        }
        let (socket, _) = timeout_at(
            deadline,
            client_async_with_config(request, stream, Some(config())),
        )
        .await
        .map_err(|_| Error::Timeout {
            dispatch: Dispatch::NotDispatched,
        })?
        .map_err(|_| Error::Handshake)?;
        Ok(Self::start_profile(socket, react_native))
    }
    /// Upgrade an already selected stream without discovery, redirects or DNS.
    /// The launch owner must prove endpoint/process ownership before calling.
    /// Success means transport readiness, not accepted Chrome/schema identity.
    pub async fn open<S>(
        stream: S,
        endpoint: &str,
        deadline: Instant,
    ) -> Result<(Self, Events), Error>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        if !endpoint_valid(endpoint) {
            return Err(Error::Endpoint);
        }
        if deadline <= Instant::now() {
            return Err(Error::Timeout {
                dispatch: Dispatch::NotDispatched,
            });
        }
        Self::upgrade(stream, endpoint, deadline, false).await
    }

    #[cfg(test)]
    fn start<S>(socket: WebSocketStream<S>) -> (Self, Events)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        Self::start_profile(socket, false)
    }
    fn start_profile<S>(socket: WebSocketStream<S>, react_native: bool) -> (Self, Events)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (sink, stream) = socket.split();
        Self::start_transport(sink, stream, react_native)
    }
    fn start_transport<W, R>(sink: W, stream: R, react_native: bool) -> (Self, Events)
    where
        W: Sink<Message, Error = tungstenite::Error> + Unpin + Send + 'static,
        R: Stream<Item = Result<Message, tungstenite::Error>> + Unpin + Send + 'static,
    {
        let events = Buffer::default();
        let (frame_acks, acks) = mpsc::channel(32);
        let shared = Arc::new(Shared {
            react_native,
            edge_target_metadata: AtomicBool::new(false),
            mac_keyboard: AtomicBool::new(false),
            external_browser: AtomicBool::new(false),
            state: Mutex::new(State {
                next_id: 1,
                pending: HashMap::new(),
                retained: VecDeque::new(),
                retained_bytes: 0,
                retired: HashMap::new(),
                retiring: BTreeSet::new(),
                retirement_reserved: 0,
                loss: None,
            }),
            closed: Notify::new(),
            events: events.clone(),
            frames: Default::default(),
            ordinal: AtomicU64::new(0),
            frame_acks,
        });
        let (ordinary, ordinary_rx) = mpsc::channel(MAX_PENDING - CONTROL_RESERVE);
        let (control, control_rx) = mpsc::channel(CONTROL_RESERVE);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let client = Self(Arc::new(Client {
            ordinary,
            control,
            ordinary_slots: Arc::new(Semaphore::new(MAX_PENDING - CONTROL_RESERVE)),
            control_slots: Arc::new(Semaphore::new(CONTROL_RESERVE)),
            shutdown,
            shared: shared.clone(),
        }));
        tokio::spawn(run(
            (sink, stream),
            ordinary_rx,
            control_rx,
            shutdown_rx,
            shared,
            acks,
            Arc::downgrade(&client.0),
        ));
        (client, Events(events))
    }

    pub(super) fn accept_edge_target_metadata(&self) {
        self.0
            .shared
            .edge_target_metadata
            .store(true, Ordering::Release);
    }

    pub(super) fn is_edge(&self) -> bool {
        self.0.shared.edge_target_metadata.load(Ordering::Acquire)
    }

    pub(super) fn set_keyboard_platform(&self, user_agent: &str) {
        self.0
            .shared
            .mac_keyboard
            .store(user_agent.contains("Macintosh"), Ordering::Release);
    }

    pub(super) fn uses_mac_keyboard(&self) -> bool {
        self.0.shared.mac_keyboard.load(Ordering::Acquire)
    }

    pub(super) fn accept_additive_browser_fields(&self) {
        self.0
            .shared
            .external_browser
            .store(true, Ordering::Release);
    }

    /// Low-level backend call, never an agent-facing arbitrary-CDP forwarding API.
    /// Admission above this layer must resolve owned references and validate the
    /// pinned method schema. A dropped dispatched call retains its late reply.
    pub async fn call(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        lane: Lane,
        deadline: Instant,
    ) -> Reply {
        self.call_sized(session, method, params, lane, deadline, false)
            .await
    }

    pub(super) async fn call_sized(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        lane: Lane,
        deadline: Instant,
        fixture_payload: bool,
    ) -> Reply {
        self.validate_call(session, method, &params, lane, deadline)?;
        let (slots, queue) = match lane {
            Lane::Ordinary => (&self.0.ordinary_slots, &self.0.ordinary),
            Lane::Control => (&self.0.control_slots, &self.0.control),
        };
        let permit = slots.clone().try_acquire_owned().map_err(|_| Error::Busy)?;
        let state = Arc::new(AtomicU8::new(QUEUED));
        let waiting = Arc::new(AtomicBool::new(true));
        let guard = CancelQueued(state.clone(), waiting.clone());
        let (reply, receive) = oneshot::channel();
        let id = {
            let mut owner = self.0.shared.state.lock().expect("CDP pending owner");
            owner.prune_retired();
            if let Some(reason) = owner.loss {
                return Err(Error::Lost {
                    reason,
                    dispatch: Dispatch::NotDispatched,
                });
            }
            if session.is_some_and(|session| owner.retiring.contains(session)) {
                return Err(Error::ScopeClosing);
            }
            if owner.next_id > wire::MAX_ID {
                return Err(Error::Exhausted);
            }
            let id = owner.next_id;
            let text = wire::command(id, session, method, params, fixture_payload)?;
            owner.next_id += 1;
            owner.pending.insert(
                id,
                Pending {
                    session: session.map(str::to_owned),
                    method: method.into(),
                    state: state.clone(),
                    reply,
                    permit,
                    waiting,
                },
            );
            if queue
                .try_send(Outbound {
                    id,
                    text,
                    deadline,
                    state: state.clone(),
                })
                .is_err()
            {
                owner.pending.remove(&id);
                return Err(Error::Busy);
            }
            id
        };
        match timeout_at(deadline, receive).await {
            Ok(Ok(Ok(()))) => {
                let mut owner = self.0.shared.state.lock().expect("CDP pending owner");
                if owner.retired.get(&id).is_some_and(|reply| reply.reply_seen) {
                    owner.retired.remove(&id);
                    return Err(Error::ScopeClosed {
                        dispatch: Dispatch::PossiblyDispatched,
                    });
                }
                let index = owner
                    .retained
                    .iter()
                    .position(|reply| reply.reply.id == id)
                    .expect("Reply remains owned until its caller acknowledges it");
                let retained = owner.retained.remove(index).expect("Known reply");
                owner.retained_bytes -= retained.bytes;
                retained.reply.result.map_err(Error::Remote)
            }
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(_)) => Err(Error::Lost {
                reason: self.status().loss.unwrap_or(Loss::Closed),
                dispatch: dispatch(&state),
            }),
            Err(_) => {
                // Race cancellation against the writer before classifying it.
                drop(guard);
                Err(Error::Timeout {
                    dispatch: dispatch(&state),
                })
            }
        }
    }

    fn validate_call(
        &self,
        session: Option<&str>,
        method: &str,
        params: &Value,
        lane: Lane,
        deadline: Instant,
    ) -> Result<(), Error> {
        if deadline <= Instant::now() {
            return Err(Error::Timeout {
                dispatch: Dispatch::NotDispatched,
            });
        }
        if lane == Lane::Control
            && !(if self.0.shared.react_native {
                super::react_native::control(method, params)
            } else {
                control_method(method, params)
            })
        {
            return Err(Error::ControlMethod);
        }
        if let Some(reason) = self.status().loss {
            return Err(Error::Lost {
                reason,
                dispatch: Dispatch::NotDispatched,
            });
        }
        if self.0.shared.react_native {
            if session.is_some() || !super::react_native::command(method, params) {
                return Err(Error::InvalidCommand);
            }
        } else {
            super::schema::command(method, params)?;
        }
        Ok(())
    }

    pub fn status(&self) -> Status {
        let mut state = self.0.shared.state.lock().expect("CDP pending owner");
        state.prune_retired();
        Status {
            open: state.loss.is_none(),
            pending: state.pending.len(),
            retained_replies: state.retained.len(),
            retired_calls: state.retired.len(),
            loss: state.loss,
        }
    }
    pub(crate) fn frame_lane(&self) -> crate::browser::frames::Lane<String> {
        self.0.shared.frames.clone()
    }

    /// Transfer late-reply ownership to the workflow owner, releasing its slot.
    pub fn take_late_reply(&self) -> Option<SettledReply> {
        let mut state = self.0.shared.state.lock().expect("CDP pending owner");
        let index = state
            .retained
            .iter()
            .position(|reply| !reply.waiting.load(Ordering::SeqCst))?;
        let retained = state.retained.remove(index)?;
        state.retained_bytes -= retained.bytes;
        Some(retained.reply)
    }

    /// Stop and drop both transport halves before classifying pending outcomes.
    /// This does not send Browser.close or release an owned browser process.
    /// Cancellation leaves shutdown requested, so another close can await it.
    pub async fn close(&self, deadline: Instant) -> Result<(), Error> {
        let _ = self.0.shutdown.send(true);
        loop {
            let notified = self.0.shared.closed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.status().open {
                return Ok(());
            }
            timeout_at(deadline, notified)
                .await
                .map_err(|_| Error::Timeout {
                    dispatch: Dispatch::NotDispatched,
                })?;
        }
    }
}

impl Shared {
    fn retire_unsent(&self, id: u64) {
        if let Some(pending) = self
            .state
            .lock()
            .expect("CDP pending owner")
            .pending
            .remove(&id)
        {
            let _ = pending.reply.send(Err(Error::Timeout {
                dispatch: Dispatch::NotDispatched,
            }));
        }
    }

    fn incoming(&self, text: &str) -> Result<(), Loss> {
        match wire::incoming(text)? {
            Incoming::Event(mut event) => {
                if self.react_native {
                    if event.session_id.is_some() {
                        return Err(Loss::Protocol);
                    }
                } else {
                    if self.edge_target_metadata.load(Ordering::Acquire) {
                        super::schema::discard_edge_target_pid(&event.method, &mut event.params);
                    }
                    if self.external_browser.load(Ordering::Acquire) {
                        super::schema::project_incoming(&event.method, true, &mut event.params)
                            .map_err(|_| Loss::Protocol)?;
                    }
                    super::schema::event(&event.method, &event.params)
                        .map_err(|_| Loss::Protocol)?;
                }
                let ordinal = self.ordinal.fetch_add(1, Ordering::SeqCst) + 1;
                if event.method == "Page.screencastFrame" {
                    let session = event.session_id.ok_or(Loss::Protocol)?;
                    if self.frames.epoch(&session).is_none() {
                        return Ok(());
                    }
                    let ack = event.params["sessionId"].clone();
                    self.frames.push(session.clone(), ordinal, event.params);
                    if self.frame_acks.try_send((session, ack)).is_err() {
                        self.frames.fail("MEDIA_ACK_CAPACITY");
                    }
                    return Ok(());
                }
                if matches!(
                    event.method.as_str(),
                    "Page.frameNavigated"
                        | "Page.documentOpened"
                        | "Runtime.executionContextsCleared"
                ) && let Some(session) = event.session_id.as_ref()
                {
                    self.frames.fence(session);
                }
                self.events.push(event, text.len())
            }
            Incoming::Reply {
                id,
                session,
                mut result,
            } => {
                let mut state = self.state.lock().expect("CDP pending owner");
                state.prune_retired();
                if let Some(retired) = state.retired.get_mut(&id) {
                    if retired.reply_seen {
                        return Err(Loss::Protocol);
                    }
                    let browser_error = session.is_none() && result.is_err();
                    if retired.session != session && !browser_error {
                        return Err(Loss::Protocol);
                    }
                    if let Ok(result) = &mut result {
                        self.validate_result(&retired.method, result)?;
                    }
                    // The acknowledged closed target cannot retain an allocation.
                    // Validate the original reply, then discard its payload once.
                    retired.reply_seen = true;
                    state.prune_retired();
                    return Ok(());
                }
                let pending = state.pending.get(&id).ok_or(Loss::Protocol)?;
                // Browser routing errors (for example a just-closed attachment)
                // can omit sessionId. A dispatched, globally unique request ID
                // still owns that error; it cannot grant resource authority.
                let browser_error = session.is_none() && result.is_err();
                if (pending.session != session && !browser_error)
                    || dispatch(&pending.state) != Dispatch::PossiblyDispatched
                {
                    return Err(Loss::Protocol);
                }
                if text.len() > MAX_RETAINED_BYTES - state.retained_bytes {
                    return Err(Loss::ReplyCapacity);
                }
                if let Ok(result) = &mut result {
                    self.validate_result(&pending.method, result)?;
                }
                let pending = state.pending.remove(&id).expect("Validated pending reply");
                state.retained_bytes += text.len();
                state.retained.push_back(Retained {
                    reply: SettledReply {
                        id,
                        method: pending.method,
                        session_id: pending.session,
                        result,
                    },
                    bytes: text.len(),
                    _permit: pending.permit,
                    waiting: pending.waiting,
                });
                // Notification carries no result bytes: cancellation between
                // send and receive cannot discard an allocation's reply.
                let _ = pending.reply.send(Ok(()));
                Ok(())
            }
        }
    }

    fn validate_result(&self, method: &str, result: &mut Value) -> Result<(), Loss> {
        if self.react_native {
            return Ok(());
        }
        if self.edge_target_metadata.load(Ordering::Acquire) {
            super::schema::discard_edge_target_pid(method, result);
        }
        if self.external_browser.load(Ordering::Acquire) {
            super::schema::project_incoming(method, false, result).map_err(|_| Loss::Protocol)?;
        }
        super::schema::result(method, result).map_err(|_| Loss::Protocol)
    }

    fn lose(&self, reason: Loss) {
        let mut state = self.state.lock().expect("CDP pending owner");
        state.loss = Some(reason);
        state.retired.clear();
        for (_, pending) in state.pending.drain() {
            let _ = pending.reply.send(Err(Error::Lost {
                reason,
                dispatch: dispatch(&pending.state),
            }));
        }
        drop(state);
        self.events.close();
        self.frames.close();
        self.closed.notify_waiters();
    }
}

async fn run<W, R>(
    (sink, stream): (W, R),
    ordinary: mpsc::Receiver<Outbound>,
    control: mpsc::Receiver<Outbound>,
    mut shutdown: watch::Receiver<bool>,
    shared: Arc<Shared>,
    acks: mpsc::Receiver<(String, Value)>,
    client: std::sync::Weak<Client>,
) where
    W: Sink<Message, Error = tungstenite::Error> + Unpin,
    R: Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    let reason = {
        // Both futures are dropped at this block's end before lose() can mark
        // queued work unsent. No writer task can dispatch after that witness.
        tokio::select! {
            _ = shutdown.changed() => Loss::Closed,
            reason = read(stream, shared.clone()) => reason,
            reason = write(sink, ordinary, control, shared.clone()) => reason,
            ()=acknowledge(acks,client,shared.clone()) => Loss::Closed,
        }
    };
    shared.lose(reason);
}

async fn acknowledge(
    mut queue: mpsc::Receiver<(String, Value)>,
    client: std::sync::Weak<Client>,
    shared: Arc<Shared>,
) {
    use futures_util::stream::FuturesUnordered;
    let mut pending = FuturesUnordered::new();
    loop {
        tokio::select! {
            item=queue.recv(),if pending.len()<4=>{
                let Some((session,id))=item else{return};
                let Some(client)=client.upgrade() else{return};
                pending.push(async move {Connection(client).call(Some(&session),"Page.screencastFrameAck",serde_json::json!({"sessionId":id}),Lane::Control,Instant::now()+Duration::from_secs(5)).await});
            }
            result=pending.next(),if !pending.is_empty()=>{
                if result.is_some_and(|result|result.is_err()){shared.frames.fail("MEDIA_ACK_FAILED");}
            }
        }
    }
}

async fn read<R>(mut stream: R, shared: Arc<Shared>) -> Loss
where
    R: Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    while let Some(message) = stream.next().await {
        match message {
            Ok(Message::Text(text)) => {
                if let Err(reason) = shared.incoming(text.as_str()) {
                    return reason;
                }
            }
            Ok(Message::Close(_)) => return Loss::Eof,
            // Chrome uses JSON text. Unnegotiated binary data loses this epoch.
            Ok(Message::Binary(_) | Message::Frame(_)) => return Loss::Protocol,
            Ok(Message::Ping(_) | Message::Pong(_)) => {}
            Err(tungstenite::Error::Capacity(_)) => return Loss::Protocol,
            Err(_) => return Loss::Read,
        }
    }
    Loss::Eof
}

async fn write<W>(
    mut sink: W,
    mut ordinary: mpsc::Receiver<Outbound>,
    mut control: mpsc::Receiver<Outbound>,
    shared: Arc<Shared>,
) -> Loss
where
    W: Sink<Message, Error = tungstenite::Error> + Unpin,
{
    loop {
        let next = tokio::select! {
            biased;
            call = control.recv() => call,
            call = ordinary.recv() => call,
        };
        let Some(call) = next else {
            return Loss::Closed;
        };
        if call.deadline <= Instant::now() {
            let _ =
                call.state
                    .compare_exchange(QUEUED, CANCELLED, Ordering::SeqCst, Ordering::SeqCst);
        }
        if call
            .state
            .compare_exchange(QUEUED, DISPATCHED, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            shared.retire_unsent(call.id);
            continue;
        }
        // An interrupted or timed-out send loses the socket rather than retrying
        // a frame that may already have reached Chrome.
        let expires = call.deadline.min(Instant::now() + WRITE_BUDGET);
        match timeout_at(expires, sink.send(Message::Text(call.text.into()))).await {
            Ok(Ok(())) => {}
            _ => return Loss::Write,
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
