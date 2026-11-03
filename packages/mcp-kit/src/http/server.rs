//! A blocking Streamable HTTP server over `std::net`, behind the `http-server` feature.
//!
//! [`serve`] runs a [`Handler`] over a bound [`TcpListener`]. An accept thread
//! starts one thread per connection, which reads the request, checks it with the
//! [`Endpoint`] and writes its response. The calling thread is
//! the dispatcher: it owns the one [`Session`] and the shared scheduler, and it
//! never writes to a socket. Each POST is independent; the dispatcher gives every
//! admitted request a server-unique ID and restores the client's ID in replies.

mod connection;

use super::{Admitted, Endpoint, EndpointOptions, Response};
use crate::{
    json,
    rpc::{self, RpcError},
    session::{Inbound, Session},
    stdio::{
        self, CHANGE_BUFFER, Handler, Options, Outgoing, PROGRESS_BUFFER, ServeError, Trace,
        scheduler::{Scheduler, Submission},
    },
    subscription::{Change, SUBSCRIPTION_KEY},
    tool::{Catalog, ToolCall},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, Thread},
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

/// The default number of connections served at once.
pub const DEFAULT_MAX_CONNECTIONS: usize = 64;
/// The most connections [`HttpOptions::max_connections`] allows.
pub const MAX_CONNECTIONS: usize = 1024;
/// The default time to receive a request's line, headers and body.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// The default time one response write may take.
pub const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// The default interval between keep-alive comments on a quiet listen stream.
pub const DEFAULT_KEEP_ALIVE: Duration = Duration::from_secs(15);
/// The longest request line, in bytes, including its line break.
pub const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
/// The largest header section, in bytes.
pub const MAX_HEADER_BYTES: usize = 32 * 1024;
/// The most header fields in one request.
pub const MAX_HEADER_FIELDS: usize = 100;
/// Notifications one listen stream may have pending before it is ended.
pub const LISTEN_BUFFER: usize = 64;

/// How the HTTP server listens and bounds its connections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpOptions {
    /// The exact endpoint path, `/mcp` by default.
    pub path: String,
    /// Browser origins allowed to call the endpoint; see [`EndpointOptions`].
    pub allowed_origins: Vec<String>,
    /// Connections served at once, within 1..=[`MAX_CONNECTIONS`]. Others get `503`.
    pub max_connections: usize,
    /// Time from accept to a complete request, within 1..=60 seconds.
    pub read_timeout: Duration,
    /// Time one response write may take, within 1..=300 seconds.
    pub write_timeout: Duration,
    /// Keep-alive interval for quiet listen streams, within 1..=300 seconds.
    pub keep_alive: Duration,
}

impl Default for HttpOptions {
    fn default() -> Self {
        Self {
            path: super::DEFAULT_PATH.to_owned(),
            allowed_origins: Vec::new(),
            max_connections: DEFAULT_MAX_CONNECTIONS,
            read_timeout: DEFAULT_READ_TIMEOUT,
            write_timeout: DEFAULT_WRITE_TIMEOUT,
            keep_alive: DEFAULT_KEEP_ALIVE,
        }
    }
}

/// Stops a running [`serve`] from another thread.
///
/// Shutdown stops accepting connections, ends listen streams with their final
/// results and applies the stdio end-of-input rules to admitted work: queued
/// mutations are refused with [`rpc::REQUEST_CANCELLED`], running mutations are
/// cancelled after [`Options::disconnect_grace`], and read-only work continues
/// to its deadline. A handle serves one [`serve`] call.
#[derive(Debug, Clone, Default)]
pub struct Shutdown(Arc<ShutdownState>);

#[derive(Debug, Default)]
struct ShutdownState {
    requested: AtomicBool,
    wake: Mutex<Option<(SocketAddr, Thread)>>,
}

impl Shutdown {
    /// A handle for one server.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the server to stop; [`serve`] returns once every connection has closed.
    pub fn shutdown(&self) {
        self.0.requested.store(true, Ordering::SeqCst);
        let wake = self
            .0
            .wake
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some((address, dispatcher)) = wake {
            dispatcher.unpark();
            // A blocking accept returns for this connection and sees the request.
            let _ = TcpStream::connect_timeout(&address, Duration::from_secs(1));
        }
    }

