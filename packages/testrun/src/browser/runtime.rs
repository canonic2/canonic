//! Browser connections inside the manager's pool. The pool holds any number
//! of browser connections, each a `browser:<connectionId>` target addressed
//! like a device and usable by every client.
use super::{
    contract::{ConnectOptions, Request},
    owned::{Bundle, Session},
};
use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;
use uuid::Uuid;

mod status_record;

pub(crate) fn launch_options_error(code: &'static str) -> Error {
    let message = match code {
        "MEDIA_FIXTURES_UNSUPPORTED" => "mediaFixtures require engine chrome",
        "MEDIA_FIXTURE_PATH_INVALID" => "Media fixtures require a bounded host file path",
        "BACKEND_UNAVAILABLE" => "Unsupported browser engine",
        "BROWSER_CHANNEL_UNSUPPORTED" => "Installed Chrome and Edge channels require engine chrome",
        "PROFILE_SELECTION_INVALID" => {
            "Use a retained profile name or absolute borrowed directory path"
        }
        "DISPOSABLE_PROFILE_UNSUPPORTED" => {
            "The disposable regular profile requires the Testrun-managed Chrome for Testing build"
        }
        "PROFILE_MEDIA_FIXTURES_UNSUPPORTED" => "Media fixtures require an isolated launch",
        "PROFILE_INITIAL_STORAGE_UNSUPPORTED" => {
            "Use an explicit storage_apply after connecting to the regular profile context"
        }
        "PROFILE_CONTEXT_NETWORK_UNSUPPORTED" => {
            "A Chrome profile supports launch proxy defaults; context proxy and client certificates require an isolated context"
        }
        "ATTACHMENT_LAUNCH_OPTIONS_UNSUPPORTED" => {
            "Attachment selects a running browser; use only attachment and new-context options"
        }
        "ATTACHMENT_ADOPTED_CONTEXT_OPTIONS_UNSUPPORTED" => {
            "Creation options require an isolated context created by this connection"
        }
        "ATTACHMENT_CONTEXT_HOST_RESOURCES_UNSUPPORTED" => {
            "Attached contexts cannot use a host download directory or Chrome's host client-certificate tunnel"
        }
        "ATTACHMENT_ENDPOINT_INVALID" => {
            "Use a loopback ws or an authenticated wss endpoint without credentials, queries or fragments"
        }
        "ATTACHMENT_ADOPTION_UNSUPPORTED" => {
            "Only Chrome CDP can adopt an existing isolated context"
        }
        _ => "Unsupported browser launch options",
    };
    Error::new(code, message)
}

/// The target ID of browser connection `connection`.
pub(crate) fn target(connection: Uuid) -> String {
    format!("{TARGET_PREFIX}{connection}")
}
pub(crate) const TARGET_PREFIX: &str = "browser:";

/// The browser connection a `browser:` target ID names.
pub(crate) fn connection(target: &str) -> Result<Uuid> {
    target
        .strip_prefix(TARGET_PREFIX)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| {
            Error::invalid("A browser targetId is browser:<connectionId> from browser_connect")
        })
}

