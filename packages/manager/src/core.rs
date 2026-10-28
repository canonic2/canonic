//! I/O-free admission reservations, product obligations, exit policies and events.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
    task::Waker,
    time::Duration,
};

/// Externally visible lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Starting,
    Serving,
    Idle,
    Draining,
    Exiting,
}

/// Payload-free transition evidence; consumers choose how to log it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Ready,
    IdleEntered,
    IdleCancelled,
    DrainAdmitted,
    DrainRefused,
    RetirementRequested,
    Exit(ExitReason),
}

/// Why admission closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Policy,
    Retired,
    Drained,
}

/// A policy receives elapsed idle time supplied by the runtime's clock.
pub trait ExitPolicy: Send + Sync {
    /// Whether an otherwise empty manager should close admission.
    fn exit(&self, idle: Duration) -> bool;
    /// Time until another idle evaluation is needed, or None to wait for a
    /// state-change notification. Runs outside the admission lock. A timed
    /// policy must supply its remaining deadline; zero requests immediate evaluation.
    /// If unchanged idle state must keep serving, return a positive delay or None.
    fn next_check(&self, idle: Duration) -> Option<Duration>;
}

/// Exit after a monotonic idle grace. Observers never renew it.
pub struct IdleGrace(pub Duration);
impl ExitPolicy for IdleGrace {
    fn exit(&self, idle: Duration) -> bool {
        idle >= self.0
    }
    fn next_check(&self, idle: Duration) -> Option<Duration> {
        Some(self.0.saturating_sub(idle))
    }
}

/// Exit immediately when no work or product obligation is held.
pub struct WhenEmpty;
impl ExitPolicy for WhenEmpty {
    fn exit(&self, _: Duration) -> bool {
        true
    }
    fn next_check(&self, _: Duration) -> Option<Duration> {
        Some(Duration::ZERO)
    }
}

/// Require an explicit retirement or drain, even when empty.
pub struct Explicit;
impl ExitPolicy for Explicit {
    fn exit(&self, _: Duration) -> bool {
        false
    }
    fn next_check(&self, _: Duration) -> Option<Duration> {
        None
    }
}

/// A coherent count snapshot; held obligations are distinct from bounded replies.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Snapshot {
    pub state: State,
    pub work: usize,
    pub observers: usize,
    pub held: usize,
    /// A requested replacement will bypass the idle policy once work settles.
    #[serde(default)]
    pub retirement_pending: bool,
}

struct Inner {
    state: State,
    work: usize,
    observers: usize,
    held: usize,
    retirement_pending: bool,
    idle_since: Option<Duration>,
    changed: bool,
    reason: ExitReason,
    events: VecDeque<Event>,
    waker: Option<Arc<Waker>>,
}

