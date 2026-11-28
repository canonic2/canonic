//! Retained native Chrome lifecycle operations in the shared browser owner.

use super::{
    Connection, Event, EventItem, Events, Index, Kind, Lane,
    process::{Process, media_fixtures},
};
use crate::browser::{
    contract::{self, ContextOptions, LaunchOptions, MAX_TIMEOUT_MS, Reference, Request},
    journal::Journal,
    lifecycle::{Operation, Outcome, Phase, State},
    preparation::ChromeBundle,
    session::Error,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Notify, mpsc},
    time::Instant,
};
use uuid::Uuid;

type NativeReply = Result<Value, super::Error>;

#[derive(Clone)]
pub(crate) struct Session(Arc<Client>);
struct Client {
    shared: Arc<Shared>,
    commands: mpsc::Sender<Command>,
    executions: mpsc::Sender<Command>,
    capacity: Arc<tokio::sync::Semaphore>,
    cleanup: mpsc::Sender<Command>,
    responses: mpsc::Sender<Command>,
    controls: mpsc::Sender<control::ControlCommand>,
    journal: Journal,
    startup: Uuid,
}
struct Shared {
    state: Mutex<State>,
    resources: Mutex<Value>,
    details: Mutex<Value>,
    control: control::ControlState,
    changed: Notify,
    #[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
    native_pid: std::sync::atomic::AtomicU32,
}
struct Command {
    id: Uuid,
    request: Option<Request>,
    deadline: Instant,
    queue_slot: Option<tokio::sync::OwnedSemaphorePermit>,
}
struct Worker {
    shared: Arc<Shared>,
    process: Option<Process>,
    fixture_stage: Option<media_fixtures::Pending>,
    connection: Option<Connection>,
    events: Option<Events>,
    index: Index,
    journal: Journal,
    lease: Option<crate::lease::ControlLease>,
    profile: Option<crate::browser::profiles::Profile>,
    persistent_context: Option<Reference>,
    /// Held only while explicitly adopting a borrowed isolated context.
    adoption: Option<(Reference, crate::lease::ControlLease)>,
    attached: bool,
    configured: BTreeSet<String>,
    startup: startup::Startup,
    closed: BTreeMap<Reference, Kind>,
    navigation: navigation::NavigationEvents,
    diagnostics: diagnostics::NativeDiagnostics,
    argument_groups: diagnostic_arguments::Arguments,
    evaluation: evaluation::Evaluation,
    network_evidence: crate::browser::network::Evidence,
    har_archives: crate::browser::har::Store,
    routing_rules: crate::browser::routing::Rules,
    fetch: fetch::Policies,
    har_servicing: bool,
    http_contexts: crate::browser::http::Contexts,
    request_inputs: crate::browser::http::request_inputs::store::Store,
    http_requests: BTreeMap<Uuid, http::PendingFetch>,
    http_state: http_state::State,
    /// Private native `storage_*` snapshots.
    snapshots: crate::browser::storage::Store,
    /// Per-context virtual WebAuthn authenticators and their private keys.
    authenticators: credentials::Authenticators,
    developer_tools: crate::browser::developer_tools::Registries,
    webmcp_debug: webmcp_debug::DebugTools,
    managed_apps: bool,
    extension_profile: bool,
    app_tab_probe: Option<app_tabs::Probe>,
    extensions: extensions::Extensions,
    pwas: pwa::Apps,
    frameworks: crate::browser::framework::Registries,
    heap: heap::Heap,
    performance: performance::Performance,
    websockets: websockets::Sockets,
    developers: developer::Developers,
    profiling: profiling::Profiling,
    trace: Option<tracing::Trace>,
    trace_last: Option<tracing::Completion>,
    trace_tabs: tracing::Tabs,
    debuggers: debugging::Debuggers,
    interactions: interactions::Interactions,
    transfers: transfers::Transfers,
    responses: Option<mpsc::Receiver<Command>>,
    executions: Option<mpsc::Receiver<Command>>,
    deferred_executions: VecDeque<Command>,
    waits: BTreeMap<Uuid, Command>,
    condition_probes: BTreeMap<Uuid, conditions::Probe>,
    inputs: crate::browser::input::State,
    pointer_positions: BTreeMap<Reference, (f64, f64)>,
    touch_contexts: BTreeSet<Reference>,
    context_layout: BTreeMap<Reference, ContextOptions>,
    certificate_tunnels: client_certificates::Tunnels,
    page_headers: BTreeMap<Reference, Vec<crate::browser::http::Header>>,
    emulation: emulation::State,
    capture_store: crate::browser::capture::Store,
    media: media::MediaState,
    control_commands: Option<mpsc::Receiver<control::ControlCommand>>,
}

pub(in crate::browser) enum BackendStart {
    Launch {
        bundle: ChromeBundle,
        disposable_profile: bool,
    },
    Attach(crate::browser::attachment::Attachment),
}

