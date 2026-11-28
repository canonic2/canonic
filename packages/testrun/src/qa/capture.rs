//! Capture profiles, image transformation and private screenshot evidence.
use super::arguments::{SessionRef, only_fields, parse, session_ref};
use super::{ReadContext, Server, id};
use crate::error::{Error, Result};
use crate::process;
use crate::targets::Target;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::capture::settling::Sample;

impl ReadContext {
    pub(super) async fn simulator_source(
        &self,
        target: &Target,
        _reference: &SessionRef,
    ) -> Result<std::sync::Arc<crate::simulator::Source>> {
        let identity = self.incarnation().await?;
        let existing = self
            .simulator_source
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let source = match existing {
            Some(source) => source,
            None => {
                self.simulator_sources
                    .acquire(
                        &target.platform_id,
                        &json!({"target":target.identity,"incarnation":identity}),
                        &target.epoch,
                    )
                    .await?
            }
        };
        if source.incarnation()? != identity {
            return Err(Error::new(
                "STALE_TARGET",
                "Capture resolved a different simulator incarnation",
            ));
        }
        self.simulator_source
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert_with(|| source.clone());
        Ok(source)
    }
    pub(super) async fn capture_target(
        &self,
        target: &Target,
        reference: &SessionRef,
    ) -> Result<Vec<u8>> {
        match target.kind.as_str() {
            "android" => Ok(self.android_raw(target).await?.0),
            "device" => Ok(self
                .runner
                .as_ref()
                .map_err(Clone::clone)?
                .call(
                    "screenshot",
                    json!({"format":"png"}),
                    None,
                    Duration::from_secs(5),
                )
                .await?
                .payload),
            _ => {
                self.simulator_source(target, reference)
                    .await?
                    .screenshot()
                    .await
            }
        }
    }

    async fn android_raw(&self, target: &Target) -> Result<(Vec<u8>, Value)> {
        match self
            .android_capture
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
        {
            Some(source) => source.raw_capture().await,
            None => {
                crate::android::Android::default()
                    .capture(&target.platform_id)
                    .await
            }
        }
    }

    pub(super) async fn screenshot(&self, args: Value) -> Result<Value> {
        self.capture(args, false, None).await
    }
    /// With `settle`, a physical device's capture first waits for its screen
    /// to stop changing; the result reports whether it did.
    async fn capture(&self, args: Value, settle: bool, baseline: Option<Sample>) -> Result<Value> {
        validate_capture(&args)?;
        let reference = session_ref(&args)?;
        let fast = match args.get("profile") {
            None => self.control_path == "hid",
            Some(Value::String(profile)) if profile == "evidence" => false,
            Some(Value::String(profile)) if profile == "fast" => true,
            _ => {
                return Err(Error::invalid(
                    "Screenshot profile must be evidence or fast",
                ));
            }
        };
        let target = self.live_target().await?;
        let android_owner = self
            .android_capture
            .as_ref()
            .and_then(std::sync::Weak::upgrade);
        let _capture_admission = match &android_owner {
            Some(source) => Some(source.admit().await),
            None => None,
        };
        let (android_image, android_settled) = if settle && target.kind == "android" {
            let (bytes, status, timings) = self.settle_screencap(&target).await?;
            (Some((bytes, timings)), Some(status))
        } else {
            (None, None)
        };
        #[cfg(target_os = "macos")]
        let video_start = std::time::Instant::now();
        #[cfg(target_os = "macos")]
        let settled_frame = match self.video.as_ref().and_then(std::sync::Weak::upgrade) {
            Some(video) if settle => Some(video.settle(baseline).await?),
            _ => None,
        };
        #[cfg(target_os = "macos")]
        let (settled, checked_image) = match settled_frame {
            Some(frame) => (Some(frame.status), Some(frame.image)),
            None => (android_settled, None),
        };
        #[cfg(not(target_os = "macos"))]
        let settled = {
            let _ = baseline;
            android_settled
        };
        #[cfg(target_os = "macos")]
        let video = if fast {
            self.video.as_ref().and_then(std::sync::Weak::upgrade)
        } else {
            None
        };
        #[cfg(target_os = "macos")]
        let (output, source_size, native_timings) = if let Some(video) = video {
            let image = match checked_image {
                Some(image) => image,
                None => video.snapshot().await?,
            };
            (
                process::Output { bytes: image.bytes },
                Some((image.width, image.height)),
                json!({"backend":"coredevice_video","framePtsNs":image.pts.to_string(),"observationMs":video_start.elapsed().as_secs_f64()*1000.0,"captureAgeKnown":false}),
            )
        } else if let Some((bytes, timings)) = android_image {
            (process::Output { bytes }, None, timings)
        } else {
            self.profile_capture(&target, &reference, fast).await?
        };
        #[cfg(not(target_os = "macos"))]
        let (output, source_size, native_timings) = match android_image {
            Some((bytes, timings)) => (process::Output { bytes }, None, timings),
            None => self.profile_capture(&target, &reference, fast).await?,
        };
        if target.kind == "simulator" {
            self.incarnation().await?;
        }
        let captured = Captured {
            target,
            evidence: self.evidence.clone(),
            output,
            source_size,
            native_timings,
            fast,
            args,
        };
        let mut result = self.process(move || publish(captured)).await?;
        if result["captureTimingsMs"]["backend"] == "adb_mediacodec_h264" {
            result["source"] = json!("adb_mediacodec_h264");
        }
        if let Some(settled) = settled {
            result["settle"] = json!({"changeDetected":settled.change_detected,"settled":settled.settled,"waitedMs":settled.waited.as_millis(),"samples":settled.samples});
        }
        self.screenshot_verified(fast);
        Ok(result)
    }

