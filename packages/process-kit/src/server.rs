//! Synchronous bounded ordinary/control pools driven by the shared admission core.

use crate::{
    Error, Result,
    connection::{self, Connection, Handoff, Slots},
    core::{Admission, ControlRole, ExitPolicy, Permit},
    endpoint::{self, Lease},
    frame,
    net::Stream,
    protocol::{self, Code, Control, Identity, Reply, Request},
    version::Version,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as driver;
#[cfg(any(windows, test))]
mod windows;
#[cfg(windows)]
use windows as driver;

const ACCEPT_REFUSAL_TIMEOUT: Duration = Duration::from_millis(20);

/// Product callbacks. No callback runs inside the admission lock.
/// Extension handlers must return promptly or hand work to a product-owned worker.
pub trait Handler: Send + Sync + 'static {
    /// Product dispatch; keep transferred responsibility pinned through `context.permit.hold()`.
    ///
    /// # Errors
    /// Returns the product's bounded structured error for a final admitted reply.
    fn call(
        &self,
        request: &Request,
        context: &Context<'_>,
    ) -> std::result::Result<Value, protocol::ProductError>;
    /// Declare read-only inspection after reserving admission, before dispatch.
    fn observer(&self, _request: &Request) -> bool {
        false
    }
    /// Additional control action names; unknown actions are refused before admission.
    fn extensions(&self) -> &[&str] {
        &[]
    }
    /// Classify an advertised extension before acknowledgement or effects.
    /// Cleanup may settle existing obligations during drain; it must never start
    /// unrelated work. Inspection cannot create holds or renew idle grace.
    fn control_role(&self, _request: &Control) -> ControlRole {
        ControlRole::Work
    }
    /// Prepare a product-owned connection, independently of one-shot dispatch.
    /// The receiver runs only after the final reply succeeds and must return
    /// promptly. It owns session scheduling and drain cleanup thereafter.
    ///
    /// # Errors
    /// Returns a product refusal; no connection is transferred on failure.
    fn open(
        &self,
        _request: &Request,
        _context: &Context<'_>,
    ) -> std::result::Result<Handoff, protocol::ProductError> {
        Err(protocol::ProductError(Value::Null))
    }
    /// A bounded, secret-free blocker page or summary, computed only when requested.
    fn blockers(&self) -> Value {
        Value::Null
    }
    /// Product-specific controls, independent of the product protocol version.
    ///
    /// # Errors
    /// Returns the product's bounded structured error for a final admitted reply.
    fn control(
        &self,
        _request: &Control,
        _context: &Context<'_>,
    ) -> std::result::Result<Value, protocol::ProductError> {
        Err(protocol::ProductError(Value::Null))
    }
    /// Start product cleanup after the first drain admission. Never wait here for work
    /// whose completion requires an ordinary worker; transfer it with a held obligation.
    fn drain(&self) {}
}

/// An admitted dispatch and its same-user connection. Products can clone the
/// stream to establish a separately bounded owner channel before authorizing work.
pub struct Context<'a> {
    pub permit: &'a Permit,
    pub stream: &'a Stream,
}

/// Pool capacities count active plus queued connections, including unread frames.
#[derive(Clone)]
pub struct Options {
    pub workers: usize,
    pub capacity: usize,
    pub control_workers: usize,
    pub control_capacity: usize,
    /// Maximum transferred connections, including prepared handshakes. Zero
    /// disables handoff. Slots remain occupied through product cleanup.
    pub connection_capacity: usize,
    pub frame_limit: usize,
    /// Total frame read/write deadline, not a per-byte renewable timeout.
    pub io_timeout: Duration,
    /// Enable exact-incarnation `force_stop`. It exits the entire process with
    /// status 75 after a bounded reply attempt, bypassing destructors, callbacks
    /// and live holds. Enable only when product crash recovery is established.
    /// Graceful drain and version replacement never invoke it automatically.
    pub allow_force_stop: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            workers: 8,
            capacity: 24,
            control_workers: 1,
            control_capacity: 9,
            connection_capacity: 64,
            frame_limit: frame::DEFAULT_LIMIT,
            io_timeout: Duration::from_secs(3),
            allow_force_stop: false,
        }
    }
}