    /// Whether shutdown was requested.
    #[must_use]
    pub fn requested(&self) -> bool {
        self.0.requested.load(Ordering::SeqCst)
    }
}

/// Serves Streamable HTTP on `listener` until `shutdown` is requested.
///
/// Requires [`SessionOptions::modern`](crate::SessionOptions::modern); the
/// legacy lifecycle is never reached and [`Handler::initialize`] is never
/// called. [`Handler::connected`] receives one server-wide change sender when
/// subscriptions are enabled. The request body limit is
/// [`SessionOptions::max_message_bytes`](crate::SessionOptions::max_message_bytes).
/// The server authenticates no one: bind a loopback address.
///
/// # Errors
///
/// Returns [`ServeError`] for invalid options or endpoint settings, a thread
/// that cannot start, a listener without a local address, or a published
/// change the session refuses, which ends serving after the drain.
pub fn serve<H: Handler>(
    handler: Arc<H>,
    catalog: Arc<Catalog>,
    listener: TcpListener,
    http: &HttpOptions,
    options: &Options,
    shutdown: &Shutdown,
) -> Result<(), ServeError> {
    validate(http, options)?;
    let endpoint = Endpoint::new(EndpointOptions {
        path: http.path.clone(),
        allowed_origins: http.allowed_origins.clone(),
        max_body_bytes: options.session.max_message_bytes,
    })?;
    let session = Session::try_new(catalog, options.session.clone())?;
    let address = listener.local_addr().map_err(ServeError::Input)?;
    let dispatcher = thread::current();
    let scheduler = Scheduler::start(&handler, options, dispatcher.clone())?;
    let (events_sender, events) = mpsc::sync_channel(2 * http.max_connections + 1);
    let shared = Arc::new(Shared {
        endpoint,
        notifications: options
            .session
            .extension_notifications
            .iter()
            .cloned()
            .collect(),
        events: events_sender,
        dispatcher: dispatcher.clone(),
        next_id: AtomicU64::new(1),
        live: AtomicUsize::new(0),
        max_connections: http.max_connections,
        read_timeout: http.read_timeout,
        write_timeout: http.write_timeout,
        keep_alive: http.keep_alive,
    });
    *shutdown
        .0
        .wake
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some((wake_address(address), dispatcher));
    let accepting = {
        let shared = Arc::clone(&shared);
        let shutdown = shutdown.clone();
        thread::Builder::new()
            .name("mcp-http-accept".into())
            .spawn(move || accept(&listener, &shared, &shutdown))
            .map_err(ServeError::Spawn)?
    };
    let mut server = Dispatcher {
        handler,
        session,
        scheduler,
        changes: None,
        pending: HashMap::new(),
        limit: options.max_output_bytes,
        stopping: false,
        error: None,
    };
    if options.session.subscriptions.is_some() {
        let (sender, receiver) = stdio::change_channel(&options.session);
        server.changes = Some(receiver);
        server.handler.connected(sender);
    }
    server.run(&events, &shared, shutdown);
    server.scheduler.shutdown();
    shutdown.shutdown();
    let _ = accepting.join();
    server.error.map_or(Ok(()), |error| Err(error.into()))
}

fn validate(http: &HttpOptions, options: &Options) -> Result<(), ServeError> {
    stdio::validate(options)?;
    if options.session.modern.is_none() {
        return Err(ServeError::Options(
            "HTTP serving requires modern protocol options",
        ));
    }
    if options.session.max_messages.is_some() {
        return Err(ServeError::Options(
            "HTTP serving does not support a session message limit",
        ));
    }
    let within = |value: Duration, most: u64| {
        value >= Duration::from_secs(1) && value <= Duration::from_secs(most)
    };
    if !(1..=MAX_CONNECTIONS).contains(&http.max_connections)
        || !within(http.read_timeout, 60)
        || !within(http.write_timeout, 300)
        || !within(http.keep_alive, 300)
    {
        return Err(ServeError::Options(
            "HTTP connection and time limits are out of range",
        ));
    }
    Ok(())
}

