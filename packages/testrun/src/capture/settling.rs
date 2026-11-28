//! Visual change detection and stillness timing, independent of frame acquisition.
use super::{STILL_DIFFERENCE, Stability, decode, difference};
use crate::error::Result;
use image::{GrayImage, imageops::FilterType};
use std::time::{Duration, Instant};

const CHANGE_LIMIT: Duration = Duration::from_secs(2);
const QUIET: Duration = Duration::from_millis(300);
const STILL_LIMIT: Duration = Duration::from_millis(1500);

/// A small image retained only for the observation associated with its input.
pub(crate) struct Sample(GrayImage);
impl Sample {
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        Ok(Self(
            decode(bytes)?
                .resize_exact(60, 130, FilterType::Triangle)
                .to_luma8(),
        ))
    }
}

pub(crate) struct Status {
    /// None means there was no pre-input baseline, not a negative detection.
    pub(crate) change_detected: Option<bool>,
    pub(crate) settled: bool,
    pub(crate) waited: Duration,
    pub(crate) samples: u32,
}

/// A baseline starts a bounded change phase; otherwise only stillness is checked.
pub(crate) struct Settling {
    baseline: Option<Sample>,
    started: Instant,
    phase_started: Instant,
    change_detected: Option<bool>,
    stability: Stability,
    settled: bool,
    samples: u32,
}
impl Settling {
    pub(crate) fn new(baseline: Option<Sample>, at: Instant) -> Self {
        Self {
            change_detected: baseline.as_ref().map(|_| false),
            baseline,
            started: at,
            phase_started: at,
            stability: Stability::new(QUIET),
            settled: false,
            samples: 0,
        }
    }

    pub(crate) fn remaining(&self, at: Instant) -> Duration {
        let limit = if self.change_detected == Some(false) {
            CHANGE_LIMIT
        } else {
            STILL_LIMIT
        };
        limit.saturating_sub(at.saturating_duration_since(self.phase_started))
    }

    /// True only when stillness is established and no change phase is pending.
    pub(crate) fn sample(&mut self, sample: Sample, at: Instant) -> bool {
        self.samples += 1;
        if self.change_detected == Some(false)
            && at.saturating_duration_since(self.started) < CHANGE_LIMIT
            && self
                .baseline
                .as_ref()
                .is_some_and(|baseline| difference(&baseline.0, &sample.0) > STILL_DIFFERENCE)
        {
            self.change_detected = Some(true);
            self.phase_started = at;
            self.stability = Stability::new(QUIET);
        }
        self.settled = self.stability.sample_image(sample.0, at);
        self.settled && self.change_detected != Some(false)
    }

    pub(crate) fn status(&self, at: Instant) -> Status {
        Status {
            change_detected: self.change_detected,
            settled: self.settled,
            waited: at.saturating_duration_since(self.started),
            samples: self.samples,
        }
    }
}

#[cfg(test)]
mod tests;
