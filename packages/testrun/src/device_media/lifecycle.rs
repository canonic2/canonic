//! Cancellation-safe ownership of remote-media and observation-worker cleanup.
use crate::error::{Error, Result};
use std::{sync::Mutex, time::Duration};
use tokio::sync::oneshot;

/// Read-only remote verification is the replaceable device I/O boundary.
pub(super) trait Release: Send + Sync {
    fn verify(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
}
pub(super) struct Lifecycle {
    pub(super) stream_error: std::sync::Arc<Mutex<Option<Error>>>,
    pub(super) remote: Option<std::sync::Arc<dyn Release>>,
    pub(super) observation: Option<std::sync::Arc<super::observation::Observation>>,
    pub(super) stop: Mutex<Option<oneshot::Sender<()>>>,
    pub(super) join: tokio::sync::Mutex<JoinState>,
}
pub(super) struct JoinState {
    pub(super) job: Option<tokio::task::JoinHandle<Result<()>>>,
    pub(super) result: Option<Result<()>>,
}
impl Lifecycle {
    pub(super) fn stream_error(&self) -> Option<Error> {
        self.stream_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub(super) fn request_stop(&self) {
        if let Some(observation) = &self.observation {
            observation.request_stop();
        }
        if let Some(stop) = self
            .stop
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = stop.send(());
        }
    }
    pub(super) async fn close(&self) -> Result<()> {
        self.request_stop();
        let mut state = self.join.lock().await;
        // Keep the join handle in owned state across await: a signal or a
        // competing close can cancel this caller while remote cleanup runs.
        let result = if let Some(result) = &state.result {
            if result.as_ref().is_err_and(|error| {
                error
                    .details
                    .as_ref()
                    .is_some_and(|d| d["remoteReleasePending"] == true)
            }) && let Some(remote) = &self.remote
            {
                remote.verify().await.map_err(|error| {
                    Error::new("CLEANUP_FAILED", "Owned media release is unverified")
                        .details(serde_json::json!({"remoteReleasePending":true,"cause":error}))
                })
            } else {
                result.clone()
            }
        } else if let Some(job) = state.job.as_mut() {
            match tokio::time::timeout(Duration::from_secs(30), &mut *job).await {
                Ok(result) => {
                    let result = result
                        .map_err(|_| {
                            // Unwinding releases local task state, but does not
                            // prove remote release. Retain the UUID verifier.
                            Error::new("CLEANUP_FAILED", "Media source task failed").details(
                                serde_json::json!({"remoteReleasePending":self.remote.is_some()}),
                            )
                        })
                        .and_then(|result| result);
                    // The source's own release may have run while the device was
                    // unreachable, such as during USB loss. Ask the device now;
                    // a still-present session keeps the original failure.
                    match (result, &self.remote) {
                        (Err(error), Some(remote))
                            if error
                                .details
                                .as_ref()
                                .is_some_and(|d| d["remoteReleasePending"] == true) =>
                        {
                            remote.verify().await.map_err(|cause| {
                                let mut details = error.details.clone().unwrap_or_default();
                                details["verification"] = serde_json::json!(cause);
                                Error {
                                    details: Some(details),
                                    ..error
                                }
                            })
                        }
                        (result, _) => result,
                    }
                }
                Err(_) => {
                    job.abort();
                    let _ = job.await;
                    Err(
                        Error::new("CLEANUP_FAILED", "Device media shutdown timed out").details(
                            serde_json::json!({"remoteReleasePending":self.remote.is_some()}),
                        ),
                    )
                }
            }
        } else {
            Ok(())
        };
        state.job = None;
        state.result = Some(result.clone());
        drop(state);
        let observation = match &self.observation {
            Some(observation) => observation.close().await,
            None => Ok(()),
        };
        match (result, observation) {
            (Err(error), Err(observation)) => {
                let source = error.details.clone();
                Err(error.details(
                    serde_json::json!({"sourceCleanup":source,"observationCleanup":observation}),
                ))
            }
            (Err(error), _) | (_, Err(error)) => Err(error),
            _ => Ok(()),
        }
    }
}
