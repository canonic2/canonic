//! Supervised Firefox/WebKit session operations and retained lifecycle ownership.
//!
//! This is the library composition boundary, not another MCP server. Callers
//! supply the trusted session incarnation and an inspected private driver bundle.

use super::{
    connection::{self, Connection},
    contract::{self, CallbackResponse, LaunchOptions, Reference, Request},
    driver::{DiagnosticBundle, Driver},
    events::Events,
    journal::Journal,
    lifecycle::{Kind, Operation, Outcome, Phase, State},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex as AsyncMutex, Notify, Semaphore},
    task::JoinHandle,
    time::Instant,
};
use uuid::Uuid;

/// The lock's data even when a panicking holder poisoned it. Only paths that
/// settle, cancel or stage an operation, fence the session, read status or
/// release it (disconnect and staged shutdown with its close helpers) use
/// this, so a panic still ends in a settled operation, a readable lost session
/// and a released driver and browser; ordinary work keeps refusing poisoned
/// state.
fn recovered<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

mod automation_debugger;
mod bindings;
mod capture;
mod compound_input;
mod conditions;
pub(crate) mod control;
mod developer_tools;
mod diagnostic_arguments;
mod diagnostics;
mod directories;
mod evaluation;
mod event_waits;
mod events;
mod evidence;
mod failure;
mod framework;
mod har;
mod har_capture;
mod http;
mod input;
mod network;
mod page;
mod preview;
mod read_url;
mod recording;
mod routing;
mod settings;
mod socket_routes;
mod storage;
mod styles;
mod tracing;
mod transfer;
mod trusted_sign_in;
mod workflows;

#[derive(Debug, thiserror::Error)]
#[error("Browser lifecycle request refused: {code}")]
pub struct Error {
    pub code: &'static str,
    pub(crate) partial: Option<Value>,
    #[source]
    cause: Option<Box<dyn std::error::Error + Send + Sync>>,
}
impl Error {
    pub(crate) fn caused(
        code: &'static str,
        cause: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            code,
            partial: None,
            cause: Some(Box::new(cause)),
        }
    }

    pub(crate) fn with_batch_result(mut self, results: Vec<Value>, cleanup: Option<Value>) -> Self {
        let mut receipt = json!({"completedActions":results.len(),"results":results,"failedIndex":results.len(),"partial":self.partial.take(),"replayed":false});
        if let Some(cleanup) = cleanup {
            receipt["cleanup"] = cleanup;
        }
        if self.code == "AMBIGUOUS_LOCATOR" {
            receipt["hint"] = json!(
                "Narrow the failed step to one element with an exact name, a test ID, or a more specific selector."
            );
        }
        self.partial = Some(receipt);
        self
    }
}
impl From<&'static str> for Error {
    fn from(code: &'static str) -> Self {
        Self {
            code,
            cause: None,
            partial: None,
        }
    }
}
impl From<connection::Error> for Error {
    fn from(error: connection::Error) -> Self {
        let code = match &error {
            connection::Error::Timeout {
                dispatch: connection::Dispatch::NotDispatched,
            } => "NOT_DISPATCHED",
            connection::Error::Lost {
                dispatch: connection::Dispatch::NotDispatched,
            } => "BACKEND_LOST",
            connection::Error::Timeout { .. } | connection::Error::Lost { .. } => "OUTCOME_UNKNOWN",
            connection::Error::Busy => "BACKEND_BUSY",
            connection::Error::Remote(error)
                if error.error["error"]["name"] == "TargetClosedError" =>
            {
                "STALE_REFERENCE"
            }
            connection::Error::Remote(error) if error.error["error"]["name"] == "TimeoutError" => {
                "OUTCOME_UNKNOWN"
            }
            connection::Error::Remote(error)
                if error.error["error"]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("canonic:STALE_REFERENCE")) =>
            {
                "STALE_REFERENCE"
            }
            connection::Error::Remote(error)
                if error.error["error"]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("strict mode violation")) =>
            {
                "AMBIGUOUS_LOCATOR"
            }
            connection::Error::Remote(_) => "BROWSER_METHOD_FAILED",
            _ => "BACKEND_PROTOCOL",
        };
        Self::caused(code, error)
    }
}

struct Client {
    host: Arc<Host>,
    startup: Uuid,
}
#[derive(Clone)]
pub struct Session(Arc<Client>);

pub(super) enum BackendStart {
    Launch(DiagnosticBundle),
    Attach(super::attachment::Attachment),
}

struct Host {
    state: Mutex<State>,
    changed: Notify,
    driver: AsyncMutex<Option<Driver>>,
    connection: Mutex<Option<Connection>>,
    journal: Journal,
    lane: AsyncMutex<()>,
    /// The hard ceiling on one running operation, in milliseconds. A
    /// dispatched driver call that outlives it settles as unknown and fences
    /// the session.
    ceiling_ms: AtomicU64,
    capacity: Arc<Semaphore>,
    responsive: Arc<Semaphore>,
    stopping: AtomicBool,
    cancelling: Mutex<BTreeSet<Uuid>>,
    callbacks: Mutex<BTreeMap<Reference, Instant>>,
    responding: Mutex<BTreeSet<Reference>>,
    events: AsyncMutex<Option<JoinHandle<()>>>,
    cleanup: AsyncMutex<()>,
    lease: Mutex<Option<crate::lease::ControlLease>>,
    profile: Mutex<Option<super::profiles::Profile>>,
    persistent_context: Mutex<Option<Reference>>,
    attached: AtomicBool,
    private_failures: Mutex<VecDeque<String>>,
    capture_store: Mutex<crate::browser::capture::Store>,
    transfers: Mutex<super::transfer::Transfers>,
    transfer_tasks: AsyncMutex<Vec<JoinHandle<()>>>,
    diagnostics: Mutex<super::diagnostics::Diagnostics>,
    argument_groups: Mutex<diagnostic_arguments::Arguments>,
    network_evidence: Mutex<super::network::Evidence>,
    trace: Mutex<Option<Arc<tracing::Trace>>>,
    trace_evidence: Mutex<super::artifacts::Store>,
    automation_debuggers: Mutex<automation_debugger::Debuggers>,
    har_archives: Mutex<super::har::Store>,
    routing: routing::Runtime,
    http_contexts: Mutex<super::http::Contexts>,
    request_inputs: Mutex<super::http::request_inputs::store::Store>,
    http_callers: Mutex<BTreeSet<Uuid>>,
    http_browser_user_agent: tokio::sync::OnceCell<String>,
    developer_tools: Mutex<super::developer_tools::Registries>,
    frameworks: Mutex<super::framework::Registries>,
    init_scripts: Mutex<super::scripts::Scripts<crate::browser::objects::ObjectRef>>,
    bindings: Mutex<super::bindings::Registrations<crate::browser::objects::ObjectRef>>,
    binding_calls: Mutex<BTreeMap<Reference, Uuid>>,
    socket_routes: super::socket_routes::Shared,
    activity: Mutex<super::activity::History>,
    previews: Mutex<super::preview::Sources>,
    media: AsyncMutex<Option<crate::media::Media>>,
    frames_task: AsyncMutex<Option<JoinHandle<()>>>,
    preview_lane: AsyncMutex<()>,
    recordings: Mutex<BTreeMap<Reference, Arc<super::recording::Recording>>>,
    inputs: Mutex<super::input::State>,
    input_epoch: AtomicU64,
    authority: Arc<crate::control::Authority>,
    input_credentials: Mutex<BTreeMap<Uuid, crate::control::Credential>>,
    viewer: Mutex<Option<Arc<crate::control::channel::Channel>>>,
    viewer_page: Mutex<Option<Reference>>,
    control_lane: AsyncMutex<()>,
    control_tasks: Mutex<Vec<JoinHandle<()>>>,
    control_pending: AtomicBool,
    control_outcome: Mutex<Value>,
    storage: Mutex<super::storage::Store>,
}