#[derive(Default)]
pub(crate) struct Owners {
    /// Each session's browser connections by connection ID.
    entries: Mutex<HashMap<String, std::collections::BTreeMap<Uuid, Arc<Entry>>>>,
    closed: Mutex<std::collections::BTreeSet<String>>,
}
struct Entry {
    connection: Uuid,
    state: Mutex<Starting>,
    task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    changed: tokio::sync::Notify,
}
struct Starting {
    operation: Uuid,
    cancel_requested: bool,
    settled: bool,
    stage: String,
    session: Option<Session>,
    failure: Option<String>,
    closing: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Command {
    request: Request,
    #[serde(default)]
    operation_id: Option<Uuid>,
    #[serde(default = "timeout")]
    timeout_ms: u64,
}
fn timeout() -> u64 {
    30000
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Status {
    #[serde(default)]
    operation_id: Option<Uuid>,
    #[serde(default)]
    wait_ms: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cancel {
    operation_id: Uuid,
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|error| Error::invalid(error.to_string()))
}
fn lifecycle(error: super::session::Error) -> Error {
    let message = match error.code {
        "invalid_cursor" => "invalid_cursor: restart without a cursor",
        "CAPABILITY_UNAVAILABLE" => {
            "Read browser_doctor for this engine's operation scope and use a supported request"
        }
        "PRIVATE_STATE_REQUIRED" => {
            "Use an absolute private regular state JSON file in a private directory owned by this user"
        }
        "STORAGE_BOUNDS" => "Use a storage-state file of at most 1 MiB",
        "STORAGE_CAPACITY" => "Forget an owned storage snapshot before registering another",
        "STORAGE_MERGE_UNSUPPORTED" => {
            "Firefox and WebKit only replace storage; use mode replace, or merge on Chrome"
        }
        "STORAGE_FORMAT_UNSUPPORTED" => {
            "Register a testrun-chrome-storage-v1 or Playwright storage-state file"
        }
        "STORAGE_BACKEND_MISMATCH" => {
            "Register a snapshot that Chrome exported, or a Playwright storage-state file"
        }
        "STORAGE_TYPE_NOT_INCLUDED" => {
            "Declare every storage type the snapshot's origins carry in its included section"
        }
        "CACHE_STORAGE_UNSUPPORTED" => "Cache Storage is available on Chrome only",
        "CACHE_STORAGE_PAGE_REQUIRED" => {
            "Open a page in the context with page_create, then read its Cache Storage"
        }
        "CACHE_STORAGE_ENTRY_BOUNDS" => {
            "The cache holds more than 1000 entries; read one entry by url instead"
        }
        "WEB_STORAGE_READ_BOUNDS" | "CACHE_STORAGE_READ_BOUNDS" => {
            "The stored value exceeds 1 MiB and cannot be returned"
        }
        "EVENT_HISTORY_EXPIRED" => {
            "Read events_read for the retained cursor range and inspect current state before starting a new event wait; do not repeat the triggering action automatically"
        }
        "EVENT_CURSOR_AHEAD" => "Use a cursor returned by events_read for this browser connection",
        "CONTROL_NOT_OWNED" | "STALE_CONTROL_LEASE" | "CONTROL_HANDOFF_PENDING" => {
            "Read control_status and explicitly acquire the current controlVersion before new input"
        }
        "SESSION_UNCERTAIN" => {
            "Close owned pages with uncertain input before retrying control handoff"
        }
        _ => "Browser lifecycle request refused; inspect browser_status",
    };
    let mut result = Error::new(error.code, message);
    if let Some(partial) = error.partial {
        let next = if matches!(
            error.code,
            "SESSION_UNCERTAIN" | "CONTROL_HANDOFF_FAILED" | "STALE_CONTROL_LEASE"
        ) {
            "control_status"
        } else {
            "browser_status"
        };
        result = result.details(json!({"partial":partial,"nextAction":next}));
    }
    result
}

impl Owners {
    #[cfg(test)]
    pub(crate) fn fixture_diagnostic(
        &self,
        pool: &str,
        state: super::lifecycle::State,
        directory: &Path,
    ) -> Uuid {
        self.fixture_chrome(
            pool,
            state.connection,
            super::cdp::session::Session::fixture_retained(state, directory),
        )
    }
    #[cfg(test)]
    pub(crate) fn fixture_chrome(
        &self,
        pool: &str,
        connection: Uuid,
        session: super::cdp::session::Session,
    ) -> Uuid {
        self.entries
            .lock()
            .unwrap()
            .entry(pool.into())
            .or_default()
            .insert(
                connection,
                Arc::new(Entry {
                    connection,
                    state: Mutex::new(Starting {
                        operation: Uuid::new_v4(),
                        cancel_requested: false,
                        settled: true,
                        stage: "ready".into(),
                        session: Some(Session::Chrome(session)),
                        failure: None,
                        closing: false,
                    }),
                    task: Default::default(),
                    changed: Default::default(),
                }),
            );
        connection
    }
    /// Borrow one exact settled operation. No status scan, target queue or native I/O.
    pub(crate) fn diagnostic_operation(
        &self,
        pool: &str,
        target: &str,
        operation: &str,
        request: &Value,
    ) -> Result<Value> {
        let connection = self::connection(target)?;
        let id = Uuid::parse_str(operation)
            .map_err(|_| Error::invalid("Expected browser operation UUID"))?;
        let request: Request = serde_json::from_value(request.clone())
            .map_err(|_| Error::invalid("Expected the original typed browser request"))?;
        let entry = self.resolve(pool, &json!({}), Some(connection))?.unwrap();
        let session = entry
            .state
            .lock()
            .expect("Browser startup")
            .session
            .clone()
            .ok_or_else(|| {
                Error::new(
                    "BROWSER_PREPARING",
                    "Select a ready browser with a retained settled operation",
                )
            })?;
        session.diagnostic_operation(id, &request)
    }
    #[cfg(test)]
    pub(crate) fn fixture_preparing(&self, id: &str) -> Uuid {
        let connection = Uuid::new_v4();
        self.entries
            .lock()
            .unwrap()
            .entry(id.to_owned())
            .or_default()
            .insert(
                connection,
                Arc::new(Entry {
                    connection,
                    state: Mutex::new(Starting {
                        operation: Uuid::new_v4(),
                        cancel_requested: false,
                        settled: false,
                        stage: "preparing".into(),
                        session: None,
                        failure: Some("CLEANUP_PENDING".into()),
                        closing: true,
                    }),
                    task: Arc::default(),
                    changed: Default::default(),
                }),
            );
        connection
    }
    /// The browser connections session `id` holds.
    pub(crate) fn connections(&self, id: &str) -> Vec<Uuid> {
        self.entries
            .lock()
            .expect("Browser owners")
            .get(id)
            .map(|entries| entries.keys().copied().collect())
            .unwrap_or_default()
    }
    /// Run a browser request other than connect on the connection it names:
    /// `target`, a reference or operation in its arguments, or the only
    /// connected browser. List status pages the browsers in the requesting pool.
    fn status_page(&self, id: &str, args: &Value) -> Result<Value> {
        let limit = compact_limit(args, false)?;
        let entries = self.entries.lock().expect("Browser owners");
        let page = crate::pagination::collect(
            "browser_status",
            &id,
            args,
            entries
                .get(id)
                .into_iter()
                .flat_map(|entries| entries.iter())
                .map(|(id, entry)| (id.as_bytes().to_vec(), entry)),
            |entry| Ok(entry.status_snapshot(limit)),
            "browser_status with targetId",
        )?;
        Ok(
            json!({"browsers":page.items,"nextCursor":page.next_cursor,"testrunVersion":env!("CARGO_PKG_VERSION")}),
        )
    }
    pub(crate) async fn call(
        &self,
        id: &str,
        name: &str,
        args: Value,
        target: Option<Uuid>,
    ) -> Result<Value> {
        let is_status = name == "browser_status";
        let waiting = is_status && args["waitMs"].as_u64().unwrap_or(0) > 0;
        if is_status && target.is_none() && args["operationId"].is_null() && !waiting {
            return self.status_page(id, &args);
        }
        let entry = match self.resolve(id, &args, target)? {
            Some(entry) => entry,
            None => {
                if waiting {
                    return Err(Error::new(
                        "TARGET_REQUIRED",
                        "Name one browser with targetId or operationId to wait for it",
                    ));
                }
                return Ok(json!({"browsers":self.snapshots(id)}));
            }
        };
        if is_status {
            return entry.inspect_status(args).await;
        }
        if name == "browser_cancel" {
            let command: Cancel = parse(args.clone())?;
            let mut state = entry.state.lock().expect("Browser startup");
            if command.operation_id == state.operation {
                if !state.settled {
                    state.cancel_requested = true;
                    state.stage = "cancelling_preparation".into();
                }
                let operation = state.operation();
                entry.changed.notify_waiters();
                return Ok(json!({"operation":operation}));
            }
        }
        let session = {
            let state = entry.state.lock().expect("Browser startup");
            if state.closing {
                return Err(Error::new("SESSION_CLOSING", "The browser is closing"));
            }
            state.session.clone().ok_or_else(|| {
                Error::new(
                    "BROWSER_PREPARING",
                    "Browser dependencies are still preparing; inspect browser_status",
                )
            })?
        };
        if name == "browser_control_status" {
            return Ok(session.control_status());
        }
        if matches!(name, "browser_control_acquire" | "browser_control_release") {
            return session.change_control(name, args).await.map_err(lifecycle);
        }
        match name {
            "browser_send" => {
                let command: Command = parse(args)?;
                let id = session
                    .submit(
                        command.operation_id.unwrap_or_else(Uuid::new_v4),
                        command.request,
                        command.timeout_ms,
                    )
                    .map_err(lifecycle)?;
                Ok(json!({"operation":session.operation(id)}))
            }
            "browser_cancel" => {
                let command: Cancel = parse(args)?;
                session
                    .cancel(command.operation_id)
                    .await
                    .map_err(lifecycle)?;
                Ok(json!({"operation":session.operation(command.operation_id)}))
            }
            _ => Err(Error::invalid("Unknown browser operation")),
        }
    }
    /// The connection a request names. `None` is a status of a session with
    /// several browsers and nothing naming one.
    fn resolve(&self, id: &str, args: &Value, target: Option<Uuid>) -> Result<Option<Arc<Entry>>> {
        let entries = self.entries.lock().expect("Browser owners");
        let held = entries.get(id);
        let find = |connection: Uuid| held.and_then(|entries| entries.get(&connection).cloned());
        if let Some(connection) = target {
            return find(connection).map(Some).ok_or_else(|| {
                Error::new("NOT_CONNECTED", "That browser is not connected").details(
                    json!({"targetId":self::target(connection),"nextAction":"browser_connect"}),
                )
            });
        }
        // A context, page or other reference names its connection.
        if let Some(connection) = referenced(&args["request"]).and_then(find) {
            return Ok(Some(connection));
        }
        // An operation belongs to the browser that accepted it.
        if let Some(operation) = args["operationId"]
            .as_str()
            .and_then(|operation| Uuid::parse_str(operation).ok())
            && let Some(entry) = held.and_then(|entries| {
                entries.values().find(|entry| {
                    let state = entry.state.lock().expect("Browser startup");
                    state.operation == operation
                        || state
                            .session
                            .as_ref()
                            .is_some_and(|session| session.operation(operation).is_some())
                })
            })
        {
            return Ok(Some(entry.clone()));
        }
        let all: Vec<&Arc<Entry>> = held
            .map(|entries| entries.values().collect())
            .unwrap_or_default();
        let names_nothing = args.get("request").is_none() && args.get("operationId").is_none();
        match all.as_slice() {
            [entry] => Ok(Some((*entry).clone())),
            [] => Err(Error::new(
                "NOT_CONNECTED",
                "No browser is connected; connect one first",
            )
            .details(json!({"nextAction":"browser_connect"}))),
            _ if names_nothing => Ok(None),
            many => Err(Error::new(
                "TARGET_REQUIRED",
                "Several browsers are connected; name one with targetId",
            )
            .details(json!({
                "admitted":false,"dispatched":false,
                "targets":many.iter().map(|entry| self::target(entry.connection)).collect::<Vec<_>>(),
            }))),
        }
    }
    /// Start browser connection `connection` for session `id`, beside any
    /// browsers it already holds.
    pub(crate) async fn connect(
        &self,
        id: &str,
        generation: u64,
        directory: &Path,
        connection: Uuid,
        options: ConnectOptions,
    ) -> Result<Value> {
        options.validate().map_err(launch_options_error)?;
        if super::processes::windows_bundle_host() {
            if options.channel.is_some() && !super::processes::windows_console_session() {
                return Err(Error::new(
                    "HOST_UNAVAILABLE",
                    "Windows installed browser channels require the active Windows console session",
                ));
            }
            if options.attachment.is_none()
                && !super::processes::windows_mode_supported(options.launch.mode)
            {
                return Err(Error::new(
                    "HOST_UNAVAILABLE",
                    "Headed browser launch requires the active Windows console session",
                ));
            }
        } else if !super::processes::initial_host() {
            return Err(Error::new(
                "HOST_UNAVAILABLE",
                "Browser launch requires macOS 15 x64 or Windows x64",
            ));
        }
        if self.closed.lock().expect("Browser end fences").len() >= 4096 {
            return Err(Error::new(
                "OWNER_CAPACITY",
                "Retire the empty owner to reset its browser incarnation fences",
            ));
        }
        let entry = Arc::new(Entry {
            connection,
            state: Mutex::new(Starting {
                operation: Uuid::new_v4(),
                cancel_requested: false,
                settled: false,
                stage: "preparing".into(),
                session: None,
                failure: None,
                closing: false,
            }),
            task: Arc::default(),
            changed: Default::default(),
        });
        let mut slot = entry.task.clone().lock_owned().await;
        {
            let mut entries = self.entries.lock().expect("Browser owners");
            if self.closed.lock().expect("Browser end fences").contains(id) {
                return Err(Error::new(
                    "SESSION_CLOSING",
                    "This browser session incarnation ended",
                ));
            }
            let held = entries.entry(id.to_owned()).or_default();
            if let Some(entry) = held.get(&connection) {
                return Ok(entry.snapshot());
            }
            held.insert(connection, entry.clone());
        }
        // Lock before publishing a task: cleanup cannot miss an allocation whose
        // worker has not yet been installed in its owner slot.
        let worker = entry.clone();
        let directory = directory.to_path_buf();
        let owner =
            Uuid::parse_str(id).map_err(|_| Error::invalid("Invalid trusted session ID"))?;
        let mut generation_bytes = [0u8; 16];
        generation_bytes[8..].copy_from_slice(&generation.to_be_bytes());
        let owner_generation = Uuid::from_bytes(generation_bytes);
        *slot = Some(tokio::spawn(worker.prepare(
            directory,
            owner,
            owner_generation,
            options,
        )));
        drop(slot);
        Ok(entry.snapshot())
    }
    /// Every browser of session `id`, for its durable record and status.
    pub(crate) fn snapshots(&self, id: &str) -> Vec<Value> {
        self.entries
            .lock()
            .expect("Browser owners")
            .get(id)
            .map(|entries| entries.values().map(|entry| entry.snapshot()).collect())
            .unwrap_or_default()
    }
    /// End session `id`'s browsers: every connection closes, and the ended
    /// incarnation starts no new browser. One failed close does not skip
    /// the others; the first failure is reported after all ran.
    pub(crate) async fn close(&self, id: &str) -> Result<()> {
        self.closed
            .lock()
            .expect("Browser end fences")
            .insert(id.to_owned());
        // Browsers close concurrently, so one slow preparation or disconnect
        // does not delay the others' release.
        let results = futures_util::future::join_all(
            self.connections(id)
                .into_iter()
                .map(|connection| self.close_connection(id, connection)),
        )
        .await;
        results
            .into_iter()
            .find_map(Result::err)
            .map_or(Ok(()), Err)
    }
    /// Close one browser connection of session `id`, keeping its others.
    pub(crate) async fn close_connection(&self, id: &str, connection: Uuid) -> Result<Value> {
        let entry = self
            .entries
            .lock()
            .expect("Browser owners")
            .get(id)
            .and_then(|entries| entries.get(&connection).cloned());
        let Some(entry) = entry else {
            return Ok(json!({"closed":true}));
        };
        entry.state.lock().expect("Browser startup").closing = true;
        let mut task = entry.task.lock().await;
        if let Some(worker) = task.as_mut() {
            tokio::time::timeout(Duration::from_secs(300), worker)
                .await
                .map_err(|_| cleanup_pending("Browser preparation is still finishing"))?
                .map_err(|_| Error::new("CLEANUP_PENDING", "Browser preparation worker stopped"))?;
        }
        *task = None;
        let session = entry.state.lock().expect("Browser startup").session.clone();
        let mut receipt = json!({"closed":true});
        if let Some(session) = session {
            let operation = session.disconnect(30000).map_err(lifecycle)?;
            let outcome = session
                .wait(operation, Instant::now() + Duration::from_secs(31))
                .await
                .map_err(lifecycle)?;
            if !outcome.settled || outcome.outcome != super::lifecycle::Outcome::Succeeded {
                let error = cleanup_pending(
                    "Browser cleanup is unfinished. The manager will retry, or run browser_disconnect to retry now.",
                );
                let mut details = outcome
                    .result
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({}));
                details["operationId"] = json!(operation);
                details["operationCode"] = json!(outcome.code);
                return Err(error.details(details));
            }
            receipt = outcome.result.unwrap_or(receipt);
            // A concurrent close may already hold this entry. Retire its
            // session under the same task lock before removing the map entry.
            entry.state.lock().expect("Browser startup").session = None;
        }
        let mut entries = self.entries.lock().expect("Browser owners");
        if let Some(held) = entries.get_mut(id) {
            held.remove(&connection);
            if held.is_empty() {
                entries.remove(id);
            }
        }
        Ok(receipt)
    }
}

fn cleanup_pending(message: &str) -> Error {
    let mut error = Error::new("CLEANUP_PENDING", message);
    error.retryable = true;
    error
}

pub(super) const STATUS_OPERATION_LIMITS: canonic_mcp_kit::page::Limits =
    canonic_mcp_kit::page::Limits {
        default: 3,
        max: 16,
        ..canonic_mcp_kit::page::Limits::STANDARD
    };

fn compact_limit(args: &Value, single_browser: bool) -> Result<Option<usize>> {
    use canonic_mcp_kit::page::{Limits, Request};
    let compact = args["includeDetails"] != true && args["operationId"].is_null();
    let limits = if single_browser && compact {
        STATUS_OPERATION_LIMITS
    } else {
        Limits::STANDARD
    };
    let page = Request::from_mcp(args, limits)?;
    let count = if single_browser {
        page.limit.get()
    } else {
        STATUS_OPERATION_LIMITS.default
    };
    Ok(compact.then(|| usize::try_from(count).expect("Bounded status limit")))
}

impl Entry {
    async fn inspect_status(&self, mut args: Value) -> Result<Value> {
        let limit = compact_limit(&args, true)?;
        if args.get("cursor").is_some() {
            return Err(canonic_mcp_kit::page::Error::InvalidCursor.into());
        }
        let properties = args.as_object_mut().expect("arguments");
        properties.remove("limit");
        properties.remove("includeDetails");
        let status: Status = parse(args)?;
        if status.wait_ms > 30000 {
            return Err(Error::invalid("waitMs must be at most 30000"));
        }
        if status.operation_id.is_none() && status.wait_ms == 0 {
            return Ok(self.status_snapshot(limit));
        }
        let deadline = Instant::now() + Duration::from_millis(status.wait_ms);
        if let Some(operation_id) = status.operation_id {
            let preparing = operation_id == self.state.lock().expect("Browser startup").operation;
            let mut result = json!({"targetId":self::target(self.connection),"testrunVersion":env!("CARGO_PKG_VERSION")});
            result["operation"] = if preparing {
                self.wait_preparation(deadline).await
            } else {
                let session = self
                    .state
                    .lock()
                    .expect("Browser startup")
                    .session
                    .clone()
                    .ok_or_else(|| lifecycle("UNKNOWN_OPERATION".into()))?;
                let operation = session
                    .wait(operation_id, deadline)
                    .await
                    .map_err(lifecycle)?;
                json!(operation)
            };
            return Ok(result);
        }
        let preparation = self.wait_preparation(deadline).await;
        let mut expired = preparation["settled"] != true;
        let session = self.state.lock().expect("Browser startup").session.clone();
        let operation = session.as_ref().and_then(Session::pending_operation);
        if let (Some(session), Some(operation)) = (session, operation) {
            expired = !session
                .wait(operation, deadline)
                .await
                .map_err(lifecycle)?
                .settled;
        }
        let mut snapshot = self.status_snapshot(limit);
        snapshot["waitExpired"] = json!(expired);
        if let Some(operation) = operation {
            snapshot["waitedOperationId"] = json!(operation);
        }
        Ok(snapshot)
    }

