//! Registry-owned acquisition and release witnesses survive cancelled callers.
use super::{
    lifecycle::{Lifecycle, Release},
    source::Source,
};
use crate::{
    error::{Error, Result},
    lease::ControlLease,
};
use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
};

pub(super) struct Ownership {
    abandoned: AtomicBool,
    lease: Mutex<Option<ControlLease>>,
    remote: Mutex<Option<Arc<dyn Release>>>,
    lifecycle: Mutex<Option<Arc<Lifecycle>>>,
}
impl Ownership {
    fn new(serial: &str) -> Result<Arc<Self>> {
        let lease =
            ControlLease::acquire(&format!("device-media:{serial}")).map_err(|mut error| {
                if error.code == "TARGET_BUSY" {
                    error.code = "CAPTURE_CONFLICT".into();
                    error.message =
                        "Another Testrun process owns or is releasing this device media source"
                            .into();
                }
                error
            })?;
        Ok(Arc::new(Self {
            abandoned: AtomicBool::new(false),
            lease: Mutex::new(Some(lease)),
            remote: Mutex::new(None),
            lifecycle: Mutex::new(None),
        }))
    }
    pub(super) fn allocated(&self, remote: Arc<dyn Release>) {
        *self.remote.lock().unwrap_or_else(|e| e.into_inner()) = Some(remote);
    }
    pub(super) fn streaming(&self, lifecycle: Arc<Lifecycle>) {
        if self.abandoned.load(Ordering::Acquire) {
            lifecycle.request_stop();
        }
        *self.lifecycle.lock().unwrap_or_else(|e| e.into_inner()) = Some(lifecycle);
        // Also cover cancellation between checking the flag and publication.
        if self.abandoned.load(Ordering::Acquire) {
            self.cancel();
        }
    }
    fn cancel(&self) {
        self.abandoned.store(true, Ordering::Release);
        if let Some(lifecycle) = self
            .lifecycle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            lifecycle.request_stop();
        }
    }
    pub(super) fn released(&self) {
        self.lease.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
    pub(super) async fn close(&self) -> Result<()> {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let remote = self
            .remote
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(lifecycle) = lifecycle {
            lifecycle.close().await?;
        } else if self
            .lease
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
            && let Some(remote) = remote
        {
            remote.verify().await?;
        }
        self.released();
        Ok(())
    }
}

pub(super) struct Startup {
    pub(super) ownership: Arc<Ownership>,
    job: Option<tokio::task::JoinHandle<Result<Arc<Source>>>>,
    result: Option<Result<Arc<Source>>>,
}
impl Startup {
    pub(super) fn new<F, Fut>(serial: &str, start: F) -> Result<Self>
    where
        F: FnOnce(Arc<Ownership>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Source>> + Send + 'static,
    {
        let ownership = Ownership::new(serial)?;
        let retained = ownership.clone();
        let job = tokio::spawn(async move {
            let result = start(retained.clone()).await.map(Arc::new);
            if result.is_err()
                && retained
                    .remote
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_none()
            {
                // Prerequisite refusal acquired no remote resource.
                retained.released();
            }
            if retained.abandoned.load(Ordering::Acquire) {
                let cleanup = retained.close().await;
                return Err(Error::new("CANCELLED", "Media acquisition caller ended")
                    .details(serde_json::json!({"cause":result.err(),"cleanup":cleanup.err()})));
            }
            result
        });
        Ok(Self {
            ownership,
            job: Some(job),
            result: None,
        })
    }
    pub(super) fn failed(&self) -> bool {
        self.ownership.abandoned.load(Ordering::Acquire)
            || self.result.as_ref().is_some_and(Result::is_err)
    }
    pub(super) async fn wait(&mut self) -> Result<Arc<Source>> {
        if let Some(result) = &self.result {
            return result.clone();
        }
        let mut waiter = Waiter {
            ownership: self.ownership.clone(),
            completed: false,
        };
        let result = self
            .job
            .as_mut()
            .unwrap()
            .await
            .map_err(|_| Error::new("DEVICE_MEDIA_UNAVAILABLE", "Media acquisition task failed"))
            .and_then(|result| result);
        self.result = Some(result.clone());
        self.job.take();
        waiter.completed = true;
        result
    }
    pub(super) async fn close(&mut self) -> Result<()> {
        // Do not abort protocol negotiation: its owned task must settle whether
        // allocation happened before another caller can claim release.
        let _ = self.wait().await;
        self.ownership.close().await
    }
}

struct Waiter {
    ownership: Arc<Ownership>,
    completed: bool,
}
impl Drop for Waiter {
    fn drop(&mut self) {
        if !self.completed {
            self.ownership.cancel();
        }
    }
}
