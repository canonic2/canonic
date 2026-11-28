//! Capture pipe routing and child-process disposal, separate from source subscriptions.
use super::{Request, protocol::Command};
use crate::{
    error::{Error, Result},
    simulator::ipc::{self, Message},
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    process::Child,
    sync::{Notify, mpsc, oneshot, watch},
};

pub(super) struct Transport {
    pub(super) child: Child,
    pub(super) input: tokio::process::ChildStdin,
    pub(super) output: tokio::process::ChildStdout,
    pub(super) requests: mpsc::Receiver<Request>,
    pub(super) profiles: Arc<[AtomicUsize; 2]>,
    pub(super) changed: Arc<Notify>,
    pub(super) frames: [watch::Sender<Option<Arc<Message>>>; 2],
    pub(super) health: watch::Sender<Option<std::time::Instant>>,
    pub(super) failure: watch::Sender<Option<Error>>,
    pub(super) stop: watch::Receiver<bool>,
}
impl Transport {
    async fn send(input: &mut tokio::process::ChildStdin, command: &Command) -> Result<()> {
        let mut bytes = serde_json::to_vec(command)
            .map_err(|_| Error::new("INTERNAL", "Cannot encode capture request"))?;
        bytes.push(b'\n');
        tokio::time::timeout(Duration::from_secs(1), input.write_all(&bytes))
            .await
            .map_err(|_| Error::new("CAPTURE_STALLED", "Capture command pipe stalled"))?
            .map_err(|_| ipc::ended())
    }
    #[expect(
        clippy::too_many_lines,
        reason = "The pipe pump retains requests, frame delivery and process teardown together"
    )]
    pub(super) async fn run(mut self) -> Result<()> {
        let (messages, mut receiver) = mpsc::channel(4);
        let mut output = self.output;
        // Keep partial reads alive across command arrival. No select branch owns a pipe read.
        let reader = tokio::spawn(async move {
            loop {
                let message = tokio::time::timeout(Duration::from_secs(10), ipc::read(&mut output))
                    .await
                    .unwrap_or_else(|_| {
                        Err(Error::new(
                            "CAPTURE_STALLED",
                            "Simulator capture source stopped responding",
                        ))
                    });
                let failed = message.is_err();
                if messages.send(message).await.is_err() || failed {
                    break;
                }
            }
        });
        let mut pending = HashMap::<String, oneshot::Sender<Result<Message>>>::new();
        let outcome = async {
            loop {
                tokio::select! {
                    _ = self.stop.changed() => break Ok(()),
                    message = receiver.recv() => {
                        let message = message.ok_or_else(ipc::ended)??;
                        self.health.send_replace(Some(std::time::Instant::now()));
                        match message.metadata["event"].as_str() {
                            Some("reply") => {
                                let id = message.metadata["id"].as_str().ok_or_else(ipc::ended)?;
                                if let Some(reply) = pending.remove(id) {
                                    let error = message.metadata.get("error").map(|error| Error::new(error["code"].as_str().unwrap_or("CAPTURE_FAILED"), error["message"].as_str().unwrap_or("Simulator capture failed")));
                                    let _ = reply.send(if let Some(error) = error { Err(error) } else { Ok(message) });
                                }
                            }
                            Some("frame") => {
                                let index = usize::from(message.metadata["codec"] == "h264");
                                self.frames[index].send_replace(Some(Arc::new(message)));
                            }
                            Some("heartbeat") => {}
                            _ => break Err(Error::new("PLATFORM_PROTOCOL", "Unknown capture event")),
                        }
                    }
                    request = self.requests.recv() => {
                        let Some(request) = request else { break Ok(()); };
                        let Some(reply) = request.reply else {
                            Self::send(&mut self.input, &request.command).await?;
                            continue;
                        };
                        pending.retain(|_, reply| !reply.is_closed());
                        if reply.is_closed() { continue; }
                        if pending.len() >= 32 {
                            let _ = reply.send(Err(Error::new("RESOURCE_LIMIT", "Too many pending capture requests")));
                            continue;
                        }
                        let id = match &request.command { Command::Screenshot { id } | Command::Compact { id } | Command::Geometry { id } | Command::Record { id, .. } => id.clone(), _ => unreachable!() };
                        Self::send(&mut self.input, &request.command).await?;
                        pending.insert(id, reply);
                    }
                    _ = self.changed.notified() => {
                        let command = Command::Profiles { jpeg: self.profiles[0].load(Ordering::Acquire)>0, h264: self.profiles[1].load(Ordering::Acquire)>0 };
                        Self::send(&mut self.input, &command).await?;
                    }
                }
            }
        }.await;
        if let Err(error) = &outcome {
            self.failure.send_replace(Some(error.clone()));
        }
        self.health.send_replace(None);
        let error = outcome.err().unwrap_or_else(ipc::ended);
        for (_, reply) in pending.drain() {
            let _ = reply.send(Err(error.clone()));
        }
        // Close delivery can race a worker that already exited. Reaping, not
        // successful pipe delivery, proves disposal of these local capture handles.
        let _close = Self::send(&mut self.input, &Command::Close).await;
        drop(self.input);
        let reaping = async {
            loop {
                tokio::select! {
                    status = self.child.wait() => break status,
                    Some(_) = receiver.recv() => {},
                }
            }
        };
        let reaped = match tokio::time::timeout(Duration::from_secs(10), reaping).await {
            Ok(Ok(_)) => Ok(()),
            Ok(_) => Err(Error::new(
                "CLEANUP_FAILED",
                "Simulator capture process failed",
            )),
            Err(_) => {
                self.child.kill().await.map_err(|_| {
                    Error::new("CLEANUP_FAILED", "Cannot reap simulator capture process")
                })?;
                Err(Error::new(
                    "CLEANUP_FAILED",
                    "Simulator capture required forced termination",
                ))
            }
        };
        reader.abort();
        let _ = reader.await;
        for frames in &self.frames {
            frames.send_replace(None);
        }
        self.health.send_replace(None);
        for (_, reply) in pending {
            let _ = reply.send(Err(ipc::ended()));
        }
        reaped?;
        // A failed capture operation remains visible through health/replies. Once
        // its process is reaped, its local handles cannot leak; do not turn that
        // prior operation failure into a permanent teardown failure.
        Ok(())
    }
}