impl Session {
    /// Starts one retained operation. Dropping a wait or caller cannot drop an
    /// allocated browser; the supervisor owns it until verified shutdown.
    pub fn launch(
        bundle: DiagnosticBundle,
        owner: Uuid,
        owner_generation: Uuid,
        options: LaunchOptions,
        private_state: &Path,
    ) -> Result<Self, Error> {
        Self::launch_connection(
            bundle,
            Uuid::new_v4(),
            owner,
            owner_generation,
            options,
            private_state,
        )
    }
    /// Starts browser connection `connection` of a session incarnation; one
    /// incarnation can hold several connections, each with its own lease.
    pub fn launch_connection(
        bundle: DiagnosticBundle,
        connection: Uuid,
        owner: Uuid,
        owner_generation: Uuid,
        options: LaunchOptions,
        private_state: &Path,
    ) -> Result<Self, Error> {
        Self::launch_selected(
            BackendStart::Launch(bundle),
            connection,
            owner,
            owner_generation,
            options,
            None,
            private_state,
        )
    }
    pub(super) fn launch_selected(
        bundle: BackendStart,
        connection: Uuid,
        owner: Uuid,
        owner_generation: Uuid,
        options: LaunchOptions,
        profile: Option<super::profiles::Profile>,
        private_state: &Path,
    ) -> Result<Self, Error> {
        if options.engine.backend() != contract::Backend::Playwright {
            return Err("BACKEND_UNAVAILABLE".into());
        }
        options.validate()?;
        let windows_supported = super::processes::windows_bundle_host()
            && (matches!(&bundle, BackendStart::Attach(_))
                || super::processes::windows_mode_supported(options.mode));
        if !super::processes::initial_host() && !windows_supported {
            return Err("HOST_UNAVAILABLE".into());
        }
        if matches!(&bundle, BackendStart::Launch(bundle) if !super::preparation::matches_node_identity(bundle))
        {
            return Err("RUNTIME_IDENTITY_MISMATCH".into());
        }
        let state = State::connection(connection, owner, owner_generation, &options);
        let journal = Journal::create(private_state, state.connection)?;
        #[cfg(unix)]
        let lease = Some(
            crate::lease::ControlLease::acquire(&format!(
                "browser-session:{owner}:{owner_generation}:{connection}"
            ))
            .map_err(|error| {
                Error::from(if error.code == "TARGET_BUSY" {
                    "SESSION_TARGET_CONFLICT"
                } else {
                    "LEASE_IO"
                })
            })?,
        );
        // The manager owns this pool and the journal directory was created exclusively.
        #[cfg(windows)]
        let lease = None;
        let host = Arc::new(Host::new(state, journal, lease));
        *host.profile.lock().expect("Profile owner") = profile;
        let startup = Uuid::new_v4();
        host.state
            .lock()
            .expect("Browser owner")
            .begin(startup, "launch".into())?;
        let worker = host.clone();
        tokio::spawn(async move {
            worker.start(bundle, options, startup).await;
        });
        Ok(Self(Arc::new(Client { host, startup })))
    }
    pub fn startup(&self) -> Uuid {
        self.0.startup
    }
    pub fn status(&self) -> Value {
        let mut status = recovered(&self.0.host.state).snapshot();
        if let Some(resources) = status["resources"].as_array_mut() {
            let rules = recovered(&self.0.host.routing.rules);
            let owners = recovered(&self.0.host.routing.owners);
            for resource in resources {
                if let Ok(reference) =
                    serde_json::from_value::<Reference>(resource["reference"].clone())
                    && let Some(scope) = owners.get(&reference)
                {
                    resource["policyScope"] = json!(scope);
                    resource["handlerId"] = json!(rules.handlers.current(reference));
                }
            }
        }
        status["transfers"] = recovered(&self.0.host.transfers).snapshot();
        status["input"] = recovered(&self.0.host.inputs).snapshot();
        status["control"] = self.control_status();
        status["storage"] = recovered(&self.0.host.storage).summary();
        status["diagnostics"] = recovered(&self.0.host.diagnostics).summary();
        status["trace"] = self.0.host.trace_status();
        status["automationDebuggers"] = recovered(&self.0.host.automation_debuggers).summary();
        status["automationDebuggerCleanup"] =
            recovered(&self.0.host.automation_debuggers).cleanup_summary();
        status["previews"] = recovered(&self.0.host.previews).snapshot();
        status["recordings"] = json!(
            recovered(&self.0.host.recordings)
                .values()
                .map(|recording| recording.snapshot())
                .collect::<Vec<_>>()
        );
        status
    }
    pub(crate) fn compact_status(&self, limit: usize) -> Value {
        recovered(&self.0.host.state).compact_status(limit)
    }
    pub(crate) fn pending_operation(&self) -> Option<Uuid> {
        recovered(&self.0.host.state).pending_operation()
    }
    pub fn operation(&self, id: Uuid) -> Option<Operation> {
        recovered(&self.0.host.state).operations.get(&id).cloned()
    }
    pub(crate) fn diagnostic_operation(
        &self,
        id: Uuid,
        request: &Request,
    ) -> crate::error::Result<Value> {
        super::investigation::snapshot(
            &self.0.host.state.lock().expect("Browser owner"),
            id,
            request,
        )
    }
    pub fn journal_directory(&self) -> &Path {
        self.0.host.journal.directory()
    }
    /// Trusted diagnostic access only. These bounded causes may contain page
    /// data and are deliberately absent from status, results and journals.
    pub fn private_failures(&self) -> Vec<String> {
        let mut failures: Vec<_> = self
            .0
            .host
            .private_failures
            .lock()
            .expect("Private diagnostic owner")
            .iter()
            .cloned()
            .collect();
        if let Ok(connection) = self.0.host.connection() {
            failures.push(format!("Connection: {:?}", connection.status()));
        }
        failures
    }

