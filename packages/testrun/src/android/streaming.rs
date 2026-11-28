//! Controller-owned Android video policy, freshness admission and raw fallback.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[cfg(target_os = "macos")]
mod decoder;
#[cfg(target_os = "macos")]
mod host;
#[cfg(any(target_os = "macos", test))]
mod protection;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod protocol;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mode {
    #[default]
    Screenshot,
    Stream,
}
impl Mode {
    pub(crate) fn parse(value: Option<&Value>, android: bool) -> Result<Option<Self>> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(None);
        };
        if !android {
            return Err(Error::invalid("androidCapture applies to Android only"));
        }
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|_| Error::invalid("androidCapture must be screenshot or stream"))
    }
}

pub(crate) struct Image {
    pub(crate) bytes: Vec<u8>,
    pub(crate) size: (u32, u32),
    pub(crate) timings: Value,
}

/// Owned by a QA session, borrowed by reads. Teardown revokes reads first.
pub(crate) struct Source {
    mode: Mode,
    serial: String,
    admission: tokio::sync::Mutex<()>,
    #[cfg(target_os = "macos")]
    host: host::Host,
}
impl Source {
    pub(crate) fn new(serial: String, mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            mode,
            admission: tokio::sync::Mutex::new(()),
            #[cfg(target_os = "macos")]
            host: host::Host::new(serial.clone(), mode),
            serial,
        })
    }
    pub(crate) fn status(&self) -> Value {
        #[cfg(target_os = "macos")]
        {
            self.host.status()
        }
        #[cfg(not(target_os = "macos"))]
        {
            json!({"requested":self.mode,"state":"raw_fallback","reason":"unsupported_host"})
        }
    }
    pub(crate) async fn admit(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.admission.lock().await
    }
    pub(crate) async fn capture(&self, fast: bool) -> Result<(Option<Image>, f64)> {
        let started = std::time::Instant::now();
        let image = self.capture_inner(fast).await?;
        Ok((image, started.elapsed().as_secs_f64() * 1000.0))
    }
    async fn capture_inner(&self, fast: bool) -> Result<Option<Image>> {
        self.safe_for_raw()?;
        if self.mode != Mode::Stream || !fast {
            return Ok(None);
        }
        // Fallback is latched until reconnect; do not repeat stream admission.
        if matches!(
            self.status()["state"].as_str(),
            Some("raw_fallback" | "blocked")
        ) {
            return Ok(None);
        }
        #[cfg(target_os = "macos")]
        {
            if self.serial.contains(':') || self.serial.starts_with("emulator-") {
                self.host.invalidate("unsupported_transport").await?;
                return Ok(None);
            }
            let adapter = super::Android::default();
            let transport = crate::process::run(
                &adapter.adb,
                &["-s", &self.serial, "get-devpath"],
                std::time::Duration::from_secs(1),
                1024,
            )
            .await;
            if !transport.is_ok_and(|output| output.bytes.starts_with(b"usb:")) {
                self.host.invalidate("unsupported_transport").await?;
                return Ok(None);
            }
            let api = adapter
                .shell(
                    &self.serial,
                    &["getprop", "ro.build.version.sdk"],
                    std::time::Duration::from_secs(1),
                    128,
                )
                .await;
            if !api.is_ok_and(|bytes| bytes.trim_ascii() == b"31") {
                self.host.invalidate("unsupported_api").await?;
                return Ok(None);
            }
            let Some(before) = self.inspect_window(&adapter).await? else {
                return Ok(None);
            };
            if !before.admissible() {
                self.host.invalidate("protection_unknown").await?;
                return Ok(None);
            }
            let size = match adapter.dimensions(&self.serial).await {
                Ok(size) => size,
                Err(_) => {
                    self.host.invalidate("geometry_unavailable").await?;
                    return Ok(None);
                }
            };
            let mut image = self.host.snapshot(size).await?;
            if image.is_none() {
                return Ok(None);
            }
            let validation_started = std::time::Instant::now();
            let Some(after) = self.inspect_window(&adapter).await? else {
                return Ok(None);
            };
            if !after.admissible() || before != after {
                self.host.invalidate("window_changed").await?;
                return Ok(None);
            }
            if let Some(image) = &mut image {
                if self.status()["state"] != "healthy"
                    || self.status()["generation"] != image.timings["sourceGeneration"]
                {
                    return Ok(None);
                }
                if let Some(restart_ms) = image.timings["restartMs"].as_f64() {
                    let total = restart_ms + validation_started.elapsed().as_secs_f64() * 1000.0;
                    self.host.report_restart(total, total <= 2000.0)?;
                    image.timings["restartMs"] = json!(total);
                    if total > 2000.0 {
                        self.host
                            .invalidate("restart_availability_target_missed")
                            .await?;
                        return Ok(None);
                    }
                }
            }
            Ok(image)
        }
        #[cfg(not(target_os = "macos"))]
        Ok(None)
    }
    #[cfg(target_os = "macos")]
    async fn inspect_window(&self, adapter: &super::Android) -> Result<Option<protection::Window>> {
        match protection::inspect(adapter, &self.serial).await {
            Ok(window) => Ok(Some(window)),
            Err(error) => {
                self.host
                    .invalidate(if error.code == "CAPTURE_BLOCKED" {
                        "protected_content"
                    } else {
                        "protection_check_failed"
                    })
                    .await?;
                if error.code == "CAPTURE_BLOCKED" {
                    Err(error)
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub(crate) async fn disable(&self, reason: &'static str) -> Result<()> {
        #[cfg(target_os = "macos")]
        self.host.invalidate(reason).await?;
        #[cfg(not(target_os = "macos"))]
        let _ = reason;
        Ok(())
    }
    pub(crate) async fn close(&self) -> Result<()> {
        self.disable("closed").await
    }
    /// Raw fallback is a new platform capture, still subject to confirmed protection.
    pub(crate) async fn raw_capture(&self) -> Result<(Vec<u8>, Value)> {
        self.safe_for_raw()?;
        let started = std::time::Instant::now();
        let adapter = super::Android::default();
        #[cfg(target_os = "macos")]
        let before = match protection::inspect(&adapter, &self.serial).await {
            Ok(window) => Some(window),
            Err(error) if error.code == "CAPTURE_BLOCKED" => {
                self.disable("protected_content").await?;
                return Err(error);
            }
            Err(_) => None,
        };
        #[cfg(target_os = "macos")]
        if !before.as_ref().is_some_and(|window| window.admissible()) {
            self.disable("protection_unknown").await?;
        } else if self.status()["state"] == "blocked" {
            self.disable("protection_epoch_invalidated").await?;
        }
        let (bytes, mut timings) = adapter
            .capture(&self.serial)
            .await
            .map_err(|error| self.capture_error(error))?;
        #[cfg(target_os = "macos")]
        {
            let after = match protection::inspect(&adapter, &self.serial).await {
                Ok(window) => Some(window),
                Err(error) if error.code == "CAPTURE_BLOCKED" => {
                    self.disable("protected_content").await?;
                    return Err(error);
                }
                Err(_) => None,
            };
            if !after.as_ref().is_some_and(|window| window.admissible()) {
                self.disable("protection_unknown").await?;
            }
            if before.as_ref().is_some_and(|window| window.admissible())
                && after.as_ref().is_some_and(|window| window.admissible())
                && before != after
            {
                self.disable("window_changed").await?;
                // A raw screenshot is a new platform-enforced acquisition.
                // Layer identity changes invalidate video, not these pixels.
            }
            timings["protectionState"] =
                json!(if before.as_ref().is_some_and(|window| window.admissible())
                    && before == after
                {
                    "native_policy_snapshots"
                } else {
                    "unknown"
                });
        }
        timings["imageCompletenessKnown"] = json!(false);
        let raw_ms = timings["totalMs"].as_f64().unwrap_or(0.0);
        let total_ms = started.elapsed().as_secs_f64() * 1000.0;
        timings["rawCaptureMs"] = json!(raw_ms);
        timings["policyMs"] = json!((total_ms - raw_ms).max(0.0));
        timings["totalMs"] = json!(total_ms);
        Ok((bytes, timings))
    }
    pub(crate) fn safe_for_raw(&self) -> Result<()> {
        let status = self.status();
        if status["state"] == "cleanup_pending" {
            return Err(Error::new(
                "CLEANUP_FAILED",
                "Android stream cleanup is unfinished; retry disconnect",
            ));
        }
        if status["releasePending"] == true {
            return Err(Error::new(
                "CAPTURE_PENDING",
                "Android stream release is still running; retry observation after releasePending clears",
            ));
        }
        Ok(())
    }
    pub(crate) fn stamp(&self, timings: &mut Value, effective: Option<&str>) {
        timings["androidCapture"] = self.status();
        timings["androidCapture"]["effectiveBackend"] = json!(effective);
        timings["captureAgeKnown"] = json!(false);
        if timings["freshnessBasis"].is_null() {
            timings["freshnessBasis"] = json!("raw_capture_after_request");
        }
    }
    pub(crate) fn capture_error(&self, mut error: Error) -> Error {
        let mut details = error.details.take().unwrap_or_else(|| json!({}));
        if !details.is_object() {
            details = json!({"cause":details});
        }
        details["androidCapture"] = self.status();
        error.details(details)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_selection_is_android_only_and_has_no_implicit_opt_in() {
        assert_eq!(Mode::parse(None, true).unwrap(), None);
        assert_eq!(
            Mode::parse(Some(&json!("stream")), true).unwrap(),
            Some(Mode::Stream)
        );
        for value in [json!(true), json!("auto"), json!(12)] {
            assert!(Mode::parse(Some(&value), true).is_err());
        }
        assert!(Mode::parse(Some(&json!("stream")), false).is_err());
    }
}
