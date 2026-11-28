//! The manager process on `canonic-process-kit`: the data root's lease
//! and sockets, peer checks, framing, admission, retirement and drain come
//! from the library. Testrun supplies request routing, the control
//! extensions, MCP attachments and the busy hold that mirrors its own
//! emptiness decision.
use super::{BUILD_ID, service::Service, storage, wire};
use crate::error::{Error, Result};
use canonic_process_kit::{
    self as runtime,
    connection::{Connection, Handoff},
    core::{Admission, ControlRole, Event, ExitReason, IdleGrace},
    endpoint::{Lease, Root},
    net::Stream,
    protocol::{Control, Identity, ProductError, Request},
    server::{self, Context, Handler},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};

/// Product controls every manager of this protocol answers on the control lane.
pub(crate) const EXTENSIONS: &[&str] = &[
    "pool",
    "blockers",
    "retry",
    "operation",
    "inspect",
    "task_end",
];

/// How long a starting manager waits for an exiting predecessor's lease.
const LEASE_WAIT: Duration = Duration::from_secs(15);

/// Library limits sized to Testrun's own admission bounds: 64 calls, 32
/// reads and 8 inspections, plus room for attachment handshakes.
fn options() -> server::Options {
    server::Options {
        workers: 112,
        capacity: 128,
        control_workers: 9,
        control_capacity: 32,
        connection_capacity: 256,
        frame_limit: wire::MAX_REPLY_FRAME,
        io_timeout: Duration::from_secs(5),
        allow_force_stop: false,
    }
}

/// The library root for a Testrun data root.
pub(crate) fn root(path: &Path) -> Result<Root> {
    Root::explicit(path).map_err(|error| failure(&error))
}

pub(crate) fn absent() -> Error {
    Error::new("OWNER_ABSENT", "The manager is not running")
}

pub(crate) fn draining() -> Error {
    let mut error = Error::new(
        "OWNER_DRAINING",
        "The manager is draining: it finishes its work, releases its devices and retires",
    )
    .details(json!({"admitted":false,"nextAction":"status"}));
    error.retryable = true;
    error
}

pub(crate) fn overloaded() -> Error {
    let mut error = Error::new("OVERLOADED", "The manager's connection capacity is full")
        .details(json!({"admitted":false,"dispatched":false}));
    error.retryable = true;
    error
}

pub(crate) fn unavailable() -> Error {
    let mut error = Error::new(
        "OWNER_UNAVAILABLE",
        "The manager could not be reached; check its log in the data root",
    )
    .details(json!({"retryable":true}));
    error.retryable = true;
    error
}

/// The manager this client started exited before it was ready; its log in
/// the data root says why.
pub(crate) fn exited(status: std::process::ExitStatus, log: &Path) -> Error {
    Error::new(
        "OWNER_FAILED",
        format!("The manager exited before it was ready ({status}); see its log"),
    )
    .details(json!({"log":log}))
}

pub(crate) fn outcome_unknown(request_id: &Value) -> Error {
    Error::new(
        "OUTCOME_UNKNOWN",
        "The manager admitted this request but its reply was lost; check status before repeating it",
    )
    .details(json!({"requestId":request_id}))
}

/// The refusal for an inspection naming capabilities this manager lacks.
fn missing_capabilities(requires: &Value) -> Option<Error> {
    let missing: Vec<&str> = requires
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|capability| !wire::CAPABILITIES.contains(capability))
        .collect();
    (!missing.is_empty()).then(|| {
        Error::new(
            "OWNER_INCOMPATIBLE",
            "The running manager lacks a capability this request needs; drain it with testrun manager drain, then retry",
        )
        .details(json!({"admitted":false,"missingCapabilities":missing}))
    })
}

/// A library failure as a Testrun error. Busy, starting and replaced
/// managers are retryable; only malformed exchanges are protocol errors.
pub(crate) fn failure(error: &runtime::Error) -> Error {
    match error {
        runtime::Error::Absent => absent(),
        runtime::Error::Draining => draining(),
        runtime::Error::Capacity => overloaded(),
        runtime::Error::Starting | runtime::Error::StaleManager => unavailable(),
        runtime::Error::OutcomeUnknown(id) => outcome_unknown(&json!(id)),
        runtime::Error::Incompatible => Error::new(
            "OWNER_INCOMPATIBLE",
            "The running manager uses another protocol; retire it when empty or drain it",
        )
        .details(json!({"admitted":false})),
        runtime::Error::Unsupported => Error::new(
            "UNSUPPORTED_HOST",
            "The Testrun manager requires macOS, Linux or Windows",
        ),
        runtime::Error::Peer | runtime::Error::PrivatePath => Error::new(
            "UNAUTHORIZED",
            "The manager's data root or socket is not private to this user",
        ),
        runtime::Error::PathTooLong => Error::new(
            "INVALID_ARGUMENT",
            "The data root path is too long for a manager socket",
        ),
        runtime::Error::AlreadyRunning => {
            Error::new("OWNER_RUNNING", "Another manager serves this data root")
        }
        runtime::Error::Io(_) => manager_io_error(),
        #[cfg(unix)]
        runtime::Error::Unix(_) => manager_io_error(),
        _ => Error::new("OWNER_PROTOCOL", format!("Manager failure: {error}")),
    }
}