struct Shared<H> {
    identity: Identity,
    version: Option<Version>,
    gate: Admission,
    handler: Arc<H>,
    options: Options,
    ordinary_count: Arc<AtomicUsize>,
    control_count: Arc<AtomicUsize>,
    connections: Arc<Slots>,
}
struct Job {
    stream: Stream,
    permit: Permit,
    until: Instant,
}
struct Pool {
    sender: Option<mpsc::SyncSender<Job>>,
    workers: Vec<JoinHandle<()>>,
    count: Arc<AtomicUsize>,
    capacity: usize,
}

impl Pool {
    fn new<H: Handler>(shared: &Arc<Shared<H>>, control: bool) -> Self {
        let (workers, capacity) = if control {
            (
                shared.options.control_workers,
                shared.options.control_capacity,
            )
        } else {
            (shared.options.workers, shared.options.capacity)
        };
        let (sender, receiver) = mpsc::sync_channel::<Job>(capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let count = Arc::clone(if control {
            &shared.control_count
        } else {
            &shared.ordinary_count
        });
        let workers = (0..workers)
            .map(|_| {
                let shared = Arc::clone(shared);
                let receiver = Arc::clone(&receiver);
                let count = Arc::clone(&count);
                std::thread::spawn(move || {
                    loop {
                        let message = receiver
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .recv();
                        let Ok(mut job) = message else {
                            break;
                        };
                        // A panicking callback must not abandon other admitted replies.
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            handle(&shared, &mut job, control)
                        }));
                        drop(job);
                        count.fetch_sub(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        Self {
            sender: Some(sender),
            workers,
            count,
            capacity,
        }
    }

    fn enqueue<H: Handler>(&self, shared: &Shared<H>, stream: Stream, control: bool) {
        if endpoint::check_peer(&stream).is_err() {
            return;
        }
        // Darwin accepted sockets inherit the listener's nonblocking mode.
        // Every worker read uses a total deadline on a blocking stream.
        if stream.set_nonblocking(false).is_err() {
            return;
        }
        // A refusal must not stall admission to the other lane for a full frame budget.
        let refusal_until = Instant::now() + shared.options.io_timeout.min(ACCEPT_REFUSAL_TIMEOUT);
        if !control
            && matches!(
                shared.gate.snapshot().state,
                crate::core::State::Draining | crate::core::State::Exiting
            )
        {
            let _ = refuse_until(shared, &mut &stream, "", Code::Draining, refusal_until);
            return;
        }
        let reserved = self
            .count
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < self.capacity).then_some(n + 1)
            })
            .is_ok();
        if !reserved {
            let _ = refuse_until(shared, &mut &stream, "", Code::Capacity, refusal_until);
            return;
        }
        let permit = if control {
            shared.gate.control()
        } else {
            shared.gate.enter(false)
        };
        let Ok(permit) = permit else {
            self.count.fetch_sub(1, Ordering::SeqCst);
            let _ = refuse_until(shared, &mut &stream, "", Code::Draining, refusal_until);
            return;
        };
        let job = Job {
            stream,
            permit,
            until: Instant::now() + shared.options.io_timeout,
        };
        if let Some(sender) = &self.sender
            && let Err(error) = sender.try_send(job)
        {
            self.count.fetch_sub(1, Ordering::SeqCst);
            let job = match error {
                mpsc::TrySendError::Full(j) | mpsc::TrySendError::Disconnected(j) => j,
            };
            let _ = refuse_until(shared, &mut &job.stream, "", Code::Capacity, refusal_until);
        }
    }

    fn close(&mut self) -> Result<()> {
        self.sender.take();
        let mut failed = false;
        for worker in self.workers.drain(..) {
            failed |= worker.join().is_err();
        }
        if failed { Err(Error::Worker) } else { Ok(()) }
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Serve until the supplied policy or an explicit control closes admission and
/// every admitted reply and product-held obligation has settled. Lease release
/// occurs after both pools join and both owned endpoints are removed.
/// An enabled explicit `force_stop` instead terminates the entire process with
/// status 75; the kernel releases exclusion and the successor replaces stale
/// endpoints. It does not return or run product cleanup/destructors.
///
/// # Errors
/// Returns invalid options/identity, endpoint, poll, cleanup or worker errors.
/// Product dispatch errors are framed replies; they never stop the server.
pub fn serve<H: Handler>(
    lease: impl Into<Arc<Lease>>,
    mut identity: Identity,
    gate: Admission,
    handler: Arc<H>,
    policy: &impl ExitPolicy,
    options: Options,
) -> Result<()> {
    let lease = lease.into();
    let version = Version::parse(&identity.build).ok();
    identity.capabilities.retain(|c| {
        c != protocol::VERSION_RETIREMENT
            && c != protocol::FORCE_STOP
            && c != protocol::CONNECTION_HANDOFF
    });
    if options.connection_capacity > 0 {
        identity
            .capabilities
            .push(protocol::CONNECTION_HANDOFF.into());
    }
    if version.is_some() {
        identity
            .capabilities
            .push(protocol::VERSION_RETIREMENT.into());
    }
    if options.allow_force_stop {
        identity.capabilities.push(protocol::FORCE_STOP.into());
    }
    validate(&identity, &options)?;
    let ordinary = lease.bind(false)?;
    let control = lease.bind(true)?;
    let shared = Arc::new(Shared {
        connections: Slots::new(options.connection_capacity),
        identity,
        version,
        gate,
        handler,
        options,
        ordinary_count: Arc::new(AtomicUsize::new(0)),
        control_count: Arc::new(AtomicUsize::new(0)),
    });
    let mut work = Pool::new(&shared, false);
    let mut controls = Pool::new(&shared, true);
    let start = Instant::now();
    let result = driver::run(
        &shared,
        policy,
        [(&control, &controls, true), (&ordinary, &work, false)],
        start,
    );
    // Even an accept failure waits for previously admitted work, never dropping it.
    if result.is_err() {
        if shared.gate.drain() {
            shared.handler.drain();
        }
        while !shared.gate.tick(start.elapsed(), &crate::core::Explicit) {
            std::thread::park();
        }
    }
    let joined = work.close().and(controls.close());
    let removed = ordinary.unlink().and(control.unlink());
    shared.gate.set_waker(None);
    result.and(joined).and(removed)
}

fn validate(identity: &Identity, o: &Options) -> Result<()> {
    if identity.control_version != 1
        || !protocol::valid_id(&identity.manager_id)
        || identity.build.len() > 128
        || identity.capabilities.len() > 128
        || identity.capabilities.iter().any(|s| !protocol::valid_id(s))
        || o.workers == 0
        || o.control_workers == 0
        || o.capacity < o.workers
        || o.control_capacity < o.control_workers
        || o.capacity > 65536
        || o.control_capacity > 65536
        || o.connection_capacity > 65536
        || o.frame_limit < 1024
        || o.frame_limit > u32::MAX as usize
        || o.io_timeout.is_zero()
    {
        return Err(Error::Options);
    }
    Ok(())
}

fn response<H>(
    s: &Shared<H>,
    id: &str,
    accepted: Option<bool>,
    code: Option<Code>,
    data: Value,
) -> Reply {
    Reply {
        control_version: 1,
        manager_id: s.identity.manager_id.clone(),
        request_id: id.into(),
        accepted,
        code,
        data,
    }
}
fn send<H>(s: &Shared<H>, stream: &mut &Stream, reply: &Reply, until: Instant) -> Result<()> {
    let mut output = Deadline { stream, until };
    match frame::write_json(&mut output, reply, s.options.frame_limit) {
        Err(Error::FrameTooLarge) => {
            let bounded = response(
                s,
                &reply.request_id,
                reply.accepted,
                Some(Code::FrameTooLarge),
                Value::Null,
            );
            frame::write_json(&mut output, &bounded, s.options.frame_limit)
        }
        other => other,
    }
}
fn refuse<H>(s: &Shared<H>, stream: &mut &Stream, id: &str, code: Code) -> Result<()> {
    refuse_until(s, stream, id, code, Instant::now() + s.options.io_timeout)
}
fn refuse_until<H>(
    s: &Shared<H>,
    stream: &mut &Stream,
    id: &str,
    code: Code,
    until: Instant,
) -> Result<()> {
    send(
        s,
        stream,
        &response(s, id, Some(false), Some(code), Value::Null),
        until,
    )
}

fn handle<H: Handler>(s: &Shared<H>, job: &mut Job, control_lane: bool) -> Result<()> {
    let mut stream = &job.stream;
    let remaining = job.until.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return refuse(s, &mut stream, "", Code::Capacity);
    }
    job.stream.set_read_timeout(Some(remaining))?;
    job.stream.set_write_timeout(Some(s.options.io_timeout))?;
    // A deadline wrapper updates the socket timeout before every partial read.
    let bytes = match frame::read(
        &mut Deadline {
            stream: &job.stream,
            until: job.until,
        },
        s.options.frame_limit,
    ) {
        Ok(bytes) => bytes,
        Err(Error::FrameTooLarge) => return refuse(s, &mut stream, "", Code::FrameTooLarge),
        Err(e) => {
            let _ = refuse(s, &mut stream, "", Code::Invalid);
            return Err(e);
        }
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return refuse(s, &mut stream, "", Code::Invalid),
    };
    let id = value
        .get("request_id")
        .and_then(Value::as_str)
        .filter(|id| protocol::valid_id(id))
        .unwrap_or_default()
        .to_owned();
    if value.get("control_version").is_some() {
        if !control_lane {
            job.permit.observe();
        }
        let request: Control = match serde_json::from_value(value) {
            Ok(request) => request,
            Err(_) => return refuse(s, &mut stream, &id, Code::Invalid),
        };
        return handle_control(s, job, &request);
    }
    if control_lane {
        return refuse(s, &mut stream, "", Code::UnknownAction);
    }
    handle_product(s, job, value, &id)
}

fn handle_product<H: Handler>(s: &Shared<H>, job: &mut Job, value: Value, id: &str) -> Result<()> {
    let mut stream = &job.stream;
    let opening = value.get("connection").is_some();
    let decoded = if opening {
        serde_json::from_value::<protocol::ConnectionRequest>(value).map(|v| v.connection)
    } else {
        serde_json::from_value::<Request>(value)
    };
    let Ok(request) = decoded else {
        return refuse(s, &mut stream, id, Code::Invalid);
    };
    if !protocol::valid_id(&request.request_id) {
        return refuse(s, &mut stream, "", Code::Invalid);
    }
    if s.identity
        .require(request.protocol, &request.requires)
        .is_err()
        || (opening
            && !s
                .identity
                .capabilities
                .iter()
                .any(|c| c == protocol::CONNECTION_HANDOFF))
    {
        return refuse(s, &mut stream, &request.request_id, Code::Incompatible);
    }
    if s.handler.observer(&request) {
        if opening {
            job.permit.observe();
            return refuse(s, &mut stream, &request.request_id, Code::Invalid);
        }
        job.permit.observe();
    } else {
        job.permit.activate();
    }
    let slot = if opening {
        let Some(slot) = s.connections.reserve() else {
            return refuse(s, &mut stream, &request.request_id, Code::Capacity);
        };
        Some(slot)
    } else {
        None
    };
    send(
        s,
        &mut stream,
        &response(s, &request.request_id, Some(true), None, Value::Null),
        Instant::now() + s.options.io_timeout,
    )?;
    if let Some(slot) = slot {
        return handoff(s, job, &request, slot);
    }
    let result = s.handler.call(
        &request,
        &Context {
            permit: &job.permit,
            stream: &job.stream,
        },
    );
    let (code, data) = outcome(result);
    send(
        s,
        &mut stream,
        &response(s, &request.request_id, None, code, data),
        Instant::now() + s.options.io_timeout,
    )
}

fn handoff<H: Handler>(
    s: &Shared<H>,
    job: &Job,
    request: &Request,
    slot: connection::Slot,
) -> Result<()> {
    let connection = Connection::new(job.stream.try_clone()?, job.permit.hold()?, slot);
    let prepared = s.handler.open(
        request,
        &Context {
            permit: &job.permit,
            stream: &job.stream,
        },
    );
    let (code, data, receive) = match prepared {
        Ok(handoff) => (None, handoff.reply, Some(handoff.receive)),
        Err(error) => (Some(Code::Product), error.0, None),
    };
    let reply = response(s, &request.request_id, None, code, data);
    // Oversized results get a bounded error, never a successful handoff.
    let fits = frame::encode_json(&reply, s.options.frame_limit).is_ok();
    send(
        s,
        &mut &job.stream,
        &reply,
        Instant::now() + s.options.io_timeout,
    )?;
    if fits && let Some(receive) = receive {
        connection::clear_deadlines(connection.stream())?;
        receive(connection);
    }
    Ok(())
}

fn outcome(result: std::result::Result<Value, protocol::ProductError>) -> (Option<Code>, Value) {
    match result {
        Ok(v) => (None, v),
        Err(v) => (Some(Code::Product), v.0),
    }
}

fn handle_control<H: Handler>(s: &Shared<H>, job: &mut Job, r: &Control) -> Result<()> {
    let id = &r.request_id;
    let built_in = matches!(
        r.action.as_str(),
        "identity" | "status" | "retire_if_empty" | "retire_if_older" | "drain" | "force_stop"
    );
    let refusal = if r.control_version != 1
        || (r.action == "force_stop" && !s.options.allow_force_stop)
        || r.requires
            .iter()
            .any(|c| !s.identity.capabilities.contains(c))
    {
        Some(Code::Incompatible)
    } else if !protocol::valid_id(id) {
        Some(Code::Invalid)
    } else if !built_in && !s.handler.extensions().contains(&r.action.as_str()) {
        Some(Code::UnknownAction)
    } else if matches!(
        r.action.as_str(),
        "retire_if_empty" | "retire_if_older" | "drain" | "force_stop"
    ) && r.manager_id.as_ref() != Some(&s.identity.manager_id)
    {
        Some(Code::StaleManager)
    } else {
        None
    };
    if let Some(code) = refusal {
        return refuse(s, &mut &job.stream, id, code);
    }
    if r.action == "force_stop" && !r.arguments.is_null() {
        return refuse(s, &mut &job.stream, id, Code::Invalid);
    }
    let newer = if r.action == "retire_if_older" {
        match newer_version(s, r) {
            Ok(newer) => newer,
            Err(code) => return refuse(s, &mut &job.stream, id, code),
        }
    } else {
        false
    };
    if !built_in {
        let role = s.handler.control_role(r);
        if job.permit.classify_control(role).is_err() {
            return refuse(s, &mut &job.stream, id, Code::Draining);
        }
    }
    let mut stream = &job.stream;
    send(
        s,
        &mut stream,
        &response(s, id, Some(true), None, Value::Null),
        Instant::now() + s.options.io_timeout,
    )?;
    if r.action == "force_stop" {
        // This opt-in crash path deliberately skips graceful joins and cleanup.
        // Losing the reply must not cancel a stop already acknowledged.
        let _ = send(
            s,
            &mut stream,
            &response(s, id, None, None, json!({"stopping":true})),
            Instant::now() + s.options.io_timeout,
        );
        std::process::exit(75);
    }
    let (code, data) = outcome(dispatch_control(s, job, r, newer));
    send(
        s,
        &mut stream,
        &response(s, id, None, code, data),
        Instant::now() + s.options.io_timeout,
    )
}

fn newer_version<H>(s: &Shared<H>, r: &Control) -> std::result::Result<bool, Code> {
    let arguments = r.arguments.as_object().ok_or(Code::Invalid)?;
    if arguments.len() != 1 {
        return Err(Code::Invalid);
    }
    let candidate = arguments
        .get("version")
        .and_then(Value::as_str)
        .ok_or(Code::Invalid)?;
    let candidate = Version::parse(candidate).map_err(|_| Code::InvalidVersion)?;
    let running = s.version.as_ref().ok_or(Code::InvalidVersion)?;
    Ok(candidate.is_newer_than(running))
}

fn dispatch_control<H: Handler>(
    s: &Shared<H>,
    job: &Job,
    r: &Control,
    newer: bool,
) -> std::result::Result<Value, protocol::ProductError> {
    match r.action.as_str() {
        "identity" => {
            let mut value = json!(s.identity);
            value["lifecycle"] = json!(s.gate.snapshot().state);
            Ok(value)
        }
        "status" => Ok(
            json!({"identity":s.identity,"lifetime":s.gate.snapshot(),"blockers":s.handler.blockers(),"connections":{"ordinary":s.ordinary_count.load(Ordering::SeqCst),"control":s.control_count.load(Ordering::SeqCst),"transferred":s.connections.used()}}),
        ),
        "retire_if_empty" => {
            let retired = s.gate.retire_if_empty();
            Ok(
                json!({"retired":retired,"blockers":if retired { Value::Null } else { s.handler.blockers() }}),
            )
        }
        "retire_if_older" => {
            let retired = newer && s.gate.retire_when_empty();
            Ok(
                json!({"newer":newer,"retired":retired,"retirement_pending":s.gate.snapshot().retirement_pending,"blockers":if newer && !retired { s.handler.blockers() } else { Value::Null }}),
            )
        }
        "drain" => {
            if s.gate.drain() {
                s.handler.drain();
            }
            Ok(json!({"draining":true}))
        }
        _ => s.handler.control(
            r,
            &Context {
                permit: &job.permit,
                stream: &job.stream,
            },
        ),
    }
}

struct Deadline<'a> {
    stream: &'a Stream,
    until: Instant,
}
impl std::io::Read for Deadline<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self.until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(buffer)
    }
}

