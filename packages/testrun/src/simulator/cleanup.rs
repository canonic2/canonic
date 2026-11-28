//! Retained input/capture cleanup, independent of acquisition and close waiters.
use crate::error::{Error, Result};
use std::sync::Arc;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};

struct Outcome {
    job: Option<JoinHandle<Result<()>>>,
    result: Option<Result<()>>,
}
pub(super) struct Cleanup {
    stop: watch::Sender<bool>,
    outcome: Mutex<Outcome>,
}
impl Cleanup {
    pub(super) fn new(stop: watch::Sender<bool>, job: JoinHandle<Result<()>>) -> Arc<Self> {
        Arc::new(Self {
            stop,
            outcome: Mutex::new(Outcome {
                job: Some(job),
                result: None,
            }),
        })
    }
    pub(super) fn stop(&self) {
        self.stop.send_replace(true);
    }
    pub(super) async fn close(&self) -> Result<()> {
        self.stop();
        let mut outcome = self.outcome.lock().await;
        if let Some(result) = &outcome.result {
            return result.clone();
        }
        let result = outcome
            .job
            .as_mut()
            .ok_or_else(|| Error::new("CLEANUP_FAILED", "Simulator worker cleanup unavailable"))?
            .await
            .map_err(|_| Error::new("CLEANUP_FAILED", "Simulator worker task stopped"))
            .and_then(|result| result);
        outcome.job.take();
        outcome.result = Some(result.clone());
        result
    }
}