    pub fn submit(&self, id: Uuid, request: Request, timeout_ms: u64) -> Result<Uuid, Error> {
        self.submit_as(id, request, timeout_ms, None)
    }
    fn submit_as(
        &self,
        id: Uuid,
        request: Request,
        timeout_ms: u64,
        viewer: Option<crate::control::Credential>,
    ) -> Result<Uuid, Error> {
        if !(1..=contract::MAX_TIMEOUT_MS).contains(&timeout_ms) {
            return Err("INVALID_TIMEOUT".into());
        }
        workflows::validate(request.syntax_probe().as_ref().unwrap_or(&request))?;
        let bytes = serde_json::to_vec(&request).map_err(|_| Error::from("INVALID_ARGUMENT"))?;
        let fingerprint = format!("{:x}", Sha256::digest(&bytes));
        // A poisoned lock still answers admission: the lost phase refuses.
        let mut state = recovered(&self.0.host.state);
        if let Some(previous) = state.operations.get(&id) {
            return if previous.fingerprint == fingerprint {
                Ok(id)
            } else {
                Err("OPERATION_CONFLICT".into())
            };
        }
        if state.phase != Phase::Ready || self.0.host.stopping.load(Ordering::SeqCst) {
            return Err("SESSION_NOT_READY".into());
        }
        if matches!(
            request,
            Request::Batch { .. } | Request::CallbackResolve { .. }
        ) {
            workflows::validate_engine(&request, state.engine)?;
        }
        let credential = if control::fenced(&request) {
            Some(self.0.host.admit_controller(viewer.as_ref())?)
        } else {
            None
        };
        let responsive = control::responsive(&request);
        let independent = control::independent(&request, responsive);
        let slot = if responsive {
            self.0.host.responsive.clone()
        } else {
            self.0.host.capacity.clone()
        }
        .try_acquire_owned()
        .map_err(|_| Error::from("OPERATION_CAPACITY"))?;
        state.begin(id, fingerprint)?;
        if let Some(credential) = credential {
            self.0
                .host
                .input_credentials
                .lock()
                .expect("Browser input credentials")
                .insert(id, credential);
        }
        if matches!(request, Request::InputRelease {}) {
            self.0.host.input_epoch.fetch_add(1, Ordering::SeqCst);
        }
        let input_epoch = self.0.host.input_epoch.load(Ordering::SeqCst);
        drop(state);
        let host = self.0.host.clone();
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        tokio::spawn(async move {
            let _slot = slot;
            host.perform(id, request, deadline, independent, input_epoch)
                .await;
        });
        Ok(id)
    }
    pub async fn cancel(&self, id: Uuid) -> Result<(), Error> {
        {
            let mut state = recovered(&self.0.host.state);
            let operation = state.operations.get_mut(&id).ok_or("UNKNOWN_OPERATION")?;
            if operation.settled {
                return Ok(());
            }
            operation.cancel_requested = true;
            recovered(&self.0.host.cancelling).insert(id);
        }
        self.0.host.changed.notify_waiters();
        self.0
            .host
            .record(json!({"operationId":id,"cancelRequested":true}))
            .await
    }
    /// Status wait expiration never abandons the supervisor's operation.
    pub async fn wait(&self, id: Uuid, deadline: Instant) -> Result<Operation, Error> {
        loop {
            let notified = self.0.host.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let mut operation = self.operation(id).ok_or("UNKNOWN_OPERATION")?;
            if operation.settled {
                return Ok(operation);
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                operation = self.operation(id).ok_or("UNKNOWN_OPERATION")?;
                if !operation.settled {
                    operation.code = Some("WAIT_EXPIRED".into());
                }
                return Ok(operation);
            }
        }
    }
    pub fn disconnect(&self, timeout_ms: u64) -> Result<Uuid, Error> {
        if !(1..=contract::MAX_TIMEOUT_MS).contains(&timeout_ms) {
            return Err("INVALID_TIMEOUT".into());
        }
        self.0.host.stopping.store(true, Ordering::SeqCst);
        let mut state = recovered(&self.0.host.state);
        if let Some(operation) = state
            .operations
            .values()
            .find(|o| o.fingerprint == "disconnect" && !o.settled)
        {
            return Ok(operation.id);
        }
        let id = Uuid::new_v4();
        state.begin(id, "disconnect".into())?;
        drop(state);
        let host = self.0.host.clone();
        tokio::spawn(async move {
            host.disconnect_operation(id, Instant::now() + Duration::from_millis(timeout_ms))
                .await;
        });
        Ok(id)
    }
}

