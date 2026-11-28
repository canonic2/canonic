//! Retained recording jobs and independent preview/source consumer ownership.

use super::*;
use crate::browser::recording::{Input, Job, Options, Recording};
use tokio::sync::oneshot;

impl Host {
    #[expect(
        clippy::too_many_lines,
        reason = "The retained recording admission keeps encoder ownership, first-frame readiness and terminal publication joined to the shared source"
    )]
    pub(super) async fn recording_start(
        self: &Arc<Self>,
        page: Reference,
        options: Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        #[cfg(windows)]
        if !crate::browser::recording::available() {
            return Err("RECORDING_UNAVAILABLE".into());
        }
        {
            let mut recordings = self.recordings.lock().expect("Browser recordings");
            if recordings
                .values()
                .any(|recording| recording.page == page && !recording.terminal())
            {
                return Err("RECORDING_ALREADY_ACTIVE".into());
            }
            if recordings.len() >= 32 {
                let finished = recordings
                    .iter()
                    .find(|(_, recording)| recording.terminal())
                    .map(|(reference, _)| *reference)
                    .ok_or("RECORDING_CAPACITY")?;
                recordings.remove(&finished);
            }
            if recordings
                .values()
                .filter(|recording| !recording.terminal())
                .count()
                >= 8
            {
                return Err("RECORDING_CAPACITY".into());
            }
        }
        let directory = crate::browser::recording::private_directory(self.journal.directory())
            .map_err(|cause| Error::caused("RECORDING_IO", cause))?;
        let source_options = self
            .previews
            .lock()
            .expect("Browser previews")
            .entries
            .values()
            .find(|source| source.page == page && source.state != "stopped")
            .map(|source| source.options.clone())
            .unwrap_or_default();
        let source = self
            .source_start(page, source_options, false, deadline)
            .await?;
        let source: Reference = serde_json::from_value(source["preview"].clone())
            .map_err(|_| Error::from("MEDIA_SOURCE_UNAVAILABLE"))?;
        let reference = Reference {
            connection_id: page.connection_id,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let (frames, first, alive, interruptions, initial_interruptions) = {
            let mut sources = self.previews.lock().expect("Browser previews");
            let source = sources.get_mut(source)?;
            if source.state != "live" {
                return Err("FIRST_FRAME_TIMEOUT".into());
            }
            let first = source
                .frames
                .borrow()
                .clone()
                .ok_or("FIRST_FRAME_TIMEOUT")?;
            source.recordings.insert(reference.id);
            (
                source.frames.subscribe(),
                first,
                source.alive.clone(),
                source.interruptions.clone(),
                source.interruptions.load(Ordering::SeqCst),
            )
        };
        let started = std::time::Instant::now();
        let (signal, stop) = oneshot::channel();
        let finalizing = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(AtomicBool::new(false));
        let input = Input {
            recording: reference,
            page,
            options: options.clone(),
            directory,
            frames,
            first,
            started,
            interruptions,
            initial_interruptions,
            alive,
            finalizing: finalizing.clone(),
            ready: ready.clone(),
        };
        let terminal = Arc::new(Mutex::new(None));
        let published = terminal.clone();
        let host = Arc::downgrade(self);
        let worker = tokio::spawn(async move {
            let mut result = match crate::browser::recording::run(input, stop).await {
                Ok(result) => result,
                Err(error) => {
                    json!({"recording":reference,"page":page,"status":"failed","artifact":error.details.as_ref().map(|details|&details["publishedArtifact"]),"error":error})
                }
            };
            if let Some(host) = host.upgrade() {
                let release = {
                    let mut sources = host.previews.lock().expect("Browser previews");
                    if let Ok(source) = sources.get_mut(source) {
                        source.recordings.remove(&reference.id);
                        !source.preview_active
                            && source.recordings.is_empty()
                            && source.state != "stopped"
                    } else {
                        false
                    }
                };
                if release
                    && let Err(error) = host
                        .release_source(source, Instant::now() + Duration::from_secs(5))
                        .await
                {
                    result["sourceCleanup"] = json!({"pending":true,"code":error.code});
                }
            }
            *published.lock().expect("Recording terminal result") = Some(result.clone());
            if let Some(host) = host.upgrade() {
                host.changed.notify_waiters();
            }
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
        let result = recording.snapshot();
        self.recordings
            .lock()
            .expect("Browser recordings")
            .insert(reference, recording);
        Ok(result)
    }

    pub(super) async fn recording_result(
        &self,
        reference: Reference,
        stop: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let recording = self
            .recordings
            .lock()
            .expect("Browser recordings")
            .get(&reference)
            .cloned()
            .ok_or("STALE_RECORDING")?;
        if stop {
            recording.stop();
        }
        let mut job = match tokio::time::timeout_at(deadline, recording.job.lock()).await {
            Ok(job) => job,
            Err(_) => return Ok(recording.snapshot()),
        };
        if job.result.is_none() {
            let worker = job.worker.as_mut().ok_or("RECORDING_OWNER_FAILED")?;
            if !stop && !worker.is_finished() {
                drop(job);
                return Ok(recording.snapshot());
            }
            let completed = match tokio::time::timeout_at(deadline, worker).await {
                Ok(result) => result,
                Err(_) => {
                    drop(job);
                    return Ok(recording.snapshot());
                }
            };
            job.worker.take();
            let result = match completed {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => {
                    json!({"recording":reference,"page":recording.page,"status":"failed","error":error,"artifact":null})
                }
                Err(_) => {
                    json!({"recording":reference,"page":recording.page,"status":"failed","error":{"code":"RECORDING_OWNER_FAILED"},"artifact":null})
                }
            };
            job.result = Some(result);
        }
        let result = job.result.clone().expect("Recording terminal result");
        drop(job);
        let release = {
            let mut sources = self.previews.lock().expect("Browser previews");
            if let Ok(source) = sources.get_mut(recording.source) {
                source.recordings.remove(&reference.id);
                !source.preview_active && source.recordings.is_empty() && source.state != "stopped"
            } else {
                false
            }
        };
        if release && let Err(error) = self.release_source(recording.source, deadline).await {
            let mut result = result;
            result["sourceCleanup"] = json!({"pending":true,"code":error.code});
            return Ok(result);
        }
        Ok(result)
    }

    pub(super) fn signal_recordings(&self) {
        for recording in recovered(&self.recordings).values() {
            recording.stop();
        }
    }

    pub(super) async fn close_recordings(&self, deadline: Instant) -> Result<(), Error> {
        self.signal_recordings();
        let references = recovered(&self.recordings)
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for reference in references {
            let result = self.recording_result(reference, true, deadline).await?;
            if !matches!(
                result["status"].as_str(),
                Some("complete" | "partial" | "failed")
            ) {
                return Err("RECORDING_CLEANUP_PENDING".into());
            }
        }
        Ok(())
    }
}
