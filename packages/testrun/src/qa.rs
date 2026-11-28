//! QA composition and owned run/session state; workflows live with their command family.
use crate::targets::Target;
pub(crate) use discovery::Reader as DiscoveryReader;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};
use uuid::Uuid;

mod arguments;
pub(crate) mod batch;
pub(crate) use batch::{BATCH_TIMEOUT_MS, delay_step, duration_budget};
mod capabilities;
mod capture;
mod control;
mod discovery;
mod dispatch;
mod input;
mod inspection;
pub(crate) mod journal;
mod lifecycle;
mod media;
#[cfg_attr(
    windows,
    allow(
        dead_code,
        reason = "Network and playback mutations use the Unix retained controller"
    )
)]
pub(crate) mod network;
#[cfg_attr(
    windows,
    allow(
        dead_code,
        reason = "Playback mutations use the Unix retained controller"
    )
)]
mod playback;
mod reads;
mod runs;
pub(crate) mod scheduling;
mod semantic;
pub(crate) use reads::ReadContext;
mod sessions;

pub struct Server {
    #[cfg(unix)]
    pub(crate) control_proxy: Option<crate::control::channel::Proxy>,
    scheduling: Option<std::sync::Arc<scheduling::Runtimes>>,
    processing: reads::Processing,
    processing_target: reads::Processing,
    pub(crate) managed: crate::managed::Client,
    archive: Option<journal::Archive>,
    scoped_archive: bool,
    discovery: std::sync::Arc<discovery::Registry>,
    runs: HashMap<String, Run>,
    sessions: HashMap<String, Session>,
    cleanup_reports: std::collections::BTreeMap<String, Value>,
    media: Option<crate::media::Media>,
    recordings: crate::recording::Recordings,
    playback: crate::playback::Owners,
    simulator_sources: std::sync::Arc<crate::simulator::Sources>,
    simulator_boots: crate::simulator::lifecycle::Boots,
    emulator_starts: crate::android::lifecycle::Starts,
    #[cfg(target_os = "macos")]
    device_media: crate::device_media::Sources,
    notifications: Option<tokio::sync::mpsc::Sender<Value>>,
    progress_token: Option<Value>,
    runners: crate::device::Runners,
}
#[derive(Clone)]
struct Run {
    goals: HashSet<String>,
    evidence: PathBuf,
    end_attempted: bool,
}
struct Session {
    archive: Option<journal::Archive>,
    scheduler_slot: Option<tokio::sync::OwnedSemaphorePermit>,
    simulator_incarnation:
        std::sync::Arc<std::sync::OnceLock<crate::simulator::incarnation::Incarnation>>,
    simulator_input: std::sync::OnceLock<std::sync::Arc<crate::simulator::input::Input>>,
    simulator_source:
        std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<crate::simulator::Source>>>>,
    read_gate: std::sync::Arc<reads::Gate>,
    read_capabilities: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, Value>>>,
    closing: bool,
    cleanup_error: Option<crate::error::Error>,
    cleanup: lifecycle::Cleanup,
    control_path: String,
    android_capture: Option<std::sync::Arc<crate::android::streaming::Source>>,
    #[cfg(target_os = "macos")]
    video: Option<std::sync::Arc<crate::device_media::Source>>,
    #[cfg(target_os = "macos")]
    hid: Option<std::sync::Arc<crate::hid::Hid>>,
    _control_lock: Option<crate::lease::ControlLease>,
    run_id: String,
    goal_id: String,
    target: Target,
    control: bool,
    lease_id: String,
    journal: HashMap<String, (Value, Value)>,
    batches: HashMap<String, (Value, Value)>,
    last_observation: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    streams: HashSet<String>,
    recordings: HashSet<String>,
}

fn id() -> String {
    Uuid::new_v4().to_string()
}

impl Server {
    pub fn new() -> Self {
        Self {
            #[cfg(unix)]
            control_proxy: None,
            scheduling: None,
            processing: reads::Processing::default(),
            processing_target: reads::Processing::target(),
            managed: crate::managed::Client::default(),
            archive: None,
            scoped_archive: false,
            discovery: std::sync::Arc::default(),
            runs: HashMap::new(),
            sessions: HashMap::new(),
            cleanup_reports: std::collections::BTreeMap::new(),
            media: None,
            recordings: crate::recording::Recordings::default(),
            playback: crate::playback::Owners::default(),
            simulator_sources: std::sync::Arc::default(),
            simulator_boots: Default::default(),
            emulator_starts: Default::default(),
            #[cfg(target_os = "macos")]
            device_media: crate::device_media::Sources::default(),
            notifications: None,
            progress_token: None,
            runners: crate::device::Runners::default(),
        }
    }
    pub fn notifications(&mut self, sender: tokio::sync::mpsc::Sender<Value>) {
        if let Some(scheduling) = &self.scheduling {
            scheduling.notifications(sender.clone());
        }
        self.notifications = Some(sender);
    }
    pub fn progress_token(&mut self, token: Option<Value>) {
        self.progress_token = token;
    }
    fn progress(&self, step: u32, message: &str) {
        if let (Some(sender), Some(token)) = (&self.notifications, &self.progress_token) {
            let _=sender.try_send(json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":token,"progress":step,"total":2,"message":message}}));
        }
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