impl Inner {
    fn event(&mut self, event: Event) {
        if self.events.len() == 64 {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
    fn empty(&self) -> bool {
        self.work == 0 && self.held == 0
    }
    fn closed(&self) -> bool {
        matches!(self.state, State::Draining | State::Exiting)
    }
}

/// Shared atomic boundary. Its short lock never encloses I/O or a product callback.
#[derive(Clone)]
pub struct Admission(Arc<Mutex<Inner>>);

impl Default for Admission {
    fn default() -> Self {
        Self::new()
    }
}

impl Admission {
    /// Create a closed-to-requests starting state. Initial obligations may be added.
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Inner {
            state: State::Starting,
            work: 0,
            observers: 0,
            held: 0,
            retirement_pending: false,
            idle_since: None,
            changed: false,
            reason: ExitReason::Policy,
            events: VecDeque::with_capacity(64),
            waker: None,
        })))
    }
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Install the single driving runtime's wake target, or remove it on detach.
    /// Register before observing state or calculating a wait. Mutations wake
    /// outside the lock; observing state and consuming diagnostics do not wake.
    pub fn set_waker(&self, waker: Option<Waker>) {
        let waker = waker.map(Arc::new);
        let previous = {
            let mut inner = self.lock();
            std::mem::replace(&mut inner.waker, waker)
        };
        drop(previous);
    }

    /// Notify the driving runtime after an external product-policy change.
    /// Does not change admission or reset the idle epoch.
    pub fn wake(&self) {
        Self::notify(self.lock());
    }

    fn notify(inner: MutexGuard<'_, Inner>) {
        // Clone only an Arc here; a custom waker's vtable also runs outside the lock.
        let waker = inner.waker.clone();
        drop(inner);
        if let Some(waker) = waker {
            waker.wake_by_ref();
        }
    }

    /// Open admission after product initialization and endpoint binding.
    pub fn ready(&self) {
        let mut inner = self.lock();
        if inner.state == State::Starting {
            inner.state = State::Serving;
            inner.event(Event::Ready);
            Self::notify(inner);
        }
    }

    /// Reserve before product preflight; keep the permit through final reply writing.
    ///
    /// # Errors
    /// Returns `Starting` before readiness or `Draining` after closure.
    pub fn enter(&self, observer: bool) -> Result<Permit> {
        let mut inner = self.lock();
        if inner.state == State::Starting {
            return Err(Error::Starting);
        }
        if inner.closed() {
            return Err(Error::Draining);
        }
        if observer {
            inner.observers += 1;
        } else {
            inner.work += 1;
        }
        Self::notify(inner);
        Ok(Permit {
            gate: self.clone(),
            observer,
        })
    }

    pub(crate) fn control(&self) -> Result<Permit> {
        let mut inner = self.lock();
        // Once work settles, existing replies form a finite set. They must
        // not admit more observers while they finish.
        if inner.state == State::Exiting || (inner.state == State::Draining && inner.empty()) {
            return Err(Error::Draining);
        }
        inner.observers += 1;
        Self::notify(inner);
        Ok(Permit {
            gate: self.clone(),
            observer: true,
        })
    }

    /// Pin a live product resource before publishing it. May be used during startup.
    ///
    /// # Errors
    /// Returns `Draining` after admission closes. An already admitted permit can
    /// transfer its obligation using [`Permit::hold`] even during drain.
    pub fn hold(&self) -> Result<Hold> {
        let mut inner = self.lock();
        if inner.closed() {
            return Err(Error::Draining);
        }
        inner.held += 1;
        inner.changed = true;
        if inner.state == State::Idle {
            inner.state = State::Serving;
            inner.idle_since = None;
            inner.event(Event::IdleCancelled);
        }
        Self::notify(inner);
        Ok(Hold(self.clone()))
    }

    /// Close admission first, then check product obligations and mutations.
    /// Observers finish their responses but cannot starve retirement.
    #[must_use]
    pub fn retire_if_empty(&self) -> bool {
        let mut inner = self.lock();
        if inner.closed() {
            return inner.empty();
        }
        let previous = inner.state;
        inner.state = State::Draining;
        if !inner.empty() {
            inner.state = previous;
            inner.event(Event::DrainRefused);
            return false;
        }
        inner.reason = ExitReason::Retired;
        inner.event(Event::DrainAdmitted);
        Self::notify(inner);
        true
    }

    /// Remember an explicit replacement request without interrupting live work.
    /// Returns true if admission is already closed or can close immediately.
    /// A busy manager keeps serving and closes on an atomic empty recheck,
    /// regardless of its normal idle grace or explicit-only policy.
    #[must_use]
    pub fn retire_when_empty(&self) -> bool {
        let mut inner = self.lock();
        if !inner.retirement_pending {
            inner.retirement_pending = true;
            inner.event(Event::RetirementRequested);
        }
        if inner.closed() {
            Self::notify(inner);
            return true;
        }
        if !inner.empty() {
            Self::notify(inner);
            return false;
        }
        inner.state = State::Draining;
        inner.reason = ExitReason::Retired;
        inner.event(Event::DrainAdmitted);
        Self::notify(inner);
        true
    }

    /// Close new admission; admitted work and holds must settle before exit.
    /// Returns true only on the first drain transition.
    #[must_use]
    pub fn drain(&self) -> bool {
        let mut inner = self.lock();
        if inner.closed() {
            return false;
        }
        inner.state = State::Draining;
        inner.reason = ExitReason::Drained;
        inner.event(Event::DrainAdmitted);
        Self::notify(inner);
        true
    }

    /// Advance using a caller-supplied monotonic timestamp, without polling state.
    /// The policy callback runs outside the admission lock. Its epoch is rechecked.
    #[must_use]
    pub fn tick(&self, now: Duration, policy: &impl ExitPolicy) -> bool {
        let idle = {
            let mut inner = self.lock();
            if inner.state == State::Starting {
                return false;
            }
            if !inner.closed() && inner.empty() {
                if inner.changed || inner.idle_since.is_none() {
                    inner.idle_since = Some(now);
                    inner.changed = false;
                    inner.state = State::Idle;
                    inner.event(Event::IdleEntered);
                }
                inner.idle_since
            } else {
                None
            }
        };
        let exit = idle.is_some_and(|since| policy.exit(now.saturating_sub(since)));
        let mut inner = self.lock();
        if (exit || inner.retirement_pending)
            && !inner.closed()
            && inner.empty()
            && !inner.changed
            && inner.idle_since == idle
        {
            inner.state = State::Draining;
            inner.reason = if inner.retirement_pending {
                ExitReason::Retired
            } else {
                ExitReason::Policy
            };
            inner.event(Event::DrainAdmitted);
        }
        if inner.state == State::Draining && inner.empty() && inner.observers == 0 {
            inner.state = State::Exiting;
            let reason = inner.reason;
            inner.event(Event::Exit(reason));
        }
        inner.state == State::Exiting
    }

    /// Compute the next relative policy deadline using a caller clock. None
    /// means wait for a state change; zero means call `tick` before blocking.
    /// Register a waker before calling this to avoid losing a concurrent change.
    #[must_use]
    pub fn next_check(&self, now: Duration, policy: &impl ExitPolicy) -> Option<Duration> {
        let idle = {
            let inner = self.lock();
            if inner.state == State::Starting || inner.closed() || !inner.empty() {
                return None;
            }
            if inner.changed {
                return Some(Duration::ZERO);
            }
            let Some(since) = inner.idle_since else {
                return Some(Duration::ZERO);
            };
            now.saturating_sub(since)
        };
        policy.next_check(idle)
    }

    /// Read counts without enumerating product records.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let inner = self.lock();
        Snapshot {
            state: inner.state,
            work: inner.work,
            observers: inner.observers,
            held: inner.held,
            retirement_pending: inner.retirement_pending,
        }
    }

    /// Consume up to 64 retained payload-free events; oldest overflow is discarded.
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        self.lock().events.drain(..).collect()
    }
}

