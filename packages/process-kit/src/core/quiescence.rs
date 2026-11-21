//! Reversible mutation fencing. Resource transfer and durable commit are product-owned.

use super::{Admission, Event, ExitReason, Handover, State};
use crate::{Error, Result};

/// A unique preparation guard. Drop resumes admission only before fencing.
/// Keep it through product snapshot/transfer preparation. No timeout silently
/// releases this fence; products bound preparation and reconcile fenced failures.
#[must_use = "retain preparation through abort or irreversible retirement"]
pub struct Quiescence {
    gate: Admission,
    active: bool,
}

impl Admission {
    /// Pause new ordinary requests, work controls and unscoped holds, without
    /// running cleanup. Existing permits and holds may finish or split their
    /// obligations. Inspection and cleanup controls remain available.
    ///
    /// # Errors
    /// Returns `Starting` before readiness, `Draining` after closure, or
    /// `HandoverState` if another preparation already owns this fence.
    pub fn quiesce(&self) -> Result<Quiescence> {
        let mut inner = self.lock();
        if inner.state == State::Starting {
            return Err(Error::Starting);
        }
        if inner.closed() {
            return Err(Error::Draining);
        }
        if inner.quiescing() {
            return Err(Error::HandoverState);
        }
        inner.handover = Handover::Preparing;
        Admission::notify(inner);
        Ok(Quiescence {
            gate: self.clone(),
            active: true,
        })
    }
}

impl Quiescence {
    /// Irreversibly fence mutation admission before durable product commit.
    /// Unlike final retirement, resource holds may remain during this phase.
    /// New cleanup controls are refused; inspection still works. Products must
    /// also pause any background writers represented by their own holds.
    /// After success neither drop nor abort can resume the old writer. A failed
    /// product commit requires explicit reconciliation, drain or process exit.
    ///
    /// # Errors
    /// Returns `HandoverBusy` while mutation permits remain, or `HandoverState`
    /// if this preparation was committed or preempted by drain.
    pub fn fence(&mut self) -> Result<()> {
        let mut inner = self.gate.lock();
        if !self.active || !inner.quiescing() {
            return Err(Error::HandoverState);
        }
        if inner.work != 0 {
            return Err(Error::HandoverBusy);
        }
        inner.handover = Handover::Fenced;
        Admission::notify(inner);
        Ok(())
    }

    /// Whether all mutations and resource obligations have settled. Inspection
    /// replies may remain; commit keeps them accounted until the server returns.
    /// A true result is advisory; `commit` repeats this check atomically.
    #[must_use]
    pub fn ready(&self) -> bool {
        let inner = self.gate.lock();
        self.active && inner.quiescing() && inner.empty()
    }

    /// Irreversibly retire this manager after product transfer has committed.
    /// Release old holds only after the corresponding resource has safely
    /// settled or its responsibility has transferred. This never clears holds,
    /// invokes drain cleanup, releases a lease or activates another writer.
    /// Wait for `server::serve` to return and release all product writer locks
    /// before allowing the successor to initialize under a fresh lease.
    ///
    /// # Errors
    /// Returns `HandoverBusy` while mutations/holds remain, keeping this fence;
    /// `HandoverState` if already committed or an explicit drain preempted it.
    pub fn commit(&mut self) -> Result<()> {
        let mut inner = self.gate.lock();
        if !self.active || !inner.quiescing() {
            return Err(Error::HandoverState);
        }
        if !inner.empty() {
            return Err(Error::HandoverBusy);
        }
        inner.handover = Handover::Fenced;
        inner.state = State::Draining;
        inner.reason = ExitReason::Retired;
        inner.event(Event::DrainAdmitted);
        self.active = false;
        Admission::notify(inner);
        Ok(())
    }

    /// Abandon reversible preparation. A concurrent drain remains permanent.
    ///
    /// # Errors
    /// Returns `HandoverState` after irreversible fencing or commit. Admission
    /// stays closed in that case; dropping this guard cannot roll it back.
    pub fn abort(self) -> Result<()> {
        if self.gate.lock().handover == Handover::Fenced {
            return Err(Error::HandoverState);
        }
        Ok(())
    }
}

