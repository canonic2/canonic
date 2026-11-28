//! Target release and manager exit. The manager owns admitted disconnects,
//! idle device release and drain. A failed release keeps the target pooled
//! with its blocker and is retried automatically; it never holds another target.
//! The manager exits when nothing is left.
use super::{Kind, Service};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// One release attempt in progress. However it stops, even by a panic, the
/// attempt is no longer in progress afterwards; one that never finished
/// leaves a failed release the next retry picks up.
pub(super) struct Releasing {
    pub(super) service: Arc<Service>,
    pub(super) target: String,
    pub(super) finished: bool,
}
impl Drop for Releasing {
    fn drop(&mut self) {
        if !self.finished {
            self.service.registry().release_failed(
                &self.target,
                json!(Error::new("RELEASE_STOPPED", "The release attempt stopped")),
            );
        }
        self.service
            .releases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.target);
        self.service.prune();
        self.service.notify();
    }
}

impl Service {
    /// Release `target` for every client. Concurrent callers share one attempt.
    pub(super) async fn release(
        self: &Arc<Self>,
        target: &str,
        kind: Kind,
        request: Option<String>,
    ) -> Result<Value> {
        let mut receiver = {
            // The retirement check also takes the registry lock before releases.
            let mut registry = self.registry();
            let mut releases = self
                .releases
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(receiver) = releases.get(target) {
                receiver.clone()
            } else {
                let (sender, receiver) = tokio::sync::watch::channel(None);
                releases.insert(target.to_owned(), receiver.clone());
                registry.release_started(target);
                let service = self.clone();
                let target = target.to_owned();
                // The release owns itself: a caller's deadline never abandons it.
                super::detached(move || async move {
                    let mut releasing = Releasing {
                        service: service.clone(),
                        target: target.clone(),
                        finished: false,
                    };
                    let result = service.release_now(&target, kind, request).await;
                    sender.send_replace(Some(result));
                    releasing.finished = true;
                });
                receiver
            }
        };
        receiver
            .wait_for(Option::is_some)
            .await
            .ok()
            .and_then(|result| result.clone())
            .unwrap_or_else(|| Err(Error::new("INTERNAL", "The release stopped")))
    }

    async fn release_now(
        self: &Arc<Self>,
        target: &str,
        kind: Kind,
        request: Option<String>,
    ) -> Result<Value> {
        let browser = target.starts_with(crate::browser::runtime::TARGET_PREFIX);
        let result = if browser {
            self.release_browser(target).await
        } else {
            let Some(mut caller) = self.caller(kind) else {
                return Ok(json!({"state":"closed","closed":true,"targetId":target}));
            };
            caller.target = Some(target.to_owned());
            caller.request_id = request;
            caller.call("disconnect", json!({})).await
        };
        match &result {
            Ok(closed) if !browser && closed["closed"] == true => self.release_target(target),
            Ok(_) => {}
            Err(error) => {
                self.registry().release_failed(target, json!(error));
            }
        }
        self.persist();
        self.notify();
        result
    }

    /// The hosted manager's lifetime loop: release idle targets, retry failed
    /// releases and keep the library's busy hold while anything is held. The
    /// library closes admission and exits once the hold and all admitted work
    /// are gone; this loop never decides exit itself.
    pub(crate) async fn keep_alive(self: &Arc<Self>) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            self.release_idle();
            self.keep(None);
            let _ = tokio::time::timeout(Duration::from_millis(500), changed).await;
        }
    }

    /// Start releases of idle targets, at once while draining, and of
    /// failed releases whose retry is due.
    fn release_idle(self: &Arc<Self>) {
        if self.registry().draining {
            self.diagnostics.stop_all(None, "manager_draining");
        }
        let (idle, due) = {
            let registry = self.registry();
            let idle = if registry.draining {
                registry.idle_after(Duration::ZERO)
            } else {
                registry.idle_after(self.idle_release)
            };
            (idle, registry.due_releases())
        };
        for target in idle.into_iter().chain(due) {
            let releasing = self
                .releases
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(&target);
            if !releasing {
                let service = self.clone();
                tokio::spawn(async move {
                    let _ = service.release(&target, Kind::Cli, None).await;
                });
            }
        }
    }

    /// Finish the queues and release every device as soon as nothing runs on
    /// it, then retire. The library refuses new requests meanwhile.
    pub(crate) fn drain(&self) {
        self.registry().draining = true;
        self.notify();
    }
}

/// A failed release is retried by the manager: say so, and name the human
/// prerequisite when the device reported one. Applied once, to the error a
/// client receives.
pub(crate) fn retrying(mut error: Error) -> Error {
    if error.code != "CLEANUP_FAILED" {
        return error;
    }
    let text = serde_json::to_string(&error).unwrap_or_default();
    let next = if text.contains("PasswordProtected") || text.contains("\"locked\"") {
        "unlock_device"
    } else if text.contains("USB_UNAVAILABLE") || text.contains("disconnected") {
        "reattach_device"
    } else {
        "status"
    };
    let mut details = error.details.take().unwrap_or_else(|| json!({}));
    if !details.is_object() {
        details = json!({"cause":details});
    }
    details["retrying"] = json!(true);
    details["nextAction"] = json!(next);
    error.details = Some(details);
    error.retryable = true;
    error
}

impl crate::managed::Caller {
    /// Stop running recordings through a target's device actor, keeping
    /// their evidence. `None` when the target has no live device.
    pub(crate) async fn finalize_recordings(&self, directory: &std::path::Path) -> Option<Value> {
        let connection = self.selected(directory).ok()?;
        let serving =
            crate::managed::storage::read::<Value>(&connection.directory.join("state.json"))
                .ok()
                .flatten()
                .is_some_and(|state| state["state"] == "ready");
        if !serving || !connection.held().unwrap_or(false) {
            return None;
        }
        tokio::time::timeout(
            Duration::from_secs(30),
            Self::send(&connection, json!({"kind":"finalize_recordings"})),
        )
        .await
        .ok()?
        .ok()
        .map(|result| result["recordings"].clone())
    }
}
