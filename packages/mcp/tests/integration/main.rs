//! Drives the stdio server through in-memory pipes, as a host would.

mod confirmation;
mod disconnection;
#[cfg(feature = "docs-build")]
mod docs;
mod fixture;
mod lifecycle;
mod modern;
mod resources;
mod scheduling;
mod skills;