impl Drop for Quiescence {
    fn drop(&mut self) {
        let mut inner = self.gate.lock();
        if self.active && inner.handover == Handover::Preparing && !inner.closed() {
            inner.handover = Handover::None;
            if inner.state == State::Idle {
                inner.event(Event::IdleCancelled);
            }
            inner.state = State::Serving;
            inner.idle_since = None;
            inner.changed = true;
            Admission::notify(inner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ControlRole, Explicit, WhenEmpty};
    use std::time::Duration;

    #[test]
    fn quiescence_preserves_obligations_and_abort_reopens_admission() {
        let gate = Admission::new();
        assert!(matches!(gate.quiesce(), Err(Error::Starting)));
        gate.ready();
        let request = gate.enter(false).unwrap();
        let hold = gate.hold().unwrap();
        let mut fence = gate.quiesce().unwrap();
        assert!(matches!(gate.quiesce(), Err(Error::HandoverState)));
        assert!(matches!(gate.enter(false), Err(Error::Draining)));
        assert!(matches!(gate.hold(), Err(Error::Draining)));
        assert!(matches!(fence.commit(), Err(Error::HandoverBusy)));
        let child = hold.split();
        let admitted = request.hold().unwrap();
        let mut control = gate.control().unwrap();
        assert!(control.classify_control(ControlRole::Work).is_err());
        control.classify_control(ControlRole::Inspection).unwrap();
        assert!(matches!(control.hold(), Err(Error::ReadOnly)));
        assert!(!gate.retire_if_empty());
        assert!(!gate.retire_when_empty());
        assert!(!gate.tick(Duration::ZERO, &WhenEmpty));
        assert_eq!(gate.next_check(Duration::ZERO, &WhenEmpty), None);
        fence.abort().unwrap();
        assert!(!gate.snapshot().quiescing);
        assert!(gate.enter(false).is_ok());
        drop((request, hold, child, admitted, control));
        assert!(gate.tick(Duration::ZERO, &Explicit));

        // Use an idle manager without the pending retirement exercised above.
        let gate = Admission::new();
        gate.ready();
        assert!(!gate.tick(Duration::ZERO, &Explicit));
        assert_eq!(gate.events(), vec![Event::Ready, Event::IdleEntered]);
        gate.quiesce().unwrap().abort().unwrap();
        assert!(!gate.tick(Duration::from_secs(1), &Explicit));
        assert_eq!(
            gate.events(),
            vec![Event::IdleCancelled, Event::IdleEntered]
        );
    }

    #[test]
    fn quiescence_commit_waits_for_obligations_and_finishes_existing_replies() {
        let gate = Admission::new();
        gate.ready();
        let hold = gate.hold().unwrap();
        let mut fence = gate.quiesce().unwrap();
        let reply = gate.control().unwrap();
        assert!(!fence.ready());
        drop(hold);
        assert!(fence.ready());
        fence.commit().unwrap();
        assert!(matches!(fence.commit(), Err(Error::HandoverState)));
        drop(fence);
        assert!(gate.enter(false).is_err());
        assert!(!gate.tick(Duration::ZERO, &Explicit));
        drop(reply);
        assert!(gate.tick(Duration::ZERO, &Explicit));
    }

    #[test]
    fn quiescence_abort_cannot_undo_explicit_drain() {
        let gate = Admission::new();
        gate.ready();
        let mut fence = gate.quiesce().unwrap();
        assert!(gate.drain());
        assert!(!fence.ready());
        assert!(matches!(fence.commit(), Err(Error::HandoverState)));
        fence.abort().unwrap();
        assert!(gate.enter(false).is_err());
        assert!(gate.tick(Duration::ZERO, &Explicit));
    }

    #[test]
    fn quiescence_external_drain_preempts_preparation_but_refuses_irreversible_fence() {
        let gate = Admission::new();
        gate.ready();
        let preparation = gate.quiesce().unwrap();
        assert!(gate.try_drain().unwrap());
        preparation.abort().unwrap();
        assert_eq!(gate.snapshot().state, State::Draining);

        let gate = Admission::new();
        gate.ready();
        let hold = gate.hold().unwrap();
        let mut preparation = gate.quiesce().unwrap();
        preparation.fence().unwrap();
        assert!(matches!(gate.try_drain(), Err(Error::HandoverState)));
        assert_eq!(gate.snapshot().state, State::Serving);
        assert!(gate.snapshot().handover_fenced);
        drop(hold);
        preparation.commit().unwrap();
    }

    #[test]
    fn quiescence_local_reconciliation_cannot_reopen_external_cleanup_after_fence() {
        let gate = Admission::new();
        gate.ready();
        let hold = gate.hold().unwrap();
        let mut preparation = gate.quiesce().unwrap();
        preparation.fence().unwrap();
        assert!(gate.drain());
        let mut cleanup = gate.control().unwrap();
        assert!(cleanup.classify_control(ControlRole::Cleanup).is_err());
        cleanup.classify_control(ControlRole::Inspection).unwrap();
        drop((cleanup, hold, preparation));
        assert!(gate.tick(Duration::ZERO, &Explicit));
    }

    #[test]
    fn quiescence_irreversible_fence_cannot_reopen_after_durable_commit_failure() {
        let gate = Admission::new();
        gate.ready();
        let mutation = gate.enter(false).unwrap();
        let hold = gate.hold().unwrap();
        let mut fence = gate.quiesce().unwrap();
        assert!(matches!(fence.fence(), Err(Error::HandoverBusy)));
        drop(mutation);
        fence.fence().unwrap();
        assert!(gate.snapshot().handover_fenced);
        let mut cleanup = gate.control().unwrap();
        assert!(cleanup.classify_control(ControlRole::Cleanup).is_err());
        cleanup.classify_control(ControlRole::Inspection).unwrap();
        assert!(matches!(fence.abort(), Err(Error::HandoverState)));
        drop((cleanup, hold));
        assert!(gate.enter(false).is_err());
        assert!(!gate.tick(Duration::ZERO, &WhenEmpty));
        // An intentional drain is still possible after failed product commit.
        assert!(gate.drain());
        assert!(gate.tick(Duration::ZERO, &Explicit));
    }

    #[test]
    fn quiescence_racing_idle_exit_keeps_one_preparation_and_suppresses_exit() {
        use std::sync::Mutex;
        struct PrepareDuringPolicy {
            gate: Admission,
            preparation: Mutex<Option<Quiescence>>,
        }
        impl super::super::ExitPolicy for PrepareDuringPolicy {
            fn exit(&self, _: Duration) -> bool {
                *self.preparation.lock().unwrap() = Some(self.gate.quiesce().unwrap());
                true
            }
            fn next_check(&self, _: Duration) -> Option<Duration> {
                None
            }
        }
        let gate = Admission::new();
        gate.ready();
        let policy = PrepareDuringPolicy {
            gate: gate.clone(),
            preparation: Mutex::new(None),
        };
        assert!(!gate.tick(Duration::ZERO, &policy));
        assert!(gate.snapshot().quiescing);
        assert!(matches!(gate.quiesce(), Err(Error::HandoverState)));
        assert_eq!(gate.next_check(Duration::ZERO, &policy), None);
        policy
            .preparation
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .abort()
            .unwrap();
        assert!(gate.enter(false).is_ok());
    }
}