impl Host {
    fn new(state: State, journal: Journal, lease: Option<crate::lease::ControlLease>) -> Self {
        Self {
            state: Mutex::new(state),
            changed: Notify::new(),
            driver: AsyncMutex::new(None),
            connection: Mutex::new(None),
            journal,
            lane: AsyncMutex::new(()),
            ceiling_ms: AtomicU64::new(contract::MAX_TIMEOUT_MS),
            capacity: Arc::new(Semaphore::new(32)),
            responsive: Arc::new(Semaphore::new(8)),
            stopping: AtomicBool::new(false),
            cancelling: Mutex::new(BTreeSet::new()),
            callbacks: Mutex::new(BTreeMap::new()),
            responding: Mutex::new(BTreeSet::new()),
            events: AsyncMutex::new(None),
            cleanup: AsyncMutex::new(()),
            lease: Mutex::new(lease),
            profile: Mutex::default(),
            persistent_context: Mutex::default(),
            attached: AtomicBool::new(false),
            private_failures: Mutex::new(VecDeque::new()),
            capture_store: Mutex::default(),
            transfers: Mutex::default(),
            transfer_tasks: AsyncMutex::new(Vec::new()),
            diagnostics: Mutex::default(),
            argument_groups: Mutex::default(),
            network_evidence: Mutex::default(),
            trace: Mutex::default(),
            trace_evidence: Mutex::default(),
            automation_debuggers: Mutex::default(),
            har_archives: Mutex::default(),
            routing: Default::default(),
            http_contexts: Mutex::default(),
            request_inputs: Mutex::default(),
            http_callers: Mutex::default(),
            http_browser_user_agent: tokio::sync::OnceCell::new(),
            developer_tools: Mutex::default(),
            frameworks: Mutex::default(),
            init_scripts: Mutex::default(),
            bindings: Mutex::default(),
            socket_routes: Default::default(),
            binding_calls: Mutex::default(),
            activity: Mutex::default(),
            previews: Mutex::default(),
            recordings: Mutex::default(),
            media: AsyncMutex::new(None),
            frames_task: AsyncMutex::new(None),
            preview_lane: AsyncMutex::new(()),
            inputs: Mutex::default(),
            input_epoch: AtomicU64::new(0),
            authority: Arc::new(crate::control::Authority::new(
                Uuid::new_v4().to_string(),
                true,
            )),
            input_credentials: Mutex::default(),
            viewer: Mutex::default(),
            viewer_page: Mutex::default(),
            control_lane: AsyncMutex::new(()),
            control_tasks: Mutex::default(),
            control_pending: AtomicBool::new(false),
            control_outcome: Mutex::new(Value::Null),
            storage: Mutex::default(),
        }
    }
    fn connection(&self) -> Result<Connection, Error> {
        self.connection
            .lock()
            .expect("Connection owner")
            .clone()
            .ok_or_else(|| "BACKEND_UNAVAILABLE".into())
    }
    fn wire(&self, reference: Reference, kind: Kind) -> Result<super::objects::ObjectRef, Error> {
        Ok(self
            .state
            .lock()
            .expect("Browser owner")
            .resource(reference, Some(kind))?
            .wire
            .clone())
    }
    fn set_failure(&self, code: &str) {
        let mut state = recovered(&self.state);
        if !matches!(
            state.phase,
            Phase::Closing | Phase::Closed | Phase::CleanupPending
        ) {
            state.phase = Phase::Lost;
        }
        state.failure = Some(code.into());
        self.authority.close();
        self.changed.notify_waiters();
    }
    async fn record(&self, value: Value) -> Result<(), Error> {
        self.journal.record(value).await.map_err(Error::from)
    }
    async fn stage(&self, id: Uuid, outcome: Outcome) -> Result<(), Error> {
        self.record(json!({"operationId":id,"outcome":outcome,"settled":false}))
            .await?;
        if let Some(operation) = recovered(&self.state).operations.get_mut(&id) {
            operation.outcome = outcome;
        }
        self.changed.notify_waiters();
        Ok(())
    }
    fn remember_failure(&self, error: &Error) {
        let mut diagnostic = format!("{error:?}");
        if diagnostic.len() > 8192 {
            let mut end = 8192;
            while !diagnostic.is_char_boundary(end) {
                end -= 1;
            }
            diagnostic.truncate(end);
        }
        let mut failures = recovered(&self.private_failures);
        if failures.len() == 16 {
            failures.pop_front();
        }
        failures.push_back(diagnostic);
    }
    async fn finish(&self, id: Uuid, result: Result<Value, Error>) {
        let failure_evidence = result.as_ref().err().and_then(Error::failure_evidence);
        if let Err(error) = &result {
            self.remember_failure(error);
        }
        let (outcome, value, code) = match result {
            Ok(value) => (Outcome::Succeeded, Some(value), None),
            Err(error) => (
                match error.code {
                    "CANCELLED" => Outcome::Cancelled,
                    "OUTCOME_UNKNOWN" | "HTTP_REQUEST_OUTCOME_UNKNOWN" => Outcome::Unknown,
                    _ => Outcome::Failed,
                },
                error.partial,
                Some(error.code.to_owned()),
            ),
        };
        let cancelled = recovered(&self.state)
            .operations
            .get(&id)
            .is_some_and(|operation| operation.cancel_requested);
        // The journal keeps the resources a result names, never its page
        // content, form values or other data.
        let mut record = json!({"operationId":id,"outcome":outcome,"settled":true,"references":references(value.as_ref()),"code":code,"cancelRequested":cancelled});
        if let Some(evidence) = failure_evidence {
            record["backendFailure"] = evidence;
        }
        let mut journal_failure = self.record(record).await.err().map(|error| error.code);
        if let Some(operation) = recovered(&self.state).operations.get_mut(&id) {
            journal_failure = operation.settle(outcome, value, code, journal_failure);
        }
        if let Some(code) = journal_failure {
            self.stopping.store(true, Ordering::SeqCst);
            self.set_failure(code);
        }
        recovered(&self.cancelling).remove(&id);
        recovered(&self.input_credentials).remove(&id);
        recovered(&self.http_callers).remove(&id);
        self.changed.notify_waiters();
    }
    async fn start(self: Arc<Self>, bundle: BackendStart, options: LaunchOptions, id: Uuid) {
        let result = self.initialize(bundle, options, id).await;
        if let Err(error) = &result {
            self.set_failure(error.code);
            if self
                .shutdown(Instant::now() + Duration::from_secs(30))
                .await
                .is_err()
            {
                self.state.lock().expect("Browser owner").phase = Phase::CleanupPending;
            }
        }
        self.finish(id, result).await;
    }
    async fn initialize(
        self: &Arc<Self>,
        bundle: BackendStart,
        options: LaunchOptions,
        id: Uuid,
    ) -> Result<Value, Error> {
        let _lane = self.lane.lock().await;
        if self.stopping.load(Ordering::SeqCst)
            || self
                .cancelling
                .lock()
                .expect("Cancellation owner")
                .contains(&id)
        {
            return Err("CANCELLED".into());
        }
        self.stage(id, Outcome::Running).await?;
        {
            let state = self.state.lock().expect("Browser owner");
            self.journal_manifest(
                state.connection,
                state.owner,
                state.owner_generation,
                &options,
            )
        }
        .await?;
        let profile = self
            .profile
            .lock()
            .expect("Profile owner")
            .as_ref()
            .map(|profile| (profile.path().to_path_buf(), profile.summary().clone()));
        let (connection, reference, persistent) = match bundle {
            BackendStart::Launch(bundle) => {
                let connection = self.start_driver(bundle, &options).await?;
                let (reference, persistent) = self
                    .launch_browser(
                        &connection,
                        &options,
                        profile.as_ref().map(|(path, _)| path.as_path()),
                    )
                    .await?;
                (connection, reference, persistent)
            }
            BackendStart::Attach(attachment) => {
                let (connection, reference) = self
                    .attach_server(
                        attachment,
                        options.engine,
                        id,
                        Instant::now() + Duration::from_secs(30),
                    )
                    .await?;
                (connection, reference, None)
            }
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        let is_persistent = persistent.is_some();
        let context = if let Some(context) = persistent {
            let context = self
                .register_launched_context(
                    &connection,
                    reference,
                    context,
                    options
                        .context
                        .effective_for(options.engine, options.proxy.as_ref())?,
                    deadline,
                )
                .await?;
            *self.persistent_context.lock().expect("Profile context") = Some(context);
            context
        } else {
            self.context_create(id, reference, options.context, deadline)
                .await?
        };
        let page = if is_persistent
            && let Some(page) = self.profile_startup_page(context, deadline).await?
        {
            page
        } else {
            self.page_create(context, false, Instant::now() + Duration::from_secs(30))
                .await?
        };
        if self.stopping.load(Ordering::SeqCst) {
            return Err("CANCELLED".into());
        }
        {
            let mut state = self.state.lock().expect("Browser owner");
            if state.phase != Phase::Starting {
                return Err("LIFECYCLE_LOST_DURING_START".into());
            }
            if let Some((_, summary)) = profile {
                state.identity.as_mut().expect("Runtime identity")["profile"] = summary;
            }
            state.resource(page, Some(Kind::Page))?;
            state.selected_context = Some(context);
            state.selected_page = Some(page);
            state.phase = Phase::Ready;
        }
        Ok(json!({"context":context,"page":page}))
    }
    async fn attach_server(
        self: &Arc<Self>,
        attachment: super::attachment::Attachment,
        engine: contract::Engine,
        id: Uuid,
        deadline: Instant,
    ) -> Result<(Connection, Reference), Error> {
        self.attached.store(true, Ordering::SeqCst);
        let startup = self.driver.lock().await;
        self.admissible(id, deadline)?;
        self.record(
            json!({"kind":"attachment","operationId":id,"backend":"playwright","engine":engine}),
        )
        .await?;
        let user_agent = format!("Playwright/{}", super::DRIVER_VERSION)
            .parse()
            .expect("Pinned user agent");
        let socket = attachment
            .connect(
                Some(user_agent),
                super::transport::MAX_FRAME_BYTES,
                deadline,
            )
            .await?;
        let (connection, events) = Connection::open_server(socket, deadline).await?;
        *self.connection.lock().expect("Connection owner") = Some(connection.clone());
        self.start_events(events).await;
        self.start_frames(connection.frame_lane().claim()?).await;
        drop(startup);
        self.admissible(id, deadline)?;
        let bootstrap = connection.bootstrap()?;
        let playwright = connection
            .initializer(&bootstrap.playwright)
            .await?
            .ok_or("BROWSER_IDENTITY_MISSING")?;
        let browser = connection
            .resolve(
                playwright["preLaunchedBrowser"]["guid"]
                    .as_str()
                    .ok_or("ATTACHMENT_SERVER_UNSUPPORTED")?,
                "Browser",
            )
            .await?;
        let identity = connection
            .initializer(&browser)
            .await?
            .ok_or("BROWSER_IDENTITY_MISSING")?;
        if identity["browserName"] != engine.name() {
            return Err("BROWSER_IDENTITY_MISMATCH".into());
        }
        let version = engine.pinned_version().ok_or("ENGINE_UNAVAILABLE")?;
        if identity["version"] != version {
            return Err("BROWSER_VERSION_MISMATCH".into());
        }
        let mut state = self.state.lock().expect("Browser owner");
        state.identity = Some(
            json!({"launch":"attached","bindingVersion":super::DRIVER_VERSION,"browserVersion":version,"engine":engine}),
        );
        let reference = state.issue(Kind::Browser, browser, None)?;
        Ok((connection, reference))
    }
    async fn start_driver(
        self: &Arc<Self>,
        bundle: DiagnosticBundle,
        options: &LaunchOptions,
    ) -> Result<Connection, Error> {
        let mut slot = self.driver.lock().await;
        if self.stopping.load(Ordering::SeqCst) {
            return Err("CANCELLED".into());
        }
        #[cfg(windows)]
        let temporary = {
            let path = self.journal.directory().join("driver-temp");
            crate::artifacts::directory(&path)
                .map_err(|cause| Error::caused("DRIVER_START_FAILED", cause))?;
            Some(path)
        };
        #[cfg(unix)]
        let temporary = None::<std::path::PathBuf>;
        let mut driver = Driver::start_in(
            bundle,
            Instant::now() + Duration::from_secs(30),
            temporary.as_deref(),
        )
        .await
        .map_err(|cause| Error::caused("DRIVER_START_FAILED", cause))?;
        let driver_record = json!({"kind":"driver","pid":driver.pid(),"startWitness":driver.pid().and_then(crate::process::start_time)});
        let connection = driver.connection().clone();
        let identity = driver.identity();
        self.state.lock().expect("Browser owner").identity = Some(
            json!({"driverVersion":identity.driver_version,"nodeVersion":identity.node_version,"nodeSha256":identity.node_sha256,"coreSha256":identity.core_sha256,"browserRevision":options.engine.revision(),"mode":options.mode}),
        );
        let events = driver.take_events().ok_or("EVENT_OWNER_MISSING")?;
        *self.connection.lock().expect("Connection owner") = Some(connection.clone());
        *slot = Some(driver);
        drop(slot);
        self.record(driver_record).await?;
        self.start_events(events).await;
        self.start_frames(connection.frame_lane().claim()?).await;
        Ok(connection)
    }
    async fn launch_browser(
        &self,
        connection: &Connection,
        options: &LaunchOptions,
        profile_path: Option<&Path>,
    ) -> Result<(Reference, Option<super::objects::ObjectRef>), Error> {
        let browser_type = connection
            .bootstrap()?
            .browser_types
            .get(options.engine.name())
            .cloned()
            .ok_or("ENGINE_UNAVAILABLE")?;
        let browser_type_info = connection
            .initializer(&browser_type)
            .await?
            .ok_or("BROWSER_IDENTITY_MISSING")?;
        let executable = Path::new(
            browser_type_info["executablePath"]
                .as_str()
                .ok_or("BROWSER_IDENTITY_MISSING")?,
        );
        let revision = format!(
            "{}-{}",
            options.engine.name(),
            options.engine.revision().ok_or("ENGINE_UNAVAILABLE")?
        );
        let incoming = self.journal.directory().join("incoming");
        crate::artifacts::directory(&incoming).map_err(|e| Error::caused("DOWNLOAD_IO", e))?;
        let mut launch = options.wire();
        launch["downloadsPath"] = json!(incoming);
        let traces = self.journal.directory().join("traces");
        crate::artifacts::directory(&traces).map_err(|error| Error::caused("TRACE_IO", error))?;
        launch["tracesDir"] = json!(traces);
        if !executable
            .components()
            .any(|component| component.as_os_str() == revision.as_str())
            || !tokio::fs::metadata(executable)
                .await
                .is_ok_and(|metadata| metadata.is_file())
        {
            return Err("BROWSER_BUILD_UNAVAILABLE".into());
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        let method = if let Some(path) = profile_path {
            add_persistent_launch_options(&mut launch, options, path, deadline).await?;
            "launchPersistentContext"
        } else {
            "launch"
        };
        let result = connection
            .retained(&browser_type, method, launch, deadline, false)
            .await?;
        let browser = workflows::channel(connection, result.clone(), "browser", "Browser").await?;
        if let Some(driver) = self.driver.lock().await.as_mut() {
            driver
                .observe_descendants()
                .await
                .map_err(|cause| Error::caused("PROCESS_IDENTITY_UNKNOWN", cause))?;
        }
        let reference = self.state.lock().expect("Browser owner").issue(
            Kind::Browser,
            browser.clone(),
            None,
        )?;
        let initialized = connection
            .initializer(&browser)
            .await?
            .ok_or("BROWSER_IDENTITY_MISSING")?;
        self.state
            .lock()
            .expect("Browser owner")
            .identity
            .as_mut()
            .expect("Runtime identity")["browserVersion"] = initialized["version"].clone();
        self.record(json!({"kind":"browser","reference":reference,"engine":initialized["browserName"],"version":initialized["version"]})).await?;
        if initialized["browserName"] != options.engine.name() {
            return Err("BROWSER_IDENTITY_MISMATCH".into());
        }
        let expected = options
            .engine
            .pinned_version()
            .ok_or("ENGINE_UNAVAILABLE")?;
        if initialized["version"] != expected {
            return Err("BROWSER_VERSION_MISMATCH".into());
        }
        let context = if profile_path.is_some() {
            Some(workflows::channel(connection, result, "context", "BrowserContext").await?)
        } else {
            None
        };
        Ok((reference, context))
    }
    async fn journal_manifest(
        &self,
        connection: Uuid,
        owner: Uuid,
        generation: Uuid,
        options: &LaunchOptions,
    ) -> Result<(), Error> {
        self.record(json!({"kind":"session","connectionId":connection,"sessionId":owner,"sessionGeneration":generation,"backend":"playwright","engine":options.engine,"mode":options.mode,"driverVersion":super::DRIVER_VERSION,"nodeVersion":contract::NODE_VERSION,"browserRevision":options.engine.revision()})).await
    }
    async fn perform(
        self: Arc<Self>,
        id: Uuid,
        request: Request,
        deadline: Instant,
        responsive: bool,
        input_epoch: u64,
    ) {
        let ceiling =
            Instant::now() + Duration::from_millis(self.ceiling_ms.load(Ordering::SeqCst));
        let body = async {
            self.stage(id, Outcome::Queued).await?;
            let _lane = if responsive {
                None
            } else {
                Some(self.lane_for(id, deadline).await?)
            };
            self.admissible(id, deadline)?;
            self.stage(id, Outcome::Running).await?;
            // A dispatched driver call keeps its late result past the
            // deadline, but never past the ceiling: then its effect is
            // unknown, and the session admits no further work.
            let execution = async {
                let mut request = request;
                // Waits bypass `execute`, so resolve their URL conditions here.
                if matches!(request, Request::DiagnosticsWait { .. }) {
                    self.resolve_request_base(&mut request)?;
                }
                match request {
                    Request::EventsWait {
                        context,
                        after,
                        conditions,
                    } => {
                        self.events_wait(id, context, after, conditions, deadline)
                            .await
                    }
                    Request::DiagnosticsWait {
                        context,
                        after,
                        conditions,
                    } => {
                        self.diagnostics_wait(id, context, after, conditions, deadline)
                            .await
                    }
                    Request::Batch { requests } => {
                        self.execute_batch(id, requests, input_epoch, deadline)
                            .await
                    }
                    Request::PageInput {
                        page,
                        observation,
                        actions,
                    } => {
                        self.page_input(id, page, observation, actions, input_epoch, deadline)
                            .await
                    }
                    Request::InputRelease {} => self.input_release(deadline).await,
                    request @ (Request::DiagnosticsArgumentsSet { .. }
                    | Request::DiagnosticsArgumentsList { .. }
                    | Request::DiagnosticsArgumentsRead { .. }
                    | Request::DiagnosticsArgumentsRemove { .. }) => {
                        self.diagnostic_arguments_operation(id, request, deadline)
                            .await
                    }
                    request @ (Request::PageWait { .. } | Request::PageAction { .. }) => {
                        self.execute_page_control(id, request, deadline).await
                    }
                    request => self.execute(id, request, deadline).await,
                }
            };
            match tokio::time::timeout_at(ceiling, execution).await {
                Ok(result) => result,
                Err(_) => {
                    self.set_failure("OUTCOME_UNKNOWN");
                    let mut error = Error::from("OUTCOME_UNKNOWN");
                    error.partial = self
                        .state
                        .lock()
                        .expect("Browser owner")
                        .operations
                        .get(&id)
                        .and_then(|operation| operation.result.clone());
                    if let Some(partial) = error.partial.as_mut() {
                        partial["failedIndex"] = partial["completedActions"].clone();
                        partial["replayed"] = json!(false);
                        partial["nextAction"] = json!("browser_disconnect");
                    }
                    Err(error)
                }
            }
        };
        // Queueing, admission and execution panics all settle the operation.
        let result = self.unwinding(body).await;
        self.finish(id, result).await;
    }
    /// A panic in an operation task still settles its operation. Earlier calls
    /// may have reached the driver, so the effect is unknown and, as at the
    /// ceiling, the session admits no more work. A panic that poisoned the
    /// state lock cannot be settled here.
    async fn unwinding(
        &self,
        execution: impl std::future::Future<Output = Result<Value, Error>>,
    ) -> Result<Value, Error> {
        use futures_util::FutureExt;
        std::panic::AssertUnwindSafe(execution)
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                self.set_failure("OUTCOME_UNKNOWN");
                let mut error = Error::from("OUTCOME_UNKNOWN");
                error.partial = Some(
                    json!({"internalFailure":true,"replayed":false,"nextAction":"browser_disconnect"}),
                );
                Err(error)
            })
    }
    /// Each step keeps its retained effects. Failure, cancellation or expiry
    /// stops admission of the next step without rolling back completed work.
    async fn execute_batch(
        self: &Arc<Self>,
        id: Uuid,
        requests: Vec<Request>,
        input_epoch: u64,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let mut results = Vec::new();
        for request in requests {
            let result = async {
                self.admissible(id, deadline)?;
                self.controller_current(id, true)?;
                if self.state.lock().expect("Browser owner").phase != Phase::Ready {
                    return Err(Error::from("SESSION_NOT_READY"));
                }
                if self.input_epoch.load(Ordering::SeqCst) != input_epoch {
                    return Err(Error::from("INPUT_CANCELLED"));
                }
                match request {
                    Request::EventsWait {
                        context,
                        after,
                        conditions,
                    } => {
                        self.events_wait(id, context, after, conditions, deadline)
                            .await
                    }
                    Request::DiagnosticsWait {
                        context,
                        after,
                        conditions,
                    } => {
                        self.diagnostics_wait(id, context, after, conditions, deadline)
                            .await
                    }
                    Request::PageInput {
                        page,
                        observation,
                        actions,
                    } => {
                        self.page_input(id, page, observation, actions, input_epoch, deadline)
                            .await
                    }
                    request @ (Request::DiagnosticsArgumentsSet { .. }
                    | Request::DiagnosticsArgumentsList { .. }
                    | Request::DiagnosticsArgumentsRead { .. }
                    | Request::DiagnosticsArgumentsRemove { .. }) => {
                        self.diagnostic_arguments_operation(id, request, deadline)
                            .await
                    }
                    request @ (Request::PageWait { .. } | Request::PageAction { .. }) => {
                        self.execute_page_control(id, request, deadline).await
                    }
                    request => self.execute(id, request, deadline).await,
                }
            }
            .await;
            match result {
                Ok(value) => {
                    results.push(value);
                    if let Some(operation) = self
                        .state
                        .lock()
                        .expect("Browser owner")
                        .operations
                        .get_mut(&id)
                    {
                        operation.result =
                            Some(json!({"completedActions":results.len(),"results":results}));
                    }
                }
                Err(error) => {
                    self.input_epoch.fetch_add(1, Ordering::SeqCst);
                    let cleanup = match self
                        .input_release(Instant::now() + Duration::from_secs(2))
                        .await
                    {
                        Ok(cleanup) => cleanup,
                        Err(error) => json!({"complete":false,"code":error.code}),
                    };
                    return Err(error.with_batch_result(results, Some(cleanup)));
                }
            }
        }
        Ok(json!({"completedActions":results.len(),"results":results}))
    }
    /// Wait for the mutation lane in arrival order. A cancellation, the
    /// operation's deadline or the session closing ends the wait.
    async fn lane_for(
        &self,
        id: Uuid,
        deadline: Instant,
    ) -> Result<tokio::sync::MutexGuard<'_, ()>, Error> {
        let lane = self.lane.lock();
        tokio::pin!(lane);
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            self.admissible(id, deadline)?;
            tokio::select! {
                guard = &mut lane => return Ok(guard),
                () = tokio::time::sleep_until(deadline) => return Err("NOT_DISPATCHED".into()),
                () = changed => {}
            }
        }
    }
    fn admissible(&self, id: Uuid, deadline: Instant) -> Result<(), Error> {
        let queued = self
            .state
            .lock()
            .expect("Browser owner")
            .operations
            .get(&id)
            .is_some_and(|operation| operation.outcome == Outcome::Queued);
        self.controller_current(id, queued)?;
        if self
            .cancelling
            .lock()
            .expect("Cancellation owner")
            .contains(&id)
        {
            return Err("CANCELLED".into());
        }
        if Instant::now() >= deadline {
            return Err("NOT_DISPATCHED".into());
        }
        if self.stopping.load(Ordering::SeqCst) {
            return Err("SESSION_CLOSING".into());
        }
        Ok(())
    }
    async fn disconnect_operation(self: Arc<Self>, id: Uuid, deadline: Instant) {
        let journal_failure = self.stage(id, Outcome::Running).await.err();
        let result = self.shutdown_report(deadline).await.map(|mut report| {
            if let Some(error) = journal_failure {
                report["journalFailure"] = json!(error.code);
            }
            json!({"closed":true,"cleanup":report})
        });
        if result.is_err() {
            recovered(&self.state).phase = Phase::CleanupPending;
        }
        self.finish(id, result).await;
    }
    /// Shut down, attaching payload-free stage timings to an unfinished cleanup.
    async fn shutdown(&self, deadline: Instant) -> Result<(), Error> {
        self.shutdown_report(deadline).await.map(|_| ())
    }
    async fn shutdown_report(&self, deadline: Instant) -> Result<Value, Error> {
        let mut stages = CleanupStages::new();
        self.shutdown_staged(deadline, &mut stages)
            .await
            .map(|()| stages.report())
            .map_err(|mut error| {
                if error.partial.is_none() {
                    error.partial = Some(stages.report());
                }
                error
            })
    }
    /// Ask the driver to close its browser before the pipe closes, as the pinned
    /// client's `browser.close()` does. On pipe EOF the driver gracefully closes
    /// each launched browser and waits for its process and directory cleanup.
    /// This ordering does not establish a fix for intermittent Firefox shutdown
    /// stalls. If the close budget runs out, EOF repeats
    /// the driver's graceful close, which the driver then escalates to killing
    /// its own browser process. Either way, shutdown still waits for the driver
    /// and every witnessed descendant. Errors are ignored: EOF is the fallback.
    async fn close_browser(&self, deadline: Instant) -> &'static str {
        // Release reads its references from recovered locks; `connection()` and
        // `wire()` keep refusing poisoned state for ordinary work.
        let wire = {
            let state = recovered(&self.state);
            let Some(browser) = state
                .resources
                .values()
                .find(|resource| resource.kind == Kind::Browser)
                .map(|resource| resource.reference)
            else {
                return "BROWSER_REFERENCE_UNAVAILABLE";
            };
            match state.resource(browser, Some(Kind::Browser)) {
                Ok(resource) => resource.wire.clone(),
                Err(code) => return code,
            }
        };
        let Some(connection) = recovered(&self.connection).clone() else {
            return "BACKEND_UNAVAILABLE";
        };
        match connection
            .retained(&wire, "close", json!({}), deadline, false)
            .await
        {
            Ok(_) => "ACKNOWLEDGED",
            Err(error) => Error::from(error).code,
        }
    }
    async fn shutdown_staged(
        &self,
        deadline: Instant,
        stages: &mut CleanupStages,
    ) -> Result<(), Error> {
        stages.mark("cleanup_lock");
        let _cleanup = self.cleanup.lock().await;
        stages.mark("previews_and_traces");
        self.stopping.store(true, Ordering::SeqCst);
        recovered(&self.http_contexts).shutdown_work();
        self.authority.close();
        self.changed.notify_waiters();
        recovered(&self.state).phase = Phase::Closing;
        self.input_epoch.fetch_add(1, Ordering::SeqCst);
        // Reserve time for driver EOF/reaping even when an in-flight media
        // allocation holds its lane. Driver loss then settles that retained
        // allocation, allowing the same cleanup to finish on retry.
        let media_deadline = deadline.min(Instant::now() + Duration::from_secs(2));
        self.signal_recordings();
        let media_cleanup = self.close_previews(media_deadline).await;
        let trace_cleanup = self.close_trace(None, media_deadline).await;
        stages.mark("driver_lock");
        let mut slot = tokio::time::timeout_at(deadline, self.driver.lock())
            .await
            .map_err(|_| Error::from("CLEANUP_PENDING"))?;
        if let Some(driver) = slot.as_mut() {
            stages.mark("process_witnesses");
            stages.pre_close_inspection = Some(
                driver
                    .observe_before_close(deadline.min(Instant::now() + Duration::from_secs(2)))
                    .await
                    .is_ok(),
            );
            driver.note_teardown_inspection(stages.pre_close_inspection == Some(true));
            stages.mark("browser_close");
            stages.browser_close = Some(
                self.close_browser(deadline.min(Instant::now() + Duration::from_secs(5)))
                    .await,
            );
        }
        stages.mark("driver_shutdown");
        let result = if let Some(driver) = slot.as_mut() {
            let result = driver
                .shutdown(deadline)
                .await
                .map(|_| ())
                .map_err(|cause| Error::caused("CLEANUP_PENDING", cause));
            stages.driver = Some(driver.shutdown_report());
            result
        } else {
            let connection = recovered(&self.connection).clone();
            if let Some(connection) = connection {
                // A Playwright server owns its browser. Ending this socket
                // makes it clean up the contexts created by this client.
                tokio::time::timeout_at(deadline, connection.close())
                    .await
                    .map_err(|_| Error::from("CLEANUP_PENDING"))??;
            }
            Ok(())
        };
        if result.is_err() {
            recovered(&self.state).phase = Phase::CleanupPending;
            return result;
        }
        #[cfg(windows)]
        crate::artifacts::windows::remove_tree(
            &self.journal.directory().join("driver-temp"),
            deadline,
        )
        .await
        .map_err(|cause| Error::caused("CLEANUP_PENDING", cause))?;
        recovered(&self.profile).take();
        stages.mark("event_owner");
        if let Some(task) = self.events.lock().await.as_mut() {
            tokio::time::timeout_at(deadline, task)
                .await
                .map_err(|_| Error::from("CLEANUP_PENDING"))?
                .map_err(|_| Error::from("EVENT_OWNER_FAILED"))?;
        }
        *self.events.lock().await = None;
        stages.mark("trace_finalization");
        if trace_cleanup.is_err() {
            self.trace_after_driver_exit(deadline).await?;
        }
        stages.mark("har_tasks");
        self.close_har_tasks(deadline).await?;
        self.finish_media_and_transfers(deadline, media_cleanup.is_err(), stages)
            .await?;
        stages.mark("control_http_and_archives");
        self.close_control(deadline).await?;
        self.close_http_owned(deadline).await?;
        stages.mark("http_callers");
        self.close_http_callers(deadline).await?;
        recovered(&self.har_archives).finalize_all(self.journal.directory())?;
        self.retire_released_session();
        stages.mark("closed");
        Ok(())
    }

    fn retire_released_session(&self) {
        let mut state = recovered(&self.state);
        state.resources.clear();
        recovered(&self.automation_debuggers).released();
        state.context_options.clear();
        recovered(&self.persistent_context).take();
        state.authenticators.clear();
        state.launch_proxy = None;
        *recovered(&self.inputs) = super::input::State::default();
        recovered(&self.diagnostics).close();
        recovered(&self.request_inputs).clear();
        state.selected_context = None;
        state.selected_page = None;
        state.phase = Phase::Closed;
        recovered(&self.callbacks).clear();
        recovered(&self.binding_calls).clear();
        *recovered(&self.bindings) = Default::default();
        recovered(&self.lease).take();
        self.changed.notify_waiters();
    }
}

