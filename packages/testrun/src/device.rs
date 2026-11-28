//! XCTest runner registry, readiness publication and retained per-target cleanup.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, time::Duration};
mod cache;
mod host;
mod preparation;
mod rpc;
mod signing;
mod termination;
use preparation::{PreparedRunner, wait_ready};
pub(crate) use rpc::Runner;

#[derive(Default)]
pub struct Runners {
    runners: HashMap<String, RunnerState>,
}
enum RunnerState {
    Ready(PreparedRunner),
    Closing {
        job: Option<tokio::task::JoinHandle<(PreparedRunner, Result<()>)>>,
        retry: Option<PreparedRunner>,
    },
}
impl Runners {
    pub async fn shutdown(&mut self) -> Result<()> {
        let ids: Vec<_> = self.runners.keys().cloned().collect();
        let mut failures = Vec::new();
        for id in ids {
            if let Err(error) = self.stop(&id).await {
                failures.push(error.code);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(
                Error::new("CLEANUP_FAILED", "Owned device runners could not all stop")
                    .details(json!({"failures":failures})),
            )
        }
    }
    pub fn runner(&self, id: &str) -> Result<&Runner> {
        match self.runners.get(id) {
            Some(RunnerState::Ready(prepared)) => {
                if prepared.process.exited()?.is_some() {
                    return Err(Error::new("RUNNER_EXITED", "The owned XCTest host exited; disconnect before reconnecting with runner setup"));
                }
                Ok(&prepared.runner)
            }
            Some(RunnerState::Closing { .. }) => Err(Error::new(
                "SESSION_CLOSING",
                "Owned runner cleanup is pending or failed",
            )),
            None => Err(Error::new(
                "RUNNER_SETUP_REQUIRED",
                "This operation needs Testrun's signed XCTest runner on the device. HID connections tap, swipe, type, launch and restart apps without it; call connect again with setup: true to add it. Signing reuses the host default or runs first-use selection; manage the default with testrun signing setup",
            )
            .details(json!({"nextAction":"connect","connect":{"setup":true}}))),
        }
    }
    pub(crate) async fn prepare(
        &mut self,
        serial: &str,
        device_id: u32,
        team: Option<&str>,
        source: &Path,
    ) -> Result<Value> {
        let selection = crate::host_setup::select(team).await?;
        if self.runners.contains_key(serial) {
            return Err(Error::new(
                "TARGET_BUSY",
                "This physical target already has an owned runner",
            ));
        }
        let mut prepared =
            PreparedRunner::start(serial, device_id, &selection.runner_team_id, source).await?;
        prepared.signing["source"] = json!(selection.source);
        let runner = prepared.runner.clone();
        let directory = prepared.configuration_dir.clone();
        self.publish_when_ready(serial, prepared, async move {
            wait_ready(&runner)
                .await
                .map_err(|error| preparation::with_host_diagnostic(&directory, error))
        })
        .await
    }
    async fn publish_when_ready(
        &mut self,
        serial: &str,
        mut prepared: PreparedRunner,
        readiness: impl std::future::Future<Output = Result<Value>>,
    ) -> Result<Value> {
        // Until readiness, this transaction alone owns the child and files.
        // Error/cancellation drops them without publishing a busy registry entry.
        let capabilities = readiness.await?;
        prepared.native_pid = Some(
            capabilities["runnerPid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
                .filter(|pid| *pid > 0)
                .ok_or_else(|| {
                    Error::new(
                        "PLATFORM_PROTOCOL",
                        "Authenticated runner hello omitted its native process identity",
                    )
                })?,
        );
        let runner = prepared.runner.clone();
        prepared.heartbeat = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if runner
                    .call("hello", json!({}), None, Duration::from_secs(5))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
        let signing = prepared.signing.clone();
        self.runners
            .insert(serial.to_owned(), RunnerState::Ready(prepared));
        Ok(json!({"prepared":true,"ready":true,"signing":signing}))
    }
    pub async fn stop(&mut self, id: &str) -> Result<()> {
        self.stop_with(id, |mut prepared| async move {
            let result = prepared.stop().await;
            (prepared, result)
        })
        .await
    }
    /// The task owns cleanup across waiter cancellation. A failed attempt gives
    /// its resources back to the registry for the next explicit close.
    async fn stop_with<F, Fut>(&mut self, id: &str, stop: F) -> Result<()>
    where
        F: FnOnce(PreparedRunner) -> Fut,
        Fut: std::future::Future<Output = (PreparedRunner, Result<()>)> + Send + 'static,
    {
        if matches!(self.runners.get(id), Some(RunnerState::Ready(_))) {
            let Some(RunnerState::Ready(prepared)) = self.runners.remove(id) else {
                unreachable!()
            };
            self.runners.insert(
                id.to_owned(),
                RunnerState::Closing {
                    job: None,
                    retry: Some(prepared),
                },
            );
        }
        let Some(RunnerState::Closing { job, retry }) = self.runners.get_mut(id) else {
            return Ok(());
        };
        if let Some(prepared) = retry.take() {
            *job = Some(tokio::spawn(stop(prepared)));
        }
        let pending = job
            .as_mut()
            .ok_or_else(|| Error::new("CLEANUP_FAILED", "Owned runner cleanup is unavailable"))?;
        let settled = pending.await;
        job.take();
        let (prepared, outcome) = settled
            .map_err(|_| Error::new("CLEANUP_FAILED", "Owned runner cleanup task failed"))?;
        if outcome.is_err() {
            *retry = Some(prepared);
        } else {
            self.runners.remove(id);
        }
        outcome
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
