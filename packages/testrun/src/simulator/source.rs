//! Capture source acquisition, consumer subscriptions and bounded request admission.
use super::cleanup::Cleanup;
use super::ipc::{self, Message};
use protocol::Command;
pub(crate) mod protocol;
mod transport;
use crate::error::{Error, Result};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    process::{Child, Command as Process},
    sync::{Notify, mpsc, oneshot, watch},
};
use transport::Transport;

struct Request {
    command: Command,
    reply: Option<oneshot::Sender<Result<Message>>>,
}
pub(crate) struct Source {
    epoch: String,
    identity: Option<super::incarnation::Incarnation>,
    requests: mpsc::Sender<Request>,
    profiles: Arc<[AtomicUsize; 2]>,
    changed: Arc<Notify>,
    frames: [watch::Receiver<Option<Arc<Message>>>; 2],
    health: watch::Receiver<Option<std::time::Instant>>,
    failure: watch::Receiver<Option<Error>>,
    pub(super) cleanup: Arc<Cleanup>,
}
pub(crate) struct Subscription {
    source: Arc<Source>,
    index: usize,
    pub(crate) frames: watch::Receiver<Option<Arc<Message>>>,
    pub(crate) health: watch::Receiver<Option<std::time::Instant>>,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.source.profiles[self.index].fetch_sub(1, Ordering::AcqRel);
        self.source.changed.notify_one();
    }
}
impl Subscription {
    pub(crate) fn keyframe(&self) {
        self.source.keyframe();
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        self.cleanup.stop();
    }
}

