//! The awake clock that times idle release and release retries: it advances
//! only while the host is awake, so a sleeping host keeps its devices.
use std::time::Duration;

#[cfg(windows)]
#[link(name = "mincore")]
unsafe extern "system" {
    fn QueryUnbiasedInterruptTimePrecise(ticks: *mut u64);
}

/// A point on the host's awake clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Awake(Duration);

impl Awake {
    /// Now on the host clock that stops while the host sleeps.
    #[cfg(unix)]
    pub(crate) fn now() -> Self {
        #[cfg(target_os = "macos")]
        let clock = libc::CLOCK_UPTIME_RAW;
        #[cfg(not(target_os = "macos"))]
        let clock = libc::CLOCK_MONOTONIC;
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `time` is a valid, writable timespec for this read-only clock query.
        let status = unsafe { libc::clock_gettime(clock, &raw mut time) };
        if status != 0 {
            // The supported clocks cannot fail with a valid pointer; stay
            // monotonic rather than treating a failure as elapsed time.
            return Self(Duration::ZERO);
        }
        Self(Duration::new(
            u64::try_from(time.tv_sec).unwrap_or(0),
            u32::try_from(time.tv_nsec).unwrap_or(0),
        ))
    }

    /// Now on the Windows clock that excludes sleep and hibernation.
    #[cfg(windows)]
    pub(crate) fn now() -> Self {
        let mut ticks = 0_u64;
        // SAFETY: the function writes one 64-bit time count to this pointer.
        unsafe { QueryUnbiasedInterruptTimePrecise(&raw mut ticks) };
        const TICKS_PER_SECOND: u64 = 10_000_000;
        Self(Duration::new(
            ticks / TICKS_PER_SECOND,
            ((ticks % TICKS_PER_SECOND) * 100) as u32,
        ))
    }

    /// Awake time elapsed since `self`; zero if the clock reads earlier.
    pub(crate) fn elapsed(self) -> Duration {
        Self::now().0.saturating_sub(self.0)
    }
    #[cfg(test)]
    pub(crate) fn earlier(self, by: Duration) -> Self {
        Self(self.0.saturating_sub(by))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_awake_clock_is_monotonic_and_advances() {
        let start = Awake::now();
        let mut later = Awake::now();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while later == start {
            assert!(std::time::Instant::now() < deadline);
            later = Awake::now();
        }
        assert!(later > start);
        assert!(start.elapsed() > Duration::ZERO);
        assert!(later.earlier(Duration::from_secs(600)).elapsed() >= Duration::from_secs(600));
    }
}