    fn screenshot_verified(&self, fast: bool) {
        *self
            .last_observation
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
        let mut profiles = vec![if fast { "fast" } else { "evidence" }];
        let existing = self
            .capabilities
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get("screenshots")
            .cloned()
            .or_else(|| self.target.capabilities.get("screenshots").cloned());
        if existing.is_some_and(|v| {
            v["profiles"]
                .as_array()
                .is_some_and(|p| p.contains(&json!(if fast { "evidence" } else { "fast" })))
        }) {
            profiles.push(if fast { "evidence" } else { "fast" });
        }
        self.verified("screenshots", json!({"profiles":profiles}));
    }

    /// Repeated raw screencaps establish Android stillness independently of video;
    /// the last screencap is the observation within the shared settling limits.
    async fn settle_screencap(
        &self,
        target: &Target,
    ) -> Result<(Vec<u8>, crate::capture::settling::Status, Value)> {
        use std::time::Instant;
        let source = self
            .android_capture
            .as_ref()
            .and_then(std::sync::Weak::upgrade);
        if let Some(source) = &source {
            source.safe_for_raw()?;
        }
        let mut wait = crate::capture::settling::Settling::new(None, Instant::now());
        let mut samples = 0;
        loop {
            let (bytes, mut timings) = self.android_raw(target).await?;
            samples += 1;
            let done = wait.sample(Sample::decode(&bytes)?, Instant::now());
            if done || wait.remaining(Instant::now()).is_zero() {
                // The timings describe the returned capture; settling took `samples` of them.
                timings["settleSamples"] = json!(samples);
                if let Some(source) = &source {
                    let backend = timings["backend"]
                        .as_str()
                        .unwrap_or("adb_screencap_raw")
                        .to_owned();
                    source.stamp(&mut timings, Some(&backend));
                }
                return Ok((bytes, wait.status(Instant::now()), timings));
            }
        }
    }

