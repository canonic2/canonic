//! Public-boundary acceptance using owned sockets, kernel locks and re-executed fixtures.
#![cfg(any(unix, windows))]
#![allow(
    clippy::unwrap_used,
    reason = "test fixture failures must panic; production APIs return typed errors"
)]

mod connections;
mod controls;
mod fixtures;
mod lifecycle;
mod processes;
mod recovery;
mod transport;
mod upgrades;
#[cfg(windows)]
mod windows_integrity;
