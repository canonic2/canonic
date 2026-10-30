//! Bounded product-owned connections transferred only after a successful reply.

use crate::{Result, core::Hold, net::Stream};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// A successful handshake and the product's ownership receiver.
/// Dropping an uncommitted handoff drops the receiver and its captured guards.
pub struct Handoff {
    pub(crate) reply: Value,
    pub(crate) receive: Box<dyn FnOnce(Connection) + Send>,
}

impl Handoff {
    /// Prepare a reply and a receiver called only after its successful write.
    /// The receiver must promptly enqueue or transfer ownership to product code;
    /// running a session inline would occupy the handshake worker. Capture RAII
    /// guards for any prepared resources so a failed reply safely drops them.
    pub fn new(reply: Value, receive: impl FnOnce(Connection) + Send + 'static) -> Self {
        Self {
            reply,
            receive: Box::new(receive),
        }
    }
}

/// A same-user connection plus its capacity slot and resource hold.
/// Product code owns framing, reads, writes, EOF handling and drain cancellation.
/// Keep the guard until resource cleanup completes, including after socket EOF.
pub struct Connection {
    stream: Stream,
    guard: Guard,
}

impl Connection {
    pub(crate) fn new(stream: Stream, hold: Hold, slot: Slot) -> Self {
        Self {
            stream,
            guard: Guard {
                _hold: hold,
                _slot: slot,
            },
        }
    }

    /// Borrow the blocking stream. Handshake read/write timeouts are cleared.
    #[must_use]
    pub const fn stream(&self) -> &Stream {
        &self.stream
    }

    /// Transfer into an alternative runtime, retaining the guard through cleanup.
    /// All cloned descriptors and product resources must settle before the guard.
    #[must_use]
    pub fn into_parts(self) -> (Stream, Guard) {
        (self.stream, self.guard)
    }
}

/// Keeps both manager residence and a connection slot pinned through cleanup.
pub struct Guard {
    _hold: Hold,
    _slot: Slot,
}

pub(crate) struct Slots {
    used: AtomicUsize,
    limit: usize,
}
impl Slots {
    pub(crate) fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            used: AtomicUsize::new(0),
            limit,
        })
    }
    pub(crate) fn reserve(self: &Arc<Self>) -> Option<Slot> {
        self.used
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < self.limit).then_some(n + 1)
            })
            .ok()
            .map(|_| Slot(self.clone()))
    }
    pub(crate) fn used(&self) -> usize {
        self.used.load(Ordering::SeqCst)
    }
}
pub(crate) struct Slot(Arc<Slots>);
impl Drop for Slot {
    fn drop(&mut self) {
        self.0.used.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Clear handshake deadlines before transfer. Darwin rejects timeout changes
/// with `EINVAL` once the peer has closed; that stream already reads EOF, so
/// its owner observes the closure instead of a handshake failure.
pub(crate) fn clear_deadlines(stream: &Stream) -> Result<()> {
    stream.set_nonblocking(false)?;
    for cleared in [
        stream.set_read_timeout(None),
        stream.set_write_timeout(None),
    ] {
        match cleared {
            Ok(()) => {}
            Err(error) if peer_closed_timeout(&error) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn peer_closed_timeout(error: &std::io::Error) -> bool {
    #[cfg(target_os = "macos")]
    {
        error.raw_os_error() == Some(nix::errno::Errno::EINVAL as i32)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = error;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peer_closed_after_the_handshake_reads_eof_instead_of_failing_transfer() {
        let (stream, peer) = Stream::pair().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        drop(peer);
        clear_deadlines(&stream).unwrap();
        let mut byte = [0];
        assert_eq!(std::io::Read::read(&mut &stream, &mut byte).unwrap(), 0);
    }
}