impl Session {
    #[cfg(test)]
    pub(crate) fn fixture_retained(state: State, directory: &Path) -> Self {
        // Closed dispatch lanes: evidence borrowing must not execute browser work.
        let shared = Arc::new(Shared {
            state: Mutex::new(state),
            resources: Mutex::new(json!([])),
            details: Mutex::new(json!({})),
            control: Default::default(),
            changed: Default::default(),
            #[cfg(feature = "acceptance-browser-lifecycle")]
            native_pid: Default::default(),
        });
        let connection = shared.state.lock().unwrap().connection;
        Self(Arc::new(Client {
            journal: Journal::create(directory, connection).unwrap(),
            shared,
            commands: mpsc::channel(1).0,
            executions: mpsc::channel(1).0,
            capacity: Arc::new(tokio::sync::Semaphore::new(1)),
            cleanup: mpsc::channel(1).0,
            responses: mpsc::channel(1).0,
            controls: mpsc::channel(1).0,
            startup: Uuid::new_v4(),
        }))
    }
    #[cfg(test)]
    pub(crate) fn launch_connection(
        bundle: ChromeBundle,
        connection: Uuid,
        owner: Uuid,
        generation: Uuid,
        options: LaunchOptions,
        directory: &Path,
    ) -> Result<Self, Error> {
        Self::launch_selected(
            BackendStart::Launch {
                bundle,
                disposable_profile: false,
            },
            connection,
            owner,
            generation,
            options,
            None,
            directory,
        )
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Constructs the single connection owner and its bounded actor lanes before publishing the supervisor"
    )]
    pub(in crate::browser) fn launch_selected(
        bundle: BackendStart,
        connection: Uuid,
        owner: Uuid,
        generation: Uuid,
        options: LaunchOptions,
        profile: Option<crate::browser::profiles::Profile>,
        directory: &Path,
    ) -> Result<Self, Error> {
        if options.engine != crate::browser::contract::Engine::Chrome {
            return Err("BACKEND_UNAVAILABLE".into());
        }
        options.validate()?;
        if crate::browser::processes::windows_bundle_host() {
            let supported = match &bundle {
                BackendStart::Attach(_) => true,
                BackendStart::Launch { bundle, .. } => {
                    crate::browser::processes::windows_mode_supported(options.mode)
                        && (bundle.installed.is_none()
                            || crate::browser::processes::windows_console_session())
                }
            };
            if !supported {
                return Err("HOST_UNAVAILABLE".into());
            }
        } else if !crate::browser::processes::initial_host() {
            return Err("HOST_UNAVAILABLE".into());
        }
        chrome_options(&options.context)?;
        let journal = Journal::create(directory, connection)?;
        #[cfg(unix)]
        let lease = Some(
            crate::lease::ControlLease::acquire(&format!(
                "browser-session:{owner}:{generation}:{connection}"
            ))
            .map_err(|_| Error::from("SESSION_TARGET_CONFLICT"))?,
        );
        // The Windows manager holds the exclusive pool lease. Creating this
        // connection's journal directory also refuses an existing identity.
        #[cfg(windows)]
        let lease = None;
        let startup = Uuid::new_v4();
        let mut state = State::connection(connection, owner, generation, &options);
        state.begin(startup, "launch".into())?;
        let shared = Arc::new(Shared {
            state: Mutex::new(state),
            resources: Mutex::new(json!([])),
            details: Mutex::new(json!({})),
            control: Default::default(),
            changed: Notify::new(),
            #[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
            native_pid: Default::default(),
        });
        let (commands, ordinary) = mpsc::channel::<Command>(32);
        let (executions, execution_queue) = mpsc::channel::<Command>(32);
        let (cleanup, controls) = mpsc::channel::<Command>(8);
        let (responses, response_queue) = mpsc::channel::<Command>(8);
        let (control_sender, control_queue) = mpsc::channel(4);
        let client_journal = journal.clone();
        let worker = Worker {
            shared: shared.clone(),
            process: None,
            fixture_stage: None,
            connection: None,
            events: None,
            index: Index::new(connection),
            journal,
            lease,
            profile,
            persistent_context: None,
            adoption: None,
            attached: false,
            configured: BTreeSet::new(),
            startup: Default::default(),
            closed: BTreeMap::new(),
            navigation: Default::default(),
            diagnostics: Default::default(),
            argument_groups: Default::default(),
            evaluation: Default::default(),
            network_evidence: Default::default(),
            har_archives: Default::default(),
            routing_rules: Default::default(),
            fetch: Default::default(),
            har_servicing: false,
            http_contexts: Default::default(),
            request_inputs: Default::default(),
            http_requests: Default::default(),
            http_state: Default::default(),
            snapshots: Default::default(),
            authenticators: Default::default(),
            developer_tools: Default::default(),
            webmcp_debug: Default::default(),
            managed_apps: false,
            extension_profile: false,
            app_tab_probe: None,
            extensions: Default::default(),
            pwas: Default::default(),
            frameworks: Default::default(),
            heap: Default::default(),
            performance: Default::default(),
            websockets: Default::default(),
            developers: Default::default(),
            profiling: Default::default(),
            trace: None,
            trace_last: None,
            trace_tabs: Default::default(),
            debuggers: Default::default(),
            interactions: Default::default(),
            transfers: Default::default(),
            responses: Some(response_queue),
            executions: Some(execution_queue),
            deferred_executions: VecDeque::new(),
            waits: BTreeMap::new(),
            condition_probes: BTreeMap::new(),
            inputs: Default::default(),
            pointer_positions: BTreeMap::new(),
            touch_contexts: BTreeSet::new(),
            context_layout: BTreeMap::new(),
            certificate_tunnels: Default::default(),
            page_headers: BTreeMap::new(),
            emulation: Default::default(),
            capture_store: Default::default(),
            media: Default::default(),
            control_commands: Some(control_queue),
        };
        tokio::spawn(supervisor::run(
            worker, bundle, options, startup, ordinary, controls,
        ));
        Ok(Self(Arc::new(Client {
            shared,
            commands,
            executions,
            capacity: Arc::new(tokio::sync::Semaphore::new(32)),
            cleanup,
            responses,
            controls: control_sender,
            startup,
            journal: client_journal,
        })))
    }

    pub(crate) fn startup(&self) -> Uuid {
        self.0.startup
    }
    pub(crate) fn status(&self) -> Value {
        let mut value = self
            .0
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .snapshot();
        value["resources"] = self
            .0
            .shared
            .resources
            .lock()
            .expect("Chrome resources")
            .clone();
        value.as_object_mut().expect("Chrome status").extend(
            self.0
                .shared
                .details
                .lock()
                .expect("Chrome details")
                .as_object()
                .expect("Chrome details object")
                .clone(),
        );
        value
    }
    pub(crate) fn compact_status(&self, limit: usize) -> Value {
        self.0
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .compact_status(limit)
    }

    pub(crate) fn pending_operation(&self) -> Option<Uuid> {
        self.0
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .pending_operation()
    }
    pub(crate) fn operation(&self, id: Uuid) -> Option<Operation> {
        self.0
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .operations
            .get(&id)
            .cloned()
    }
    pub(crate) fn control_status(&self) -> Value {
        self.0.shared.control.snapshot()
    }
    pub(crate) async fn change_control(&self, name: &str, args: Value) -> Result<Value, Error> {
        self.native_change_control(name, args).await
    }
    pub(crate) fn submit(
        &self,
        id: Uuid,
        request: Request,
        timeout_ms: u64,
    ) -> Result<Uuid, Error> {
        supported(request.syntax_probe().as_ref().unwrap_or(&request))?;
        self.enqueue(id, Some(request), timeout_ms)
    }
    fn enqueue(&self, id: Uuid, request: Option<Request>, timeout_ms: u64) -> Result<Uuid, Error> {
        self.enqueue_as(id, request, timeout_ms, None)
    }
    #[expect(
        clippy::too_many_lines,
        reason = "one operation's validation, admission and queueing share their locals"
    )]
    fn enqueue_as(
        &self,
        id: Uuid,
        request: Option<Request>,
        timeout_ms: u64,
        viewer: Option<&crate::control::Credential>,
    ) -> Result<Uuid, Error> {
        if !(1..=MAX_TIMEOUT_MS).contains(&timeout_ms) {
            return Err("INVALID_TIMEOUT".into());
        }
        let fingerprint = if let Some(request) = &request {
            supported(request.syntax_probe().as_ref().unwrap_or(request))?;
            format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(request).map_err(|_| Error::from("INVALID_ARGUMENT"))?
                )
            )
        } else {
            "disconnect".into()
        };
        let mut state = self.0.shared.state.lock().expect("Chrome lifecycle");
        if let Some(operation) = state.operations.get(&id) {
            return if operation.fingerprint == fingerprint {
                Ok(id)
            } else {
                Err("OPERATION_CONFLICT".into())
            };
        }
        if request.is_some() && state.phase != Phase::Ready {
            return Err("SESSION_NOT_READY".into());
        }
        let debugger_response = request.as_ref().is_some_and(|request| {
            matches!(request,Request::PageDebugger{action,..} if action.responsive())
                || matches!(request,Request::WebMcpDebug{action,..} if action.responsive())
                || matches!(request,Request::WebMcp{action,..} if action.responsive())
        });
        let (queue, ordinary_capacity) = if debugger_response
            || matches!(
                request,
                Some(
                    Request::CallbackResolve { .. }
                        | Request::HandleDispose { .. }
                        | Request::RouteFetch { .. }
                        | Request::NetworkRoute {
                            action: crate::browser::routing::HandlerAction::List {},
                            ..
                        }
                        | Request::WebSocketRouteSend { .. }
                        | Request::WebSocketRouteHandlers { .. }
                        | Request::WebSocketRouteClose { .. }
                        | Request::DiagnosticsArgumentsList { .. }
                        | Request::DiagnosticsArgumentsRemove { .. }
                        | Request::DiagnosticsArgumentsSet { enabled: false, .. }
                        | Request::HttpContextList {}
                        | Request::HttpRequestInputRemove { .. }
                        | Request::HttpRequestInputsSet { enabled: false, .. }
                        | Request::HarList {}
                        | Request::HarCaptureStatus { .. }
                        | Request::HarFinalize { .. }
                        | Request::HarRemove { .. }
                        | Request::HttpContextRemove { .. }
                        | Request::HttpContextDispose { .. }
                        | Request::HttpContextRead { .. }
                        | Request::HttpResponseBodyRead { .. }
                        | Request::HttpResponseDispose { .. }
                        | Request::HttpContextCookiesClear { .. }
                        | Request::HttpContextCookiesAdd { .. }
                        | Request::HttpContextCookiesRead { .. }
                        | Request::HttpContextStorageState { .. }
                        | Request::HttpEvidenceFinalize { .. }
                        | Request::HttpResponseFinalize { .. }
                        | Request::HttpResponseDiscard { .. }
                        | Request::HttpResponseRead { .. }
                        | Request::HttpResponseRemove { .. }
                        | Request::HttpEvidenceList {}
                        | Request::HttpEvidenceRead { .. }
                        | Request::HttpEvidenceRemove { .. }
                        | Request::BindingRead { .. }
                        | Request::BindingList { .. }
                        | Request::BindingRemove { .. }
                        | Request::DeveloperClose { .. }
                        | Request::DeveloperStatus { .. }
                        | Request::ProfileStatus { .. }
                        | Request::ProfileStop { .. }
                        | Request::ProfileTakeCoverage { .. }
                        | Request::HeapStop { .. }
                        | Request::TraceStatus { .. }
                        | Request::TraceStop {
                            analysis_developer: None,
                            analysis_page: None,
                            ..
                        }
                        | Request::ChooserResolve { .. }
                        | Request::DownloadCancel { .. }
                )
            ) {
            (&self.0.responses, false)
        } else if request.is_none() || matches!(request, Some(Request::InputRelease {})) {
            (&self.0.cleanup, false)
        } else if request.as_ref().is_some_and(execution::independent) {
            (&self.0.executions, true)
        } else {
            (&self.0.commands, true)
        };
        let queue_slot = ordinary_capacity
            .then(|| self.0.capacity.clone().try_acquire_owned())
            .transpose()
            .map_err(|_| Error::from("OPERATION_CAPACITY"))?;
        let slot = queue
            .try_reserve()
            .map_err(|_| Error::from("OPERATION_CAPACITY"))?;
        if let Some(request) = request.as_ref() {
            self.admit_native_control(id, request, viewer)?;
        }
        if let Err(error) = state.begin(id, fingerprint) {
            self.0.shared.control.finish(id);
            return Err(error.into());
        }
        if request.is_none() {
            state.phase = Phase::Closing;
        }
        if matches!(request, Some(Request::InputRelease {})) {
            self.0.shared.control.fence_inputs();
        }
        slot.send(Command {
            id,
            request,
            deadline: Instant::now() + Duration::from_millis(timeout_ms),
            queue_slot,
        });
        self.0.shared.changed.notify_waiters();
        Ok(id)
    }
    pub(crate) fn diagnostic_operation(
        &self,
        id: Uuid,
        request: &Request,
    ) -> crate::error::Result<Value> {
        crate::browser::investigation::snapshot(
            &self.0.shared.state.lock().expect("Chrome lifecycle"),
            id,
            request,
        )
    }
    pub(crate) fn disconnect(&self, timeout_ms: u64) -> Result<Uuid, Error> {
        if let Some(operation) = self
            .0
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .operations
            .values()
            .find(|operation| operation.fingerprint == "disconnect" && !operation.settled)
            .cloned()
        {
            return Ok(operation.id);
        }
        self.enqueue(Uuid::new_v4(), None, timeout_ms)
    }
    pub(crate) async fn cancel(&self, id: Uuid) -> Result<(), Error> {
        {
            let mut state = self.0.shared.state.lock().expect("Chrome lifecycle");
            let operation = state.operations.get_mut(&id).ok_or("UNKNOWN_OPERATION")?;
            if operation.settled {
                return Ok(());
            }
            operation.cancel_requested = true;
        }
        self.0.shared.changed.notify_waiters();
        self.0
            .journal
            .record(json!({"operationId":id,"cancelRequested":true}))
            .await?;
        Ok(())
    }
    pub(crate) async fn wait(&self, id: Uuid, deadline: Instant) -> Result<Operation, Error> {
        loop {
            let notified = self.0.shared.changed.notified();
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
}

async fn next_event(events: &mut Option<Events>) -> Option<EventItem> {
    match events {
        Some(events) => events.next().await,
        None => std::future::pending().await,
    }
}
async fn next_response(responses: &mut Option<mpsc::Receiver<Command>>) -> Option<Command> {
    match responses {
        Some(responses) => responses.recv().await,
        None => std::future::pending().await,
    }
}

async fn next_execution(
    deferred: &mut VecDeque<Command>,
    executions: &mut Option<mpsc::Receiver<Command>>,
) -> Option<Command> {
    match deferred.pop_front() {
        Some(command) => Some(command),
        None => next_response(executions).await,
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "One backend admission table validates every executable request before queue allocation"
)]
fn supported(request: &Request) -> Result<(), Error> {
    match request {
        Request::PageHarSet { .. } | Request::ContextHarSet { .. } => Ok(()),
        Request::HarCaptureStart { .. }
        | Request::HarCaptureStatus { .. }
        | Request::HarCaptureStop { .. } => Ok(()),
        Request::HarRegister { path } => {
            if path.is_empty() || path.len() > 4096 {
                Err("HAR_PRIVATE_FILE_REQUIRED".into())
            } else {
                Ok(())
            }
        }
        Request::HarList {}
        | Request::HarFinalize { .. }
        | Request::HarRemove { .. }
        | Request::CallbackResolve {
            response: contract::CallbackResponse::RouteHar { .. },
            ..
        } => Ok(()),
        Request::HttpContextCreate { options } => options.validate(),
        Request::HttpContextDispose { options, .. } => options.validate(),
        Request::HttpFetch { request, .. } => request.validate(),
        Request::HttpFetchRequest { options, .. } => options.validate(),
        Request::RouteFetch { .. } => crate::browser::http::RouteFetchOptions::of(request)
            .expect("route_fetch options")
            .validate(),
        Request::HttpContextList {}
        | Request::HttpRequestInputsSet { .. }
        | Request::HttpRequestInputRemove { .. }
        | Request::HttpContextRemove { .. }
        | Request::HttpContextRead { .. }
        | Request::HttpResponseBodyRead { .. }
        | Request::HttpResponseDispose { .. }
        | Request::HttpContextCookiesClear { .. }
        | Request::HttpContextCookiesAdd { .. }
        | Request::HttpContextCookiesRead { .. }
        | Request::HttpContextStorageState { .. }
        | Request::HttpEvidenceFinalize { .. }
        | Request::HttpResponseFinalize { .. }
        | Request::HttpResponseDiscard { .. }
        | Request::HttpResponseRead { .. }
        | Request::HttpResponseRemove { .. }
        | Request::HttpEvidenceList {}
        | Request::HttpEvidenceRead { .. }
        | Request::HttpEvidenceRemove { .. } => Ok(()),
        Request::WebSocketRouteAdd { url, .. } => {
            crate::browser::socket_routes::validate_url(url).map_err(Error::from)
        }
        Request::WebSocketRouteAddMatcher { matcher, .. } => {
            matcher.prepare().map(|_| ()).map_err(Error::from)
        }
        Request::WebSocketRouteList { .. } | Request::WebSocketRouteRemove { .. } => Ok(()),
        Request::WebSocketRouteSend {
            socket_id, message, ..
        } => crate::browser::socket_routes::validate_send(socket_id, message).map_err(Error::from),
        Request::WebSocketRouteClose {
            socket_id,
            to,
            options,
            ..
        } => crate::browser::socket_routes::validate_close(socket_id, *to, options)
            .map_err(Error::from),
        Request::WebSocketRouteHandlers {
            socket_id,
            handlers,
            ..
        } => crate::browser::socket_routes::validate_handlers(socket_id, handlers)
            .map_err(Error::from),
        Request::BindingAdd { name, .. } => {
            crate::browser::bindings::validate(name).map_err(Error::from)
        }
        Request::BindingRead { .. }
        | Request::BindingList { .. }
        | Request::BindingRemove { .. } => Ok(()),
        Request::CallbackResolve {
            response: contract::CallbackResponse::BindingReject {},
            ..
        } => Ok(()),
        Request::CallbackResolve {
            response: contract::CallbackResponse::BindingResolve { value },
            ..
        } => crate::browser::values::argument(value)
            .map(|_| ())
            .map_err(Error::from),
        Request::CallbackResolve {
            response:
                contract::CallbackResponse::BindingResolveValue { value }
                | contract::CallbackResponse::BindingRejectValue { value },
            ..
        } => crate::browser::bindings::value_argument(value)
            .map(|_| ())
            .map_err(Error::from),
        Request::NetworkRoute { action, .. } => action.validate().map_err(Error::from),
        Request::PageRouteSet {
            url: None,
            authentication_challenges: true,
            ..
        }
        | Request::ContextRouteSet {
            url: None,
            authentication_challenges: true,
            ..
        } => Err("AUTH_ROUTE_REQUIRED".into()),
        Request::CallbackResolve {
            response: contract::CallbackResponse::AuthCredentials { username, password },
            ..
        } => crate::browser::routing::credentials(username, password).map_err(Error::from),
        Request::CallbackResolve {
            response:
                contract::CallbackResponse::RouteContinueWith { overrides }
                | contract::CallbackResponse::RouteFallbackWith { overrides },
            ..
        } => overrides.validate().map_err(Error::from),
        Request::PageRouteSet { url, .. } | Request::ContextRouteSet { url, .. } => url
            .as_deref()
            .map_or(Ok(()), crate::browser::routing::validate_url)
            .map_err(Error::from),
        Request::CallbackResolve {
            response:
                contract::CallbackResponse::RouteFulfill {
                    status,
                    body,
                    json,
                    content_type,
                    headers,
                    ..
                },
            ..
        } => crate::browser::routing::text_fulfill(
            *status,
            body.as_deref(),
            json.as_ref(),
            content_type.as_deref(),
            headers,
            None,
        )
        .and_then(|(status, body, headers)| {
            crate::browser::routing::inline_response(status, &body, &headers)
        })
        .map(|_| ())
        .map_err(Error::from),
        Request::CallbackResolve {
            response:
                contract::CallbackResponse::RouteFulfillBinary {
                    status,
                    body_base64,
                    headers,
                },
            ..
        } => crate::browser::routing::inline_response(*status, body_base64, headers)
            .map(|_| ())
            .map_err(Error::from),
        Request::CallbackResolve {
            response:
                contract::CallbackResponse::RouteFulfillHttpEvidence {
                    status, headers, ..
                },
            ..
        } => crate::browser::routing::binary_response(status.unwrap_or(200), "", headers)
            .map(|_| ())
            .map_err(Error::from),
        Request::CallbackResolve {
            response:
                contract::CallbackResponse::RouteFulfillEvidence {
                    status, headers, ..
                },
            ..
        } => crate::browser::routing::binary_response(*status, "", headers)
            .map(|_| ())
            .map_err(Error::from),
        Request::Batch { requests } => {
            if !(1..=8).contains(&requests.len()) {
                return Err("BATCH_BOUNDS".into());
            }
            for request in requests {
                if matches!(request, Request::Batch { .. } | Request::InputRelease {}) {
                    return Err("INVALID_BATCH_ACTION".into());
                }
                supported(request)?;
            }
            Ok(())
        }
        Request::ContextCreate { options } => chrome_options(options),
        Request::ContextConfigure {
            setting: crate::browser::settings::ContextSetting::ExtraHttpHeaders { headers },
            ..
        } => crate::browser::http::validate_headers(headers),
        Request::ContextConfigure {
            setting: crate::browser::settings::ContextSetting::HttpCredentials { credentials },
            ..
        } => credentials
            .as_ref()
            .map_or(Ok(()), |value| value.validate().map_err(Error::from)),
        Request::ContextConfigure { setting, .. } => {
            setting.validate()?;
            if let crate::browser::settings::ContextSetting::PermissionsGrant {
                permissions, ..
            } = setting
            {
                crate::browser::settings::validate_permissions(
                    permissions,
                    Some(crate::browser::contract::Engine::Chrome),
                )?;
            }
            Ok(())
        }
        Request::ContextClock { action, .. } => action.validate().map_err(Error::from),
        Request::ContextCredentials { action, .. } => action.validate().map_err(Error::from),
        Request::AuthBrokerList { .. } => Ok(()),
        Request::AuthSignIn {
            broker,
            account,
            journey,
            expected_identity,
            ..
        } => crate::browser::authentication::validate_sign_in(
            broker,
            account,
            journey,
            expected_identity.as_deref(),
        )
        .map_err(Error::from),
        Request::AuthStatus {
            journey,
            expected_identity,
            ..
        } => crate::browser::authentication::validate_status(journey, expected_identity.as_deref())
            .map_err(Error::from),
        Request::PageEmulateMedia { options, .. } => options.validate().map_err(Error::from),
        Request::PageEmulateThrottling { options, .. } => options.validate().map_err(Error::from),
        Request::StorageRegister { path } if !std::path::Path::new(path).is_absolute() => {
            Err("PRIVATE_STATE_REQUIRED".into())
        }
        Request::StorageExport { options, .. } => options
            .validate(contract::Engine::Chrome)
            .map_err(Error::from),
        Request::StorageRegister { .. }
        | Request::StorageApply { .. }
        | Request::StorageForget { .. } => Ok(()),
        Request::ContextCacheStorageRead {
            origin,
            cache_name,
            url,
            cursor,
            limit,
            ..
        } => crate::browser::storage::cache::validate_read(
            origin,
            url.as_deref(),
            cache_name.as_deref(),
            cursor.as_deref(),
            *limit,
        ),
        Request::ContextCacheStorageDelete { origin, .. } => {
            crate::browser::storage::cache::validate_origin(origin)
        }
        Request::ContextCookiesRead { urls, .. } => urls.parse().map(|_| ()),
        Request::ContextCookiesAdd { .. } => Ok(()),
        Request::ContextCookiesClear { options, .. } => options.validate(),
        Request::PageWebStorageRead {
            name,
            cursor,
            limit,
            ..
        } => {
            crate::browser::storage::web::validate_read(name.as_deref(), cursor.as_deref(), *limit)
        }
        Request::PageWebStorageUpdate { update, .. } => update.validate(),
        Request::PageSetExtraHttpHeaders { headers, .. } => {
            crate::browser::http::validate_headers(headers)
        }
        Request::CallbackResolve {
            response:
                contract::CallbackResponse::RouteAbort { .. }
                | contract::CallbackResponse::RouteContinue {}
                | contract::CallbackResponse::RouteFallback {}
                | contract::CallbackResponse::AuthCancel {}
                | contract::CallbackResponse::AuthDefault {},
            ..
        } => Ok(()),
        Request::DeveloperOpen { panel, .. } => {
            if panel
                .as_ref()
                .is_some_and(|panel| !developer::PANELS.contains(&panel.as_str()))
            {
                return Err("INVALID_DEVTOOLS_PANEL".into());
            }
            Ok(())
        }
        Request::DeveloperStatus { .. } | Request::DeveloperClose { .. } => Ok(()),
        Request::PageAudit { options, .. } => options.validate().map_err(Error::from),
        Request::ProfileStart { options, .. } => options.validate().map_err(Error::from),
        Request::ProfileStatus { .. }
        | Request::ProfileStop { .. }
        | Request::ProfileTakeCoverage { .. } => Ok(()),
        Request::ProfileQuery { query, .. } => query.validate().map_err(Error::from),
        Request::PerformanceLoad { .. } | Request::PerformanceRelease { .. } => Ok(()),
        Request::FrameworkPrepare { .. }
        | Request::ContextFrameworkPrepare { .. }
        | Request::FrameworkOpen { .. }
        | Request::FrameworkRelease { .. } => Ok(()),
        Request::FrameworkQuery { query, .. } => query.validate().map_err(Error::from),
        Request::WebMcpDebug { action, .. } => action.validate().map_err(Error::from),
        Request::WebMcp { action, .. } => action.validate().map_err(Error::from),
        Request::BrowserExtension { .. } => Ok(()),
        Request::BrowserPwa {
            action: crate::browser::pwa::Action::Launch { url: Some(url), .. },
        } => crate::browser::pwa::standard_url(url)
            .map(|_| ())
            .map_err(Error::from),
        Request::BrowserPwa { .. } => Ok(()),
        Request::DeveloperToolsDiscover { .. }
        | Request::DeveloperToolsRelease { .. }
        | Request::DeveloperToolResultRead { .. } => Ok(()),
        Request::DeveloperToolCall {
            tool_id,
            parameters,
            ..
        } => {
            if tool_id.is_empty() || tool_id.len() > 32 {
                return Err("DEVELOPER_TOOL_ID_BOUNDS".into());
            }
            crate::browser::developer_tools::validate_parameters(parameters).map_err(Error::from)
        }
        Request::PerformanceQuery { query, .. } => query.validate().map_err(Error::from),
        Request::HeapCapture { max_bytes, .. } => {
            if (1024..=crate::browser::heap::MAX_BYTES).contains(max_bytes) {
                Ok(())
            } else {
                Err("HEAP_CAPTURE_BOUNDS".into())
            }
        }
        Request::HeapImport {
            path, max_bytes, ..
        } => crate::browser::heap::validate_import(path, *max_bytes).map_err(Error::from),
        Request::HeapLoad { .. } | Request::HeapRelease { .. } | Request::HeapStop { .. } => Ok(()),
        Request::HeapQuery {
            query,
            max_name_length,
            ..
        } => {
            query.validate()?;
            crate::browser::heap::name_length(*max_name_length).map_err(Error::from)
        }
        Request::HeapCompare {
            offset,
            limit,
            class_index,
            max_name_length,
            ..
        } => {
            crate::browser::heap::name_length(*max_name_length)?;
            if class_index.is_some_and(|index| index > 100000) {
                return Err("HEAP_QUERY_BOUNDS".into());
            }
            crate::browser::heap::page(*offset, *limit).map_err(Error::from)
        }
        Request::TraceStart { options, .. } => options.validate(true).map_err(Error::from),
        Request::ContextDebugger { .. } => Err("AUTOMATION_DEBUGGER_UNSUPPORTED".into()),
        Request::TraceStop {
            analysis_developer: Some(_),
            analysis_page: Some(_),
            ..
        } => Err("TRACE_ANALYSIS_OPTIONS_CONFLICT".into()),
        Request::TraceStop {
            file_path: Some(path),
            ..
        } => crate::browser::tracing::export::validate(path).map_err(Error::from),
        Request::TraceStatus {} | Request::TraceStop { .. } => Ok(()),
        Request::TraceChunkStart { .. }
        | Request::TraceChunkStop {}
        | Request::TraceGroupStart { .. }
        | Request::TraceGroupEnd {} => Err("TRACE_ARCHIVE_UNSUPPORTED".into()),
        Request::PageDebugger { action, .. } => action.validate().map_err(Error::from),
        Request::DeveloperEvidenceList {} | Request::DeveloperEvidenceRemove { .. } => Ok(()),
        Request::DeveloperEvidenceFinalize { .. } => Ok(()),
        Request::PageInitScriptAdd { source, .. }
        | Request::ContextInitScriptAdd { source, .. } => {
            crate::browser::scripts::validate(source).map_err(Error::from)
        }
        Request::PageInitScriptList { .. }
        | Request::PageInitScriptRemove { .. }
        | Request::ContextInitScriptList { .. }
        | Request::ContextInitScriptRemove { .. } => Ok(()),
        Request::DiagnosticsArgumentsRead { query, .. } => query.validate().map_err(Error::from),
        Request::DiagnosticsArgumentsSet { .. }
        | Request::DiagnosticsArgumentsList { .. }
        | Request::DiagnosticsArgumentsRemove { .. } => Ok(()),
        Request::NetworkRead { options, .. } => options.validate().map_err(Error::from),
        Request::NetworkEvidenceList {} | Request::NetworkEvidenceRemove { .. } => Ok(()),
        Request::Evaluate {
            expression,
            argument,
            ..
        } => {
            if expression.is_empty() || expression.len() > 8192 {
                return Err("EXPRESSION_BOUNDS".into());
            }
            crate::browser::evaluation::validate(argument).map_err(Error::from)
        }
        Request::SelectorEvaluate { options, .. } => options.validate().map_err(Error::from),
        Request::WorkerList { .. }
        | Request::ContextWorkerList { .. }
        | Request::HandleRead { .. }
        | Request::HandleOwnerFrame { .. }
        | Request::HandleContentFrame { .. }
        | Request::FrameElement { .. }
        | Request::FrameRetain { .. }
        | Request::FrameOwnerRelease { .. }
        | Request::PageRequestGc { .. }
        | Request::HandleDispose { .. } => Ok(()),
        Request::HandleProperty { name, .. } => {
            if name.len() > 1024 {
                return Err("PROPERTY_NAME_BOUNDS".into());
            }
            Ok(())
        }
        Request::HandleProperties { query, .. } => query.validate().map_err(Error::from),
        Request::DiagnosticsStart { .. } | Request::DiagnosticsStop { .. } => Ok(()),
        Request::DiagnosticsRead { query, .. } => query.validate().map_err(Error::from),
        Request::DiagnosticsWait {
            after, conditions, ..
        } => {
            if *after > crate::browser::activity::MAX_CURSOR || !(1..=8).contains(&conditions.len())
            {
                return Err("INVALID_EVENT_CONDITION".into());
            }
            for condition in conditions {
                condition.validate()?;
            }
            Ok(())
        }
        Request::ContextList {}
        | Request::ContextSelect { .. }
        | Request::ContextClose { .. }
        | Request::PageCreate { .. }
        | Request::PageList {}
        | Request::PageSelect { .. }
        | Request::PageClose { .. } => Ok(()),
        Request::PageRead {
            locator,
            read,
            max_bytes,
            ..
        } => page::validate_read(locator, read, *max_bytes),
        Request::PageNavigate { navigation, .. } => navigation.validate().map_err(Error::from),
        Request::PageReadUrl { url, max_bytes, .. } => {
            crate::browser::page::validate_read_url(url, *max_bytes).map_err(Error::from)
        }
        Request::PageScreenshot { capture, .. } => capture.validate().map_err(Error::from),
        Request::PageResize { viewport, .. } => ContextOptions {
            viewport: *viewport,
            ..ContextOptions::default()
        }
        .validate()
        .map_err(Error::from),
        Request::PageSnapshot { locator, .. } => locator
            .as_ref()
            .map(|locator| locator.selector())
            .transpose()
            .map(|_| ())
            .map_err(Error::from),
        Request::PageAnnotate { markers, .. } => {
            crate::browser::evidence::validate_markers(markers).map_err(Error::from)
        }
        Request::PagePdf { options, .. } => options.validate().map_err(Error::from),
        Request::PreviewStart { options, .. } => options.validate().map_err(Error::from),
        Request::RecordingStart { options, .. } => options.validate().map_err(Error::from),
        Request::PreviewStatus { .. }
        | Request::PreviewStop { .. }
        | Request::RecordingStatus { .. }
        | Request::RecordingStop { .. } => Ok(()),
        Request::CaptureCompare {
            threshold,
            max_changed_ratio,
            ..
        } => crate::browser::capture::validate_comparison(*threshold, *max_changed_ratio)
            .map_err(Error::from),
        Request::PageClipboard { origin, action, .. } => {
            crate::browser::clipboard::validate(origin, action).map_err(Error::from)
        }
        Request::PageCss {
            locator,
            offset,
            limit,
            ..
        } => {
            locator.selector()?;
            if *offset > 100000 || !(1..=50).contains(limit) {
                return Err("CSS_QUERY_BOUNDS".into());
            }
            Ok(())
        }
        Request::PageStyles {
            locator,
            properties,
            max_bytes,
            ..
        } => crate::browser::page::validate_styles(locator, properties, *max_bytes)
            .map_err(Error::from),
        Request::PageAction {
            locator,
            action,
            force,
            postcondition,
            ..
        } => {
            locator.selector()?;
            action.validate(*force)?;
            if matches!(action, crate::browser::page::Action::AtSource { .. }) {
                return Err("CALL_SOURCE_UNSUPPORTED".into());
            }
            if let crate::browser::page::Action::Press { key } = action {
                crate::browser::keyboard::press_keys(key)?;
            }
            if let Some(condition) = postcondition {
                condition.validate()?;
            }
            Ok(())
        }
        Request::CallbackResolve {
            response: contract::CallbackResponse::Dialog { prompt_text, .. },
            ..
        } => {
            if prompt_text.as_ref().is_some_and(|text| text.len() > 16384) {
                return Err("INPUT_BOUNDS".into());
            }
            Ok(())
        }
        Request::EventsRead { cursor, limit, .. } => {
            canonic_mcp_kit::page::Request::from_cli(
                cursor.as_deref(),
                Some(*limit as u64),
                canonic_mcp_kit::page::Limits::STANDARD,
            )
            .map_err(|error| Error::from(error.code()))?;
            Ok(())
        }
        Request::EventsWait {
            after, conditions, ..
        } => {
            if *after > crate::browser::activity::MAX_CURSOR || !(1..=8).contains(&conditions.len())
            {
                return Err("EVENT_QUERY_BOUNDS".into());
            }
            Ok(())
        }
        Request::FixtureRegister { fixture } => fixture.validate().map_err(Error::from),
        Request::DirectoryRegister { name, entries } => {
            crate::browser::directory::validate(name, entries).map_err(Error::from)
        }
        Request::FixtureList {} | Request::FixtureVerify { .. } => Ok(()),
        Request::PageInput {
            actions,
            observation,
            ..
        } => crate::browser::input::validate(actions, *observation).map_err(Error::from),
        Request::InputRelease {} => Ok(()),
        Request::ChooserResolve { fixtures, .. } => {
            if fixtures.len() > 16 {
                Err("FIXTURE_BOUNDS".into())
            } else {
                Ok(())
            }
        }
        Request::DownloadList {}
        | Request::DownloadStatus { .. }
        | Request::DownloadCancel { .. }
        | Request::DownloadDelete { .. } => Ok(()),
        Request::PageUpload {
            locator, fixtures, ..
        } => {
            locator.selector()?;
            if fixtures.len() > 16 {
                return Err("FIXTURE_BOUNDS".into());
            }
            Ok(())
        }
        Request::FrameEnter { locator, .. } => locator.selector().map(|_| ()).map_err(Error::from),
        Request::PageWait {
            locator, condition, ..
        } => conditions::validate(locator, condition),
        _ => Err("CAPABILITY_UNAVAILABLE".into()),
    }
}

fn chrome_options(options: &ContextOptions) -> Result<(), Error> {
    options
        .validate_for(crate::browser::contract::Engine::Chrome)
        .map_err(Error::from)
}

mod actions;
mod ancestor_guards;
mod app_tabs;
mod audits;
mod authentication;
mod bindings;
mod capture;
mod choosers;
mod client_certificates;
mod clock;
mod conditions;
mod control;
mod coordinate_input;
mod credentials;
mod debugging;
mod developer;
mod developer_result;
mod developer_tools;
mod diagnostic_arguments;
mod diagnostics;
mod directories;
mod downloads;
mod emulation;
mod evaluation;
mod evidence;
mod execution;
mod execution_groups;
mod extensions;
mod fetch;
mod frame_visibility;
mod framework;
mod har;
mod har_capture;
mod heap;
mod http;
mod http_state;
mod init_scripts;
mod interactions;
mod media;
mod media_frames;
mod media_recording;
mod native_input;
mod navigation;
mod page;
mod performance;
mod profiling;
mod pwa;
mod read_url;
mod routing;
mod socket_routes;
mod startup;
mod storage;
mod styles;
mod supervisor;
mod tracing;
mod transfers;
mod trusted_sign_in;
mod webmcp_debug;
mod websockets;
mod worker;
mod workflows;
fn resource_error(error: super::ResourceError) -> Error {
    use super::ResourceError;
    match error {
        ResourceError::Foreign => "FOREIGN_REFERENCE",
        ResourceError::Stale => "STALE_REFERENCE",
        ResourceError::Scope => "INVALID_SCOPE",
        ResourceError::Capacity => "RESOURCE_CAPACITY",
        ResourceError::Protocol => "BACKEND_PROTOCOL",
        ResourceError::Lost => "BACKEND_LOST",
    }
    .into()
}
fn protocol_error(error: super::Error) -> Error {
    use super::{Dispatch, Error as E};
    // Keep backend data private; retain a bounded message for diagnosis.
    if let E::Remote(super::RemoteError::Command { code, message, .. }) = &error {
        let message: String = message.chars().take(2048).collect();
        let partial = json!({"backendFailure":{"source":"cdp-reply","kind":"method-error","dispatch":"replied","code":code,"message":message}});
        let mut error = Error::caused("BROWSER_METHOD_FAILED", error);
        error.partial = Some(partial);
        return error;
    }
    match error {
        E::Timeout {
            dispatch: Dispatch::NotDispatched,
        }
        | E::ScopeClosed {
            dispatch: Dispatch::NotDispatched,
        } => "NOT_DISPATCHED",
        E::Timeout { .. }
        | E::ScopeClosed { .. }
        | E::Lost {
            dispatch: Dispatch::PossiblyDispatched,
            ..
        } => "OUTCOME_UNKNOWN",
        E::Lost { .. } => "BACKEND_LOST",
        E::Busy => "BACKEND_BUSY",
        E::ScopeClosing => "RESOURCE_CLOSING",
        E::RetirementCapacity => "NATIVE_RETIREMENT_CAPACITY",
        E::Remote(_) => "BROWSER_METHOD_FAILED",
        _ => "BACKEND_PROTOCOL",
    }
    .into()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::lost_with_fixture;

#[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
mod native;