    async fn prepare(
        self: Arc<Self>,
        directory: PathBuf,
        owner: Uuid,
        owner_generation: Uuid,
        options: ConnectOptions,
    ) {
        let connection = self.connection;
        let result = async {
            let progress = |stage: &str| {
                let mut state = self.state.lock().expect("Browser startup");
                if !state.cancel_requested {
                    state.stage = stage.into();
                }
            };
            let disposable_profile = matches!(
                options.profile,
                Some(super::profiles::Selection::Disposable)
            );
            let bundle = if let Some(attachment) = options.attachment {
                Bundle::Attachment(attachment)
            } else if let Some(channel) = options.channel {
                progress("checking_installed_channel");
                Bundle::Chrome {
                    bundle: channel.select()?,
                    disposable_profile: false,
                }
            } else if options.launch.engine == super::contract::Engine::Chrome {
                Bundle::Chrome {
                    bundle: super::preparation::prepare_chrome(progress).await?,
                    disposable_profile,
                }
            } else {
                Bundle::Playwright(
                    super::preparation::prepare(options.launch.engine, progress).await?,
                )
            };
            let profile = options
                .profile
                .as_ref()
                .filter(|_| !disposable_profile)
                .map(|selection| {
                    let engine = options.launch.engine;
                    let version = match &bundle {
                        Bundle::Chrome { bundle, .. } => bundle
                            .installed
                            .as_ref()
                            .map(|installed| installed.version.as_str()),
                        Bundle::Playwright(_) | Bundle::Attachment(_) => None,
                    }
                    .or_else(|| engine.pinned_version())
                    .ok_or_else(|| Error::invalid("Unknown profile build"))?;
                    selection.acquire(&super::profiles::Identity {
                        engine,
                        channel: options.channel,
                        version,
                    })
                })
                .transpose()?;
            let state_directory = directory.join("browser");
            crate::managed::storage::directory(&state_directory)?;
            let mut state = self.state.lock().expect("Browser startup");
            if state.closing || state.cancel_requested {
                return Err(Error::new(
                    "CANCELLED",
                    "Browser preparation ended during cleanup",
                ));
            }
            let session = Session::launch_connection(
                bundle,
                connection,
                owner,
                owner_generation,
                options.launch,
                profile,
                &state_directory,
            )
            .map_err(lifecycle)?;
            state.stage = "starting".into();
            state.session = Some(session);
            state.settled = true;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            let _ = crate::managed::storage::write(
                &directory.join(format!("browser-preparation-error-{connection}.json")),
                &error,
            );
            let mut state = self.state.lock().expect("Browser startup");
            state.stage = if error.code == "CANCELLED" {
                "cancelled"
            } else {
                "failed"
            }
            .into();
            state.failure = Some(error.code);
            state.settled = true;
        }
        self.changed.notify_waiters();
    }

