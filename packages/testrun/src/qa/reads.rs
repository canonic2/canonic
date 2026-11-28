//! Borrowed session observations, bounded admission and revocation before owned teardown.
use super::{
    Server,
    arguments::{SessionRef, session_ref},
};
use crate::{
    error::{Error, Result},
    targets::Target,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) const READ_LIMIT: u32 = 4;
/// How long a public read waits for an image encode slot before `OVERLOADED`.
const PROCESSING_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(test)]
mod tests;
pub(super) struct Gate {
    pub(super) authority: OnceLock<Arc<crate::control::Authority>>,
    #[cfg(unix)]
    pub(super) viewer: Mutex<Option<Arc<crate::control::channel::Channel>>>,
    closed: AtomicBool,
    slots: Arc<tokio::sync::Semaphore>,
    stop: tokio::sync::watch::Sender<bool>,
    cpu: Processing,
}
impl Default for Gate {
    fn default() -> Self {
        Self {
            authority: OnceLock::new(),
            #[cfg(unix)]
            viewer: Mutex::new(None),
            closed: AtomicBool::new(false),
            slots: Arc::new(tokio::sync::Semaphore::new(READ_LIMIT as usize)),
            stop: tokio::sync::watch::channel(false).0,
            cpu: Processing::target(),
        }
    }
}
impl Gate {
    pub(super) async fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.stop.send_replace(true);
        // Every admitted read releases its resources before relinquishing its slot.
        let _drained = self.slots.clone().acquire_many_owned(READ_LIMIT).await;
        let _public = self.cpu.public.clone().acquire_owned().await;
        let _ordered = self.cpu.ordered.clone().acquire_owned().await;
    }
}

