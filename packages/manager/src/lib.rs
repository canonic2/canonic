//! Per-user manager mechanics with an I/O-free lifetime core and synchronous IPC.
//!
//! Products own requests, durable outcomes, resource cleanup and exit policy.
//! One-shot calls never replay; opt-in recovery requires product authority.
#![doc = include_str!("../docs/getting-started.md")]

#[cfg(unix)]
pub mod client;
pub mod core;
#[cfg(unix)]
pub mod endpoint;
#[cfg(not(unix))]
#[path = "unsupported.rs"]
pub mod endpoint;
pub mod frame;
pub mod launch;
#[cfg(unix)]
pub mod owner;
pub mod protocol;
#[cfg(unix)]
pub mod recovery;
#[cfg(unix)]
pub mod server;
pub mod version;
#[cfg(unix)]
mod wakeup;

/// A typed failure at the manager boundary. Messages never contain payloads.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A platform adapter has not been implemented.
    #[error("manager adapter unsupported on this platform")]
    Unsupported,
    /// A filesystem or stream operation failed.
    #[error("manager I/O: {0}")]
    Io(#[from] std::io::Error),
    /// A Unix system call failed.
    #[cfg(unix)]
    #[error("manager system call: {0}")]
    Unix(#[from] nix::errno::Errno),
    /// The peer is not the effective user.
    #[error("manager peer belongs to another user")]
    Peer,
    /// A path is not private, is a symlink, or has changed identity.
    #[error("manager path must be private, owned and stable")]
    PrivatePath,
    /// The socket path cannot fit the platform address.
    #[error("manager socket path is too long")]
    PathTooLong,
    /// Another process still holds the exclusive lease at the deadline.
    #[error("another manager holds the lease")]
    AlreadyRunning,
    /// This lease already has a live listener for the requested lane.
    #[error("manager endpoint is already bound by this lease")]
    EndpointBound,
    /// A bounded frame would be exceeded.
    #[error("manager frame exceeds limit")]
    FrameTooLarge,
    /// The envelope is malformed.
    #[error("invalid manager envelope")]
    Envelope,
    /// A bound or timeout is zero or internally inconsistent.
    #[error("invalid manager options")]
    Options,
    /// A bounded connection or worker lane is full.
    #[error("manager capacity reached")]
    Capacity,
    /// No endpoint and no live lease holder exist.
    #[error("manager is absent")]
    Absent,
    /// A lease is held but readiness was not reached by the deadline.
    #[error("manager is starting")]
    Starting,
    /// Admission is closed.
    #[error("manager is draining")]
    Draining,
    /// Version or capabilities do not meet the request's requirements.
    #[error("manager is incompatible")]
    Incompatible,
    /// A control targeted an incarnation that has since been replaced.
    #[error("manager incarnation changed; inspect again before requesting retirement")]
    StaleManager,
    /// A release label cannot be safely ordered as a bounded semantic version.
    #[error("manager version must be a semantic version of at most 128 bytes")]
    InvalidVersion,
    /// A transport failure after sending cannot establish non-admission.
    #[error("manager outcome unknown for request {0}")]
    OutcomeUnknown(String),
    /// A worker panicked; all other workers were joined before returning.
    #[error("manager worker failed")]
    Worker,
}

/// Result returned by fallible manager APIs.
pub type Result<T> = std::result::Result<T, Error>;

/// Verify availability before selecting platform-specific adapters.
///
/// # Errors
/// Returns [`Error::Unsupported`] outside implemented Unix platforms.
pub const fn supported() -> Result<()> {
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        Ok(())
    } else {
        Err(Error::Unsupported)
    }
}
