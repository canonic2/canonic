//! Dedicated owner-channel death detection without releasing product responsibility.

use crate::{Error, Result, core::Hold, endpoint, net::Stream};
use std::time::{Duration, Instant};

/// A dedicated, silent same-user connection plus its live-resource hold.
/// The client retains its endpoint while it owns work. Kernel EOF detects owner
/// exit even after SIGKILL, without PID reuse or a renewable heartbeat timeout.
/// Products bound the number of channels and drive waits on their own workers.
pub struct Connection {
    stream: Stream,
    _hold: Hold,
    disconnected: bool,
}

impl Connection {
    /// Attach a hold to a dedicated channel. Do not share its read side with
    /// application framing or send application bytes after establishing it.
    ///
    /// # Errors
    /// Returns `Peer` for a different user or a peer-credential failure.
    pub fn new(stream: Stream, hold: Hold) -> Result<Self> {
        endpoint::check_peer(&stream)?;
        Ok(Self {
            stream,
            _hold: hold,
            disconnected: false,
        })
    }

    /// Wait at most `timeout` for kernel EOF. False means the owner is still
    /// connected; timeouts are not proof of death. True remains true on later
    /// calls. The hold remains pinned: clean up product resources before dropping
    /// this connection. If cleanup is uncertain, retain it and its hold.
    ///
    /// # Errors
    /// Returns poll/socket errors, or `Envelope` if the silent owner channel
    /// contains data. An error does not establish owner death or drop the hold.
    #[cfg(unix)]
    pub fn wait(&mut self, timeout: Duration) -> Result<bool> {
        use nix::{
            errno::Errno,
            poll::{PollFd, PollFlags, PollTimeout, poll},
            sys::socket::{MsgFlags, recv},
        };
        use std::os::fd::{AsFd, AsRawFd};
        let deadline = Instant::now() + timeout;
        while !self.disconnected {
            let mut descriptors = [PollFd::new(self.stream.as_fd(), PollFlags::POLLIN)];
            let remaining = deadline.saturating_duration_since(Instant::now());
            let millis = remaining
                .as_millis()
                .saturating_add(u128::from(!remaining.is_zero()));
            let timeout = PollTimeout::try_from(i32::try_from(millis).unwrap_or(i32::MAX))
                .map_err(|_| Error::Options)?;
            match poll(&mut descriptors, timeout) {
                Err(Errno::EINTR) if Instant::now() < deadline => continue,
                Err(Errno::EINTR) | Ok(0) => return Ok(false),
                Err(error) => return Err(error.into()),
                Ok(_) => {}
            }
            match recv(
                self.stream.as_raw_fd(),
                &mut [0],
                MsgFlags::MSG_PEEK | MsgFlags::MSG_DONTWAIT,
            ) {
                Ok(0) => self.disconnected = true,
                Ok(_) => return Err(Error::Envelope),
                Err(Errno::EAGAIN) => return Ok(false),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(true)
    }

    /// Wait at most `timeout` for kernel EOF, with the same results as on Unix.
    /// It polls for readability, then peeks one byte without consuming it, and
    /// never sets a receive timeout on the retained stream. A connection reset
    /// counts as closure: Windows reports a terminated owner's endpoint that way.
    ///
    /// # Errors
    /// Returns poll/socket errors, or `Envelope` if the silent owner channel
    /// contains data. An error does not establish owner death or drop the hold.
    #[cfg(windows)]
    pub fn wait(&mut self, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        while !self.disconnected {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if !crate::windows::wait_readable(&self.stream, remaining)? {
                return Ok(false);
            }
            // Process termination closes the owner's endpoint abortively, which
            // Windows reports as a reset rather than an orderly EOF.
            match crate::windows::peek(&self.stream) {
                Ok(0) => self.disconnected = true,
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {
                    self.disconnected = true;
                }
                Ok(_) => return Err(Error::Envelope),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(true)
    }
}
