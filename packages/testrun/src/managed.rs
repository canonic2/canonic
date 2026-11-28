//! Device workflows through the manager. One manager per user ([`service`])
//! holds every device and browser; CLI commands and MCP servers are its
//! clients ([`Client`]), and every client's requests to a device run in that
//! device's FIFO queue. Inside the manager a [`Caller`] carries out one
//! admitted request against the device actor of the target it resolved to.
use crate::{
    error::{Error, Result},
    lease::ControlLease,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc, time::Duration};

mod actions;
pub(crate) use presentation::clean as clean_result;
#[cfg(any(unix, test))]
pub(crate) use validation::steps as action_steps;
mod applications;
mod browsers;
mod cli;
mod clock;
mod connection;
mod control;
mod history;
mod host;
pub(crate) mod identity;
mod inspection;
mod lifetime;
mod listing;
mod network;
mod playback;
pub(crate) mod remote;
mod selection;
pub(crate) use cli::{execute, manager as manager_command};
pub(crate) mod owner;
mod presentation;
pub(crate) mod service;
pub(crate) mod storage;
mod supervisor;
#[cfg(test)]
mod tests;
mod transition;
mod validation;
mod wire;

pub(crate) use owner::Owner;
pub(crate) use remote::Client;
pub(crate) use service::registry::Kind;
/// The private-record layout and device actor protocol.
const WIRE: u32 = 7;
/// The manager request protocol on `canonic-process-kit`.
const PROTOCOL: u32 = 8;
/// The separately versioned product control subset, reported in identity.
const CONTROL: u32 = 1;
const BUILD_ID: &str = env!("TESTRUN_BUILD_ID");

pub(crate) fn operation_timeout_ms(name: &str) -> Option<u64> {
    match name {
        "audio_probe" => Some(75000),
        "media_status" | "media_send" | "volume_get" | "volume_set" => Some(30000),
        "app_install" | "app_uninstall" => Some(240000),
        // Each preparation bounds itself (20 s and 600 s); these cover it.
        "developer_mode_reveal" => Some(30000),
        "developer_support_install" => Some(610000),
        // Offline/online readback polls up to 60 s; leave the queue its margin.
        "network_set" => Some(75000),
        "connect" | "observe" => Some(90000),
        "act" => Some(125000),
        "disconnect" => Some(45000),
        "status" => Some(5000),
        "control_status" => Some(5000),
        "control_acquire" | "control_release" => Some(125000),
        "wait" => Some(35000),
        name if selected_operation(name) => Some(90000),
        _ => None,
    }
}