fn manager_io_error() -> Error {
    Error::new(
        "HOST_ACCESS_REQUIRED",
        "Cannot reach the manager socket; grant this command local socket access",
    )
    .details(json!({"requiresHostAccess":true}))
}

/// Serve the manager for `root` until it exits or is signalled.
pub(crate) async fn serve(root: &Path) -> Result<Value> {
    storage::directory(root)?;
    let library_root = self::root(root)?;
    let lease = tokio::task::spawn_blocking({
        let library_root = library_root.clone();
        move || Lease::acquire(&library_root, LEASE_WAIT)
    })
    .await
    .map_err(|_| Error::new("INTERNAL", "The lease wait stopped"))?
    .map_err(|error| failure(&error))?;
    let service = Service::new(root.to_owned(), super::supervisor::Launcher::Product);
    storage::write(&root.join("owner.json"), &service.identity())?;
    run(service, lease, None).await
}

/// Run `service` under the library until it exits; a signal runs the
/// manager's best-effort exit cleanup first. `signal` replaces the process
/// signals in fixtures.
pub(crate) async fn run(
    service: Arc<Service>,
    lease: Lease,
    signal: Option<tokio::sync::oneshot::Receiver<()>>,
) -> Result<Value> {
    run_with(service, lease, signal, Admission::new()).await
}
/// `run` on a given admission gate, so a fixture can hold the manager
/// before it serves.
async fn run_with(
    service: Arc<Service>,
    lease: Lease,
    signal: Option<tokio::sync::oneshot::Receiver<()>>,
    gate: Admission,
) -> Result<Value> {
    service.host(gate.clone());
    // Settling what a lost manager left, such as shutting down a simulator
    // it booted, holds the manager until it finishes.
    let settling = service.clone();
    let hold = service.hold();
    super::service::detached(move || async move {
        super::transition::settle(settling).await;
        drop(hold);
    });
    let identity = Identity::new(
        BUILD_ID.into(),
        service.protocol(),
        wire::CAPABILITIES.iter().map(|&c| c.to_owned()).collect(),
    )
    .map_err(|error| failure(&error))?;
    let handler = Arc::new(Host {
        service: service.clone(),
        runtime: tokio::runtime::Handle::current(),
    });
    let policy = IdleGrace(service.idle_grace());
    // A dedicated thread, not the blocking pool: the library's poll loop
    // runs for the manager's whole life and must never hold up a runtime
    // shutdown.
    let (served, serving) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("testrun-manager".into())
        .spawn({
            let gate = gate.clone();
            move || {
                let _ = served.send(server::serve_with_request_limit(
                    lease,
                    identity,
                    gate,
                    handler,
                    &policy,
                    options(),
                    wire::MAX_REQUEST_FRAME,
                ));
            }
        })
        .map_err(|_| Error::new("HOST_IO", "Cannot start the manager thread"))?;
    tokio::pin!(serving);
    let lifetime = tokio::spawn({
        let service = service.clone();
        async move { service.keep_alive().await }
    });
    let signalled = async {
        match signal {
            // A fixture that drops its sender never signals.
            Some(signal) => {
                if signal.await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
            None => termination().await,
        }
    };
    let outcome = tokio::select! {
        served = &mut serving => Ok(served),
        () = signalled => Err(()),
    };
    match outcome {
        Ok(served) => {
            lifetime.abort();
            served
                .map_err(|_| Error::new("INTERNAL", "The manager stopped unexpectedly"))?
                .map_err(|error| failure(&error))?;
            service.closed();
            Ok(json!({"closed":true,"reason":reason(&gate)}))
        }
        Err(()) => {
            // Close admission first, so nothing new starts during cleanup;
            // status and the other controls keep answering.
            if gate.drain() {
                service.drain();
            }
            let cleanup = service.terminate().await;
            lifetime.abort();
            // Cleanup released what it could; let admitted work finish and
            // the library close its sockets, within a bound.
            service.release_hold();
            let _ = tokio::time::timeout(Duration::from_secs(10), &mut serving).await;
            cleanup.map(|()| json!({"closed":true,"reason":"signal"}))
        }
    }
}

fn reason(gate: &Admission) -> &'static str {
    match gate.events().iter().rev().find_map(|event| match event {
        Event::Exit(reason) => Some(*reason),
        _ => None,
    }) {
        Some(ExitReason::Retired) => "retired",
        Some(ExitReason::Drained) => "drained",
        _ => "empty",
    }
}