/// CPU work keeps its permits inside the blocking worker even if its caller
/// disappears. This bounds abandoned encoders and makes teardown await them.
#[derive(Clone)]
pub(super) struct Processing {
    public: Arc<tokio::sync::Semaphore>,
    ordered: Arc<tokio::sync::Semaphore>,
}
impl Default for Processing {
    fn default() -> Self {
        Self {
            public: Arc::new(tokio::sync::Semaphore::new(4)),
            ordered: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }
}
impl Processing {
    pub(super) fn target() -> Self {
        Self {
            public: Arc::new(tokio::sync::Semaphore::new(1)),
            ordered: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ReadContext {
    #[cfg(unix)]
    pub(crate) authority: Arc<crate::control::Authority>,
    #[cfg(unix)]
    pub(crate) viewer: Option<std::sync::Weak<crate::control::channel::Channel>>,
    pub(super) validated: bool,
    processing: Processing,
    processing_target: Processing,
    pub(super) target: Target,
    pub(super) evidence: PathBuf,
    pub(super) control_path: String,
    pub(super) android_capture: Option<std::sync::Weak<crate::android::streaming::Source>>,
    control: bool,
    playback: Option<crate::playback::ReadHandle>,
    pub(super) discovery: super::discovery::Reader,
    pub(super) runner: Result<crate::device::Runner>,
    pub(super) simulator_sources: Arc<crate::simulator::Sources>,
    pub(super) simulator_source: Arc<Mutex<Option<Arc<crate::simulator::Source>>>>,
    pub(super) simulator_incarnation: Arc<OnceLock<crate::simulator::incarnation::Incarnation>>,
    pub(super) last_observation: Arc<Mutex<Option<std::time::Instant>>>,
    pub(super) capabilities: Arc<Mutex<BTreeMap<String, Value>>>,
    gate: Arc<Gate>,
    #[cfg(target_os = "macos")]
    pub(super) video: Option<std::sync::Weak<crate::device_media::Source>>,
    #[cfg(target_os = "macos")]
    hid: Option<std::sync::Weak<crate::hid::Hid>>,
}
impl Server {
    pub(crate) fn read_context(&self, reference: &Value) -> Result<ReadContext> {
        self.read_session(&session_ref(reference)?)
    }
    fn read_session(&self, reference: &SessionRef) -> Result<ReadContext> {
        let session = self.session(reference)?;
        if session.closing {
            return Err(Error::new("SESSION_CLOSING", "Session cleanup has started"));
        }
        let run = self
            .runs
            .get(&reference.run_id)
            .ok_or_else(|| Error::new("RUN_NOT_FOUND", "Run ended"))?;
        Ok(ReadContext {
            #[cfg(unix)]
            authority: session.authority(),
            #[cfg(unix)]
            viewer: session
                .read_gate
                .viewer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .map(Arc::downgrade),
            validated: false,
            processing: self.processing.clone(),
            processing_target: self.processing_target.clone(),
            target: session.target.clone(),
            evidence: run.evidence.clone(),
            control_path: session.control_path.clone(),
            android_capture: session.android_capture.as_ref().map(Arc::downgrade),
            control: session.control,
            playback: self.playback.read_handle(&reference.session_id),
            discovery: self.discovery_reader(),
            runner: self.runners.runner(&session.target.platform_id).cloned(),
            simulator_sources: self.simulator_sources.clone(),
            simulator_source: session.simulator_source.clone(),
            simulator_incarnation: session.simulator_incarnation.clone(),
            last_observation: session.last_observation.clone(),
            capabilities: session.read_capabilities.clone(),
            gate: session.read_gate.clone(),
            #[cfg(target_os = "macos")]
            video: session.video.as_ref().map(Arc::downgrade),
            #[cfg(target_os = "macos")]
            hid: session.hid.as_ref().map(Arc::downgrade),
        })
    }
}
impl ReadContext {
    pub(super) async fn process<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let pool = |processing: &Processing| {
            if self.validated {
                processing.ordered.clone()
            } else {
                processing.public.clone()
            }
        };
        let reserve = |semaphore: Arc<tokio::sync::Semaphore>| async move {
            if self.validated {
                // Ordered callers already have bounded queue admission and an
                // enclosing MCP/retained-operation deadline. Its cancellation
                // drops partial reservations without ever spawning this work.
                semaphore
                    .acquire_owned()
                    .await
                    .map_err(|_| Error::new("SESSION_CLOSING", "Image processor is stopping"))
            } else {
                // Admitted read slots already bound how many wait here, so a
                // concurrent read queues briefly behind one encode instead of
                // failing at once.
                tokio::time::timeout(PROCESSING_WAIT, semaphore.acquire_owned())
                    .await
                    .map_err(|_| {
                        Error::new("OVERLOADED", "Image processing capacity is full")
                            .details(json!({"phase":"image_processing"}))
                    })?
                    .map_err(|_| Error::new("SESSION_CLOSING", "Image processor is stopping"))
            }
        };
        let session = reserve(pool(&self.gate.cpu)).await?;
        let target = reserve(pool(&self.processing_target)).await?;
        let connection = reserve(pool(&self.processing)).await?;
        tokio::task::spawn_blocking(move || {
            let _owned = (session, target, connection);
            work()
        })
        .await
        .map_err(|_| Error::new("INTERNAL", "Owned image processor stopped"))?
    }
    pub(crate) fn supports(name: &str) -> bool {
        matches!(
            name,
            "screenshot"
                | "ui_snapshot"
                | "ui_find"
                | "ui_read"
                | "wait"
                | "app_list"
                | "clipboard_read"
                | "media_status"
                | "volume_get"
        )
    }
    pub(crate) async fn call(self, name: &str, args: Value) -> Result<Value> {
        if crate::playback::operation(name) {
            crate::playback::validate_platform(name, &args, self.target.kind == "android")?;
        }
        let mut stop = self.gate.stop.subscribe();
        if self.gate.closed.load(Ordering::Acquire) {
            return Err(Error::new(
                "SESSION_CLOSING",
                "Session reads have been revoked",
            ));
        }
        let permit = self.gate.slots.clone().try_acquire_owned().map_err(|_| {
            Error::new("OVERLOADED", "Session already has four admitted reads")
                .details(json!({"admitted":false,"dispatched":false}))
        })?;
        let result = tokio::select! {
            biased;
            _ = stop.changed() => Err(Error::new("SESSION_CLOSING", "Session read was revoked for cleanup")),
            result = async {
                match name {
                    "screenshot" => self.screenshot(args).await,
                    "ui_snapshot"|"ui_find" => self.accessibility(name,args).await,
                    "ui_read"|"wait" => self.semantic(name,args).await,
                    "app_list"|"clipboard_read" => self.observe(name,args).await,
                    "media_status"|"volume_get"=>self.media(name,args).await,
                    _ => Err(Error::invalid("Unsupported independent session read")),
                }
            } => result,
        };
        // Drop borrowed sources and RPC state before cleanup's drain wakes.
        drop(self);
        drop(permit);
        result
    }
    #[cfg(unix)]
    pub(crate) fn has_media(&self) -> bool {
        self.playback.is_some()
    }
    async fn media(&self, name: &str, args: Value) -> Result<Value> {
        let target = self.live_target().await?;
        let mut result = self
            .playback
            .as_ref()
            .ok_or_else(|| {
                Error::new(
                    "MEDIA_HELPER_FAILED",
                    "Media helper must be prepared by its controller",
                )
            })?
            .call(name, &args)
            .await?;
        crate::playback::stamp(&mut result, &target);
        Ok(result)
    }
    pub(super) async fn incarnation(&self) -> Result<crate::simulator::incarnation::Incarnation> {
        crate::simulator::device_set::validate_target(&self.target)?;
        if let Some(identity) = self.simulator_incarnation.get() {
            identity.validate()?;
            return Ok(identity.clone());
        }
        let identity = crate::native::incarnation(&self.target.platform_id).await?;
        identity.matches_discovery(&self.target.epoch)?;
        let witness = self.simulator_incarnation.get_or_init(|| identity.clone());
        if witness != &identity {
            return Err(Error::new(
                "STALE_TARGET",
                "Simulator incarnation changed during concurrent admission",
            ));
        }
        witness.validate()?;
        Ok(witness.clone())
    }
    pub(super) async fn live_target(&self) -> Result<Target> {
        if self.validated {
            return Ok(self.target.clone());
        }
        if self.target.kind == "simulator" {
            self.incarnation().await?;
            return Ok(self.target.clone());
        }
        let mut target = self.discovery.validate(&self.target).await?;
        target.capabilities.clone_from(&self.target.capabilities);
        Ok(target)
    }
    pub(super) fn verified(&self, capability: &str, details: Value) {
        let mut details = details.as_object().cloned().unwrap_or_default();
        details.insert("state".into(), json!("supported"));
        self.capabilities
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(capability.into(), Value::Object(details));
    }
    fn lost(&self, cause: &Error) -> Value {
        json!({"state":"lost","reason":cause.code,"cause":cause,"target":self.target,"controlPath":self.control_path,"nextAction":if cause.code == "RUNNER_EXITED" {"disconnect"} else {"connect"}})
    }
    async fn inspection_health(&self) -> Result<Value> {
        let reply = match &self.runner {
            Ok(runner) => runner
                .call("hello", json!({}), None, std::time::Duration::from_secs(2))
                .await
                .map(|reply| reply.data["inspection"].clone()),
            Err(error) => Err(error.clone()),
        };
        match reply {
            Err(error) if error.code == "RUNNER_EXITED" && self.control_path == "xctest" => {
                Err(error)
            }
            Err(error) => Ok(json!({"state":"unavailable","error":error})),
            Ok(health) => Ok(health),
        }
    }
    async fn lock_status(&self) -> Value {
        #[cfg(target_os = "macos")]
        if let Some(video) = self.video.as_ref().and_then(std::sync::Weak::upgrade) {
            return video.lock_status().await;
        }
        Value::Null
    }
    pub(crate) async fn status(&self) -> Result<Value> {
        if self.gate.closed.load(Ordering::Acquire) {
            return Ok(json!({"state":"closing","closed":false}));
        }
        // USB loss, a replug under a new connection, or a closed video
        // transport make observation and input unusable until reconnect.
        #[cfg(target_os = "macos")]
        if let Some(video) = &self.video {
            let lost = match video.upgrade() {
                Some(source) => source.transport_error(),
                None => Some(Error::new(
                    "DEVICE_DISCONNECTED",
                    "Device video source has closed",
                )),
            };
            if let Some(cause) = lost {
                return Ok(self.lost(&cause));
            }
        }
        if self.target.kind == "device" {
            // A bounded local inventory; an unknown result never claims loss.
            let inventory = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                crate::usbmux::list(std::path::Path::new("/var/run/usbmuxd")),
            )
            .await;
            if let Ok(Ok(devices)) = inventory
                && let Some(cause) = usb_presence(&self.target, &devices)
            {
                return Ok(self.lost(&cause));
            }
        }
        // Independent read probes share the status budget instead of adding
        // the lock deadline to a slow inspection heartbeat.
        let (inspection, lock) = tokio::join!(self.inspection_health(), self.lock_status());
        let inspection = match inspection {
            Ok(health) => health,
            Err(error) => return Ok(self.lost(&error)),
        };
        let mut target = self.target.clone();
        target.capabilities.extend(
            self.capabilities
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        );
        #[cfg(target_os = "macos")]
        let input_health =
            self.hid
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
                .map_or(Value::Null, |hid| {
                    let mut health = hid.health();
                    health["videoFresh"] = json!(
                        self.video
                            .as_ref()
                            .and_then(std::sync::Weak::upgrade)
                            .is_some_and(|video| video.geometry().is_ok())
                    );
                    health
                });
        #[cfg(not(target_os = "macos"))]
        let input_health = Value::Null;
        let android_capture = self
            .android_capture
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .map_or_else(
                || {
                    if target.kind == "android" {
                        json!({"requested":"screenshot","state":"screenshot"})
                    } else {
                        Value::Null
                    }
                },
                |source| source.status(),
            );
        Ok(
            json!({"state":"ready","target":target,"mode":if self.control{"control"}else{"observe"},"controlPath":self.control_path,"androidCapture":android_capture,"inputHealth":input_health,"inspectionHealth":inspection,"lock":lock,"readCapacity":READ_LIMIT,"activeReads":READ_LIMIT as usize-self.gate.slots.available_permits()}),
        )
    }
}

/// Why a physical target's session is unusable on the current USB inventory:
/// absent, or attached under a different connection than the session opened.
fn usb_presence(target: &Target, devices: &[crate::usbmux::Device]) -> Option<Error> {
    match devices
        .iter()
        .find(|device| device.serial == target.platform_id)
    {
        None => Some(Error::new(
            "USB_UNAVAILABLE",
            "Selected device has no USB connection",
        )),
        Some(device) if device.id.to_string() != target.epoch => Some(Error::new(
            "STALE_TARGET",
            "The device reconnected; connect again for a fresh session",
        )),
        Some(_) => None,
    }
}
