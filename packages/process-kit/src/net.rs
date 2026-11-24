//! The platform's local stream socket: Unix domain sockets, which Windows 10
//! (1803) and later also provide.

#[cfg(unix)]
pub use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};
#[cfg(windows)]
pub use uds_windows::{UnixListener as Listener, UnixStream as Stream};

/// Check a silent Windows peer for closure without consuming bytes or changing
/// socket settings. The caller must own the only reader after the handshake.
/// False means closure was not observed; a spurious readiness wake may return
/// it before the timeout. True means EOF or a connection reset was observed.
///
/// # Errors
/// Returns `Envelope` for unexpected data, or a socket error if closure could
/// not be determined.
#[cfg(windows)]
pub fn wait_peer_closed(stream: &Stream, timeout: std::time::Duration) -> crate::Result<bool> {
    if !crate::windows::wait_readable(stream, timeout)? {
        return Ok(false);
    }
    match crate::windows::peek(stream) {
        Ok(0) => Ok(true),
        Ok(_) => Err(crate::Error::Envelope),
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
        Err(error) => Err(error.into()),
    }
}