    async fn wait_preparation(&self, deadline: Instant) -> Value {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let operation = self.state.lock().expect("Browser startup").operation();
            if operation["settled"] == true {
                return operation;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                let mut operation = self.state.lock().expect("Browser startup").operation();
                if operation["settled"] != true {
                    operation["code"] = json!("WAIT_EXPIRED");
                }
                return operation;
            }
        }
    }
    fn snapshot(&self) -> Value {
        self.status_snapshot(None)
    }

    fn status_snapshot(&self, limit: Option<usize>) -> Value {
        let state = self.state.lock().expect("Browser startup");
        let browser = state.session.as_ref().map(|session| match limit {
            Some(limit) => session.compact_status(limit),
            None => session.status(),
        });
        let mut snapshot = json!({"targetId":target(self.connection),"preparationOperationId":state.operation,"stage":if state.session.is_some(){"launched"}else{&state.stage},"failure":state.failure,"closing":state.closing,"startupOperationId":state.session.as_ref().map(Session::startup),"browser":browser,"testrunVersion":env!("CARGO_PKG_VERSION")});
        snapshot["preparationOperation"] = state.operation();
        // A failed start stays listed so its cause can be read; disconnect it
        // before connecting again, or two browsers stay connected.
        if (state.failure.is_some() && state.session.is_none())
            || snapshot["browser"]["phase"] == "lost"
        {
            snapshot["nextAction"] = json!("browser_disconnect");
        }
        snapshot
    }
}

