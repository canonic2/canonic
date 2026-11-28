//! Browser JPEG feeds using the existing private viewer/grant transport.

use super::{Frame, Health, Media, Preview};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, mpsc, watch};

impl Media {
    pub(crate) fn browser_preview(
        &self,
        id: &str,
        mut source: watch::Receiver<Option<Arc<Frame>>>,
        alive: Arc<AtomicBool>,
    ) -> Result<Value> {
        let (frames, _) = watch::channel(source.borrow().clone());
        let sender = frames.clone();
        let (requests, mut commands) = mpsc::channel(1);
        let health = Arc::new(Mutex::new(Health {
            last_seen: Instant::now(),
            encoded: 0,
            activity: "browser-source".into(),
            alive: true,
        }));
        let worker_health = health.clone();
        let worker = tokio::spawn(async move {
            let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    result = source.changed() => {
                        if result.is_err() { break; }
                        let frame = source.borrow_and_update().clone();
                        if let Ok(mut health) = worker_health.lock() {
                            if !health.alive { break; }
                            health.encoded += u64::from(frame.is_some());
                            sender.send_replace(frame);
                        } else { break; }
                    }
                    _ = heartbeat.tick() => {
                        if !alive.load(Ordering::Acquire) { break; }
                        if let Ok(mut health) = worker_health.lock() { health.last_seen=Instant::now(); }
                    }
                    command = commands.recv() => { if command.is_none() { break; } }
                }
            }
            sender.send_replace(None);
            if let Ok(mut health) = worker_health.lock() {
                health.alive = false;
            }
        });
        let preview = Preview {
            browser: true,
            grants: Arc::default(),
            frames,
            slots: Arc::new(Semaphore::new(8)),
            worker,
            _simulator: None,
            requests,
            health,
            #[cfg(target_os = "macos")]
            _source: None,
            delivery: None,
        };
        let mut previews = self
            .shared
            .previews
            .lock()
            .map_err(|_| Error::new("INTERNAL", "Media registry unavailable"))?;
        if previews.contains_key(id) {
            return Err(Error::new(
                "MEDIA_SOURCE_EXISTS",
                "Browser preview already registered",
            ));
        }
        previews.insert(id.into(), preview);
        Ok(
            json!({"streamId":id,"mediaUrl":format!("{}/v1/browser/streams/{id}/media",self.address),"grantBrokerPath":self.broker,"access":"read_only","codec":"jpeg","audio":false}),
        )
    }
}
