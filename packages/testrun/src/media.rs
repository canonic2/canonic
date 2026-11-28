//! Preview composition, registry and lifecycle; adapters and viewers own their I/O.
use crate::error::{Error, Result};
use axum::{Router, routing::get};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Semaphore, watch},
    task::JoinHandle,
};
#[cfg(unix)]
use uuid::Uuid;

#[cfg(target_os = "macos")]
mod device;
mod external;
mod grants;
mod packet;
mod simulator;
mod startup;
mod viewer;
use grants::Grants;
#[cfg(unix)]
use grants::grant;
pub use packet::{Frame, encode, encode_packet, h264_configuration, lengths};
use viewer::upgrade;

struct Preview {
    browser: bool,
    grants: Grants,
    frames: watch::Sender<Option<Arc<Frame>>>,
    slots: Arc<Semaphore>,
    worker: JoinHandle<()>,
    _simulator: Option<crate::simulator::Subscription>,
    requests: tokio::sync::mpsc::Sender<()>,
    health: Arc<Mutex<Health>>,
    #[cfg(target_os = "macos")]
    _source: Option<Arc<crate::device_media::Source>>,
    delivery: Option<tokio::sync::broadcast::Sender<Arc<Frame>>>,
}
struct Health {
    last_seen: std::time::Instant,
    encoded: u64,
    activity: String,
    alive: bool,
}
impl Drop for Preview {
    fn drop(&mut self) {
        self.grants
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        {
            let mut health = self
                .health
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            health.alive = false;
            self.frames.send_replace(None);
        }
        self.worker.abort();
    }
}
#[derive(Clone, Default)]
struct Shared {
    previews: Arc<Mutex<HashMap<String, Preview>>>,
}
pub struct Media {
    shared: Shared,
    address: String,
    broker: PathBuf,
    http: JoinHandle<()>,
    #[cfg(unix)]
    grants: JoinHandle<()>,
    #[cfg(windows)]
    grants: grants::windows::Broker,
    #[cfg(unix)]
    _files: BrokerFiles,
}
#[cfg(unix)]
struct BrokerFiles(PathBuf);
#[cfg(unix)]
impl Drop for BrokerFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0.join("grants.sock"));
        let _ = std::fs::remove_dir(&self.0);
    }
}
impl Drop for Media {
    fn drop(&mut self) {
        self.shared
            .previews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.http.abort();
        let _ = self.close_broker();
    }
}
impl Media {
    pub fn close(&mut self) -> Result<()> {
        self.shared
            .previews
            .lock()
            .map_err(|_| Error::new("CLEANUP_FAILED", "Cannot release media sources"))?
            .clear();
        self.http.abort();
        self.close_broker()
    }
    #[cfg(windows)]
    fn close_broker(&mut self) -> Result<()> {
        self.grants.close()
    }
    #[cfg(unix)]
    fn close_broker(&self) -> Result<()> {
        self.grants.abort();
        for path in [&self.broker, &self._files.0] {
            let result = if path == &self.broker {
                std::fs::remove_file(path)
            } else {
                std::fs::remove_dir(path)
            };
            if let Err(error) = result
                && error.kind() != std::io::ErrorKind::NotFound
            {
                return Err(Error::new(
                    "CLEANUP_FAILED",
                    "Cannot remove private media broker files",
                ));
            }
        }
        Ok(())
    }
    pub async fn start() -> Result<Self> {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| Error::new("MEDIA_UNAVAILABLE", "Cannot bind loopback media listener"))?;
        let bound = listener
            .local_addr()
            .map_err(|_| Error::new("MEDIA_UNAVAILABLE", "Cannot resolve media address"))?;
        startup::check_address(bound)?;
        let address = format!("http://{bound}");
        let shared = Shared::default();
        let app = Router::new()
            .route("/v1/ios/streams/{id}/media", get(upgrade))
            .route("/v1/browser/streams/{id}/media", get(upgrade))
            .with_state(shared.clone());
        #[cfg(unix)]
        let (broker, grants, files) = {
            use std::os::unix::fs::PermissionsExt;
            // macOS TMPDIR commonly exceeds the socket path limit.
            let root = std::fs::canonicalize("/tmp")
                .map_err(|_| Error::new("MEDIA_UNAVAILABLE", "Cannot resolve media broker root"))?;
            let directory = root.join(format!("testrun-media-{}", Uuid::new_v4()));
            crate::artifacts::directory(&directory).map_err(|_| {
                Error::new(
                    "MEDIA_UNAVAILABLE",
                    "Cannot create private media broker directory",
                )
            })?;
            let files = BrokerFiles(directory);
            startup::check_directory(&files.0)?;
            let broker = files.0.join("grants.sock");
            let listener = tokio::net::UnixListener::bind(&broker).map_err(|_| {
                Error::new(
                    "MEDIA_UNAVAILABLE",
                    "Cannot create private media grant socket",
                )
            })?;
            std::fs::set_permissions(&broker, std::fs::Permissions::from_mode(0o600)).map_err(
                |_| Error::new("MEDIA_UNAVAILABLE", "Cannot restrict media grant socket"),
            )?;
            startup::check_socket(&broker)?;
            let state = shared.clone();
            let grants = tokio::spawn(async move {
                while let Ok((socket, _)) = listener.accept().await {
                    let state = state.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::timeout(Duration::from_secs(3), grant(socket, state))
                            .await;
                    });
                }
            });
            (broker, grants, files)
        };
        #[cfg(windows)]
        let (broker, grants) = {
            let grants = grants::windows::Broker::start(shared.clone()).await?;
            (grants.path(), grants)
        };
        let http = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                eprintln!("Testrun media listener stopped: {}", error.kind());
            }
        });
        Ok(Self {
            shared,
            address,
            broker,
            http,
            grants,
            #[cfg(unix)]
            _files: files,
        })
    }
    pub fn stop(&self, id: &str) -> Result<Value> {
        let removed = self
            .shared
            .previews
            .lock()
            .map_err(|_| Error::new("INTERNAL", "Media registry unavailable"))?
            .remove(id);
        Ok(json!({"stopped":removed.is_some()}))
    }
    pub fn status(&self, id: &str) -> Result<Value> {
        let previews = self
            .shared
            .previews
            .lock()
            .map_err(|_| Error::new("INTERNAL", "Media registry unavailable"))?;
        let preview = previews
            .get(id)
            .ok_or_else(|| Error::new("NOT_FOUND", "Preview is not active"))?;
        let health = preview
            .health
            .lock()
            .map_err(|_| Error::new("INTERNAL", "Native source health unavailable"))?;
        let frame = preview.frames.borrow().clone();
        Ok(
            json!({"streamId":id,"state":if !health.alive||health.last_seen.elapsed()>Duration::from_secs(5){"ended"}else if health.last_seen.elapsed()>Duration::from_secs(2){"stalled"}else{"live"},"lastFrameAgeMs":frame.as_ref().map(|f|f.received.elapsed().as_millis()),"encodedFrames":health.encoded,"damageCallbacks":health.activity,"consumerCount":8-preview.slots.available_permits(),"metadata":frame.as_ref().map(|f|&f.metadata)}),
        )
    }
}

#[cfg(test)]
mod tests;
