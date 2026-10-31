//! Deterministic notification races using the real channel and coalescing waker.

use super::*;

fn signal() -> (Arc<Signal>, mpsc::Receiver<Event>) {
    let (sender, events) = mpsc::channel();
    (
        Arc::new(Signal {
            sender,
            pending: AtomicBool::new(false),
            driver: thread::current(),
        }),
        events,
    )
}

#[test]
fn a_wake_consumed_during_clear_does_not_suppress_a_later_notification() {
    let (signal, events) = signal();
    // Inject a state-change notification exactly when queue draining begins.
    let arriving = std::iter::once_with(|| {
        signal.wake_by_ref();
        events.try_recv().expect("wake sent while clearing")
    });
    signal.clear(arriving.chain(events.try_iter())).unwrap();
    // The next state observation covers the consumed change. A later change
    // must still wake a driver waiting without a policy deadline.
    signal.wake_by_ref();
    assert!(matches!(events.try_recv(), Ok(Event::Wake)));
}

#[test]
fn a_wake_after_clear_remains_queued_and_coalesces() {
    let (signal, events) = signal();
    signal.wake_by_ref();
    signal.clear(events.try_iter()).unwrap();
    signal.wake_by_ref();
    signal.wake_by_ref();
    assert!(matches!(events.try_recv(), Ok(Event::Wake)));
    assert!(matches!(events.try_recv(), Err(mpsc::TryRecvError::Empty)));
}