/// An admitted request, released only after dispatch and reply publication settle.
pub struct Permit {
    gate: Admission,
    observer: bool,
}
impl Permit {
    pub(crate) fn promote(&mut self) -> Result<()> {
        let mut inner = self.gate.lock();
        if inner.closed() {
            return Err(Error::Draining);
        }
        if self.observer {
            inner.observers -= 1;
            inner.work += 1;
            self.observer = false;
            Admission::notify(inner);
        }
        Ok(())
    }
    pub(crate) fn activate(&self) {
        let mut inner = self.gate.lock();
        if inner.state == State::Idle {
            inner.event(Event::IdleCancelled);
        }
        if !inner.closed() {
            inner.state = State::Serving;
        }
        inner.changed = true;
        inner.idle_since = None;
        Admission::notify(inner);
    }
    /// Convert a decoded read-only request into a bounded observer response.
    /// It no longer pins the idle deadline, but exit still waits for its reply.
    pub fn observe(&mut self) {
        if !self.observer {
            let mut inner = self.gate.lock();
            inner.observers += 1;
            inner.work -= 1;
            self.observer = true;
            Admission::notify(inner);
        }
    }
    /// Transfer live responsibility before releasing the request, including during drain.
    ///
    /// # Errors
    /// Returns `Draining` if an observer tries to create ownership after closure.
    pub fn hold(&self) -> Result<Hold> {
        let mut inner = self.gate.lock();
        if self.observer && inner.closed() {
            return Err(Error::Draining);
        }
        inner.held += 1;
        inner.changed = true;
        if inner.state == State::Idle {
            inner.state = State::Serving;
            inner.idle_since = None;
            inner.event(Event::IdleCancelled);
        }
        Admission::notify(inner);
        Ok(Hold(self.gate.clone()))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut inner = self.gate.lock();
        if self.observer {
            inner.observers -= 1;
        } else {
            inner.work -= 1;
            inner.changed = true;
        }
        Admission::notify(inner);
    }
}