    async fn profile_capture(
        &self,
        target: &Target,
        reference: &SessionRef,
        fast: bool,
    ) -> Result<(process::Output, Option<(u32, u32)>, Value)> {
        if target.kind == "simulator" {
            let source = self.simulator_source(target, reference).await?;
            if fast {
                let (bytes, size) = source.compact().await?;
                return Ok((
                    process::Output { bytes },
                    Some(size),
                    json!({"backend":"native_iosurface","sourceEpoch":source.epoch()}),
                ));
            }
            let bytes = source.screenshot().await?;
            return Ok((
                process::Output { bytes },
                None,
                json!({"backend":"native_iosurface","sourceEpoch":source.epoch()}),
            ));
        }
        if target.kind == "device" {
            let reply = self
                .runner
                .as_ref()
                .map_err(Clone::clone)?
                .call(
                    "screenshot",
                    json!({"profile":if fast { "fast" } else { "evidence" }}),
                    None,
                    Duration::from_secs(5),
                )
                .await?;
            let dimension = |key: &str| -> Result<u32> {
                reply.data[key]
                    .as_u64()
                    .filter(|v| (1..=8192).contains(v))
                    .map(|v| v as u32)
                    .ok_or_else(|| {
                        Error::new(
                            "PLATFORM_PROTOCOL",
                            "Fast screenshot omitted source geometry",
                        )
                    })
            };
            let source_size = if fast {
                Some((dimension("sourceWidth")?, dimension("sourceHeight")?))
            } else {
                None
            };
            let mut timings = reply.data["timingsMs"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            timings.insert("rpc".into(), reply.data["rpcTimingsMs"].clone());
            return Ok((
                process::Output {
                    bytes: reply.payload,
                },
                source_size,
                Value::Object(timings),
            ));
        }
        if target.kind == "android" {
            return self.android_profile_capture(target, fast).await;
        }
        Ok((
            process::Output {
                bytes: self.capture_target(target, reference).await?,
            },
            None,
            Value::Null,
        ))
    }

    async fn android_profile_capture(
        &self,
        target: &Target,
        fast: bool,
    ) -> Result<(process::Output, Option<(u32, u32)>, Value)> {
        let source = self
            .android_capture
            .as_ref()
            .and_then(std::sync::Weak::upgrade);
        let mut selection_ms = None;
        if let Some(source) = &source {
            let (image, elapsed) = source.capture(fast).await?;
            selection_ms = Some(elapsed);
            if let Some(mut image) = image {
                let worker_ms = image.timings["totalMs"].as_f64().unwrap_or(0.0);
                image.timings["workerMs"] = json!(worker_ms);
                image.timings["admissionMs"] = json!((elapsed - worker_ms).max(0.0));
                image.timings["totalMs"] = json!(elapsed);
                source.stamp(&mut image.timings, Some("adb_mediacodec_h264"));
                return Ok((
                    process::Output { bytes: image.bytes },
                    Some(image.size),
                    image.timings,
                ));
            }
        }
        let (bytes, mut timings) = self.android_raw(target).await?;
        if let Some(source) = source {
            let elapsed = selection_ms.unwrap_or(0.0);
            timings["captureSelectionMs"] = json!(elapsed);
            timings["totalMs"] = json!(timings["totalMs"].as_f64().unwrap_or(0.0) + elapsed);
            let backend = timings["backend"]
                .as_str()
                .unwrap_or("adb_screencap_raw")
                .to_owned();
            source.stamp(&mut timings, Some(&backend));
        }
        Ok((process::Output { bytes }, None, timings))
    }
}

impl Server {
    pub(super) async fn simulator_source(
        &self,
        target: &Target,
        reference: &SessionRef,
    ) -> Result<std::sync::Arc<crate::simulator::Source>> {
        self.read_context(&json!({"runId":reference.run_id,"goalId":reference.goal_id,"sessionId":reference.session_id}))?.simulator_source(target, reference).await
    }
    pub(super) async fn capture_target(
        &self,
        target: &Target,
        reference: &SessionRef,
    ) -> Result<Vec<u8>> {
        self.read_context(&json!({"runId":reference.run_id,"goalId":reference.goal_id,"sessionId":reference.session_id}))?.capture_target(target, reference).await
    }
    pub(super) async fn screenshot(&mut self, args: Value) -> Result<Value> {
        validate_capture(&args)?;
        self.live_target(&session_ref(&args)?).await?;
        let mut context = self.read_context(&args)?;
        context.validated = true;
        // The ordered owner has one reserved observation slot. Public read
        // saturation must not prevent an admitted action's evidence step.
        context.screenshot(args).await
    }
    /// The observation after input; `settle` waits for the screen to stop changing.
    pub(super) async fn observe_after_input(
        &mut self,
        args: Value,
        settle: bool,
        baseline: Option<Sample>,
    ) -> Result<Value> {
        validate_capture(&args)?;
        self.live_target(&session_ref(&args)?).await?;
        let mut context = self.read_context(&args)?;
        context.validated = true;
        context.capture(args, settle, baseline).await
    }