impl Starting {
    fn operation(&self) -> Value {
        let outcome = if !self.settled {
            "running"
        } else if self.failure.as_deref() == Some("CANCELLED") {
            "cancelled"
        } else if self.failure.is_some() {
            "failed"
        } else {
            "succeeded"
        };
        json!({"id":self.operation,"outcome":outcome,"settled":self.settled,"result":null,"code":self.failure,"cancelRequested":self.cancel_requested})
    }
}

/// Route by the operation's authority, never by data inside an evaluation
/// argument, fixture metadata or other page-supplied JSON.
fn referenced(request: &Value) -> Option<Uuid> {
    [
        "scope",
        "page",
        "context",
        "handle",
        "callback",
        "chooser",
        "download",
        "fixture",
        "preview",
        "recording",
        "snapshot",
    ]
    .into_iter()
    .find_map(|field| {
        serde_json::from_value::<super::contract::Reference>(request[field].clone()).ok()
    })
    .map(|reference| reference.connection_id)
}

/// Serve list/doctor in the client process without creating session references.
pub(crate) fn inspect(name: &str, args: Value) -> Result<Value> {
    match name {
        "browser_list" => {
            let engines = [
                ("chrome", super::contract::Engine::Chrome),
                ("firefox", super::contract::Engine::Firefox),
                ("webkit", super::contract::Engine::Webkit),
            ];
            let page = crate::pagination::collect(
                "browser_list",
                &(),
                &args,
                engines
                    .into_iter()
                    .map(|(id, engine)| (id.as_bytes().to_vec(), engine)),
                super::preparation::doctor,
                "browser_doctor",
            )?;
            Ok(json!({"browsers":page.items,"nextCursor":page.next_cursor}))
        }
        "browser_doctor" => {
            let input: super::surface::EngineInput = parse(args)?;
            super::preparation::doctor(input.engine)
        }
        _ => Err(Error::invalid("Unknown browser inspection")),
    }
}

