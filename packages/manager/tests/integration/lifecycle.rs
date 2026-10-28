//! Deterministic clock, atomic admission and responsibility-transfer acceptance.

use canonic_manager::{
    Error,
    core::{Admission, Event, Explicit, IdleGrace, State, WhenEmpty},
};
use std::time::Duration;

#[test]
fn idle_driver_waits_for_policy_deadlines_or_changes_without_periodic_checks() {
    use super::fixtures::*;
    use canonic_manager::{core::ExitPolicy, endpoint::Lease, server};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    struct Counted {
        grace: Option<Duration>,
        checks: mpsc::Sender<Duration>,
        stop: Arc<AtomicBool>,
    }
    impl ExitPolicy for Counted {
        fn exit(&self, idle: Duration) -> bool {
            self.checks.send(idle).unwrap();
            self.stop.load(Ordering::SeqCst) || self.grace.is_some_and(|grace| idle >= grace)
        }
        fn next_check(&self, idle: Duration) -> Option<Duration> {
            self.grace.map(|grace| grace.saturating_sub(idle))
        }
    }
    for grace in [None, Some(Duration::from_secs(30))] {
        let (_temp, root) = root();
        let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
        let gate = Admission::new();
        let server_gate = gate.clone();
        let (checks, observed) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = stop.clone();
        let server = std::thread::spawn(move || {
            server::serve(
                lease,
                identity(),
                server_gate,
                Arc::new(Acme::default()),
                &Counted {
                    grace,
                    checks,
                    stop: server_stop,
                },
                server::Options::default(),
            )
        });
        observed.recv_timeout(Duration::from_secs(1)).unwrap();
        let quiet = observed.recv_timeout(Duration::from_millis(100));
        if grace.is_none() {
            stop.store(true, Ordering::SeqCst);
            gate.wake();
        } else {
            assert!(gate.drain());
        }
        wait(|| server.is_finished().then_some(()));
        server.join().unwrap().unwrap();
        assert!(
            matches!(quiet, Err(mpsc::RecvTimeoutError::Timeout)),
            "idle policy was checked again without activity or a due deadline: {grace:?}"
        );
    }
}

#[test]
fn a_change_during_deadline_calculation_is_not_lost_before_poll() {
    use super::fixtures::*;
    use canonic_manager::{core::ExitPolicy, endpoint::Lease, server};
    use std::sync::Arc;

    struct DrainDuringDeadline(Admission);
    impl ExitPolicy for DrainDuringDeadline {
        fn exit(&self, _: Duration) -> bool {
            false
        }
        fn next_check(&self, _: Duration) -> Option<Duration> {
            assert!(self.0.drain());
            None
        }
    }
    let (_temp, root) = root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let gate = Admission::new();
    let server_gate = gate.clone();
    let server = std::thread::spawn(move || {
        server::serve(
            lease,
            identity(),
            server_gate,
            Arc::new(Acme::default()),
            &DrainDuringDeadline(gate),
            server::Options::default(),
        )
    });
    wait(|| server.is_finished().then_some(()));
    server.join().unwrap().unwrap();
    assert!(!root.leased().unwrap());
}

#[test]
fn retirement_fences_admission_and_preserves_accepted_work() {
    let gate = Admission::new();
    gate.ready();
    let work = gate.enter(false).unwrap();
    assert!(!gate.retire_if_empty());
    let held = work.hold().unwrap();
    drop(work);
    assert!(!gate.retire_if_empty());
    drop(held);
    let observer = gate.enter(true).unwrap();
    assert!(gate.retire_if_empty());
    assert!(matches!(gate.enter(false), Err(Error::Draining)));
    assert!(!gate.tick(Duration::ZERO, &Explicit));
    drop(observer);
    assert!(gate.tick(Duration::ZERO, &Explicit));
}

#[test]
fn drain_finishes_admitted_work_and_transferred_holds() {
    let gate = Admission::new();
    gate.ready();
    let work = gate.enter(false).unwrap();
    assert!(gate.drain());
    assert!(!gate.retire_if_empty());
    assert!(matches!(gate.enter(false), Err(Error::Draining)));
    let held = work.hold().unwrap();
    drop(work);
    assert!(!gate.tick(Duration::from_secs(100), &WhenEmpty));
    drop(held);
    assert!(gate.tick(Duration::from_secs(100), &WhenEmpty));
    assert_eq!(gate.snapshot().state, State::Exiting);
}

#[test]
fn three_policies_use_a_controllable_clock_and_observers_never_renew_grace() {
    let gate = Admission::new();
    gate.ready();
    let grace = IdleGrace(Duration::from_secs(30));
    assert!(!gate.tick(Duration::ZERO, &grace));
    for seconds in 1..30 {
        let mut observer = gate.enter(false).unwrap();
        observer.observe();
        drop(observer);
        assert!(!gate.tick(Duration::from_secs(seconds), &grace));
    }
    assert!(gate.tick(Duration::from_secs(30), &grace));
    let gate = Admission::new();
    let resource = gate.hold().unwrap();
    gate.ready();
    assert!(!gate.tick(Duration::from_secs(1000), &WhenEmpty));
    drop(resource);
    assert!(gate.tick(Duration::from_secs(1001), &WhenEmpty));
    let gate = Admission::new();
    gate.ready();
    assert!(!gate.tick(Duration::from_secs(1000), &Explicit));
    assert!(gate.retire_if_empty());
}

#[test]
fn admission_during_policy_callback_cancels_retirement() {
    struct Racing {
        gate: Admission,
        work: std::sync::Mutex<Option<canonic_manager::core::Permit>>,
    }
    impl canonic_manager::core::ExitPolicy for Racing {
        fn exit(&self, _: Duration) -> bool {
            *self.work.lock().unwrap() = Some(self.gate.enter(false).unwrap());
            true
        }
        fn next_check(&self, _: Duration) -> Option<Duration> {
            None
        }
    }
    let gate = Admission::new();
    gate.ready();
    let policy = Racing {
        gate: gate.clone(),
        work: std::sync::Mutex::new(None),
    };
    assert!(!gate.tick(Duration::ZERO, &policy));
    assert_eq!(gate.snapshot().work, 1);
}

#[test]
fn bounded_events_are_payload_free_and_consumed() {
    let gate = Admission::new();
    gate.ready();
    let hold = gate.hold().unwrap();
    for _ in 0..100 {
        assert!(!gate.retire_if_empty());
    }
    let events = gate.events();
    assert_eq!(events.len(), 64);
    assert!(events.iter().all(|event| *event == Event::DrainRefused));
    assert_eq!(gate.events(), []);
    drop(hold);
}