    /// Baselines belong to a batch observation, never to the reusable session.
    pub(super) async fn observation_baseline(
        &self,
        reference: &SessionRef,
    ) -> Result<Option<Sample>> {
        #[cfg(target_os = "macos")]
        if let Some(video) = self.session(reference)?.video.clone() {
            let image = video.snapshot().await?;
            return Sample::decode(&image.bytes).map(Some);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = reference;
        Ok(None)
    }
}

fn validate_capture(args: &Value) -> Result<()> {
    only_fields(
        args,
        &[
            "runId",
            "goalId",
            "sessionId",
            "display",
            "name",
            "format",
            "scale",
            "region",
            "privacy",
            "includeImage",
            "profile",
        ],
    )?;
    if args
        .get("privacy")
        .and_then(Value::as_str)
        .is_some_and(|p| p != "local")
    {
        return Err(Error::new(
            "REDACTION_UNAVAILABLE",
            "A validated shareable-image masking adapter is required",
        ));
    }

    Ok(())
}

struct Captured {
    target: Target,
    evidence: std::path::PathBuf,
    output: process::Output,
    source_size: Option<(u32, u32)>,
    native_timings: Value,
    fast: bool,
    args: Value,
}
fn publish(captured: Captured) -> Result<Value> {
    let Captured {
        target,
        evidence,
        mut output,
        source_size,
        native_timings,
        fast,
        args,
    } = captured;
    let (captured_width, captured_height) = crate::capture::dimensions(&output.bytes)?;
    let (source_width, source_height) = source_size.unwrap_or((captured_width, captured_height));
    if let Some(display) = args.get("display") {
        let display: crate::ui::Display = parse(display.clone())?;
        display.validate(source_width, source_height)?;
    }
    let format = args
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or(if fast { "jpeg" } else { "png" });
    let requested_scale = args.get("scale").and_then(Value::as_f64).unwrap_or(1.0);
    let scale = requested_scale
        * if fast && source_size.is_none() {
            (960.0 / f64::from(captured_width.max(captured_height))).min(1.0)
        } else {
            1.0
        };
    let region: Option<crate::capture::Region> =
        args.get("region").map(|v| parse(v.clone())).transpose()?;
    let native_format = if source_size.is_some() { "jpeg" } else { "png" };
    let (width, height, mime) = if format != native_format || scale != 1.0 || region.is_some() {
        let image = crate::capture::transform(&output.bytes, format, scale, region.as_ref())?;
        output.bytes = image.bytes;
        (image.width, image.height, image.mime)
    } else {
        (
            captured_width,
            captured_height,
            if native_format == "jpeg" {
                "image/jpeg"
            } else {
                "image/png"
            },
        )
    };
    let artifact_id = id();
    let path = evidence.join(format!("{artifact_id}.{format}"));
    crate::artifacts::write(&path, &output.bytes)?;
    let attachment = if args.get("includeImage").and_then(Value::as_bool) != Some(false) {
        if fast && output.bytes.len() <= 2 * 1024 * 1024 {
            Some(json!({"path":path,"mimeType":mime,"width":width,"height":height,"scale":1.0}))
        } else {
            let image = crate::capture::thumbnail(&output.bytes)?;
            let path = evidence.join(format!("{artifact_id}.thumbnail.png"));
            crate::artifacts::write(&path, &image.bytes)?;
            Some(
                json!({"path":path,"mimeType":image.mime,"width":image.width,"height":image.height,"scale":f64::from(image.width)/f64::from(width)}),
            )
        }
    } else {
        None
    };
    Ok(
        json!({"artifactId":artifact_id,"name":args.get("name"),"path":path,"mimeType":mime,"width":width,"height":height,"sourceWidth":source_width,"sourceHeight":source_height,"scale":requested_scale*if fast{(960.0/f64::from(source_width.max(source_height))).min(1.0)}else{1.0},"profile":if fast{"fast"}else{"evidence"},"captureTimingsMs":native_timings,"runnerTimingsMs":if target.kind=="device"&&native_timings["backend"]!="coredevice_video"{native_timings.clone()}else{Value::Null},"display":{"displayId":"main","geometryVersion":format!("{source_width}x{source_height}")},"attachment":attachment,"bytes":output.bytes.len(),"sha256":format!("{:x}",Sha256::digest(&output.bytes)),"source":if native_timings["backend"]=="coredevice_video"{"coredevice_video"}else{match target.kind.as_str(){"android"=>"adb_screencap","device"=>"xcui_device_screenshot",_=>"native_iosurface"}},"privacy":"local","target":target.identity}),
    )
}