/// Reuse the owning contracts without probing dependencies or opening a browser.
pub(crate) fn diagnostic_capabilities() -> Value {
    let host = super::processes::initial_host() || super::processes::windows_bundle_host();
    let engines = [super::contract::Engine::Chrome,super::contract::Engine::Firefox,super::contract::Engine::Webkit].map(|engine|
        json!({"engine":engine,"nativeContract":super::preparation::host_capabilities(engine).1}));
    json!({"registration":"implemented","availability":"unknown","hostSupported":host,
        "readiness":"Use browser_doctor for dependency readiness; no browser is started by this catalog",
        "engines":engines,
        "workflow":{"capture":"Use existing browser_send/act requests and retain operationId; poll browser_status until settled",
            "retain":"diagnostics_start source.kind:browser_operation with exact targetId, operationId and original request",
            "analyze":"diagnostics_analyze/query for retained operation/event witnesses; heap_query/performance_query remain native owners",
            "artifacts":"Native files/models remain browser-owned; explicit imports use their own format/size limits"},
        "gaps":["renderer PID/birth mapping unregistered","no automatic subscription, profile, debugger attach or pause","no archive or heap decoder inferred from an operation receipt"]})
}

pub(crate) async fn offline(
    directory: PathBuf,
    owner_present: bool,
    args: &Value,
) -> Result<Value> {
    let target = args["targetId"].as_str();
    let limit = compact_limit(args, target.is_some())?;
    let mut rows = status_record::read(&directory.join("browser status.json"))?;
    if let Some(operation_id) = args["operationId"].as_str() {
        if args.get("cursor").is_some() {
            return Err(canonic_mcp_kit::page::Error::InvalidCursor.into());
        }
        for row in rows
            .iter()
            .filter(|row| target.is_none_or(|target| row["targetId"] == target))
        {
            let operation = if row["preparationOperation"]["id"] == operation_id {
                Some(&row["preparationOperation"])
            } else {
                row["browser"]["operations"]
                    .as_array()
                    .and_then(|operations| {
                        operations
                            .iter()
                            .find(|operation| operation["id"] == operation_id)
                    })
            };
            if let Some(operation) = operation {
                return Ok(
                    json!({"targetId":row["targetId"],"operation":operation,"live":false,
                    "state":"not_connected","message":"Last known operation summary; no live session and no proof of cleanup"}),
                );
            }
        }
        return Err(lifecycle("UNKNOWN_OPERATION".into()));
    }
    let revision =
        canonic_mcp_kit::page::digest(serde_json::to_string(&rows).expect("JSON rows").as_bytes());
    rows.retain(|row| target.is_none_or(|target| row["targetId"] == target));
    rows.sort_by(|a, b| a["targetId"].as_str().cmp(&b["targetId"].as_str()));
    let page = crate::pagination::collect(
        "browser_status.offline",
        &(directory, revision, crate::pagination::filters(args)),
        args,
        rows.into_iter().map(|row| {
            (
                row["targetId"]
                    .as_str()
                    .unwrap_or_default()
                    .as_bytes()
                    .to_vec(),
                row,
            )
        }),
        |mut row| {
            if let Some(limit) = limit {
                status_record::compact(&mut row, limit);
            }
            Ok(row)
        },
        "browser_status with targetId",
    )?;
    Ok(
        json!({"live":false,"owner":if owner_present{"present"}else{"absent"},"browsers":page.items,"nextCursor":page.next_cursor,"state":"not_connected","message":"Last known browser records only; no live session and no proof of cleanup"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn browser_status_pages_live_and_offline_records_and_binds_the_snapshot() {
        let owners = Owners::default();
        let id = Uuid::new_v4().to_string();
        owners.fixture_preparing(&id);
        owners.fixture_preparing(&id);
        owners.fixture_preparing(&id);
        let first = owners
            .call(&id, "browser_status", json!({"limit":1}), None)
            .await
            .unwrap();
        assert_eq!(first["browsers"].as_array().unwrap().len(), 1);
        let next = owners
            .call(
                &id,
                "browser_status",
                json!({"cursor":first["nextCursor"],"limit":100}),
                None,
            )
            .await
            .unwrap();
        assert_eq!(next["browsers"].as_array().unwrap().len(), 2);
        assert!(next["nextCursor"].is_null());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state");
        crate::managed::storage::directory(&path).unwrap();
        let mut legacy = json!({"browsers":owners.snapshots(&id)});
        let operation = Uuid::new_v4();
        let pending = Uuid::new_v4();
        legacy["browsers"][0]["browser"] = json!({"operations":[{
            "id":operation,"outcome":"succeeded","settled":true,"result":{"body":"x".repeat(2 * 1024 * 1024)}
        }, {"id":Uuid::new_v4(),"outcome":"failed","settled":true,"result":null},
        {"id":Uuid::new_v4(),"outcome":"succeeded","settled":true,"result":null},
        {"id":pending,"outcome":"running","settled":false,"result":null}],"resources":[{}]});
        let record = path.join("browser status.json");
        crate::managed::storage::write(&record, &legacy).unwrap();
        assert!(std::fs::metadata(&record).unwrap().len() > 1024 * 1024);
        assert!(crate::managed::storage::read::<Value>(&record).is_err());
        let first = offline(path.clone(), false, &json!({"limit":1}))
            .await
            .unwrap();
        let operations = first["browsers"][0]["browser"]["operations"]
            .as_array()
            .unwrap();
        assert_eq!(operations.len(), 3);
        assert_eq!(operations[0]["id"], json!(pending));
        assert_eq!(operations[1]["id"], json!(operation));
        assert!(operations[1]["result"].is_null());
        assert_eq!(first["browsers"][0]["browser"]["operationsTotal"], 4);
        assert!(first["browsers"][0]["browser"].get("resources").is_none());
        let target = &first["browsers"][0]["targetId"];
        let targeted = offline(path.clone(), false, &json!({"targetId":target,"limit":1}))
            .await
            .unwrap();
        assert_eq!(
            targeted["browsers"][0]["browser"]["operations"],
            json!([operations[0]])
        );
        let selected = offline(path.clone(), false, &json!({"operationId":operation}))
            .await
            .unwrap();
        assert_eq!(selected["live"], false);
        assert_eq!(selected["operation"]["id"], json!(operation));
        assert!(selected.get("browsers").is_none());
        let args = json!({"cursor":first["nextCursor"]});
        let last = offline(path.clone(), false, &args).await.unwrap();
        assert_eq!(last["browsers"].as_array().unwrap().len(), 2);
        assert!(last["browsers"][0]["browser"].is_null());
        assert!(last["nextCursor"].is_null());
        crate::managed::storage::write(&path.join("browser status.json"), &json!({"browsers":[]}))
            .unwrap();
        assert_eq!(
            offline(path, false, &args).await.unwrap_err().code,
            "invalid_cursor"
        );
    }

    #[tokio::test]
    async fn browser_status_waits_without_an_operation_id() {
        let owners = Owners::default();
        let id = Uuid::new_v4().to_string();
        let connection = owners.fixture_preparing(&id);
        let status = owners
            .call(&id, "browser_status", json!({"waitMs":1}), None)
            .await
            .unwrap();
        assert_eq!(status["targetId"], target(connection));
        assert_eq!(status["waitExpired"], true);
        assert_eq!(status["preparationOperation"]["settled"], false);

        owners.fixture_preparing(&id);
        let error = owners
            .call(&id, "browser_status", json!({"waitMs":1}), None)
            .await
            .unwrap_err();
        assert_eq!(error.code, "TARGET_REQUIRED");
        let selected = owners
            .call(&id, "browser_status", json!({"waitMs":1}), Some(connection))
            .await
            .unwrap();
        assert_eq!(selected["waitExpired"], true);
    }

    #[tokio::test]
    async fn preparation_ids_route_cancel_and_retain_the_installation_until_settled() {
        let owners = Arc::new(Owners::default());
        let id = Uuid::new_v4().to_string();
        let connection = owners.fixture_preparing(&id);
        let entry = owners
            .resolve(&id, &json!({}), Some(connection))
            .unwrap()
            .unwrap();
        {
            let mut state = entry.state.lock().unwrap();
            state.closing = false;
            state.failure = None;
        }
        let operation = entry.state.lock().unwrap().operation;
        let (release, pending) = tokio::sync::oneshot::channel();
        let worker = entry.clone();
        *entry.task.lock().await = Some(tokio::spawn(async move {
            pending.await.unwrap();
            let mut state = worker.state.lock().unwrap();
            assert!(state.cancel_requested);
            assert!(state.session.is_none());
            state.failure = Some("CANCELLED".into());
            state.stage = "cancelled".into();
            state.settled = true;
            worker.changed.notify_waiters();
        }));
        owners.fixture_preparing(&id);
        let cancelled = owners
            .call(
                &id,
                "browser_cancel",
                json!({"operationId":operation}),
                None,
            )
            .await
            .unwrap();
        assert_eq!(cancelled["operation"]["cancelRequested"], true);
        assert_eq!(cancelled["operation"]["settled"], false);
        let status = owners
            .call(
                &id,
                "browser_status",
                json!({"operationId":operation}),
                None,
            )
            .await
            .unwrap();
        assert_eq!(status["operation"]["code"], "WAIT_EXPIRED");
        assert_eq!(status["operation"]["outcome"], "running");
        assert_eq!(owners.connections(&id).len(), 2);
        release.send(()).unwrap();
        let status = owners
            .call(
                &id,
                "browser_status",
                json!({"operationId":operation,"waitMs":30000}),
                None,
            )
            .await
            .unwrap();
        assert_eq!(status["operation"]["outcome"], "cancelled");
        assert_eq!(status["operation"]["settled"], true);
        assert!(entry.snapshot()["browser"].is_null());
        owners.close(&id).await.unwrap();
    }
    #[test]
    fn a_failed_start_stays_listed_and_points_to_disconnect() {
        let entry = |failure: Option<&str>| Entry {
            connection: Uuid::new_v4(),
            state: Mutex::new(Starting {
                operation: Uuid::new_v4(),
                cancel_requested: false,
                settled: false,
                stage: if failure.is_some() {
                    "failed"
                } else {
                    "preparing"
                }
                .into(),
                session: None,
                failure: failure.map(str::to_owned),
                closing: false,
            }),
            task: Arc::default(),
            changed: Default::default(),
        };
        let failed = entry(Some("BUNDLE_CORRUPT")).snapshot();
        assert_eq!(failed["failure"], "BUNDLE_CORRUPT");
        assert_eq!(failed["nextAction"], "browser_disconnect");
        assert!(entry(None).snapshot().get("nextAction").is_none());
    }

    #[tokio::test]
    async fn ending_a_session_closes_its_browsers_concurrently() {
        let owners = Arc::new(Owners::default());
        let id = Uuid::new_v4().to_string();
        let (release, blocked) = tokio::sync::oneshot::channel::<()>();
        let mut entries = Vec::new();
        for worker in [
            tokio::spawn(async move {
                let _ = blocked.await;
            }),
            tokio::spawn(async {}),
        ] {
            let connection = Uuid::new_v4();
            let entry = Arc::new(Entry {
                connection,
                state: Mutex::new(Starting {
                    operation: Uuid::new_v4(),
                    cancel_requested: false,
                    settled: false,
                    stage: "preparing".into(),
                    session: None,
                    failure: None,
                    closing: false,
                }),
                task: Arc::new(tokio::sync::Mutex::new(Some(worker))),
                changed: Default::default(),
            });
            owners
                .entries
                .lock()
                .unwrap()
                .entry(id.clone())
                .or_default()
                .insert(connection, entry.clone());
            entries.push(entry);
        }
        let closing = tokio::spawn({
            let owners = owners.clone();
            let id = id.clone();
            async move { owners.close(&id).await }
        });
        // Every browser starts closing while one preparation is still blocked.
        tokio::time::timeout(Duration::from_secs(5), async {
            while !entries
                .iter()
                .all(|entry| entry.state.lock().unwrap().closing)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("A blocked browser must not delay closing the others");
        assert_eq!(owners.connections(&id).len(), 1);
        release.send(()).unwrap();
        closing.await.unwrap().unwrap();
        assert!(owners.connections(&id).is_empty());
    }

    #[tokio::test]
    async fn ending_preparation_joins_its_worker_and_fences_reallocation() {
        let owners = Arc::new(Owners::default());
        let id = Uuid::new_v4().to_string();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn(async move {
            receiver.await.unwrap();
        });
        let connection = Uuid::new_v4();
        let entry = Arc::new(Entry {
            connection,
            state: Mutex::new(Starting {
                operation: Uuid::new_v4(),
                cancel_requested: false,
                settled: false,
                stage: "preparing".into(),
                session: None,
                failure: None,
                closing: false,
            }),
            task: Arc::new(tokio::sync::Mutex::new(Some(worker))),
            changed: Default::default(),
        });
        owners
            .entries
            .lock()
            .unwrap()
            .entry(id.clone())
            .or_default()
            .insert(connection, entry.clone());
        // A second browser of the same session closes with it.
        let other = owners.fixture_preparing(&id);
        assert_eq!(owners.connections(&id).len(), 2);
        let closing = tokio::spawn({
            let owners = owners.clone();
            let id = id.clone();
            async move { owners.close(&id).await }
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            while !entry.state.lock().unwrap().closing {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!closing.is_finished());
        sender.send(()).unwrap();
        closing.await.unwrap().unwrap();
        assert!(owners.snapshots(&id).is_empty());
        assert!(!owners.connections(&id).contains(&other));
        assert!(owners.closed.lock().unwrap().contains(&id));
        owners.close(&id).await.unwrap();
    }

    #[tokio::test]
    async fn requests_name_their_browser_or_use_the_only_one() {
        let owners = Owners::default();
        let id = Uuid::new_v4().to_string();
        let refused = owners.resolve(&id, &json!({}), None).err().unwrap();
        assert_eq!(refused.code, "NOT_CONNECTED");
        let first = owners.fixture_preparing(&id);
        let only = owners.resolve(&id, &json!({"request":{}}), None).unwrap();
        assert_eq!(only.unwrap().connection, first);
        let second = owners.fixture_preparing(&id);
        // Several browsers: an operation naming none is refused, status lists.
        let refused = owners
            .resolve(&id, &json!({"request":{"operation":"page_list"}}), None)
            .err()
            .unwrap();
        assert_eq!(refused.code, "TARGET_REQUIRED");
        assert_eq!(
            refused.details.unwrap()["targets"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(owners.resolve(&id, &json!({}), None).unwrap().is_none());
        // A target or a reference inside the request names its browser.
        let named = owners.resolve(&id, &json!({}), Some(second)).unwrap();
        assert_eq!(named.unwrap().connection, second);
        let referenced = owners
            .resolve(
                &id,
                &json!({"request":{"operation":"page_close","page":{"connectionId":first,"id":Uuid::new_v4(),"generation":Uuid::new_v4()}}}),
                None,
            )
            .unwrap();
        assert_eq!(referenced.unwrap().connection, first);
        // JSON application data may contain reference-shaped objects; only
        // the operation's selected scope controls routing.
        let evaluated = owners.resolve(
            &id,
            &json!({"request":{"operation":"evaluate","scope":{"connectionId":first,"id":Uuid::new_v4(),"generation":Uuid::new_v4()},"argument":{"kind":"json","value":{"connectionId":second,"id":Uuid::new_v4(),"generation":Uuid::new_v4()}}}}),
            None,
        ).unwrap();
        assert_eq!(evaluated.unwrap().connection, first);
        assert_eq!(connection(&target(second)).unwrap(), second);
        assert_eq!(
            owners
                .resolve(&id, &json!({}), Some(Uuid::new_v4()))
                .err()
                .unwrap()
                .code,
            "NOT_CONNECTED"
        );
    }
}