/// A product-owned resource, worker, connection or uncertainty pin.
pub struct Hold(Admission);
impl Drop for Hold {
    fn drop(&mut self) {
        let mut inner = self.0.lock();
        inner.held -= 1;
        inner.changed = true;
        Admission::notify(inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_policy_check_uses_the_idle_epoch_and_live_obligations() {
        let gate = Admission::new();
        gate.ready();
        assert_eq!(
            gate.next_check(Duration::ZERO, &Explicit),
            Some(Duration::ZERO)
        );
        assert!(!gate.tick(Duration::ZERO, &Explicit));
        assert_eq!(gate.next_check(Duration::ZERO, &Explicit), None);
        let grace = IdleGrace(Duration::from_secs(30));
        assert_eq!(
            gate.next_check(Duration::from_secs(10), &grace),
            Some(Duration::from_secs(20))
        );
        let observer = gate.enter(true).unwrap();
        assert_eq!(
            gate.next_check(Duration::from_secs(20), &grace),
            Some(Duration::from_secs(10))
        );
        drop(observer);
        let hold = gate.hold().unwrap();
        assert_eq!(gate.next_check(Duration::from_secs(100), &grace), None);
        drop(hold);
        assert_eq!(
            gate.next_check(Duration::from_secs(100), &grace),
            Some(Duration::ZERO)
        );
        assert!(!gate.tick(Duration::from_secs(100), &grace));
        assert_eq!(
            gate.next_check(Duration::from_secs(110), &grace),
            Some(Duration::from_secs(20))
        );
        assert!(gate.tick(Duration::from_secs(130), &grace));
        assert_eq!(gate.next_check(Duration::from_secs(130), &grace), None);
    }

    #[test]
    fn runtime_notifications_run_outside_the_lock_and_observations_do_not_wake() {
        use std::{
            sync::atomic::{AtomicUsize, Ordering},
            task::Wake,
        };
        struct Reentrant {
            gate: Admission,
            calls: AtomicUsize,
        }
        impl Wake for Reentrant {
            fn wake(self: Arc<Self>) {
                assert!(
                    self.gate.0.try_lock().is_ok(),
                    "wake ran under the admission lock"
                );
                let _ = self.gate.snapshot();
                self.calls.fetch_add(1, Ordering::SeqCst);
            }
        }
        let gate = Admission::new();
        let wake = Arc::new(Reentrant {
            gate: gate.clone(),
            calls: AtomicUsize::new(0),
        });
        gate.set_waker(Some(Waker::from(wake.clone())));
        gate.ready();
        let mut work = gate.enter(false).unwrap();
        let hold = work.hold().unwrap();
        drop(hold);
        work.observe();
        drop(work);
        assert_eq!(wake.calls.load(Ordering::SeqCst), 6);
        assert!(!gate.tick(Duration::ZERO, &Explicit));
        let _ = gate.snapshot();
        let _ = gate.events();
        assert_eq!(wake.calls.load(Ordering::SeqCst), 6);
        assert!(gate.drain());
        assert_eq!(wake.calls.load(Ordering::SeqCst), 7);
        gate.wake();
        assert_eq!(wake.calls.load(Ordering::SeqCst), 8);
        gate.set_waker(None);
    }

    #[test]
    fn pending_retirement_waits_for_work_and_bypasses_the_idle_policy() {
        let gate = Admission::new();
        gate.ready();
        let held = gate.hold().unwrap();
        assert!(!gate.retire_when_empty());
        assert!(!gate.retire_when_empty());
        assert!(gate.snapshot().retirement_pending);
        let work = gate.enter(false).unwrap();
        assert!(!gate.tick(Duration::from_secs(100), &Explicit));
        drop(held);
        assert!(!gate.tick(Duration::from_secs(200), &Explicit));
        let transferred = work.hold().unwrap();
        drop(work);
        assert!(!gate.tick(Duration::from_secs(300), &Explicit));
        drop(transferred);
        let reply = gate.control().unwrap();
        assert!(!gate.tick(Duration::from_secs(301), &Explicit));
        assert!(matches!(gate.enter(false), Err(Error::Draining)));
        drop(reply);
        assert!(gate.tick(Duration::from_secs(301), &Explicit));
        let events = gate.events();
        assert_eq!(
            events
                .iter()
                .filter(|e| **e == Event::RetirementRequested)
                .count(),
            1
        );
        assert_eq!(events.last(), Some(&Event::Exit(ExitReason::Retired)));
    }

    #[test]
    fn control_admission_closes_when_draining_work_and_holds_settle() {
        let gate = Admission::new();
        gate.ready();
        let work = gate.enter(false).unwrap();
        let hold = work.hold().unwrap();
        assert!(gate.drain());
        let reply = gate.control().unwrap();
        assert!(!gate.tick(Duration::ZERO, &Explicit));
        drop(work);
        drop(gate.control().unwrap());
        drop(hold);
        for _ in 0..64 {
            assert!(matches!(gate.control(), Err(Error::Draining)));
            assert!(!gate.tick(Duration::ZERO, &Explicit));
            assert_eq!(gate.snapshot().observers, 1);
        }
        drop(reply);
        assert!(gate.tick(Duration::ZERO, &Explicit));
    }

    #[test]
    fn retirement_and_idle_exit_close_controls_but_preserve_existing_replies() {
        for retire in [false, true] {
            let gate = Admission::new();
            gate.ready();
            let reply = gate.control().unwrap();
            if retire {
                assert!(gate.retire_if_empty());
            } else {
                assert!(!gate.tick(Duration::ZERO, &WhenEmpty));
            }
            assert!(matches!(gate.control(), Err(Error::Draining)));
            assert!(!gate.tick(Duration::ZERO, &Explicit));
            drop(reply);
            assert!(gate.tick(Duration::ZERO, &Explicit));
        }
    }
}
