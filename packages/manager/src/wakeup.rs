//! Coalesced nonblocking runtime wakeups for socket polling and failure cleanup.

use crate::Result;
use std::{
    io::{Read, Write},
    os::{
        fd::{AsFd, BorrowedFd},
        unix::net::UnixStream,
    },
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Wake, Waker},
    thread::{self, Thread},
};

struct Signal {
    socket: UnixStream,
    pending: AtomicBool,
    driver: Thread,
    failure: Mutex<Option<std::io::Error>>,
}

impl Signal {
    fn fail(&self, error: std::io::Error) {
        *self
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error);
        // Closing the write side makes the peer readable, so poll observes the
        // failure. If the peer detached already there is no waiter to signal.
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }
}

impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        // The park token also wakes cleanup if a listener/poll failure stopped
        // socket admission. Tokens and queued notifications are both bounded.
        self.driver.unpark();
        if !self.pending.swap(true, Ordering::AcqRel) {
            let mut socket = &self.socket;
            loop {
                match socket.write(&[1]) {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => {
                        self.fail(error);
                        break;
                    }
                    Ok(0) => {
                        self.fail(std::io::Error::new(
                            std::io::ErrorKind::WriteZero,
                            "manager wake write failed",
                        ));
                        break;
                    }
                    Ok(_) => break,
                }
            }
        }
    }
}

pub(crate) struct Wakeup {
    receiver: UnixStream,
    signal: Arc<Signal>,
}

impl Wakeup {
    /// Create the driving thread's private notification socket pair.
    ///
    /// # Errors
    /// Returns socket creation or nonblocking configuration errors.
    pub(crate) fn new() -> Result<Self> {
        let (receiver, socket) = UnixStream::pair()?;
        receiver.set_nonblocking(true)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            receiver,
            signal: Arc::new(Signal {
                socket,
                pending: AtomicBool::new(false),
                driver: thread::current(),
                failure: Mutex::new(None),
            }),
        })
    }

    pub(crate) fn waker(&self) -> Waker {
        Waker::from(self.signal.clone())
    }

    /// Clear before inspecting state, never between inspection and blocking.
    ///
    /// # Errors
    /// Returns a receive error other than interruption or an empty queue.
    pub(crate) fn clear(&self) -> Result<()> {
        if let Some(error) = self
            .signal
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            return Err(error.into());
        }
        let mut receiver = &self.receiver;
        loop {
            match receiver.read(&mut [0; 64]) {
                Ok(0) => {
                    // A writer can fail after the initial failure check. Its
                    // cause is stored before shutdown makes this EOF observable.
                    let error = self
                        .signal
                        .failure
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                        .unwrap_or_else(|| {
                            std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "manager wake channel closed",
                            )
                        });
                    return Err(error.into());
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error.into()),
            }
        }
        self.signal.pending.store(false, Ordering::Release);
        Ok(())
    }
}

impl AsFd for Wakeup {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.receiver.as_fd()
    }
}

#[cfg(test)]
mod tests {
    //! Bounded notification queues and retained wakeups across wait boundaries.

    use super::*;
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn notifications_coalesce_and_a_wake_before_poll_remains_readable() {
        let wakeup = Wakeup::new().unwrap();
        let waker = wakeup.waker();
        for _ in 0..1000 {
            waker.wake_by_ref();
        }
        let mut receiver = &wakeup.receiver;
        assert_eq!(receiver.read(&mut [0]).unwrap(), 1);
        assert_eq!(
            receiver.read(&mut [0]).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        wakeup.clear().unwrap();
        waker.wake_by_ref();
        let mut descriptors = [PollFd::new(wakeup.as_fd(), PollFlags::POLLIN)];
        assert_eq!(
            poll(&mut descriptors, PollTimeout::from(100_u16)).unwrap(),
            1
        );
        wakeup.clear().unwrap();
        assert_eq!(poll(&mut descriptors, PollTimeout::from(0_u16)).unwrap(), 0);
    }

    #[test]
    fn wake_failures_preserve_the_io_cause_and_a_closed_channel_does_not_spin() {
        let wakeup = Wakeup::new().unwrap();
        wakeup
            .signal
            .socket
            .shutdown(std::net::Shutdown::Write)
            .unwrap();
        wakeup.waker().wake_by_ref();
        let mut descriptors = [PollFd::new(wakeup.as_fd(), PollFlags::POLLIN)];
        assert_eq!(poll(&mut descriptors, PollTimeout::from(0_u16)).unwrap(), 1);
        let kind = wakeup
            .signal
            .failure
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .kind();
        assert!(matches!(wakeup.clear(), Err(crate::Error::Io(error)) if error.kind() == kind));
        let closed = wakeup.clear();
        assert!(
            matches!(closed, Err(crate::Error::Io(ref error)) if error.kind() == std::io::ErrorKind::UnexpectedEof),
            "{closed:?}"
        );
    }

    #[test]
    fn failure_cleanup_retains_a_wake_received_before_parking() {
        let (finished, completion) = mpsc::sync_channel(0);
        let driver = thread::spawn(move || {
            let wakeup = Wakeup::new().unwrap();
            wakeup.waker().wake_by_ref();
            thread::park();
            finished.send(()).unwrap();
        });
        let woke = completion.recv_timeout(Duration::from_secs(1)).is_ok();
        if !woke {
            driver.thread().unpark();
            completion.recv_timeout(Duration::from_secs(1)).unwrap();
        }
        driver.join().unwrap();
        assert!(woke, "cleanup lost a notification before its park");
    }
}