pub(crate) fn selected_operation(name: &str) -> bool {
    matches!(
        name,
        "app_list"
            | "clipboard_read"
            | "ui_snapshot"
            | "ui_find"
            | "ui_read"
            | "wait"
            | "preview_start"
            | "preview_status"
            | "preview_stop"
            | "recording_start"
            | "recording_status"
            | "recording_stop"
    )
}
pub(crate) fn independent_read(name: &str, args: &Value) -> bool {
    args.get("sessionId").is_none()
        && matches!(
            name,
            "observe"
                | "control_status"
                | "control_acquire"
                | "control_release"
                | "app_list"
                | "clipboard_read"
                | "ui_snapshot"
                | "ui_find"
                | "ui_read"
                | "wait"
                | "media_status"
                | "volume_get"
        )
}
/// Accepted mutations tracked as operations: they keep their target from
/// idle release until they settle, and status lists them.
fn operation(name: &str) -> bool {
    matches!(
        name,
        "act"
            | "app_install"
            | "app_uninstall"
            | "audio_probe"
            | "developer_mode_reveal"
            | "developer_support_install"
    ) || crate::playback::mutation(name)
        || crate::network::operation(name)
        || matches!(name, "recording_start" | "recording_stop")
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Connection {
    version: u32,
    #[serde(default)]
    build_id: Option<String>,
    task: String,
    target: String,
    runner_team_id: Option<String>,
    control_path: Option<String>,
    #[serde(default)]
    android_capture: Option<crate::android::streaming::Mode>,
    bundle_id: Option<String>,
    setup: bool,
    #[serde(default)]
    boot: bool,
    /// Show the window of an emulator this connection starts.
    #[serde(default)]
    window: bool,
    #[serde(default)]
    simulator_device_set: Option<PathBuf>,
    epoch: String,
    directory: PathBuf,
    created: u64,
    /// The host label of the client that started it, when one was present.
    #[serde(default)]
    host_label: Option<String>,
}
impl Connection {
    fn socket(&self) -> PathBuf {
        self.directory.join("s/control.sock")
    }
    fn authority(&self) -> String {
        format!("managed:{}", self.target)
    }
    fn held(&self) -> Result<bool> {
        Ok(
            ControlLease::inspect(&self.authority())?.is_some_and(|(held, record)| {
                held && record["epoch"] == self.epoch && record["task"] == self.task
            }),
        )
    }
    fn admit_work(&self) -> Result<()> {
        self.require_build()?;
        if storage::read::<Value>(&self.directory.join("state.json"))?.is_some_and(|state| {
            state["state"] == "closing"
                || state["state"] == "closed"
                || state["state"] == "failed"
                    && matches!(
                        state["error"]["code"].as_str(),
                        Some("CLEANUP_FAILED" | "ACTOR_QUARANTINED")
                    )
        }) {
            let mut error = Error::new(
                "RELEASE_PENDING",
                "This device's release is unfinished; Testrun retries it automatically. Connect again once status shows it released",
            )
            .details(json!({"admitted":false,"inputDispatched":false,"nextAction":"connect"}));
            error.retryable = true;
            return Err(error);
        }
        Ok(())
    }
    fn require_build(&self) -> Result<()> {
        if self.version != WIRE || self.build_id.as_deref() != Some(BUILD_ID) {
            return Err(Error::new(
                "VERSION_MISMATCH",
                "Reconnect to hand over the previous controller build",
            )
            .details(json!({"admitted":false,"inputDispatched":false})));
        }
        Ok(())
    }
    fn resolve_capture(&mut self, previous: &Self, explicit: bool, held: bool) -> Result<()> {
        if previous.target != self.target {
            return Ok(());
        }
        if !explicit {
            self.android_capture = previous.android_capture;
        } else if held
            && previous.android_capture.unwrap_or_default()
                != self.android_capture.unwrap_or_default()
        {
            return Err(Error::new(
                "SESSION_SETTINGS_CONFLICT",
                "Disconnect before changing androidCapture",
            ));
        }
        Ok(())
    }
    fn compatible(&self, other: &Self) -> bool {
        self.version == other.version
            && self.build_id == other.build_id
            && self.task == other.task
            && self.target == other.target
            && self.runner_team_id == other.runner_team_id
            && self.control_path == other.control_path
            && self.android_capture.unwrap_or_default() == other.android_capture.unwrap_or_default()
            && self.bundle_id == other.bundle_id
            && self.setup == other.setup
            && self.boot == other.boot
            && self.window == other.window
            && self.simulator_device_set == other.simulator_device_set
    }
}
#[derive(Serialize, Deserialize)]
struct Receipt {
    connection: Connection,
    command: String,
    created: u64,
    signature: String,
}

/// One admitted request inside the manager, bound to the pool's records and,
/// through the service, to one pooled target's device actor.
pub(crate) struct Caller {
    input_lease: Option<String>,
    /// The pooled target this request works on; its records live in their
    /// own directory inside the pool's. `None` addresses the pool itself.
    pub(crate) target: Option<String>,
    root: PathBuf,
    kind: Kind,
    connection_id: Option<String>,
    host: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) media_deadline: Option<tokio::time::Instant>,
    /// The manager's service; `None` reads durable records offline.
    service: Option<Arc<service::Service>>,
    /// The pool this request was admitted to; `None` when the manager has
    /// none and durable records answer.
    pub(crate) pool: Option<String>,
    /// The requesting client's simulator device set.
    pub(crate) device_set: Option<PathBuf>,
}
fn hash(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}
pub(crate) fn request_key(connection: &str, id: &Value) -> String {
    hash(format!("{connection}:{id}"))
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// The user's data root: `TESTRUN_DATA_ROOT`, used by tests and development
/// to run a separate manager, or the platform's default per-user location.
pub(crate) fn data_root() -> crate::error::Result<PathBuf> {
    if let Some(root) = std::env::var_os("TESTRUN_DATA_ROOT").filter(|root| !root.is_empty()) {
        return Ok(PathBuf::from(root));
    }
    #[cfg(unix)]
    {
        // SAFETY: read-only query of the calling user identity.
        let uid = unsafe { libc::geteuid() };
        Ok(PathBuf::from("/tmp").join(format!("testrun-qa-{uid}")))
    }
    #[cfg(windows)]
    {
        dirs::data_local_dir()
            .map(|path| path.join("Testrun").join("qa"))
            .ok_or_else(|| {
                crate::error::Error::new(
                    "HOST_IO",
                    "Cannot locate this user's Windows local data directory",
                )
            })
    }
}

impl Caller {
    pub(crate) fn new(service: Option<Arc<service::Service>>, root: PathBuf, kind: Kind) -> Self {
        Self {
            input_lease: None,
            target: None,
            root,
            kind,
            connection_id: None,
            host: None,
            request_id: None,
            media_deadline: None,
            service,
            pool: None,
            device_set: None,
        }
    }
    /// A caller for the same pool and request, working on `target`.
    fn with_target(&self, target: &str) -> Self {
        let mut caller = Self::new(self.service.clone(), self.root.clone(), self.kind);
        caller.pool.clone_from(&self.pool);
        caller.request_id.clone_from(&self.request_id);
        caller.device_set.clone_from(&self.device_set);
        caller.target = Some(target.to_owned());
        caller
    }
    fn service(&self) -> Result<&Arc<service::Service>> {
        self.service
            .as_ref()
            .ok_or_else(|| Error::new("OWNER_ABSENT", "The manager is not running"))
    }
    pub(crate) async fn call(&mut self, name: &str, args: Value) -> Result<Value> {
        if crate::browser::surface::handles(name) {
            return self.browser_call(name, args).await;
        }
        if name == "connect" && self.target.is_none() {
            self.target = args["targetId"].as_str().map(str::to_owned);
        }
        let directory = self.directory()?;
        self.admitted_call(&directory, name, args, false).await
    }
    async fn connect_if_absent(&mut self, args: Value) -> Result<Value> {
        let directory = self.directory()?;
        self.admitted_call(&directory, "connect", args, true).await
    }
    async fn admitted_call(
        &mut self,
        directory: &std::path::Path,
        name: &str,
        args: Value,
        connect_if_absent: bool,
    ) -> Result<Value> {
        let directory = directory.to_path_buf();
        let status = name == "status";
        let independent = independent_read(name, &args);
        let _admitted = if status {
            None
        } else {
            Some(self.service()?.admission.admit()?)
        };
        let _read = if independent && !name.starts_with("control_") {
            Some(self.service()?.admission.read(self.target.as_deref())?)
        } else {
            None
        };
        let _probe = if name == "audio_probe" {
            Some(
                ControlLease::acquire(&format!("audio-probe-request:{}", directory.display()))
                    .map_err(|error| {
                        if error.code == "TARGET_BUSY" {
                            Error::new(
                                "CAPTURE_CONFLICT",
                                "An audio probe is already running for this controller",
                            )
                        } else {
                            error
                        }
                    })?,
            )
        } else {
            None
        };
        let budget = Duration::from_millis(operation_timeout_ms(name).unwrap_or(5000));
        let caller_deadline = self.media_deadline.take();
        let queue_deadline = if self.kind == Kind::Mcp {
            caller_deadline
        } else {
            let deadline = tokio::time::Instant::now() + budget;
            Some(caller_deadline.map_or(deadline, |requested| deadline.min(requested)))
        };
        if queue_deadline.is_some_and(|deadline| deadline <= tokio::time::Instant::now()) {
            return Err(Error::new(
                "TIMEOUT",
                "Media operation deadline expired before controller dispatch",
            )
            .details(json!({"admitted":false,"dispatched":false})));
        }
        self.input_lease = self.input_lease_for(&directory, name, &args).await?;
        // Every client shares one FIFO queue per target for device-changing
        // work; a disconnect waits its turn so earlier accepted work settles
        // first. Without a pool, durable records are settled offline.
        let queued = !(status || independent || name == "disconnect" && self.pool.is_none());
        let _queue = if queued {
            Some(
                self.queue(queue_deadline)
                    .await
                    .map_err(presentation::error)?,
            )
        } else {
            None
        };
        // An explicit connect queued ahead of this read owns the settings.
        if connect_if_absent
            && let Some(target) = self.target.as_deref()
            && self.service()?.connected_target(target)
        {
            return Ok(Value::Null);
        }
        let operation_deadline = tokio::time::Instant::now() + budget;
        let deadline = queue_deadline.map_or(operation_deadline, |deadline| {
            deadline.min(operation_deadline)
        });
        let result =
            tokio::time::timeout_at(deadline, self.perform(&directory, name, args, deadline))
                .await
                .unwrap_or_else(|_| {
                    Err(Error::new(
                        "RECOVERING",
                        "Testrun is still finishing the operation; use status before further input",
                    ))
                });
        result
            .map(presentation::clean)
            .map(|mut value| {
                if status {
                    self.pool_status(&mut value);
                }
                value
            })
            .map_err(|error| {
                let _ = storage::write(&directory.join("diagnostic.json"), &error);
                if name == "wait" && error.code == "TIMEOUT" {
                    presentation::observation_timeout(error)
                } else if matches!(name, "connect" | "disconnect") {
                    service::retrying(presentation::error(error))
                } else {
                    presentation::error(error)
                }
            })
    }
    /// Wait in this target's FIFO queue, with an optional deadline.
    async fn queue(&self, deadline: Option<tokio::time::Instant>) -> Result<service::Turn> {
        let queue = self.service()?.queue(self.target.as_deref());
        queue.turn(deadline).await.ok_or_else(|| {
            Error::new(
                "RECOVERING",
                "An earlier request for this device is still settling",
            )
        })
    }
    /// Route one admitted request to its workflow while the queue is held.
    async fn perform(
        &mut self,
        directory: &std::path::Path,
        name: &str,
        args: Value,
        deadline: tokio::time::Instant,
    ) -> Result<Value> {
        if crate::playback::operation(name) {
            return self.playback(directory, name, args, deadline).await;
        }
        if crate::network::operation(name) {
            return self.network(directory, name, args, deadline).await;
        }
        match name {
            "app_install" | "app_uninstall" => self.application(directory, name, args).await,
            "developer_mode_reveal" | "developer_support_install" => {
                let target = args["targetId"]
                    .as_str()
                    .ok_or_else(|| Error::invalid("Name the device with targetId"))?;
                crate::device_setup::preparation(name, target).await
            }
            "connect" => self.connect_retry(directory, args).await,
            "act" => self.act(directory, args).await,
            "observe" => self.observe(directory, args, deadline).await,
            "status" => self.status(directory, args).await,
            "control_status" | "control_acquire" | "control_release" => {
                self.control(directory, name, args).await
            }
            name if selected_operation(name) => self.inspect(directory, name, args).await,
            "disconnect" => self.release().await,
            _ => Err(Error::invalid("Unknown managed operation")),
        }
    }
    /// Release this request's target for every client: finalize its running
    /// recordings, then release its device actor. The service removes it
    /// from the pool once the release is verified. Without a pool, durable
    /// records are settled offline.
    async fn release(&self) -> Result<Value> {
        let (Some(_), Some(target)) = (&self.pool, &self.target) else {
            return self.disconnect_recorded().await;
        };
        let directory = self.directory()?;
        let recordings = self.finalize_recordings(&directory).await;
        let mut closed = self
            .disconnect(&directory, self.request_id.as_deref())
            .await?;
        // Epochs a lost manager left on this target are settled with it; one
        // that cannot be is this release's blocker.
        if closed["closed"] == true {
            self.settle_stopped(&directory).await?;
        }
        closed["targetId"] = json!(target);
        if let Some(recordings) = recordings {
            closed["recordings"] = recordings;
        }
        Ok(closed)
    }
}

/// Present an invalid managed action without exposing controller authority.
pub(crate) fn invalid_action(error: Error) -> Error {
    presentation::error(presentation::validation_error(error))
}

/// Validate per-action caller fields before the outer MCP request schema.
pub(crate) fn validate_actions(args: &Value) -> Result<()> {
    validation::steps(args).map(|_| ())
}
