//! Simulator source identity, single-flight acquisition and retained cleanup.
use crate::error::{Error, Result};
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::watch;

mod cleanup;
pub(crate) mod device_set;
pub(crate) mod incarnation;
pub(crate) mod input;
pub(crate) mod ipc;
pub(crate) mod lifecycle;
mod source;
use cleanup::Cleanup;
#[cfg(target_os = "macos")]
pub(crate) use source::protocol::Command as CaptureCommand;
pub(crate) use source::{Source, Subscription};

type Startup = Option<Result<Arc<Source>>>;
enum State {
    Starting(watch::Receiver<Startup>),
    Ready {
        source: Weak<Source>,
        cleanup: Arc<Cleanup>,
    },
    Failed(Error),
    Closing(Arc<Cleanup>),
}
struct Entry {
    state: Mutex<State>,
}
#[derive(Default)]
pub(crate) struct Sources {
    entries: Mutex<HashMap<String, Arc<Entry>>>,
}
impl Sources {
    pub(crate) async fn acquire(
        &self,
        uuid: &str,
        identity: &serde_json::Value,
        epoch: &str,
    ) -> Result<Arc<Source>> {
        let uuid = uuid.to_owned();
        let epoch = epoch.to_owned();
        self.acquire_with(identity.to_string(), move || async move {
            Source::start(&uuid, &epoch).await
        })
        .await
    }
    async fn acquire_with<F, Fut>(&self, key: String, start: F) -> Result<Arc<Source>>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<Arc<Source>>> + Send + 'static,
    {
        let mut start = Some(start);
        loop {
            let entry = {
                let mut entries = self.entries.lock().map_err(|_| unavailable())?;
                if let Some(entry) = entries.get(&key) {
                    entry.clone()
                } else {
                    if entries.len() >= 16 {
                        return Err(Error::new(
                            "RESOURCE_LIMIT",
                            "Simulator capture source capacity reached",
                        ));
                    }
                    let (ready, receiver) = watch::channel(None);
                    let entry = Arc::new(Entry {
                        state: Mutex::new(State::Starting(receiver)),
                    });
                    entries.insert(key.clone(), entry.clone());
                    let owner = entry.clone();
                    let start = start.take().ok_or_else(unavailable)?;
                    tokio::spawn(async move {
                        let result = start().await;
                        *owner.state.lock().unwrap_or_else(|e| e.into_inner()) = match &result {
                            Ok(source) => State::Ready {
                                source: Arc::downgrade(source),
                                cleanup: source.cleanup.clone(),
                            },
                            Err(error) => State::Failed(error.clone()),
                        };
                        ready.send_replace(Some(result));
                    });
                    entry
                }
            };
            let (ready, closing) = {
                let mut state = entry.state.lock().map_err(|_| unavailable())?;
                match &*state {
                    State::Starting(ready) => (Some(ready.clone()), None),
                    State::Ready { source, cleanup } => {
                        if let Some(source) = source.upgrade().filter(|source| source.can_share()) {
                            return Ok(source);
                        }
                        let cleanup = cleanup.clone();
                        *state = State::Closing(cleanup.clone());
                        (None, Some(cleanup))
                    }
                    State::Failed(error) => return Err(error.clone()),
                    State::Closing(cleanup) => (None, Some(cleanup.clone())),
                }
            };
            if let Some(mut ready) = ready {
                loop {
                    if let Some(result) = ready.borrow().as_ref() {
                        return result.clone();
                    }
                    ready.changed().await.map_err(|_| unavailable())?;
                }
            }
            if let Some(cleanup) = closing {
                cleanup.close().await?;
                let mut entries = self.entries.lock().map_err(|_| unavailable())?;
                if entries
                    .get(&key)
                    .is_some_and(|current| Arc::ptr_eq(current, &entry))
                {
                    entries.remove(&key);
                }
            }
        }
    }
    pub(crate) async fn release_unused(&self) -> Result<()> {
        self.release(false).await
    }
    pub(crate) async fn shutdown(&self) -> Result<()> {
        self.release(true).await
    }
    async fn release(&self, all: bool) -> Result<()> {
        let entries: Vec<_> = self
            .entries
            .lock()
            .map_err(|_| unavailable())?
            .iter()
            .map(|(key, entry)| (key.clone(), entry.clone()))
            .collect();
        let mut failures = Vec::new();
        for (key, entry) in entries {
            let startup = match &*entry.state.lock().map_err(|_| unavailable())? {
                State::Starting(ready) if all => Some(ready.clone()),
                _ => None,
            };
            if let Some(mut ready) = startup
                && ready.borrow().is_none()
            {
                let _ = ready.changed().await;
            }
            let cleanup = {
                let mut state = entry.state.lock().map_err(|_| unavailable())?;
                let cleanup = match &*state {
                    State::Ready { source, cleanup } if all || source.strong_count() == 0 => {
                        Some(cleanup.clone())
                    }
                    State::Closing(cleanup) => Some(cleanup.clone()),
                    State::Failed(error) if error.code == "CLEANUP_FAILED" => {
                        failures.push(error.clone());
                        continue;
                    }
                    State::Failed(_) => None,
                    _ => continue,
                };
                if let Some(cleanup) = &cleanup {
                    *state = State::Closing(cleanup.clone());
                }
                cleanup
            };
            if let Some(cleanup) = cleanup
                && let Err(error) = cleanup.close().await
            {
                failures.push(error);
                continue;
            }
            let mut entries = self.entries.lock().map_err(|_| unavailable())?;
            if entries
                .get(&key)
                .is_some_and(|current| Arc::ptr_eq(current, &entry))
            {
                entries.remove(&key);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(
                Error::new("CLEANUP_FAILED", "Simulator capture cleanup failed")
                    .details(serde_json::json!({"failures":failures})),
            )
        }
    }
}
fn unavailable() -> Error {
    Error::new(
        "CAPTURE_UNAVAILABLE",
        "Simulator capture registry unavailable",
    )
}

#[cfg(test)]
mod tests;