/// The address `shutdown` connects to; an unspecified bind becomes loopback.
fn wake_address(address: SocketAddr) -> SocketAddr {
    let ip = match address.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, address.port())
}

/// State the accept and connection threads share with the dispatcher.
struct Shared {
    endpoint: Endpoint,
    notifications: HashSet<String>,
    events: SyncSender<Event>,
    dispatcher: Thread,
    next_id: AtomicU64,
    live: AtomicUsize,
    max_connections: usize,
    read_timeout: Duration,
    write_timeout: Duration,
    keep_alive: Duration,
}

impl Shared {
    /// Sends an event to the dispatcher; false once it has stopped.
    fn send(&self, event: Event) -> bool {
        let sent = self.events.send(event).is_ok();
        self.dispatcher.unpark();
        sent
    }
}

/// Counts one live connection until dropped.
struct Live(Arc<Shared>);

impl Live {
    fn new(shared: &Arc<Shared>) -> Self {
        shared.live.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(shared))
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.0.live.fetch_sub(1, Ordering::SeqCst);
        self.0.dispatcher.unpark();
    }
}

fn accept(listener: &TcpListener, shared: &Arc<Shared>, shutdown: &Shutdown) {
    for stream in listener.incoming() {
        if shutdown.requested() {
            break;
        }
        let Ok(stream) = stream else {
            // Back off from a persistent failure, such as a full descriptor table.
            thread::sleep(Duration::from_millis(10));
            continue;
        };
        // Count before the second check, so a stopping dispatcher that saw no
        // live connection never misses this one.
        let live = Live::new(shared);
        if shutdown.requested() {
            break;
        }
        if shared.live.load(Ordering::SeqCst) > shared.max_connections {
            drop(live);
            connection::refuse_busy(&stream);
            continue;
        }
        // A connection whose thread cannot start is closed; its count drops with it.
        let _ = thread::Builder::new()
            .name("mcp-http".into())
            .spawn(move || {
                let shared = Arc::clone(&live.0);
                connection::handle(&stream, &shared);
                drop(live);
            });
    }
}

/// A message from a connection thread to the dispatcher.
enum Event {
    /// An admitted request with its server-unique ID and reply mailbox.
    Submit {
        id: u64,
        admitted: Admitted,
        mailbox: Arc<Mailbox>,
    },
    /// A product notification the session options list.
    Notification(Value),
    /// The client closed the connection before its reply.
    Closed(u64),
}

/// What a connection writes after its events.
enum Last {
    /// The final JSON-RPC reply and its error code.
    Reply(Vec<u8>, Option<i32>),
    /// A complete refusal written instead of a reply.
    Response(Response),
}

/// The dispatcher's bounded handoff to one connection thread.
#[derive(Default)]
struct Mailbox {
    state: Mutex<MailboxState>,
    ready: Condvar,
}

#[derive(Default)]
struct MailboxState {
    events: VecDeque<Vec<u8>>,
    last: Option<Last>,
    ended: bool,
}

impl Mailbox {
    fn lock(&self) -> std::sync::MutexGuard<'_, MailboxState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues an event unless `bound` events are pending.
    fn event(&self, bytes: Vec<u8>, bound: usize) -> bool {
        let mut state = self.lock();
        if state.events.len() >= bound {
            return false;
        }
        state.events.push_back(bytes);
        self.ready.notify_one();
        true
    }

    /// Sets the final message; it always has room.
    fn finish(&self, last: Last) {
        self.lock().last = Some(last);
        self.ready.notify_one();
    }

    /// Ends a stream without a final message.
    fn end(&self) {
        self.lock().ended = true;
        self.ready.notify_one();
    }

