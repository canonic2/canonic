//! Native source recording consumers retain encoder jobs and terminal evidence across callers.
use super::*;
use crate::browser::recording::{Input, Job, Options, Recording};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex as AsyncMutex, oneshot};
impl Worker {
    #[expect(
        clippy::too_many_lines,
        reason = "Recording admission installs the independent source consumer and retained encoder job as one ownership transition"
    )]
    pub(super) async fn native_recording_start(
        &mut self,
        id: Uuid,
        page: Reference,
        options: Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        #[cfg(windows)]
        if !crate::browser::recording::available() {
            return Err("RECORDING_UNAVAILABLE".into());
        }
        if self
            .media
            .recordings
            .values()
            .any(|recording| recording.page == page && !recording.terminal())
        {
            return Err("RECORDING_ALREADY_ACTIVE".into());
        }
        if self
            .media
            .recordings
            .values()
            .filter(|recording| !recording.terminal())
            .count()
            >= 8
        {
            return Err("RECORDING_CAPACITY".into());
        }
        if self.media.recordings.len() >= 32 {
            let retired = self
                .media
                .recordings
                .iter()
                .find(|(_, job)| job.terminal())
                .map(|(reference, _)| *reference)
                .ok_or("RECORDING_CAPACITY")?;
            self.media.recordings.remove(&retired);
        }
        let directory = crate::browser::recording::private_directory(self.journal.directory())
            .map_err(|cause| Error::caused("RECORDING_IO", cause))?;
        let current = self
            .media
            .sources
            .lock()
            .expect("Native sources")
            .values()
            .find(|entry| entry.source.page == page && entry.source.state != "stopped")
            .map(|entry| entry.source.options.clone())
            .unwrap_or_default();
        let source = self
            .preview_start(id, page, current, false, deadline)
            .await?;
        let source: Reference = serde_json::from_value(source["preview"].clone())
            .map_err(|_| Error::from("MEDIA_SOURCE_UNAVAILABLE"))?;
        let reference = self.transfer_reference();
        let started = std::time::Instant::now();
        let ready = Arc::new(AtomicBool::new(false));
        let finalizing = Arc::new(AtomicBool::new(false));
        let terminal = Arc::new(Mutex::new(None));
        let (signal, stop) = oneshot::channel();
        let input = {
            let mut entries = self.media.sources.lock().expect("Native sources");
            let source = &mut entries.get_mut(&source).ok_or("STALE_PREVIEW")?.source;
            let first = source
                .frames
                .borrow()
                .clone()
                .ok_or("FIRST_FRAME_TIMEOUT")?;
            source.recordings.insert(reference.id);
            Input {
                recording: reference,
                page,
                options: options.clone(),
                directory,
                frames: source.frames.subscribe(),
                first,
                started,
                interruptions: source.interruptions.clone(),
                initial_interruptions: source.interruptions.load(Ordering::SeqCst),
                alive: source.alive.clone(),
                finalizing: finalizing.clone(),
                ready: ready.clone(),
            }
        };
        let published = terminal.clone();
        let worker = tokio::spawn(async move {
            let result = match crate::browser::recording::run(input, stop).await {
                Ok(result) => result,
                Err(error) => {
                    json!({"recording":reference,"page":page,"status":"failed","error":error,"artifact":error.details.as_ref().map(|details|&details["publishedArtifact"])})
                }
            };
            *published.lock().expect("Recording terminal") = Some(result.clone());
            Ok(result)
        });
        let recording = Arc::new(Recording {
            reference,
            source,
            page,
            started,
            options,
            signal: Mutex::new(Some(signal)),
            job: AsyncMutex::new(Job {
                worker: Some(worker),
                result: None,
            }),
            terminal,
            finalizing,
            ready,
        });
        let snapshot = recording.snapshot();
        self.media.recordings.insert(reference, recording);
        Ok(snapshot)
    }
    pub(super) async fn native_recording_result(
        &mut self,
        reference: Reference,
        stop: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.transfer_scope(reference)?;
        let recording = self
            .media
            .recordings
            .get(&reference)
            .ok_or("STALE_RECORDING")?
            .clone();
        if stop {
            recording.stop();
        }
        let mut job = tokio::time::timeout_at(deadline, recording.job.lock())
            .await
            .map_err(|_| Error::from("RECORDING_CLEANUP_PENDING"))?;
        if job.result.is_none() {
            let worker = job.worker.as_mut().ok_or("RECORDING_OWNER_FAILED")?;
            if !stop && !worker.is_finished() {
                drop(job);
                return Ok(recording.snapshot());
            }
            let result = match tokio::time::timeout_at(deadline, worker).await {
                Ok(result) => result
                    .map_err(|_| Error::from("RECORDING_OWNER_FAILED"))?
                    .map_err(|error| Error::caused("RECORDING_FAILED", error))?,
                Err(_) => {
                    drop(job);
                    return Ok(recording.snapshot());
                }
            };
            job.worker = None;
            job.result = Some(result);
        }
        Ok(job.result.clone().expect("Recording result"))
    }
}
