//! Native CDP transport, resource indexing and retained private Chrome lifetimes.
//!
//! This layer upgrades an admitted stream, validates an exact selected build and
//! tracks owned context/target/frame/world identity independently of Playwright.
//! The session supervisor binds private preparation to shared CLI/MCP ownership.

mod bootstrap;
mod connection;
mod events;
mod process;
mod react_native;
pub(crate) use react_native::PROBE as REACT_NATIVE_PROBE;
mod resources;
mod schema;
pub(crate) mod session;
mod wire;

pub use bootstrap::{Bootstrap, Identity};
pub use connection::{Connection, Dispatch, Error, Lane, Loss, SettledReply, Status};
pub use events::{Event, EventItem, Events};
pub use resources::{Index, Kind, ResourceError, Route};
pub use schema::Error as SchemaError;
pub use wire::RemoteError;
pub(crate) fn react_native_endpoint(endpoint: &str) -> bool {
    react_native::endpoint(endpoint)
}

/// Initial transport budgets, independent of the Playwright transport limits.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_COMMAND_BYTES: usize = 1024 * 1024;
pub const MAX_PENDING: usize = 64;
pub const MAX_RETIRED_CALLS: usize = 4096;
pub const CONTROL_RESERVE: usize = 8;
pub const MAX_EVENTS: usize = 8192;
pub const MAX_CRITICAL_EVENTS: usize = 256;
pub const MAX_EVENT_BYTES: usize = 1024 * 1024;
pub const MAX_EVENTS_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RETAINED_BYTES: usize = 16 * 1024 * 1024;
