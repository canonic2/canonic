//! Public-boundary acceptance using owned sockets, kernel locks and re-executed fixtures.
#![allow(
    clippy::unwrap_used,
    reason = "test fixture failures must panic; production APIs return typed errors"
)]

#[cfg(unix)]
mod fixtures;
#[cfg(unix)]
mod lifecycle;
#[cfg(unix)]
mod processes;
#[cfg(unix)]
mod recovery;
#[cfg(unix)]
mod transport;
#[cfg(unix)]
mod upgrades;