    /// Waits up to `timeout` for output, then takes what is pending.
    fn wait(&self, timeout: Duration) -> (Vec<Vec<u8>>, Option<Last>, bool) {
        let mut state = self.lock();
        if state.events.is_empty() && state.last.is_none() && !state.ended {
            state = self
                .ready
                .wait_timeout(state, timeout)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        (
            state.events.drain(..).collect(),
            state.last.take(),
            state.ended,
        )
    }
}

/// A request awaiting its reply.
struct Pending {
    client: Value,
    mailbox: Arc<Mailbox>,
    listen: bool,
}

/// The dispatcher: the one session, the scheduler and every pending reply.
struct Dispatcher<H> {
    handler: Arc<H>,
    session: Session,
    scheduler: Scheduler,
    changes: Option<Receiver<Change>>,
    pending: HashMap<u64, Pending>,
    limit: usize,
    stopping: bool,
    error: Option<crate::ConfigurationError>,
}

/// The start of every encoded reply envelope, up to its ID.
const REPLY_PREFIX: &[u8] = br#"{"jsonrpc":"2.0","id":"#;

impl<H: Handler> Dispatcher<H> {
    fn run(&mut self, events: &Receiver<Event>, shared: &Shared, shutdown: &Shutdown) {
        loop {
            let outgoing = self.scheduler.settle(&mut self.session);
            self.deliver(outgoing);
            if !self.stopping {
                let expired = self.session.expire_subscriptions(Instant::now());
                self.subscription_frames(expired);
                self.publish_changes();
                if self.error.is_some() {
                    // Stop accepting too, or steady traffic keeps `serve` running.
                    shutdown.shutdown();
                }
                if shutdown.requested() {
                    self.stop();
                }
            }
            if self.stopping && shared.live.load(Ordering::SeqCst) == 0 && self.scheduler.idle() {
                return;
            }
            if let Ok(event) = events.try_recv() {
                self.event(event);
                continue;
            }
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

    fn stop(&mut self) {
        self.stopping = true;
        self.changes = None;
        let ended = self.session.end_subscriptions();
        self.subscription_frames(ended);
        let outgoing = self.scheduler.disconnect(&mut self.session);
        self.deliver(outgoing);
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Submit {
                id,
                admitted,
                mailbox,
            } => self.submit(id, admitted, mailbox),
            Event::Notification(message) => {
                if self.stopping {
                    return;
                }
                let inbound = self.session.receive_value(message);
                if let Ok(work) = stdio::scheduled(&self.session, inbound)
                    && work.id.is_none()
                {
                    self.schedule(work);
                }
            }
            Event::Closed(id) => self.closed(id),
        }
    }

    fn submit(&mut self, id: u64, admitted: Admitted, mailbox: Arc<Mailbox>) {
        if self.stopping {
            let error = RpcError::new(rpc::REQUEST_CANCELLED, "Server is shutting down");
            let reply = serde_json::to_vec(&rpc::failure(admitted.id, &error)).unwrap_or_default();
            mailbox.finish(Last::Reply(reply, Some(error.code)));
            return;
        }
        // Checked now, while the body is at hand, but applied only once the
        // session admits the call: metadata and method refusals (steps 9 and
        // 12) come first, argument validation after.
        let parameters = admitted.check_parameters(self.session.catalog());
        let Admitted {
            id: client,
            mut message,
            ..
        } = admitted;
        message["id"] = json!(id);
        self.pending.insert(
            id,
            Pending {
                client,
                mailbox,
                listen: false,
            },
        );
        let inbound = self.session.receive_value(message);
        if let (
            Err(response),
            Inbound::Tool { id: key, .. } | Inbound::InvalidArguments { id: key, .. },
        ) = (parameters, &inbound)
        {
            self.session.finish(key);
            if let Some(pending) = self.pending.remove(&id) {
                pending.mailbox.finish(Last::Response(response));
            }
            return;
        }
        let inbound = match stdio::scheduled(&self.session, inbound) {
            Ok(work) => return self.schedule(work),
            Err(inbound) => *inbound,
        };
        match inbound {
            Inbound::Subscription {
                id: key,
                acknowledged,
            } => {
                self.acknowledge(id, &key, acknowledged);
            }
            Inbound::InvalidArguments {
                id: key,
                call,
                error,
            } => self.invalid_arguments(id, &key, &call, &error),
            Inbound::Reply(reply) | Inbound::Fatal { reply, .. } => self.answer(id, reply),
            // The modern path never yields these for a request; answer rather than hang.
            _ => {
                let key = json!(id);
                self.session.finish(&key);
                let error = RpcError::new(rpc::METHOD_NOT_FOUND, "Method not found");
                self.answer(id, rpc::failure(key, &error));
            }
        }
    }

    fn schedule(&mut self, work: stdio::Scheduled) {
        let context = work
            .id
            .as_ref()
            .and_then(|key| self.session.request_context(key));
        let admission = self
            .handler
            .admit_context(&work.request, context.as_deref());
        let outgoing = self.scheduler.admit(
            &mut self.session,
            Submission {
                trace: Trace::default(),
                id: work.id,
                request: work.request,
                admission,
                read_only: work.read_only,
                progress_token: work.progress_token,
            },
        );
        self.deliver(outgoing);
    }

    fn invalid_arguments(
        &mut self,
        id: u64,
        key: &Value,
        call: &ToolCall,
        error: &crate::schema::ArgumentError,
    ) {
        let result = self.handler.invalid_arguments(call, error);
        let encoded = self
            .session
            .request_context(key)
            .map(|context| context.encode_success(key, &result, self.limit, &|| Ok(())));
        self.session.finish(key);
        if let Some(Ok(bytes)) = encoded {
            self.answer_bytes(&bytes);
        } else {
            let error = RpcError::new(rpc::INTERNAL_ERROR, "Cannot encode tool result");
            self.answer(id, rpc::failure(key.clone(), &error));
        }
    }

    fn acknowledge(&mut self, id: u64, key: &Value, mut acknowledged: Value) {
        let Some(pending) = self.pending.get_mut(&id) else {
            let _ = self.session.end_subscription(key);
            return;
        };
        pending.listen = true;
        acknowledged["params"]["_meta"][SUBSCRIPTION_KEY] = pending.client.clone();
        if let Ok(bytes) = json::encode(&acknowledged, self.limit) {
            pending.mailbox.event(bytes, LISTEN_BUFFER);
        } else {
            self.session.finish(key);
            let error = RpcError::new(
                rpc::INTERNAL_ERROR,
                "Subscription acknowledgement exceeds output limit",
            );
            self.answer(id, rpc::failure(key.clone(), &error));
        }
    }

    fn closed(&mut self, id: u64) {
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        let key = json!(id);
        if pending.listen {
            let _ = self.session.end_subscription(&key);
        } else if self.session.finish(&key) {
            let outgoing = self
                .scheduler
                .cancel(&mut self.session, &key, &Trace::default());
            self.deliver(outgoing);
        }
    }

    fn publish_changes(&mut self) {
        for _ in 0..CHANGE_BUFFER {
            let Some(change) = self
                .changes
                .as_ref()
                .and_then(|changes| changes.try_recv().ok())
            else {
                break;
            };
            match self.session.publish(change) {
                Ok(frames) => self.subscription_frames(frames),
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            }
        }
    }

    /// Routes listen stream notifications and final results to their streams.
    fn subscription_frames(&mut self, frames: Vec<Value>) {
        for mut frame in frames {
            if let Some(id) = frame.get("id").and_then(Value::as_u64) {
                self.answer(id, frame);
                continue;
            }
            let Some(id) = frame["params"]["_meta"][SUBSCRIPTION_KEY].as_u64() else {
                continue;
            };
            let Some(pending) = self.pending.get(&id) else {
                continue;
            };
            frame["params"]["_meta"][SUBSCRIPTION_KEY] = pending.client.clone();
            let delivered = json::encode(&frame, self.limit)
                .is_ok_and(|bytes| pending.mailbox.event(bytes, LISTEN_BUFFER));
            if !delivered {
                // A slow listener, or an oversized notification, ends its stream
                // without a final result so the others keep receiving.
                if let Some(pending) = self.pending.remove(&id) {
                    pending.mailbox.end();
                }
                let _ = self.session.end_subscription(&json!(id));
            }
        }
    }

    fn deliver(&mut self, outgoing: Vec<Outgoing>) {
        for frame in outgoing {
            self.route(frame, None);
        }
    }

    fn route(&mut self, frame: Outgoing, progress: Option<u64>) {
        match frame {
            Outgoing::Observed(frame, _, _) => self.route(*frame, progress),
            Outgoing::Routed(id, frame) => {
                if let Some(id) = id.as_u64() {
                    self.route(*frame, Some(id));
                }
            }
            Outgoing::Value(value) => match progress {
                Some(id) => self.progress(id, &value),
                // Server requests and their notices have string IDs and never
                // occur for modern calls; nothing else lacks a numeric ID.
                None => {
                    if let Some(id) = value.get("id").and_then(Value::as_u64) {
                        self.answer(id, value);
                    }
                }
            },
            Outgoing::Bytes(bytes) => self.answer_bytes(&bytes),
        }
    }

    fn progress(&self, id: u64, notification: &Value) {
        if let Some(pending) = self.pending.get(&id).filter(|pending| !pending.listen)
            && let Ok(bytes) = json::encode(notification, self.limit)
        {
            // Progress beyond the buffer is dropped, as on stdio.
            pending.mailbox.event(bytes, PROGRESS_BUFFER);
        }
    }

    /// Delivers a reply value to request `id` with the client's ID restored.
    fn answer(&mut self, id: u64, mut reply: Value) {
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        let mut code = reply["error"]["code"]
            .as_i64()
            .and_then(|code| i32::try_from(code).ok());
        reply["id"] = pending.client.clone();
        if pending.listen && reply["result"].is_object() {
            reply["result"]["_meta"][SUBSCRIPTION_KEY] = pending.client.clone();
        }
        let bytes = match stdio::encode_frame(&reply, self.limit) {
            Ok(bytes) => bytes,
            Err(Some(fallback)) => {
                code = Some(rpc::INTERNAL_ERROR);
                fallback
            }
            Err(None) => return,
        };
        pending.mailbox.finish(Last::Reply(bytes, code));
    }

    /// Delivers an encoded reply, replacing the server ID after its envelope
    /// prefix with the client's ID without reparsing the result.
    fn answer_bytes(&mut self, bytes: &[u8]) {
        let Some(rest) = bytes.strip_prefix(REPLY_PREFIX) else {
            return;
        };
        let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
        let Some(id) = std::str::from_utf8(&rest[..digits])
            .ok()
            .and_then(|digits| digits.parse::<u64>().ok())
        else {
            return;
        };
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        let (reply, code) = with_client_id(&pending.client, &rest[digits..], self.limit);
        pending.mailbox.finish(Last::Reply(reply, code));
    }
}

/// An encoded reply with `client` as its ID before `rest`, and its error code.
///
/// The worker bounded the reply with the short server ID, so the result is
/// measured again: over `limit`, it becomes the internal-error reply for
/// `client`, which fits any valid limit because IDs are at most 128 bytes.
fn with_client_id(client: &Value, rest: &[u8], limit: usize) -> (Vec<u8>, Option<i32>) {
    let id = serde_json::to_vec(client).unwrap_or_default();
    if REPLY_PREFIX.len() + id.len() + rest.len() > limit {
        let error = RpcError::new(rpc::INTERNAL_ERROR, "Response exceeds output limit");
        let fallback =
            serde_json::to_vec(&rpc::failure(client.clone(), &error)).unwrap_or_default();
        return (fallback, Some(error.code));
    }
    let mut reply = Vec::with_capacity(REPLY_PREFIX.len() + id.len() + rest.len());
    reply.extend_from_slice(REPLY_PREFIX);
    reply.extend_from_slice(&id);
    reply.extend_from_slice(rest);
    (reply, error_code(rest))
}

/// The error code of an encoded envelope's remainder after its ID.
fn error_code(rest: &[u8]) -> Option<i32> {
    let code = rest.strip_prefix(br#","error":{"code":"#)?;
    let length = code
        .iter()
        .enumerate()
        .take_while(|(index, byte)| byte.is_ascii_digit() || (*index == 0 && **byte == b'-'))
        .count();
    std::str::from_utf8(&code[..length]).ok()?.parse().ok()
}
