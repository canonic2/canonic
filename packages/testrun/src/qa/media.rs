//! Preview and recording workflows bound to the owning run and session.
use super::Server;
use super::arguments::{field, only_fields, session_ref};
use crate::error::{Error, Result};
use serde_json::{Value, json};

impl Server {
    #[cfg(target_os = "macos")]
    async fn release_device_media(
        &mut self,
        reference: &super::arguments::SessionRef,
    ) -> Result<()> {
        let session = self.session(reference)?;
        if session.target.kind == "device" {
            let serial = session.target.platform_id.clone();
            self.device_media.release_unused_for(&serial).await?;
        }
        Ok(())
    }
    pub(super) async fn recording_start(&mut self, args: Value) -> Result<Value> {
        only_fields(&args, &["runId", "goalId", "sessionId", "maxDurationMs"])?;
        let reference = session_ref(&args)?;
        let target = self.live_target(&reference).await?;
        if !matches!(target.kind.as_str(), "simulator" | "device" | "android") {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "MP4 recording requires a platform capture adapter",
            ));
        }
        if self
            .sessions
            .values()
            .flat_map(|s| &s.recordings)
            .filter(|id| self.recordings.active(id))
            .count()
            >= 2
        {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Connection already owns two active recordings",
            ));
        }
        if self.sessions.values().any(|s| {
            s.target.identity == target.identity
                && s.recordings.iter().any(|id| self.recordings.active(id))
        }) {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Target already has a recording in this run",
            ));
        }
        let run = self
            .runs
            .get(&reference.run_id)
            .ok_or_else(|| Error::new("RUN_NOT_FOUND", "Run ended"))?;
        let duration = args
            .get("maxDurationMs")
            .and_then(Value::as_u64)
            .unwrap_or(600000);
        if !(100..=600000).contains(&duration) {
            return Err(Error::invalid("maxDurationMs must be 100..600000"));
        }
        let result = if target.kind == "android" {
            self.release_android_capture(&target).await?;
            self.recordings
                .start_android(
                    &target.platform_id,
                    &run.evidence,
                    args.get("maxDurationMs").and_then(Value::as_u64),
                )
                .await?
        } else if target.kind == "device" {
            #[cfg(target_os = "macos")]
            {
                let source = self
                    .device_media
                    .acquire(&target.platform_id, &json!(target.identity))
                    .await?;
                self.recordings
                    .start_device(source, &run.evidence, duration)
                    .await?
            }
            #[cfg(not(target_os = "macos"))]
            {
                return Err(Error::new(
                    "UNSUPPORTED_HOST",
                    "Physical media requires macOS",
                ));
            }
        } else {
            let source = self.simulator_source(&target, &reference).await?;
            self.recordings
                .start(
                    source,
                    &run.evidence,
                    args.get("maxDurationMs")
                        .and_then(Value::as_u64)
                        .unwrap_or(600000),
                )
                .await?
        };
        self.sessions
            .get_mut(&reference.session_id)
            .ok_or_else(|| Error::new("SESSION_NOT_FOUND", "Session ended"))?
            .recordings
            .insert(field(&result, "recordingId")?.into());
        crate::targets::capabilities::support(
            &mut self.sessions.get_mut(&reference.session_id).unwrap().target,
            "recording",
            if target.kind == "android" {
                json!({"format":"h264","backend":"adb_screenrecord","targetMaxDurationMs":result["targetMaxDurationMs"]})
            } else {
                json!({"format":if target.kind=="device"{"hevc"}else{"h264"}})
            },
        );
        Ok(result)
    }

    async fn release_android_capture(&self, target: &crate::targets::Target) -> Result<()> {
        let sources: Vec<_> = self
            .sessions
            .values()
            .filter(|session| session.target.identity == target.identity)
            .filter_map(|session| session.android_capture.clone())
            .collect();
        for source in sources {
            let _admission = source.admit().await;
            source.disable("recording").await?;
        }
        Ok(())
    }

    pub(super) async fn stream_start(&mut self, args: Value) -> Result<Value> {
        only_fields(&args, &["runId", "goalId", "sessionId", "format"])?;
        let reference = session_ref(&args)?;
        let target = self.live_target(&reference).await?;
        let format =
            args.get("format")
                .and_then(Value::as_str)
                .unwrap_or(if target.kind == "device" {
                    "hevc"
                } else {
                    "h264"
                });
        if !matches!(format, "h264" | "jpeg" | "hevc") {
            return Err(Error::invalid("format must be h264, jpeg or hevc"));
        }
        if target.kind == "device" && format != "hevc" {
            return Err(Error::new(
                "CAPTURE_REQUIREMENT_UNMET",
                "Physical media uses native HEVC; a conversion profile is not available",
            ));
        }
        if target.kind == "simulator" && format == "hevc" {
            return Err(Error::new(
                "CAPTURE_REQUIREMENT_UNMET",
                "Simulator media supports H.264 and JPEG",
            ));
        }
        if !matches!(target.kind.as_str(), "simulator" | "device") {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Live preview requires an iOS capture adapter",
            ));
        }
        if self.session(&reference)?.streams.len() >= 2 {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Session already has two previews",
            ));
        }
        if self
            .sessions
            .values()
            .map(|s| s.streams.len())
            .sum::<usize>()
            >= 4
            || self
                .sessions
                .values()
                .filter(|s| s.target.identity == target.identity)
                .map(|s| s.streams.len())
                .sum::<usize>()
                >= 2
        {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Preview capacity is four per connection and two per target",
            ));
        }
        if self.media.is_none() {
            self.media = Some(crate::media::Media::start().await?);
        }
        let result = if target.kind == "device" {
            #[cfg(target_os = "macos")]
            {
                let source = self
                    .device_media
                    .acquire(&target.platform_id, &json!(target.identity))
                    .await?;
                self.media
                    .as_ref()
                    .ok_or_else(|| Error::new("INTERNAL", "Media startup failed"))?
                    .device_preview(source, &json!(target.identity))
                    .await?
            }
            #[cfg(not(target_os = "macos"))]
            {
                return Err(Error::new(
                    "UNSUPPORTED_HOST",
                    "Physical media requires macOS",
                ));
            }
        } else {
            let source = self.simulator_source(&target, &reference).await?;
            self.media
                .as_ref()
                .ok_or_else(|| Error::new("INTERNAL", "Media startup failed"))?
                .preview(source, &json!(target.identity), format)
                .await?
        };
        self.sessions
            .get_mut(&reference.session_id)
            .ok_or_else(|| Error::new("SESSION_NOT_FOUND", "Session ended"))?
            .streams
            .insert(field(&result, "streamId")?.into());
        self.capability_verified(&reference, "preview", json!({"formats":[format]}));
        Ok(preview_view(result))
    }

    pub(super) async fn stream_stop(&mut self, args: Value) -> Result<Value> {
        only_fields(&args, &["runId", "goalId", "sessionId", "previewId"])?;
        let reference = session_ref(&args)?;
        let stream = field(&args, "previewId")?;
        if !self.session(&reference)?.streams.contains(stream) {
            return Err(Error::new(
                "NOT_OWNED",
                "Preview belongs to another session",
            ));
        }
        let result = self
            .media
            .as_ref()
            .ok_or_else(|| Error::new("NOT_FOUND", "Media transport is not active"))?
            .stop(stream)?;
        self.sessions
            .get_mut(&reference.session_id)
            .ok_or_else(|| Error::new("SESSION_NOT_FOUND", "Session ended"))?
            .streams
            .remove(stream);
        #[cfg(target_os = "macos")]
        self.release_device_media(&reference).await?;
        self.simulator_sources.release_unused().await?;
        Ok(preview_view(result))
    }
    pub(super) fn stream_status(&self, args: Value) -> Result<Value> {
        only_fields(&args, &["runId", "goalId", "sessionId", "previewId"])?;
        let reference = session_ref(&args)?;
        let stream = field(&args, "previewId")?;
        if !self.session(&reference)?.streams.contains(stream) {
            return Err(Error::new(
                "NOT_OWNED",
                "Preview belongs to another session",
            ));
        }
        self.media
            .as_ref()
            .ok_or_else(|| Error::new("NOT_FOUND", "Media transport is not active"))?
            .status(stream)
            .map(preview_view)
    }
    /// Stop every active recording and keep its evidence, as when a session's
    /// last client reference goes. Each result names its recording; one
    /// failed stop does not prevent the others.
    #[cfg(unix)]
    pub(crate) async fn finalize_recordings(&mut self) -> Result<Value> {
        let active: Vec<String> = self
            .sessions
            .values()
            .flat_map(|session| session.recordings.iter().cloned())
            .filter(|id| self.recordings.active(id))
            .collect();
        let mut finalized = Vec::new();
        for id in active {
            let result = self.recordings.stop(&id).await;
            finalized.push(match result {
                Ok(result) => json!({"recordingId":id,"result":result}),
                Err(error) => json!({"recordingId":id,"error":error}),
            });
        }
        #[cfg(target_os = "macos")]
        self.device_media.release_unused().await?;
        Ok(json!({"recordings":finalized}))
    }
    pub(super) async fn recording_result(&mut self, name: &str, args: Value) -> Result<Value> {
        only_fields(&args, &["runId", "goalId", "sessionId", "recordingId"])?;
        let reference = session_ref(&args)?;
        let recording = field(&args, "recordingId")?;
        if !self.session(&reference)?.recordings.contains(recording) {
            return Err(Error::new(
                "NOT_OWNED",
                "Recording belongs to another session",
            ));
        }
        let result = if name == "recording_stop" {
            self.recordings.stop(recording).await
        } else {
            self.recordings.status(recording).await
        };
        #[cfg(target_os = "macos")]
        self.release_device_media(&reference).await?;
        result
    }
}

/// The media server names a preview `streamId`; tools call it `previewId`.
fn preview_view(mut result: Value) -> Value {
    if let Some(fields) = result.as_object_mut()
        && let Some(id) = fields.remove("streamId")
    {
        fields.insert("previewId".into(), id);
    }
    result
}
