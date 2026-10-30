//! The platform's local stream socket: Unix domain sockets, which Windows 10
//! (1803) and later also provide.

#[cfg(unix)]
pub use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};
#[cfg(windows)]
pub use uds_windows::{UnixListener as Listener, UnixStream as Stream};
