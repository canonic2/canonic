//! Deterministic clock, atomic admission and responsibility-transfer acceptance.

use canonic_manager::{
    Error,
    core::{Admission, Event, Explicit, IdleGrace, State, WhenEmpty},
};
use std::time::Duration;

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
