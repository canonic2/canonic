//! Consumer-facing video source, calibrated observations and input readiness.
#[cfg(test)]
use super::lifecycle::JoinState;
use super::{ConnectionOwner, apple, lifecycle::Lifecycle, observation::Observation};
use crate::error::{Error, Result};
use idevice::tcp::handle::AdapterHandle;
use serde_json::{Value, json};
#[cfg(test)]
use std::sync::Mutex;
use std::{sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, watch};

pub struct VideoFrame {
    pub media: crate::media::Frame,
    pub payload: Vec<u8>,
    pub key: bool,
    pub pts: u64,
    pub format: Arc<apple::Format>,
    pub configuration_sequence: u64,
}
/// The checked video frame and its change/stillness measurements stay together.
pub(crate) struct SettledFrame {
    pub(crate) image: apple::Snapshot,
    pub(crate) status: crate::capture::settling::Status,
}
/// Pause between settle samples, on top of decode time.
const SETTLE_SAMPLE: Duration = Duration::from_millis(40);
pub struct Source {
    pub(super) audio: Option<super::audio::Context>,
    pub(super) display_geometry: Result<(u32, u32)>,
    pub(super) hid: Result<(AdapterHandle, u16)>,
    pub(super) consumer: Result<(AdapterHandle, u16)>,
    pub(super) device_info: Option<(AdapterHandle, u16)>,
    pub(super) observation: Arc<Observation>,
    pub(super) target: Value,
    pub(super) frames: broadcast::Sender<Arc<VideoFrame>>,
    pub(super) latest: watch::Receiver<Option<Arc<VideoFrame>>>,
    pub(super) commands: mpsc::Sender<()>,
    pub(super) lifecycle: Arc<Lifecycle>,
}
impl Source {
    /// Bounded status read over the retained tunnel, with no input or new source.
    pub(crate) async fn lock_status(&self) -> Value {
        let Some((mut tunnel, port)) = self.device_info.clone() else {
            return super::lock::unknown(&Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Device-info service is unavailable",
            ));
        };
        super::lock::bounded(super::lock::probe(&mut tunnel, port)).await
    }
    pub(crate) fn consumer_transport(&self) -> Result<(AdapterHandle, u16)> {
        self.consumer.clone()
    }
    pub(crate) fn subscribe_frames(&self) -> broadcast::Receiver<Arc<VideoFrame>> {
        self.frames.subscribe()
    }
    pub(crate) fn subscribe_latest(&self) -> watch::Receiver<Option<Arc<VideoFrame>>> {
        self.latest.clone()
    }
    /// The error that ended the device video transport, such as USB loss.
    /// A quiet but connected stream has none.
    pub(crate) fn transport_error(&self) -> Option<Error> {
        self.lifecycle.stream_error()
    }
    pub(crate) fn geometry(&self) -> Result<(u32, u32)> {
        let frame = self
            .latest
            .borrow()
            .clone()
            .ok_or_else(|| Error::new("CAPTURE_STALLED", "Video source is not live"))?;
        if frame.media.received.elapsed() > Duration::from_millis(500) {
            return Err(Error::new(
                "CAPTURE_STALLED",
                "HID requires a recent video frame",
            ));
        }
        if frame.configuration_sequence != 1 {
            return Err(Error::new(
                "STALE_GEOMETRY",
                "Video configuration changed; reopen HID to recalibrate the display",
            ));
        }
        if self.observation.enabled() {
            let (pts, error) = self.observation.health();
            if pts.is_none_or(|pts| frame.pts.saturating_sub(pts) > 500_000_000) {
                return Err(Error::new(
                    "CAPTURE_STALLED",
                    "HID requires a healthy recent decoded video frame",
                )
                .details(json!({"cause":error})));
            }
        }
        self.display_geometry.clone()
    }
    pub(crate) async fn open_input(&self) -> Result<Arc<crate::hid::Hid>> {
        let (tunnel, port) = self.hid.clone()?;
        ConnectionOwner::open_hid(tunnel, port).await
    }
    pub(crate) async fn fresh_geometry(&self) -> Result<(u32, u32)> {
        match self.geometry() {
            Ok(size) => return Ok(size),
            Err(error) if error.code != "CAPTURE_STALLED" => return Err(error),
            Err(_) => {}
        }
        self.keyframe();
        let mut latest = self.latest.clone();
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut decoded = tokio::time::interval(Duration::from_millis(10));
            loop {
                tokio::select! {
                    changed=latest.changed()=>changed.map_err(|_|Error::new("CAPTURE_STALLED","Video source ended"))?,
                    _=decoded.tick()=>{},
                }
                if latest.has_changed().is_err() {
                    return Err(Error::new("CAPTURE_STALLED", "Video source ended"));
                }
                match self.geometry() {
                    Ok(size) => return Ok(size),
                    Err(error) if error.code != "CAPTURE_STALLED" => return Err(error),
                    Err(_) => {}
                }
            }
        })
        .await
        .map_err(|_| {
            Error::new(
                "CAPTURE_STALLED",
                "HID requires a fresh healthy video frame",
            )
        })?
    }
    /// After input, look for a baseline difference before checking stillness.
    /// A fresh checked frame is returned even if a visual phase times out.
    pub(crate) async fn settle(
        &self,
        baseline: Option<crate::capture::settling::Sample>,
    ) -> Result<SettledFrame> {
        use crate::capture::settling::{Sample, Settling};
        let mut wait = Settling::new(baseline, std::time::Instant::now());
        let mut last = None;
        loop {
            let remaining = wait.remaining(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let image = match tokio::time::timeout(remaining, self.snapshot()).await {
                Ok(image) => image?,
                Err(_) => break,
            };
            let sample = Sample::decode(&image.bytes)?;
            let done = wait.sample(sample, std::time::Instant::now());
            last = Some((image, std::time::Instant::now()));
            if done {
                break;
            }
            tokio::time::sleep(SETTLE_SAMPLE.min(wait.remaining(std::time::Instant::now()))).await;
        }
        let (image, sampled_at) = last.ok_or_else(|| {
            Error::new(
                "CAPTURE_STALLED",
                "No fresh frame during action observation",
            )
        })?;
        self.geometry()?;
        if self.latest.has_changed().is_err()
            || sampled_at.elapsed() > Duration::from_millis(500)
            || self
                .latest
                .borrow()
                .as_ref()
                .is_none_or(|frame| frame.pts.saturating_sub(image.pts) > 500_000_000)
        {
            return Err(Error::new(
                "CAPTURE_STALLED",
                "The checked action frame is no longer fresh",
            ));
        }
        Ok(SettledFrame {
            status: wait.status(std::time::Instant::now()),
            image,
        })
    }
    pub(crate) async fn snapshot(&self) -> Result<apple::Snapshot> {
        self.fresh_geometry().await?;
        self.observation.enable();
        let mut latest = self.latest.clone();
        let floor = latest.borrow().as_ref().map_or(0, |f| f.pts);
        if self.observation.health().0.is_none() {
            self.keyframe();
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            // Native decode callbacks can complete after the encoded-frame watch
            // notification, even when a static display produces no next frame.
            let mut decoded = tokio::time::interval(Duration::from_millis(10));
            loop {
                tokio::select! {
                    changed=latest.changed()=>changed.map_err(|_|Error::new("CAPTURE_STALLED","Video source ended"))?,
                    _=decoded.tick()=>{},
                }
                if latest.has_changed().is_err() {
                    return Err(Error::new("CAPTURE_STALLED", "Video source ended"));
                }
                if latest.borrow().as_ref().is_some_and(|frame| frame.configuration_sequence != 1) {
                    return Err(Error::new("STALE_GEOMETRY", "Video configuration changed; reopen to recalibrate"));
                }
                let fresh = latest.borrow().as_ref().is_some_and(|frame| {
                    frame.media.received.elapsed() < Duration::from_millis(500)
                });
                if !fresh {
                    // A brief gap during native startup is not a terminal
                    // failure. Keep waiting within the observation deadline;
                    // never return the stale image or restart the source here.
                    self.keyframe();
                    continue;
                }
                if self.observation.health().0.is_none_or(|pts| pts <= floor) { continue; }
                let image = self.observation.snapshot().await?;
                if let Some(image) = image
                    && image.pts > floor
                {
                    self.geometry()?;
                    return Ok(image);
                }
            }
        })
        .await
        .map_err(|_| {
            Error::new(
                "CAPTURE_STALLED",
                "No fresh decoded video observation within five seconds",
            )
            .details(json!({
                "encoded": self.latest.borrow().as_ref().map(|frame| json!({
                    "pts": frame.pts, "key": frame.key,
                    "sourceFrames": frame.media.metadata["sourceFrames"],
                    "droppedAccessUnits": frame.media.metadata["droppedAccessUnits"],
                    "ageMs": frame.media.received.elapsed().as_millis()
                })),
                "requestedAfterPts": floor,
                "decodedPts": self.observation.health().0,
                "decodeError": self.observation.health().1,
                "displayGeometry": self.display_geometry.clone().ok()
            }))
        })?
    }
    pub(crate) fn request_sender(&self) -> mpsc::Sender<()> {
        self.commands.clone()
    }
    pub fn keyframe(&self) {
        let _ = self.commands.try_send(());
    }
    pub async fn close(&self) -> Result<()> {
        self.lifecycle.close().await
    }
}

#[cfg(test)]
pub(super) fn recording_fixture() -> Arc<Source> {
    let (frames, _) = broadcast::channel(64);
    let (_, latest) = watch::channel(None);
    let (commands, _) = mpsc::channel(1);
    let observation = Observation::start(
        Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Fixture has no calibration",
        )),
        commands.clone(),
    )
    .unwrap();
    Arc::new(Source {
        audio: None,
        device_info: None,
        consumer: Err(Error::new(
            "MEDIA_UNSUPPORTED",
            "Fixture has no consumer channel",
        )),
        display_geometry: Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Fixture has no display calibration",
        )),
        hid: Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Fixture has no HID channel",
        )),
        observation: observation.clone(),
        target: json!({}),
        frames,
        latest,
        commands,
        lifecycle: Arc::new(Lifecycle {
            stream_error: Default::default(),
            remote: None,
            observation: Some(observation),
            stop: Mutex::new(None),
            join: tokio::sync::Mutex::new(JoinState {
                job: None,
                result: Some(Ok(())),
            }),
        }),
    })
}
impl Drop for Source {
    fn drop(&mut self) {
        self.observation.request_stop();
        self.lifecycle.request_stop();
    }
}

#[cfg(test)]
mod tests;
