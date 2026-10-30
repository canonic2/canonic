//! The Windows driver. Winsock offers no safe readiness poll over these
//! listeners, so each lane has a blocking accept thread feeding its bounded
//! pool directly. Coalesced admission wakeups and accept failures arrive on
//! a channel, which the driver waits on with the policy deadline or no timeout.

use super::{Handler, Pool, Shared};
use crate::{Error, Result, core::ExitPolicy, endpoint::Endpoint, net::Stream};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    task::{Wake, Waker},
    thread::{self, Thread},
    time::Instant,
};

enum Event {
    Wake,
    Failed(std::io::Error),
}

/// Posts at most one queued wake message until the driver clears it.
struct Signal {
    sender: mpsc::Sender<Event>,
    pending: AtomicBool,
    driver: Thread,
}

impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        // The park token also wakes failure cleanup after the driver stops.
        self.driver.unpark();
        if !self.pending.swap(true, Ordering::AcqRel) {
            // A stopped driver has no receiver left to notify.
            let _ = self.sender.send(Event::Wake);
        }
    }
}

/// Register the wake channel, declare readiness and serve until admission closes.
pub(super) fn run<H: Handler>(
    shared: &Shared<H>,
    policy: &impl ExitPolicy,
    lanes: [(&Endpoint<'_>, &Pool, bool); 2],
    start: Instant,
) -> Result<()> {
    // Complete fallible setup before starting any thread that needs shutdown.
    let listeners = lanes
        .iter()
        .map(|(endpoint, _, _)| {
            let listener = endpoint.listener().try_clone()?;
            listener.set_nonblocking(false)?;
            Ok(listener)
        })
        .collect::<Result<Vec<_>>>()?;
    let (sender, events) = mpsc::channel();
    let signal = Arc::new(Signal {
        sender: sender.clone(),
        pending: AtomicBool::new(false),
        driver: thread::current(),
    });
    shared
        .gate
        .set_waker(Some(Waker::from(Arc::clone(&signal))));
    let stop = Arc::new(AtomicBool::new(false));
    shared.gate.ready();
    thread::scope(|scope| {
        let mut accepting = Vec::new();
        for (listener, (_, pool, control)) in listeners.into_iter().zip(lanes) {
            let sender = sender.clone();
            let stop = Arc::clone(&stop);
            accepting.push(scope.spawn(move || {
                loop {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if stop.load(Ordering::SeqCst) {
                                return;
                            }
                            // Capacity and the read deadline are assigned here;
                            // no unaccounted socket queue precedes the pool.
                            pool.enqueue(shared, stream, control);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) => {
                            if !stop.load(Ordering::SeqCst) {
                                let _ = sender.send(Event::Failed(error));
                            }
                            return;
                        }
                    }
                }
            }));
        }
        drop(sender);
        let result = drive(shared, policy, &signal, &events, start);
        // Unblock each accept with a connection the stopped thread discards.
        stop.store(true, Ordering::SeqCst);
        for (endpoint, _, _) in &lanes {
            let _ = Stream::connect(endpoint.path());
        }
        for thread in accepting {
            let _ = thread.join();
        }
        result
    })
}

fn drive<H: Handler>(
    shared: &Shared<H>,
    policy: &impl ExitPolicy,
    signal: &Signal,
    events: &mpsc::Receiver<Event>,
    start: Instant,
) -> Result<()> {
    loop {
        // Clear before observing, so a change racing the observation posts again.
        signal.pending.store(false, Ordering::Release);
        if shared.gate.tick(start.elapsed(), policy) {
            return Ok(());
        }
        let event = match shared.gate.next_check(start.elapsed(), policy) {
            Some(delay) => match events.recv_timeout(delay) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Err(closed()),
            },
            None => events.recv().map_err(|_| closed())?,
        };
        match event {
            Event::Wake => {}
            Event::Failed(error) => return Err(error.into()),
        }
    }
}

#[cfg(all(test, unix))]
mod tests;

fn closed() -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "manager accept channel closed",
    ))
}
