//! Bounded big-endian length-prefixed byte frames and JSON convenience.

use crate::{Error, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

/// Framed I/O with one absolute deadline, including partial reads and writes.
/// Each operation changes the stream's corresponding socket timeout and leaves
/// it set. Do not perform concurrent I/O through other stream references/clones.
/// Before reusing the stream for blocking owner/EOF reads, drop this wrapper and
/// call `set_read_timeout(None)` and `set_write_timeout(None)` on the stream.
#[cfg(any(unix, windows))]
pub struct Deadline<'a> {
    stream: &'a crate::net::Stream,
    until: std::time::Instant,
}

#[cfg(any(unix, windows))]
impl<'a> Deadline<'a> {
    /// Borrow a connected private stream for an exchange ending at `until`.
    #[must_use]
    pub fn new(stream: &'a crate::net::Stream, until: std::time::Instant) -> Self {
        Self { stream, until }
    }

    fn remaining(&self) -> std::io::Result<std::time::Duration> {
        self.until
            .checked_duration_since(std::time::Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| std::io::ErrorKind::TimedOut.into())
    }

    fn timed<T>(
        &self,
        timeout: std::io::Result<()>,
        operation: impl FnOnce() -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        match timeout {
            Err(error) if crate::connection::peer_closed_timeout(&error) => {
                self.stream.set_nonblocking(true)?;
                let result = operation();
                let restored = self.stream.set_nonblocking(false);
                result.and_then(|value| restored.map(|()| value))
            }
            Err(error) => Err(error),
            Ok(()) => operation(),
        }
    }
}

#[cfg(any(unix, windows))]
impl Read for Deadline<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let mut stream = self.stream;
        loop {
            let result = self.timed(stream.set_read_timeout(Some(self.remaining()?)), || {
                stream.read(bytes)
            });
            if result.as_ref().is_err_and(|error| {
                matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                )
            }) && std::time::Instant::now() < self.until
            {
                std::thread::sleep(std::time::Duration::from_millis(1).min(self.remaining()?));
                continue;
            }
            return result;
        }
    }
}

#[cfg(any(unix, windows))]
impl Write for Deadline<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut stream = self.stream;
        loop {
            let result = self.timed(stream.set_write_timeout(Some(self.remaining()?)), || {
                stream.write(bytes)
            });
            if result.as_ref().is_err_and(|error| {
                matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                )
            }) && std::time::Instant::now() < self.until
            {
                std::thread::sleep(std::time::Duration::from_millis(1).min(self.remaining()?));
                continue;
            }
            return result;
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Default maximum payload: one MiB, excluding the four-byte prefix.
pub const DEFAULT_LIMIT: usize = 1024 * 1024;

/// Read exactly one frame, refusing the advertised length before allocation.
///
/// # Errors
/// Returns size-limit or I/O errors, including incomplete prefixes and bodies.
pub fn read(input: &mut impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut prefix = [0; 4];
    input.read_exact(&mut prefix)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length > limit {
        return Err(Error::FrameTooLarge);
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Write one complete frame. Oversized payloads write no prefix or bytes.
///
/// # Errors
/// Returns size-limit or I/O errors.
pub fn write(output: &mut impl Write, payload: &[u8], limit: usize) -> Result<()> {
    let length = u32::try_from(payload.len()).map_err(|_| Error::FrameTooLarge)?;
    if payload.len() > limit {
        return Err(Error::FrameTooLarge);
    }
    output.write_all(&length.to_be_bytes())?;
    output.write_all(payload)?;
    Ok(())
}

/// Decode a JSON frame without leaking its contents in an error.
///
/// # Errors
/// Returns framing errors or [`Error::Envelope`] for invalid JSON.
pub fn read_json<T: DeserializeOwned>(input: &mut impl Read, limit: usize) -> Result<T> {
    serde_json::from_slice(&read(input, limit)?).map_err(|_| Error::Envelope)
}

/// Encode a JSON frame.
///
/// # Errors
/// Returns encoding, size-limit or I/O errors.
pub fn write_json(output: &mut impl Write, value: &impl Serialize, limit: usize) -> Result<()> {
    let bytes = encode_json(value, limit)?;
    write(output, &bytes, limit)
}

/// Encode JSON without allocating beyond the frame budget.
///
/// # Errors
/// Returns `FrameTooLarge` or `Envelope`; no partial bytes are returned.
pub fn encode_json(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
        limit: usize,
        exceeded: bool,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                self.exceeded = true;
                return Err(std::io::Error::other("frame limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded {
        bytes: Vec::new(),
        limit,
        exceeded: false,
    };
    let encoded = serde_json::to_writer(&mut output, value);
    if output.exceeded {
        return Err(Error::FrameTooLarge);
    }
    encoded.map_err(|_| Error::Envelope)?;
    Ok(output.bytes)
}

#[cfg(all(test, unix))]
mod deadline_tests {
    use super::*;
    use std::{
        os::unix::net::UnixStream,
        time::{Duration, Instant},
    };

    #[test]
    fn deadline_retains_buffered_reply_after_peer_close_and_refuses_expired_io() {
        let (client, mut peer) = UnixStream::pair().unwrap();
        write_json(&mut peer, &serde_json::json!({"ready":true}), 4096).unwrap();
        drop(peer);
        let mut wire = Deadline::new(&client, Instant::now() + Duration::from_secs(1));
        let reply: serde_json::Value = read_json(&mut wire, 4096).unwrap();
        assert_eq!(reply, serde_json::json!({"ready":true}));
        let mut wire = Deadline::new(&client, Instant::now());
        assert_eq!(
            wire.read(&mut [0]).unwrap_err().kind(),
            std::io::ErrorKind::TimedOut
        );
        assert_eq!(
            wire.write(&[0]).unwrap_err().kind(),
            std::io::ErrorKind::TimedOut
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn deadline_fallback_never_blocks_on_a_live_socket() {
        let (client, mut peer) = UnixStream::pair().unwrap();
        let wire = Deadline::new(&client, Instant::now() + Duration::from_secs(1));
        let mut stream = &client;
        let error = wire
            .timed(Err(std::io::Error::from_raw_os_error(22)), || {
                stream.read(&mut [0])
            })
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        write_json(&mut peer, &serde_json::json!(true), 4096).unwrap();
        let mut wire = Deadline::new(&client, Instant::now() + Duration::from_secs(1));
        assert!(read_json::<bool>(&mut wire, 4096).unwrap());
    }
}
