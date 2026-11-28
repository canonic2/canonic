//! Rust-owned native recording jobs with bounded stop/finalization and evidence.
use crate::{
    artifacts,
    error::{Error, Result},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::{sync::oneshot, task::JoinHandle};
use uuid::Uuid;

struct Recording {
    stop: Option<oneshot::Sender<()>>,
    job: Option<JoinHandle<Result<Value>>>,
    result: Option<Value>,
    started: Instant,
    finalization_started: Option<Instant>,
    finalization_budget: Duration,
    finalization_watchdog: Option<JoinHandle<()>>,
    finalization_expired: std::sync::Arc<std::sync::atomic::AtomicBool>,
    temporary: PathBuf,
    android: Option<crate::android::AndroidRecording>,
}
struct SimulatorRecording {
    source: std::sync::Arc<crate::simulator::Source>,
    id: String,
}
impl Drop for SimulatorRecording {
    fn drop(&mut self) {
        if let Err(error) = self.source.stop_recording(&self.id) {
            eprintln!("Testrun recording cleanup request failed: {}", error.code);
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(job) = self.job.take() {
            job.abort();
        }
        if let Some(watchdog) = self.finalization_watchdog.take() {
            watchdog.abort();
        }
    }
}
#[derive(Default)]
pub struct Recordings {
    jobs: HashMap<String, Recording>,
}
impl Recordings {
    /// Session teardown releases terminal bookkeeping, never persisted evidence.
    pub(crate) fn forget(&mut self, id: &str) {
        self.jobs.remove(id);
    }
    #[cfg(target_os = "macos")]
    pub async fn start_device(
        &mut self,
        source: std::sync::Arc<crate::device_media::Source>,
        evidence: &Path,
        duration_ms: u64,
    ) -> Result<Value> {
        if !(100..=600000).contains(&duration_ms) {
            return Err(Error::invalid("maxDurationMs must be 100..600000"));
        }
        let id = Uuid::new_v4().to_string();
        let temporary = evidence.join(format!(".{id}.partial.mp4"));
        let (stop, signal) = oneshot::channel();
        let worker_id = id.clone();
        let evidence = evidence.to_owned();
        let job = tokio::spawn(async move {
            crate::device_media::record::run(source, &evidence, &worker_id, duration_ms, signal)
                .await
        });
        self.jobs.insert(
            id.clone(),
            Recording {
                stop: Some(stop),
                job: Some(job),
                result: None,
                started: Instant::now(),
                temporary,
                android: None,
                finalization_started: None,
                finalization_budget: Duration::from_secs(10),
                finalization_watchdog: None,
                finalization_expired: Default::default(),
            },
        );
        Ok(
            json!({"recordingId":id,"status":"active","maxDurationMs":duration_ms,"finalizationBudgetMs":10000,"sizeLimitBytes":512*1024*1024,"codec":"hevc","container":"mp4","audio":false}),
        )
    }
    pub(crate) async fn start(
        &mut self,
        source: std::sync::Arc<crate::simulator::Source>,
        evidence: &Path,
        duration_ms: u64,
    ) -> Result<Value> {
        if !(100..=600000).contains(&duration_ms) {
            return Err(Error::invalid("maxDurationMs must be 100..600000"));
        }
        let id = Uuid::new_v4().to_string();
        let temporary = evidence.join(format!(".{id}.partial.mp4"));
        let final_path = evidence.join(format!("{id}.mp4"));
        let (stop, signal) = oneshot::channel();
        let path = temporary.clone();
        let worker_id = id.clone();
        let job = tokio::spawn(async move {
            let guard = SimulatorRecording {
                source,
                id: worker_id.clone(),
            };
            let finalization = async {
                let recording = guard.source.record(&worker_id, path.clone(), duration_ms);
                tokio::pin!(recording);
                let result = tokio::select! {
                    biased;
                    result = &mut recording => result?,
                    _ = signal => {
                        guard.source.stop_recording(&worker_id)?;
                        recording.await?
                    },
                };
                let verification = verify_mp4(&path).await?;
                publish(&path, &final_path, &worker_id, result, verification)
            };
            tokio::time::timeout(Duration::from_millis(duration_ms + 90000), finalization)
                .await
                .map_err(|_| {
                    Error::new("TIMEOUT", "Recording finalization exceeded its deadline")
                })?
        });
        self.jobs.insert(
            id.clone(),
            Recording {
                stop: Some(stop),
                job: Some(job),
                result: None,
                started: Instant::now(),
                temporary,
                android: None,
                finalization_started: None,
                finalization_budget: Duration::from_secs(75),
                finalization_watchdog: None,
                finalization_expired: Default::default(),
            },
        );
        Ok(
            json!({"recordingId":id,"status":"active","maxDurationMs":duration_ms,"finalizationBudgetMs":75000,"sizeLimitBytes":512*1024*1024,"codec":"h264","container":"mp4","audio":false}),
        )
    }
    /// Starts one owned Android screenrecord job bounded by the target's limit,
    /// which is also the default duration.
    pub(crate) async fn start_android(
        &mut self,
        serial: &str,
        evidence: &Path,
        requested_ms: Option<u64>,
    ) -> Result<Value> {
        let adapter = crate::android::Android::default();
        let limit = adapter.recording_limit(serial).await?;
        let duration_ms = requested_ms.unwrap_or(limit);
        if !(100..=limit).contains(&duration_ms) {
            return Err(Error::invalid(format!(
                "This Android target records at most {limit} ms; choose maxDurationMs 100..{limit}"
            ))
            .details(json!({"targetMaxDurationMs":limit})));
        }
        let id = Uuid::new_v4().to_string();
        let temporary = evidence.join(format!(".{id}.partial.mp4"));
        let final_path = evidence.join(format!("{id}.mp4"));
        let (stop, signal) = oneshot::channel();
        let path = temporary.clone();
        let worker_id = id.clone();
        let android = adapter.recording(serial, &id);
        let worker = android.clone();
        let job = tokio::spawn(async move {
            let finalization = async {
                let statistics = worker.run(&path, duration_ms, signal).await?;
                let verification = verify_mp4(&path).await?;
                publish(&path, &final_path, &worker_id, statistics, verification)
            };
            tokio::time::timeout(Duration::from_millis(duration_ms + 150000), finalization)
                .await
                .map_err(|_| {
                    Error::new("TIMEOUT", "Recording finalization exceeded its deadline")
                })?
        });
        self.jobs.insert(
            id.clone(),
            Recording {
                stop: Some(stop),
                job: Some(job),
                result: None,
                started: Instant::now(),
                temporary,
                android: Some(android),
                finalization_started: None,
                finalization_budget: Duration::from_secs(140),
                finalization_watchdog: None,
                finalization_expired: Default::default(),
            },
        );
        Ok(
            json!({"recordingId":id,"status":"active","maxDurationMs":duration_ms,"targetMaxDurationMs":limit,"finalizationBudgetMs":140000,"sizeLimitBytes":512*1024*1024,"codec":"h264","container":"mp4","audio":false,"backend":"adb_screenrecord"}),
        )
    }
    pub async fn stop(&mut self, id: &str) -> Result<Value> {
        let recording = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| Error::new("NOT_FOUND", "Recording does not exist"))?;
        if recording.result.is_none() {
            let first = recording.finalization_started.is_none();
            let started = *recording
                .finalization_started
                .get_or_insert_with(Instant::now);
            if let Some(stop) = recording.stop.take() {
                let _ = stop.send(());
            }
            let job = recording.job.as_mut().ok_or_else(|| {
                Error::new("RECORDING_FAILED", "Recording finalization is unavailable")
            })?;
            if first {
                let abort = job.abort_handle();
                let expired = recording.finalization_expired.clone();
                let budget = recording.finalization_budget;
                recording.finalization_watchdog = Some(tokio::spawn(async move {
                    tokio::time::sleep(budget).await;
                    if !abort.is_finished() {
                        expired.store(true, std::sync::atomic::Ordering::Release);
                        abort.abort();
                    }
                }));
            }
            let remaining = recording
                .finalization_budget
                .saturating_sub(started.elapsed());
            let result = match tokio::time::timeout(
                remaining.min(Duration::from_secs(10)),
                &mut *job,
            )
            .await
            {
                Ok(result) => result
                    .map_err(|_| {
                        if recording
                            .finalization_expired
                            .load(std::sync::atomic::Ordering::Acquire)
                        {
                            Error::new(
                                "TIMEOUT",
                                "Recording finalization exhausted its backend budget",
                            )
                        } else {
                            Error::new("RECORDING_FAILED", "Recording worker task stopped")
                        }
                    })
                    .and_then(|result| result),
                Err(_) => {
                    if started.elapsed() < recording.finalization_budget {
                        return Ok(
                            json!({"recordingId":id,"status":"finalizing","retryAfterMs":1000,"bytes":recording.temporary.metadata().ok().map(|metadata|metadata.len())}),
                        );
                    }
                    job.abort();
                    let _ = (&mut *job).await;
                    Err(Error::new(
                        "TIMEOUT",
                        "Recording stop/finalization exceeded its backend budget",
                    ))
                }
            };
            recording.job.take();
            if let Some(watchdog) = recording.finalization_watchdog.take() {
                watchdog.abort();
            }
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    json!({"recordingId":id,"status":"failed","error":error,"partialPath":if recording.temporary.is_file(){Some(&recording.temporary)}else{None}})
                }
            };
            recording.result = Some(result);
        }
        // Cache the operation result before awaiting cleanup: a cancelled stop
        // caller can resume cleanup without polling a completed JoinHandle twice.
        if recording
            .result
            .as_ref()
            .is_some_and(|result| result["error"]["details"].get("cleanupError").is_some())
        {
            return Ok(recording.result.clone().unwrap());
        }
        if let Some(android) = &recording.android {
            match android.cleanup().await {
                Ok(()) => {
                    recording.android.take();
                }
                Err(cleanup) => {
                    recording.result = Some(json!({"recordingId":id,"status":"failed",
                    "error":Error::new("CLEANUP_FAILED", "Android recording cleanup failed")
                        .details(json!({"result":recording.result,"cleanupError":cleanup})),
                    "partialPath":if recording.temporary.is_file(){Some(&recording.temporary)}else{None}}));
                }
            }
        }
        Ok(recording.result.clone().unwrap())
    }
    pub fn active(&self, id: &str) -> bool {
        self.jobs.get(id).is_some_and(|r| {
            r.android.is_some()
                || (r.result.is_none() && r.job.as_ref().is_some_and(|job| !job.is_finished()))
        })
    }
    pub async fn status(&mut self, id: &str) -> Result<Value> {
        let recording = self
            .jobs
            .get(id)
            .ok_or_else(|| Error::new("NOT_FOUND", "Recording does not exist"))?;
        if recording.job.as_ref().is_some_and(JoinHandle::is_finished)
            || recording
                .finalization_expired
                .load(std::sync::atomic::Ordering::Acquire)
            || (recording.result.is_some() && recording.android.is_some())
        {
            return self.stop(id).await;
        }
        Ok(recording.result.clone().unwrap_or_else(||json!({"recordingId":id,"status":if recording.stop.is_none(){"finalizing"}else{"active"},"durationMs":recording.started.elapsed().as_millis(),"bytes":recording.temporary.metadata().ok().map(|m|m.len())})))
    }
}