async fn add_persistent_launch_options(
    launch: &mut Value,
    options: &LaunchOptions,
    path: &Path,
    deadline: Instant,
) -> Result<(), Error> {
    let mut context = options
        .context
        .effective_for(options.engine, options.proxy.as_ref())?;
    context.client_certificates = super::http::client_certificates::prepare_options(
        super::http::Options {
            client_certificates: context.client_certificates,
            ..Default::default()
        },
        deadline,
    )
    .await?
    .client_certificates;
    let Value::Object(context) = context.wire() else {
        unreachable!("Typed context serializes as an object");
    };
    launch
        .as_object_mut()
        .expect("Launch options")
        .extend(context);
    launch["userDataDir"] = json!(path);
    Ok(())
}

impl Host {
    async fn finish_media_and_transfers(
        &self,
        deadline: Instant,
        media_cleanup_failed: bool,
        stages: &mut CleanupStages,
    ) -> Result<(), Error> {
        stages.mark("media_frames");
        let mut frames_task = self.frames_task.lock().await;
        if let Some(task) = frames_task.as_mut() {
            let completed = tokio::time::timeout_at(deadline, task)
                .await
                .map_err(|_| Error::from("CLEANUP_PENDING"))?;
            frames_task.take();
            completed.map_err(|_| Error::from("MEDIA_OWNER_FAILED"))?;
        }
        drop(frames_task);
        stages.mark("previews_and_recordings");
        if media_cleanup_failed {
            self.close_previews(deadline).await?;
        }
        self.close_recordings(deadline).await?;
        stages.mark("transfers");
        let mut transfers = self.transfer_tasks.lock().await;
        while !transfers.is_empty() {
            let completed = tokio::time::timeout_at(deadline, &mut transfers[0])
                .await
                .map_err(|_| Error::from("CLEANUP_PENDING"))?;
            transfers.remove(0);
            completed.map_err(|_| Error::from("TRANSFER_OWNER_FAILED"))?;
        }
        drop(transfers);
        Ok(())
    }
}

