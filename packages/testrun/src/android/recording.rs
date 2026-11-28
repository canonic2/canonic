//! Owned, bounded `screenrecord` jobs: one remote process and file per recording.
use super::Android;
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::{Mutex, oneshot},
};

/// Testrun's own request bound, shared with iOS recordings.
const TESTRUN_LIMIT_MS: u64 = 600_000;
/// screenrecord's documented maximum where it cannot remove the limit.
const LEGACY_LIMIT_MS: u64 = 180_000;

/// Shared with the recording registry so cancellation cannot discard cleanup state.
#[derive(Clone)]
pub(crate) struct Recording(Arc<Mutex<Remote>>);

/// The remote identity and local adb child remain owned until cleanup completes.
struct Remote {
    adb: std::path::PathBuf,
    serial: String,
    path: String,
    pid: Option<String>,
    child: Option<tokio::process::Child>,
    finalized: bool,
}

impl Android {
    /// Reads the target's screenrecord limit without recording.
    pub(crate) async fn recording_limit(&self, serial: &str) -> Result<u64> {
        let help = self
            .shell(
                serial,
                &["sh", "-c", "screenrecord --help 2>&1"],
                Duration::from_secs(5),
                65536,
            )
            .await
            .map_err(|error| {
                Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    "screenrecord is unavailable on this Android target",
                )
                .details(json!({"cause":error.code}))
            })?;
        let help = String::from_utf8_lossy(&help);
        if !help.contains("--time-limit") {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "screenrecord on this Android target has no time limit option",
            ));
        }
        Ok(if help.contains("Set to 0") {
            TESTRUN_LIMIT_MS
        } else {
            LEGACY_LIMIT_MS
        })
    }

    pub(crate) fn recording(&self, serial: &str, id: &str) -> Recording {
        Recording(Arc::new(Mutex::new(Remote {
            adb: self.adb.clone(),
            serial: serial.to_owned(),
            path: format!("/data/local/tmp/testrun-recording-{id}.mp4"),
            pid: None,
            child: None,
            finalized: false,
        })))
    }
}

impl Recording {
    /// Called after the worker has returned or been aborted and joined. All adb
    /// cleanup commands are bounded and awaited; failures remain in the result.
    pub(crate) async fn cleanup(&self) -> Result<()> {
        let mut remote = self.0.lock().await;
        if remote.finalized {
            return Ok(());
        }
        let adapter = Android {
            adb: remote.adb.clone(),
        };
        let serial = remote.serial.clone();
        let mut failures = Vec::new();
        let pid = remote.pid.clone();
        if let Some(child) = &mut remote.child {
            match child.try_wait() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    if let Some(pid) = pid {
                        if let Err(error) = adapter
                            .shell(
                                &serial,
                                &["kill", "-INT", &pid],
                                Duration::from_secs(3),
                                4096,
                            )
                            .await
                        {
                            failures.push(error);
                        }
                    } else {
                        failures.push(Error::new(
                            "CLEANUP_FAILED",
                            "Android recording process identity was not received",
                        ));
                    }
                    let wait = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
                    if !matches!(wait, Ok(Ok(_))) {
                        failures.push(Error::new(
                            "CLEANUP_FAILED",
                            "Android recording did not exit during cleanup",
                        ));
                        if !matches!(
                            tokio::time::timeout(Duration::from_secs(2), child.kill()).await,
                            Ok(Ok(()))
                        ) {
                            failures.push(Error::new(
                                "CLEANUP_FAILED",
                                "Could not reap recording adb within its deadline",
                            ));
                        }
                    }
                }
                Err(error) => failures.push(Error::new(
                    "CLEANUP_FAILED",
                    format!("Could not inspect recording adb: {error}"),
                )),
            }
        }
        remote.child.take();
        remote.pid = None;
        if let Err(error) = adapter
            .shell(
                &serial,
                &["rm", "-f", &remote.path],
                Duration::from_secs(3),
                4096,
            )
            .await
        {
            failures.push(error);
        }
        if !failures.is_empty() {
            return Err(Error::new(
                "CLEANUP_FAILED",
                "Android recording cleanup could not be established",
            )
            .details(json!({"failures":failures})));
        }
        remote.finalized = true;
        Ok(())
    }

    /// Records until the time limit or stop, retaining partial evidence on failure.
    pub(crate) async fn run(
        &self,
        temporary: &Path,
        duration_ms: u64,
        stop: oneshot::Receiver<()>,
    ) -> Result<Value> {
        let mut remote = self.0.lock().await;
        let adapter = Android {
            adb: remote.adb.clone(),
        };
        let serial = remote.serial.clone();
        let seconds = duration_ms.div_ceil(1000).max(1);
        // `exec` keeps the reported shell PID as screenrecord's own PID.
        let script = format!(
            "echo $$; exec screenrecord --time-limit {seconds} {}",
            super::adb::quote(&remote.path)
        );
        remote.child = Some(
            tokio::process::Command::new(&adapter.adb)
                .args(["-s", &serial, "shell", &script])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|_| Error::new("PREREQUISITE_MISSING", "Could not start adb"))?,
        );
        let mut output =
            BufReader::new(remote.child.as_mut().unwrap().stdout.take().unwrap()).lines();
        let pid = tokio::time::timeout(Duration::from_secs(5), output.next_line())
            .await
            .ok()
            .and_then(std::result::Result::ok)
            .flatten()
            .map(|line| line.trim().to_owned())
            .filter(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
            .ok_or_else(|| {
                Error::new(
                    "RECORDING_FAILED",
                    "Android screenrecord did not report its process",
                )
            })?;
        remote.pid = Some(pid.clone());
        let started = std::time::Instant::now();
        let child = remote.child.as_mut().unwrap();
        let (reason, status) = tokio::select! {
            biased;
            status = child.wait() => {
                ("time_limit", status.map_err(|_| Error::new("RECORDING_FAILED", "Lost the screenrecord process"))?)
            }
            _ = stop => {
                // The remote process is still running, so its PID is current.
                adapter.shell(&serial, &["kill", "-INT", &pid], Duration::from_secs(5), 4096)
                    .await?;
                let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
                    .await
                    .map_err(|_| Error::new("TIMEOUT", "screenrecord did not finalize after stop"))?
                    .map_err(|_| Error::new("RECORDING_FAILED", "Lost the screenrecord process"))?;
                ("stopped", status)
            }
        };
        // screenrecord has exited, so its PID may be reused; cleanup must not signal it.
        remote.pid = None;
        let elapsed = started.elapsed().as_millis();
        let target = temporary
            .to_str()
            .ok_or_else(|| Error::invalid("Recording path is not UTF-8"))?;
        crate::process::run(
            &adapter.adb,
            &["-s", &serial, "pull", &remote.path, target],
            Duration::from_secs(60),
            65536,
        )
        .await
        .map_err(|error| {
            Error::new("RECORDING_FAILED", "Could not copy the Android recording")
                .details(json!({"cause":error.code}))
        })?;
        // Preserve copied partial evidence, but never verify/publish it as success.
        if !status.success() {
            return Err(Error::new(
                "RECORDING_FAILED",
                "Android screenrecord exited unsuccessfully",
            )
            .details(json!({"exitCode":status.code()})));
        }
        adapter
            .shell(
                &serial,
                &["rm", "-f", &remote.path],
                Duration::from_secs(5),
                4096,
            )
            .await?;
        remote.finalized = true;
        Ok(
            json!({"status":"complete","reason":reason,"durationMs":elapsed,"timeLimitSeconds":seconds}),
        )
    }
}

#[cfg(all(test, unix))]
mod tests;
