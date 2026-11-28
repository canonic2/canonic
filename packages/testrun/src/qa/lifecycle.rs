//! Cancellation and shutdown of all resources owned by one QA server.
use super::Server;
use crate::error::{Error, Result};
use serde_json::json;
use std::collections::HashSet;

/// Progress belongs to the session, independently of any close caller.
#[derive(Default, serde::Serialize)]
pub(super) struct Cleanup {
    attempts: u64,
    completed: std::collections::BTreeSet<String>,
    errors: Vec<serde_json::Value>,
    diagnostics: std::collections::BTreeMap<String, serde_json::Value>,
}

/// From this cleanup attempt on, a simulator or emulator Testrun started is
/// shut down even while earlier stages still fail. Earlier attempts keep it
/// running so stages that still need the device, such as a recording pull,
/// can finish on retry; their failures stay reported either way.
const FORCE_OWNED_SHUTDOWN_AFTER: u64 = 3;

enum ShutdownMode {
    Exit,
    Resumable,
}

impl Server {
    /// Release an empty lane's native owners without ending its shared managed
    /// task or publishing cloned run metadata. Failure keeps the lane owned.
    pub(crate) async fn retire_runtime(&mut self) -> Result<bool> {
        if !self.sessions.is_empty() {
            return Ok(false);
        }
        self.simulator_boots.release_unused(&HashSet::new()).await?;
        self.emulator_starts.release_unused(&HashSet::new()).await?;
        let mut failures = Vec::new();
        if let Some(media) = &mut self.media {
            match media.close() {
                Ok(()) => {
                    self.media.take();
                }
                Err(error) => failures.push(error),
            }
        }
        if let Err(error) = self.runners.shutdown().await {
            failures.push(error);
        }
        if let Err(error) = self.simulator_sources.shutdown().await {
            failures.push(error);
        }
        #[cfg(target_os = "macos")]
        if let Err(error) = self.device_media.shutdown().await {
            failures.push(error);
        }
        if failures.is_empty() {
            Ok(true)
        } else {
            Err(Error::new(
                "CLEANUP_FAILED",
                "Idle target runtime retains unfinished cleanup",
            )
            .details(json!({"failures":failures})))
        }
    }
    pub(crate) async fn cancel_startup(&mut self) -> Result<()> {
        let mut failures = Vec::new();
        let active = self
            .sessions
            .values()
            .map(|session| session.target.identity.target_id.clone())
            .collect();
        if let Err(error) = self.simulator_boots.release_unused(&active).await {
            failures.push(error);
        }
        if let Err(error) = self.emulator_starts.release_unused(&active).await {
            failures.push(error);
        }
        if let Err(error) = self.simulator_sources.release_unused().await {
            failures.push(error);
        }
        #[cfg(target_os = "macos")]
        if let Err(error) = self.device_media.release_unused().await {
            failures.push(error);
        }
        if self.sessions.is_empty()
            && let Err(error) = self.runners.shutdown().await
        {
            failures.push(error);
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::new(
                "CLEANUP_FAILED",
                "Cancelled startup retains unfinished cleanup",
            )
            .details(json!({"failures":failures})))
        }
    }
    pub(crate) fn cleanup_status(&self) -> serde_json::Value {
        json!(self.cleanup_reports.values().collect::<Vec<_>>())
    }
    fn cleanup_done(&self, id: &str, stage: &str) -> bool {
        self.sessions[id].cleanup.completed.contains(stage)
    }
    fn cleanup_stage(
        &mut self,
        id: &str,
        stage: &str,
        result: Result<()>,
        failures: &mut Vec<Error>,
    ) {
        let cleanup = &mut self.sessions.get_mut(id).unwrap().cleanup;
        match result {
            Ok(()) => {
                cleanup.completed.insert(stage.into());
            }
            Err(error) => {
                cleanup
                    .errors
                    .push(json!({"attempt":cleanup.attempts,"stage":stage,"error":error}));
                failures.push(error);
            }
        }
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Each owned cleanup stage records completion independently before the next stage"
    )]
    pub(super) async fn close_session(&mut self, id: &str) -> Result<()> {
        let Some(session) = self.sessions.get_mut(id) else {
            return Ok(());
        };
        session.closing = true;
        session.authority().close();
        #[cfg(unix)]
        {
            let viewer = session
                .read_gate
                .viewer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            if let Some(viewer) = viewer {
                viewer.close().await;
            }
        }
        let reads = session.read_gate.clone();
        reads.close().await;
        session.cleanup.attempts += 1;
        let mut failures = Vec::new();
        if !self.cleanup_done(id, "playback") {
            let result = self.playback.close(id).await;
            self.cleanup_stage(id, "playback", result, &mut failures);
        }
        if !self.cleanup_done(id, "simulator_input") {
            let result = match self.sessions[id].simulator_input.get() {
                Some(input) => input.close().await,
                None => Ok(()),
            };
            self.cleanup_stage(id, "simulator_input", result, &mut failures);
        }
        #[cfg(target_os = "macos")]
        if !self.cleanup_done(id, "hid") {
            let result = match self.sessions[id].hid.as_ref() {
                Some(hid) => hid.close().await,
                None => Ok(()),
            };
            self.cleanup_stage(id, "hid", result, &mut failures);
        }
        let device = self
            .sessions
            .get(id)
            .filter(|s| s.target.kind == "device")
            .map(|s| s.target.platform_id.clone());
        {
            let recordings: Vec<_> = self.sessions[id].recordings.iter().cloned().collect();
            for recording in recordings {
                let stage = format!("recording:{recording}");
                if self.cleanup_done(id, &stage) {
                    continue;
                }
                let result = match self.recordings.stop(&recording).await {
                    Ok(result) if result["status"] == "finalizing" => Err(Error::new(
                        "CLEANUP_PENDING",
                        "Recording is still finalizing; retry disconnect or recording status",
                    )
                    .details(result)),
                    Ok(result) if result["status"] == "failed" => {
                        // A terminal artifact failure has no live worker to retain.
                        // Report it once, preserve the partial and allow explicit
                        // cleanup retry to release the remaining device resources.
                        let cleanup = &mut self.sessions.get_mut(id).unwrap().cleanup;
                        cleanup.diagnostics.insert(stage.clone(), result.clone());
                        cleanup.completed.insert(stage.clone());
                        Err(Error::new(
                            "RECORDING_FAILED",
                            "Recording finalization failed; partial evidence is preserved",
                        )
                        .details(result))
                    }
                    Ok(_) => Ok(()),
                    Err(error) => Err(error),
                };
                self.cleanup_stage(id, &stage, result, &mut failures);
            }
        }
        if !self.cleanup_done(id, "android_capture") {
            let source = self.sessions[id].android_capture.clone();
            let result = match source {
                Some(source) => source.close().await,
                None => Ok(()),
            };
            if result.is_ok() {
                self.sessions.get_mut(id).unwrap().android_capture.take();
            }
            self.cleanup_stage(id, "android_capture", result, &mut failures);
        }
        let streams: Vec<_> = self.sessions[id].streams.iter().cloned().collect();
        for stream in streams {
            let stage = format!("stream:{stream}");
            if !self.cleanup_done(id, &stage) {
                let result = self
                    .media
                    .as_ref()
                    .map_or(Ok(()), |media| media.stop(&stream).map(|_| ()));
                self.cleanup_stage(id, &stage, result, &mut failures);
            }
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(session) = self.sessions.get_mut(id) {
                session.video.take();
            }
            if !self.cleanup_done(id, "device_media") {
                let result = match device.as_deref() {
                    Some(serial) => self.device_media.release_unused_for(serial).await,
                    None => Ok(()),
                };
                if let Some(error) = device
                    .as_deref()
                    .and_then(|serial| self.device_media.take_stream_error(serial))
                {
                    self.sessions
                        .get_mut(id)
                        .unwrap()
                        .cleanup
                        .diagnostics
                        .insert("device_media".into(), json!({"streamError":error}));
                }
                // Pending audio cleanup can still hold the borrowed video
                // source. Revisit the registry after that owner releases it.
                if result.is_err() || self.cleanup_done(id, "playback") {
                    self.cleanup_stage(id, "device_media", result, &mut failures);
                }
            }
        }
        if let Some(device) = device
            && !self.sessions.iter().any(|(other, s)| {
                other != id
                    && !s.closing
                    && s.target.kind == "device"
                    && s.target.platform_id == device
            })
            && !self.cleanup_done(id, "runner")
        {
            let result = self.runners.stop(&device).await;
            self.cleanup_stage(id, "runner", result, &mut failures);
        }
        if let Some(session) = self.sessions.get_mut(id) {
            session
                .simulator_source
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
        }
        if !self.cleanup_done(id, "simulator_source") {
            let result = self.simulator_sources.release_unused().await;
            self.cleanup_stage(id, "simulator_source", result, &mut failures);
        }
        let shut_down_owned =
            failures.is_empty() || self.sessions[id].cleanup.attempts >= FORCE_OWNED_SHUTDOWN_AFTER;
        if shut_down_owned && !self.cleanup_done(id, "simulator_boot") {
            let target = self.sessions[id].target.identity.target_id.clone();
            let another = self
                .sessions
                .iter()
                .any(|(other, session)| other != id && session.target.identity.target_id == target);
            if !another {
                let result = self.simulator_boots.release(&target).await;
                self.cleanup_stage(id, "simulator_boot", result, &mut failures);
            }
        }
        if (failures.is_empty() || shut_down_owned) && !self.cleanup_done(id, "emulator_start") {
            let target = self.sessions[id].target.identity.target_id.clone();
            let another = self
                .sessions
                .iter()
                .any(|(other, session)| other != id && session.target.identity.target_id == target);
            if !another {
                let result = self.emulator_starts.release(&target).await;
                self.cleanup_stage(id, "emulator_start", result, &mut failures);
            }
        }
        self.cleanup_reports.insert(
            id.into(),
            json!({"complete":failures.is_empty(),"progress":self.sessions[id].cleanup}),
        );
        if !failures.is_empty() {
            let error = Error::new("CLEANUP_FAILED", "Session cleanup could not be established")
                .details(json!({"failures":failures}));
            self.sessions.get_mut(id).unwrap().cleanup_error = Some(error.clone());
            return Err(error);
        }
        if let Some(session) = self.sessions.remove(id) {
            for recording in session.recordings {
                self.recordings.forget(&recording);
            }
        }
        Ok(())
    }
    pub async fn cancel_dispatched(&mut self) -> Result<()> {
        let mut release = HashSet::new();
        let mut changed = Vec::new();
        for (id, session) in &mut self.sessions {
            for (command, (signature, status)) in &mut session.journal {
                if status["state"] == "dispatched" {
                    status["state"] = json!("outcome_unknown");
                    status["reason"] = json!("request_cancelled");
                    changed.push((
                        id.clone(),
                        command.clone(),
                        signature.clone(),
                        status.clone(),
                    ));
                    if matches!(
                        status["tool"].as_str(),
                        Some("tap" | "long_press" | "swipe" | "key" | "type_text")
                    ) && session.target.kind == "simulator"
                    {
                        release.insert(session.lease_id.clone());
                    }
                }
            }
        }
        let mut failures = Vec::new();
        for (session, command, signature, status) in changed {
            if let Err(error) = self.record_command(&session, command, signature, status) {
                failures.push(error);
            }
        }
        for session in self
            .sessions
            .values()
            .filter(|s| release.contains(&s.lease_id))
        {
            if let Some(input) = session.simulator_input.get()
                && let Err(error) = input.close().await
            {
                failures.push(error);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::new(
                "CLEANUP_FAILED",
                "Cancelled commands retain persistence or release failures",
            )
            .details(json!({"failures":failures})))
        }
    }
    /// Best-effort cleanup before this server exits; every global owner is attempted.
    pub async fn shutdown(&mut self) -> Result<()> {
        self.shutdown_with(ShutdownMode::Exit).await
    }
    /// Retain failed sessions and their shared registries for another close attempt.
    pub(crate) async fn shutdown_resumable(&mut self) -> Result<()> {
        self.shutdown_with(ShutdownMode::Resumable).await
    }
    async fn shutdown_with(&mut self, mode: ShutdownMode) -> Result<()> {
        let mut failures = Vec::new();
        let failed_runs = if matches!(mode, ShutdownMode::Exit) {
            self.sessions
                .values()
                .filter_map(|session| {
                    session.cleanup_error.clone().map(|error| {
                        failures.push(error);
                        session.run_id.clone()
                    })
                })
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        if let Some(scheduling) = self.scheduling.clone()
            && let Err(error) = scheduling.shutdown().await
        {
            failures.push(error);
        }
        if let Err(error) = self.managed.shutdown().await {
            failures.push(error);
        }
        if let Err(error) = self.cancel_dispatched().await {
            failures.push(error);
        }
        let sessions: Vec<_> = self.sessions.keys().cloned().collect();
        for session in sessions {
            if let Err(error) = self.close_session(&session).await {
                failures.push(error);
            }
        }
        let active = self
            .sessions
            .values()
            .map(|session| session.target.identity.target_id.clone())
            .collect();
        if let Err(error) = self.simulator_boots.release_unused(&active).await {
            failures.push(error);
        }
        if let Err(error) = self.emulator_starts.release_unused(&active).await {
            failures.push(error);
        }
        // Completed sessions in other runs can publish their evidence even
        // while a failed session retains its own run and resource owners.
        let runs: Vec<_> = self
            .runs
            .keys()
            .filter(|run| {
                !failed_runs.contains(*run)
                    && !self
                        .scheduling
                        .as_ref()
                        .is_some_and(|scheduling| scheduling.pending_run(run))
                    && !self
                        .sessions
                        .values()
                        .any(|session| &session.run_id == *run)
            })
            .cloned()
            .collect();
        for run in runs {
            if let Err(error) = self.call("run.end", json!({"runId":run})).await {
                failures.push(error);
            }
        }
        // A retained controller must preserve failed sessions' shared owners.
        // An exiting server has no retry path, so attempt all global cleanup.
        if matches!(mode, ShutdownMode::Resumable) && !self.sessions.is_empty() {
            return Err(
                Error::new("CLEANUP_FAILED", "Owned session cleanup remains unfinished")
                    .details(json!({"failures":failures})),
            );
        }
        if let Some(media) = &mut self.media {
            match media.close() {
                Ok(()) => {
                    self.media.take();
                }
                Err(error) => failures.push(error),
            }
        }
        if let Err(error) = self.runners.shutdown().await {
            failures.push(error);
        }
        if let Err(error) = self.simulator_sources.shutdown().await {
            failures.push(error);
        }
        #[cfg(target_os = "macos")]
        if let Err(error) = self.device_media.shutdown().await {
            failures.push(error);
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(
                Error::new("CLEANUP_FAILED", "Some owned resources could not finalize")
                    .details(json!({"failures":failures})),
            )
        }
    }
}