/// Shutdown attempts after the last client is dropped, each with its own
/// 30-second deadline.
const DROP_CLEANUP_ATTEMPTS: u32 = 10;

impl Drop for Client {
    fn drop(&mut self) {
        self.host.stopping.store(true, Ordering::SeqCst);
        let host = self.host.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            // Best effort and bounded: cleanup that still cannot finish stays
            // pending in status for an explicit disconnect to retry.
            runtime.spawn(async move {
                for _ in 0..DROP_CLEANUP_ATTEMPTS {
                    if host
                        .shutdown(Instant::now() + Duration::from_secs(30))
                        .await
                        .is_ok()
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });
        }
    }
}

/// The public resource references a result names, for the journal, in a
/// bounded number; the rest of the result is never journaled.
fn references(result: Option<&Value>) -> Vec<Value> {
    let mut found = Vec::new();
    let mut pending: Vec<&Value> = result.into_iter().collect();
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(fields) => {
                if serde_json::from_value::<Reference>(value.clone()).is_ok() {
                    if found.len() == 64 {
                        break;
                    }
                    found.push(value.clone());
                } else {
                    pending.extend(fields.values());
                }
            }
            Value::Array(items) => pending.extend(items),
            _ => {}
        }
    }
    found
}

#[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
mod clock_native;
#[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
mod native;
#[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
mod page_native;
#[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
mod preview_native;
#[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
mod settings_native;
#[cfg(test)]
mod tests;

