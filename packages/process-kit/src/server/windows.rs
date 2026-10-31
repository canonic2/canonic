//! The Windows driver. Winsock offers no safe readiness poll over these
//! listeners, so each lane has a blocking accept thread feeding its bounded
//! pool directly. Coalesced admission wakeups and accept failures arrive on
//! a channel, which the driver waits on with the policy deadline or no timeout.

use super::{Handler, Pool, Shared};
use crate::{Error, Result, core::ExitPolicy, endpoint::Endpoint, net::Stream};
use std::{
    path::Path,
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

impl Signal {
    /// Consume notifications covered by the next state observation.
    fn clear(&self, events: impl Iterator<Item = Event>) -> Result<()> {
        for event in events {
            if let Event::Failed(error) = event {
                return Err(error.into());
            }
        }
        // Clear after draining: a wake consumed above must not leave pending
        // set with an empty queue and suppress every later notification.
        // Changes before this clear are covered by the next state observation;
        // changes after it can enqueue a message before the driver waits.
        self.pending.store(false, Ordering::Release);
        Ok(())
    }
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
                            // Reserve capacity here; the frame budget starts
                            // when a pool worker takes this connection.
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
        let result = {
            // Stops the accept threads even when a policy or product callback
            // panics, so the scope can join them and report the panic.
            let _stopping = StopAccepting {
                stop: &stop,
                paths: lanes.map(|(endpoint, _, _)| endpoint.path()),
            };
            drive(shared, policy, &signal, &events, start)
        };
        for thread in accepting {
            let _ = thread.join();
        }
        result
    })
}

struct StopAccepting<'a> {
    stop: &'a AtomicBool,
    paths: [&'a Path; 2],
}

impl Drop for StopAccepting<'_> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Unblock each accept with a connection the stopped thread discards.
        for path in self.paths {
            let _ = Stream::connect(path);
        }
    }
}

fn drive<H: Handler>(
    shared: &Shared<H>,
    policy: &impl ExitPolicy,
    signal: &Signal,
    events: &mpsc::Receiver<Event>,
    start: Instant,
) -> Result<()> {
    loop {
        // Wakes already queued are covered by this observation; acting on them
        // later would check the policy again with nothing changed.
        signal.clear(events.try_iter())?;
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

#[cfg(test)]
mod signal_tests;

fn closed() -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "manager accept channel closed",
    ))
}