impl std::io::Write for Deadline<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let remaining = self.until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_write_timeout(Some(remaining))?;
        self.stream.write(buffer)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Backpressured refusal writes on the accept-thread enqueue path.

    use super::*;
    use std::{io::Write, net::Shutdown};

    struct Unused;
    impl Handler for Unused {
        fn call(
            &self,
            _: &Request,
            _: &Context<'_>,
        ) -> std::result::Result<Value, protocol::ProductError> {
            Err(protocol::ProductError(Value::Null))
        }
    }

    fn backpressured_pair() -> (Stream, Stream) {
        let (mut stream, peer) = Stream::pair().unwrap();
        stream.set_nonblocking(true).unwrap();
        loop {
            match stream.write(&[0; 4096]) {
                Ok(0) => panic!("fixture socket unexpectedly closed"),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("fixture socket: {error}"),
            }
        }
        stream.set_nonblocking(false).unwrap();
        (stream, peer)
    }

    #[test]
    fn accept_thread_refusals_keep_the_short_deadline_under_backpressure() {
        for reason in ["capacity", "draining", "closed-control", "closed-queue"] {
            let gate = Admission::new();
            gate.ready();
            if matches!(reason, "draining" | "closed-control") {
                assert!(gate.drain());
            }
            let shared = Shared {
                identity: Identity::new("1.0.0".into(), 1, vec![]).unwrap(),
                version: None,
                gate,
                handler: Arc::new(Unused),
                options: Options::default(),
                ordinary_count: Arc::new(AtomicUsize::new(0)),
                control_count: Arc::new(AtomicUsize::new(0)),
                connections: Slots::new(0),
            };
            let (sender, receiver) = mpsc::sync_channel(1);
            drop(receiver);
            let pool = Pool {
                sender: (reason == "closed-queue").then_some(sender),
                workers: vec![],
                count: Arc::new(AtomicUsize::new(usize::from(reason == "capacity"))),
                capacity: 1,
            };
            let (stream, peer) = backpressured_pair();
            let (finished, completion) = mpsc::sync_channel(1);
            let enqueue = std::thread::spawn(move || {
                pool.enqueue(&shared, stream, reason == "closed-control");
                finished.send(()).unwrap();
            });
            let bounded = completion.recv_timeout(Duration::from_millis(250)).is_ok();
            // Release the peer and join the writer before reporting a timeout.
            let shutdown = peer.shutdown(Shutdown::Both);
            drop(peer);
            enqueue.join().unwrap();
            if let Err(error) = shutdown {
                assert_eq!(error.kind(), std::io::ErrorKind::NotConnected);
            }
            assert!(
                bounded,
                "{reason} refusal exceeded the accept-thread budget"
            );
        }
    }
}
