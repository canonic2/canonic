//! Signed pack catalogs, verified acquisition and dependency resolution.
//!
//! A registry is a directory or HTTPS origin that serves a `catalog.json`
//! listing immutable pack artifacts by publisher/name, version and SHA-256.
//! This crate owns the mechanics every consuming product shares: catalog
//! authentication, source identity and enrollment, bounded acquisition,
//! rollback checkpoints, registrations, selectors, dependency resolution,
//! a digest-addressed artifact cache and lockfile pack entries.
//!
//! Products own what a pack contains, how it is validated, how changes are
//! reviewed with the operator, where state is stored and what installed bytes
//! are used for. The crate never executes artifact content, never prompts,
//! never reads the wall clock and never chooses a location: the time for
//! expiry decisions and every path are inputs.
//!
//! Domain modules ([`catalog`], [`checkpoint`], [`digest`], [`lock`], [`names`],
//! [`registrations`], [`resolution`], [`selector`], [`source`], [`timestamp`])
//! are pure. [`acquisition`], [`cache`] and [`registrations::GlobalFile`]
//! perform network and filesystem I/O. The `specs/` folder of this package
//! defines the required behavior.

pub mod acquisition;
pub mod cache;
pub mod catalog;
pub mod checkpoint;
pub mod digest;
mod files;
pub mod lock;
pub mod names;
pub mod registrations;
pub mod resolution;
pub mod selector;
pub mod source;
#[cfg(test)]
mod test_support;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod timestamp;