impl Source {
    pub(super) async fn start(uuid: &str, epoch: &str) -> Result<Arc<Self>> {
        let developer = crate::native::developer_directory().await?;
        let mut command = Process::new(
            std::env::current_exe()
                .map_err(|_| Error::new("INTERNAL", "Cannot resolve capture worker"))?,
        );
        super::device_set::DeviceSet::current()?.apply_command(&mut command);
        let child = command
            .args(["native-source", "--developer", &developer, "--id", uuid])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| {
                Error::new(
                    "CAPTURE_UNAVAILABLE",
                    "Cannot start simulator capture source",
                )
            })?;
        let source = Self::attach(child).await?;
        let validated = source
            .identity
            .as_ref()
            .ok_or_else(ipc::ended)
            .and_then(|identity| identity.matches_discovery(epoch));
        if let Err(error) = validated {
            source.cleanup.close().await?;
            return Err(error);
        }
        Ok(source)
    }
    pub(super) async fn attach(mut child: Child) -> Result<Arc<Self>> {
        let mut output = child.stdout.take().ok_or_else(ipc::ended)?;
        let input = child.stdin.take().ok_or_else(ipc::ended)?;
        let ready = tokio::time::timeout(Duration::from_secs(5), ipc::read(&mut output)).await;
        if !matches!(ready, Ok(Ok(ref message)) if message.metadata["event"] == "ready" && message.metadata["epoch"].as_str().is_some_and(|epoch| !epoch.is_empty() && epoch.len() <= 128))
        {
            let failure = match &ready {
                Ok(Ok(message)) if message.metadata["event"] == "error" => {
                    ipc::failure(&message.metadata["error"])
                }
                _ => Error::new(
                    "CAPTURE_UNAVAILABLE",
                    "Simulator capture did not become ready within five seconds",
                ),
            };
            if child.try_wait().map_err(|_| ipc::ended())?.is_none() {
                child.kill().await.map_err(|_| {
                    Error::new(
                        "CLEANUP_FAILED",
                        "Cannot reap failed simulator capture startup",
                    )
                })?;
            }
            return Err(failure);
        }
        let ready = ready.unwrap()?;
        let identity = ready
            .metadata
            .get("identity")
            .and_then(|value| serde_json::from_value(value.clone()).ok());
        let epoch = ready.metadata["epoch"]
            .as_str()
            .ok_or_else(ipc::ended)?
            .to_owned();
        let (requests, receiver) = mpsc::channel(32);
        let (stopped, stop) = watch::channel(false);
        let profiles = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
        let changed = Arc::new(Notify::new());
        let (jpeg, jpeg_rx) = watch::channel(None);
        let (h264, h264_rx) = watch::channel(None);
        let (health, health_rx) = watch::channel(Some(std::time::Instant::now()));
        let (failed, failure) = watch::channel(None);
        let transport = Transport {
            child,
            input,
            output,
            requests: receiver,
            profiles: profiles.clone(),
            changed: changed.clone(),
            frames: [jpeg, h264],
            health,
            failure: failed,
            stop,
        };
        let job = tokio::spawn(transport.run());
        Ok(Arc::new(Self {
            epoch,
            identity,
            requests,
            profiles,
            changed,
            frames: [jpeg_rx, h264_rx],
            health: health_rx,
            failure,
            cleanup: Cleanup::new(stopped, job),
        }))
    }
    pub(crate) fn epoch(&self) -> &str {
        &self.epoch
    }
    pub(crate) fn incarnation(&self) -> Result<super::incarnation::Incarnation> {
        self.validate()?;
        self.identity.clone().ok_or_else(ipc::ended)
    }
    pub(super) fn can_share(&self) -> bool {
        !self.requests.is_closed()
            && self.health.borrow().is_some()
            && self.failure.borrow().is_none()
            && self
                .identity
                .as_ref()
                .is_none_or(|identity| identity.validate().is_ok())
    }
    pub(crate) fn validate(&self) -> Result<()> {
        self.identity.as_ref().ok_or_else(ipc::ended)?.validate()?;
        if let Some(error) = self.failure.borrow().clone() {
            return Err(Error::new(
                "CAPTURE_STALLED",
                "Simulator capture worker ended; reconnect for a fresh source",
            )
            .details(json!({"cause":error})));
        }
        if self.requests.is_closed() || self.health.borrow().is_none() {
            return Err(ipc::ended());
        }
        Ok(())
    }
    pub(crate) async fn geometry(&self) -> Result<(u32, u32)> {
        let reply = self
            .bounded_request(Command::Geometry {
                id: uuid::Uuid::new_v4().to_string(),
            })
            .await?;
        dimensions(&reply)
    }
    pub(crate) async fn compact(&self) -> Result<(Vec<u8>, (u32, u32))> {
        let reply = self
            .bounded_request(Command::Compact {
                id: uuid::Uuid::new_v4().to_string(),
            })
            .await?;
        let size = dimensions(&reply)?;
        Ok((reply.payload, size))
    }
    async fn bounded_request(&self, command: Command) -> Result<Message> {
        self.validate()?;
        let reply = tokio::time::timeout(Duration::from_secs(5), self.request(command))
            .await
            .map_err(|_| Error::new("CAPTURE_STALLED", "Simulator request exceeded five seconds"))?
            .map_err(|error| self.capture_error(error))?;
        self.validate()?;
        Ok(reply)
    }
    pub(crate) fn subscribe(self: &Arc<Self>, format: &str) -> Subscription {
        let index = usize::from(format == "h264");
        self.profiles[index].fetch_add(1, Ordering::AcqRel);
        self.changed.notify_one();
        Subscription {
            source: self.clone(),
            index,
            frames: self.frames[index].clone(),
            health: self.health.clone(),
        }
    }
    pub(crate) fn keyframe(&self) {
        let _ = self.requests.try_send(Request {
            command: Command::Keyframe,
            reply: None,
        });
    }
    pub(crate) fn stop_recording(&self, id: &str) -> Result<()> {
        self.enqueue(Request {
            command: Command::StopRecording { id: id.to_owned() },
            reply: None,
        })
    }
    fn enqueue(&self, request: Request) -> Result<()> {
        self.requests
            .try_send(request)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    Error::new("CAPTURE_BUSY", "Capture request queue is full")
                }
                mpsc::error::TrySendError::Closed(_) => {
                    Error::new("CAPTURE_STALLED", "Simulator capture source is unavailable")
                }
            })
    }
    async fn request(&self, command: Command) -> Result<Message> {
        let (reply, receiver) = oneshot::channel();
        self.enqueue(Request {
            command,
            reply: Some(reply),
        })?;
        receiver.await.map_err(|_| ipc::ended())?
    }
    fn capture_error(&self, error: Error) -> Error {
        if self.identity.is_some() && error.code == "NATIVE_STREAM_ENDED" {
            Error::new("CAPTURE_STALLED", "Simulator capture worker ended")
                .details(json!({"cause":error}))
        } else {
            error
        }
    }
    pub(crate) async fn screenshot(&self) -> Result<Vec<u8>> {
        // Only process fixtures attached directly by unit tests omit identity.
        // Production acquisition requires it in start before publishing a source.
        if self.identity.is_some() {
            self.validate()?;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let reply = tokio::time::timeout(
            Duration::from_secs(5),
            self.request(Command::Screenshot { id }),
        )
        .await
        .map_err(|_| {
            Error::new(
                "CAPTURE_STALLED",
                "Simulator screenshot exceeded five seconds",
            )
        })?
        .map_err(|error| self.capture_error(error))?;
        if self.identity.is_some() {
            self.validate()?;
        }
        Ok(reply.payload)
    }
    pub(crate) async fn record(
        &self,
        id: &str,
        path: std::path::PathBuf,
        duration_ms: u64,
    ) -> Result<serde_json::Value> {
        self.request(Command::Record {
            id: id.into(),
            path,
            duration_ms,
        })
        .await
        .map(|reply| reply.metadata["result"].clone())
    }
}

fn dimensions(reply: &Message) -> Result<(u32, u32)> {
    let dimension = |key| {
        reply.metadata[key]
            .as_u64()
            .filter(|n| (1..=8192).contains(n))
            .map(|n| n as u32)
            .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Missing simulator source geometry"))
    };
    Ok((dimension("sourceWidth")?, dimension("sourceHeight")?))
}