#[cfg(unix)]
async fn termination() {
    let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(windows)]
async fn termination() {
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// Testrun's request routing under the library's handler contract. Callbacks
/// run on library worker threads and drive the manager's Tokio runtime.
struct Host {
    service: Arc<Service>,
    runtime: tokio::runtime::Handle,
}

fn product(result: Result<Value>) -> std::result::Result<Value, ProductError> {
    result.map_err(|error| ProductError(json!(error)))
}

impl Handler for Host {
    fn call(
        &self,
        request: &Request,
        context: &Context,
    ) -> std::result::Result<Value, ProductError> {
        let watch = context.stream.try_clone().ok();
        let service = self.service.clone();
        let payload = request.payload.clone();
        // The request's work holds the manager until it ends, even when the
        // client abandons it.
        let hold = context.permit.hold().ok();
        let result = self.runtime.block_on(async move {
            let work = service.offload(payload, hold);
            tokio::pin!(work);
            tokio::select! {
                result = &mut work => result,
                // A client that disconnects abandons its request: queued
                // work is never dispatched; dispatched work keeps its owner.
                () = peer_closed(watch) => Err(Error::new("CANCELLED", "The client abandoned the request")),
            }
        });
        product(result)
    }

    fn extensions(&self) -> &[&str] {
        EXTENSIONS
    }

    /// Retry and build cancellation settle existing obligations, so they are
    /// cleanup: admitted while a drain still has work, and able to hold the
    /// manager. Everything else on the control lane only reads.
    fn control_role(&self, control: &Control) -> ControlRole {
        if control.action == "retry"
            || (control.action == "inspect" && control.arguments["name"] == "build_cancel")
            || (control.action == "inspect"
                && matches!(
                    control.arguments["name"].as_str(),
                    Some("diagnostics_stop" | "diagnostics_analysis_cancel")
                ))
        {
            ControlRole::Cleanup
        } else {
            ControlRole::Inspection
        }
    }

    fn control(
        &self,
        control: &Control,
        context: &Context,
    ) -> std::result::Result<Value, ProductError> {
        // Inspection carries a product request of the client's protocol.
        if control.action == "inspect"
            && control.arguments["protocol"].as_u64() != Some(u64::from(self.service.protocol()))
        {
            return Err(ProductError(json!(failure(&runtime::Error::Incompatible))));
        }
        // Admission negotiates capabilities in the library; the control lane
        // checks the ones an inspection names itself.
        if control.action == "inspect"
            && let Some(missing) = missing_capabilities(&control.arguments["requires"])
        {
            return Err(ProductError(json!(missing)));
        }
        let service = self.service.clone();
        let action = control.action.clone();
        let arguments = control.arguments.clone();
        // Inspection permits take no hold; cleanup carries one for work that
        // outlasts its reply.
        let hold = context.permit.hold().ok();
        let result = self.runtime.block_on(async move {
            match action.as_str() {
                "inspect" => service.offload(arguments, hold).await,
                "task_end" => Ok(service.task_end(&arguments)),
                kind => service.control(kind, &arguments, hold).await,
            }
        });
        product(result)
    }

    fn open(
        &self,
        request: &Request,
        _context: &Context,
    ) -> std::result::Result<Handoff, ProductError> {
        if request.payload["kind"] != "attach"
            || request.payload["client"]["connection"]
                .as_str()
                .is_none_or(str::is_empty)
        {
            return Err(ProductError(json!(Error::invalid(
                "An attachment needs its connection"
            ))));
        }
        let service = self.service.clone();
        let runtime = self.runtime.clone();
        Ok(Handoff::new(
            json!({"incarnation":self.service.incarnation}),
            move |connection: Connection| {
                runtime.spawn(service.attach(connection));
            },
        ))
    }

    fn blockers(&self) -> Value {
        self.service.holders()
    }

    fn drain(&self) {
        self.service.drain();
    }
}

/// Resolves when the peer closes `stream`, without reading it or changing
/// its descriptor, so the library can still write a reply on it. Clients
/// send nothing after a request or attachment; unexpected bytes never count
/// as a close.
#[cfg(unix)]
pub(crate) async fn peer_closed(stream: Option<Stream>) {
    use std::os::fd::AsRawFd;
    let Some(fd) = stream.and_then(|stream| {
        // SAFETY: `stream` owns its descriptor, a duplicate made for this
        // watch; it stays open and unchanged until the `AsyncFd` drops it.
        unsafe {
            tokio::io::unix::AsyncFd::register_with_interest(stream, tokio::io::Interest::READABLE)
        }
        .ok()
    }) else {
        return std::future::pending().await;
    };
    loop {
        let Ok(mut ready) = fd.readable().await else {
            return;
        };
        let mut byte = [0_u8; 1];
        // SAFETY: a nonblocking peek of one byte into a local buffer on a
        // descriptor this future owns; it consumes nothing.
        let read = unsafe {
            libc::recv(
                fd.get_ref().as_raw_fd(),
                byte.as_mut_ptr().cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        match read {
            0 => return,
            n if n > 0 => return std::future::pending().await,
            _ if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock => {
                ready.clear_ready();
            }
            _ => return,
        }
    }
}

/// Observe Windows peer closure on the same silent request channel.
#[cfg(windows)]
pub(crate) async fn peer_closed(stream: Option<Stream>) {
    if let Some(stream) = stream {
        let mut idle = crate::windows_ipc::IdleBackoff::default();
        loop {
            match runtime::net::wait_peer_closed(&stream, Duration::ZERO) {
                Ok(true) => return,
                Ok(false) => tokio::time::sleep(idle.next_delay()).await,
                Err(runtime::Error::Envelope) => break,
                Err(_) => return,
            }
        }
    }
    std::future::pending().await
}

/// A fixture manager on `root`, served through the library and returned
/// once its socket exists: a client that calls earlier finds no manager.
#[cfg(test)]
pub(crate) async fn fixture(
    service: Arc<Service>,
) -> (
    tokio::task::JoinHandle<Result<Value>>,
    tokio::sync::oneshot::Sender<()>,
) {
    start(service, Admission::new()).await
}
/// A fixture manager held past its idle grace until the returned hold
/// drops: it cannot retire before a test's first calls reach it.
#[cfg(all(test, unix))]
pub(crate) async fn fixture_held(
    service: Arc<Service>,
) -> (
    tokio::task::JoinHandle<Result<Value>>,
    tokio::sync::oneshot::Sender<()>,
    runtime::core::Hold,
) {
    let gate = Admission::new();
    let hold = gate.hold().expect("A new manager admits holds");
    let (running, stop) = start(service, gate).await;
    (running, stop, hold)
}
#[cfg(test)]
async fn start(
    service: Arc<Service>,
    gate: Admission,
) -> (
    tokio::task::JoinHandle<Result<Value>>,
    tokio::sync::oneshot::Sender<()>,
) {
    let library_root = root(&service.root).unwrap();
    let lease = Lease::acquire(&library_root, Duration::ZERO).unwrap();
    let (stop, signal) = tokio::sync::oneshot::channel();
    let root = service.root.clone();
    let running = tokio::spawn(run_with(service, lease, Some(signal), gate));
    wait_for_fixture_socket(&root, &running).await;
    (running, stop)
}

#[cfg(test)]
pub(crate) async fn wait_for_fixture_socket<T>(root: &Path, running: &tokio::task::JoinHandle<T>) {
    let socket = socket(root);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !socket.exists() {
        assert!(
            tokio::time::Instant::now() < deadline && !running.is_finished(),
            "The fixture manager did not start serving"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[cfg(test)]
pub(crate) fn socket(root: &Path) -> std::path::PathBuf {
    root.join("manager.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspection_naming_a_capability_the_manager_lacks_is_refused_before_dispatch() {
        assert!(missing_capabilities(&json!(["diagnostic_sessions_v1"])).is_none());
        assert!(missing_capabilities(&Value::Null).is_none());
        let error =
            missing_capabilities(&json!(["diagnostic_sessions_v1", "acme_future_v9"])).unwrap();
        assert_eq!(error.code, "OWNER_INCOMPATIBLE");
        let details = error.details.unwrap();
        assert_eq!(details["missingCapabilities"], json!(["acme_future_v9"]));
        assert_eq!(details["admitted"], false);
    }
}
