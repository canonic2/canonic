//! Cooperative cancellation and one monotonic deadline for a unit of work.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// The longest timeout a [`Control`] accepts; longer requests are clamped.
pub const MAX_TIMEOUT: Duration = Duration::from_hours(24);

/// Work was cancelled or reached its deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("work was cancelled or exceeded its deadline")]
pub struct Interrupted;

/// Cancellation flag and deadline shared between a scheduler and its work.
///
/// Work calls [`Control::check`] at safe points and stops when it fails.
/// Cancelling never interrupts code that does not check.
#[derive(Debug)]
pub struct Control {
    cancelled: AtomicBool,
    deadline: Instant,
}

impl Control {
    /// Starts a control whose deadline is `timeout` from now, at most [`MAX_TIMEOUT`].
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        Self::until(Instant::now() + timeout.min(MAX_TIMEOUT))
    }

    /// Starts a control with an explicit deadline.
    #[must_use]
    pub fn until(deadline: Instant) -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            deadline,
        }
    }

    /// Requests that the work stop at its next check.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Whether [`Control::cancel`] was called.
    #[must_use]
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// The deadline fixed at creation.
    #[must_use]
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Time left before the deadline, zero once it has passed.
    #[must_use]
    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    /// Whether the deadline has passed.
    #[must_use]
    pub fn expired(&self) -> bool {
        self.remaining().is_zero()
    }

    /// Succeeds while the work may continue.
    ///
    /// # Errors
    ///
    /// Returns [`Interrupted`] once the work is cancelled or its deadline has passed.
    pub fn check(&self) -> Result<(), Interrupted> {
        if self.cancelled() || self.expired() {
            Err(Interrupted)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_fails_after_cancellation_or_deadline() {
        let control = Control::new(Duration::from_secs(60));
        assert!(control.check().is_ok());
        control.cancel();
        assert_eq!(control.check(), Err(Interrupted));

        let expired = Control::until(Instant::now());
        assert!(expired.expired());
        assert_eq!(expired.check(), Err(Interrupted));
        assert!(!expired.cancelled());
    }

    #[test]
    fn oversized_timeouts_are_clamped() {
        let control = Control::new(Duration::MAX);
        assert!(control.remaining() <= MAX_TIMEOUT);
        assert!(control.check().is_ok());
    }
}