/// Payload-free timings of an owned browser cleanup; the unfinished stage names
/// where an unsettled `disconnect` spent its budget.
struct CleanupStages {
    started: Instant,
    current: &'static str,
    current_started: Instant,
    completed: Vec<(&'static str, u64)>,
    /// The driver's own step report, including completed and unfinished release.
    driver: Option<Value>,
    /// Outcome code only; no remote message, stack or browser data.
    browser_close: Option<&'static str>,
    pre_close_inspection: Option<bool>,
}
impl CleanupStages {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            current: "start",
            current_started: now,
            completed: Vec::new(),
            driver: None,
            browser_close: None,
            pre_close_inspection: None,
        }
    }
    fn millis(since: Instant) -> u64 {
        u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
    fn mark(&mut self, stage: &'static str) {
        self.completed
            .push((self.current, Self::millis(self.current_started)));
        self.current = stage;
        self.current_started = Instant::now();
    }
    pub(super) fn report(&self) -> Value {
        json!({
            "cleanupStages":self.completed.iter().map(|(stage,elapsed)|json!({"stage":stage,"elapsedMs":elapsed})).collect::<Vec<_>>(),
            "complete":self.current=="closed",
            "unfinishedStage":if self.current=="closed" {None} else {Some(self.current)},
            "unfinishedElapsedMs":if self.current=="closed" {None} else {Some(Self::millis(self.current_started))},
            "totalMs":Self::millis(self.started),
            "driverShutdown":self.driver,
            "browserCloseOutcome":self.browser_close,
            "preCloseInspection":self.pre_close_inspection,
        })
    }
}
#[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
mod transfer_native;