/// Preserve finalized-video evidence even if its separate manifest cannot be written.
fn publish(
    temporary: &Path,
    final_path: &Path,
    id: &str,
    statistics: Value,
    verification: Value,
) -> Result<Value> {
    let artifact = artifacts::register(temporary, final_path, "video/mp4")?;
    let completed = json!({"recordingId":id,"status":statistics["status"],"artifact":artifact,"statistics":statistics,"verification":verification});
    let manifest = final_path.with_extension("json");
    artifacts::write(&manifest, &serde_json::to_vec_pretty(&completed)
        .map_err(|_|Error::new("INTERNAL", "Cannot encode recording manifest"))?)
        .map_err(|cause|Error::new(&cause.code, "Finalized recording is preserved but its manifest could not be published")
            .details(json!({"recordingId":id,"artifact":completed["artifact"],"manifestPath":manifest,"manifestPublished":false,"cause":cause})))?;
    Ok(completed)
}

/// Checks the MP4 container and, on macOS, decodes it with the native verifier.
async fn verify_mp4(path: &Path) -> Result<Value> {
    let mut header = [0; 12];
    {
        use std::io::Read;
        std::fs::File::open(path)
            .and_then(|mut file| file.read_exact(&mut header))
            .map_err(|_| Error::new("RECORDING_FAILED", "Recording is missing or truncated"))?;
    }
    if &header[4..8] != b"ftyp" {
        return Err(Error::new(
            "RECORDING_FAILED",
            "Recording is not an MP4 container",
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let executable = std::env::current_exe()
            .map_err(|_| Error::new("INTERNAL", "Cannot resolve video verifier"))?;
        let output = crate::process::run(
            executable,
            &[
                "native-verify-video",
                "--path",
                path.to_str()
                    .ok_or_else(|| Error::invalid("Video path is not UTF-8"))?,
            ],
            Duration::from_secs(65),
            4096,
        )
        .await?;
        let verified: Value = serde_json::from_slice(&output.bytes).map_err(|_| {
            Error::new(
                "INVALID_ARTIFACT",
                "Video verifier returned invalid evidence",
            )
        })?;
        if verified["decodable"] != true || verified["verification"] != "full_track" {
            return Err(Error::new(
                "INVALID_ARTIFACT",
                "Video verifier did not establish full-track readability",
            ));
        }
        Ok(verified)
    }
    #[cfg(not(target_os = "macos"))]
    Ok(json!({"verification":"container_only","decodable":null}))
}

#[cfg(test)]
mod tests;
