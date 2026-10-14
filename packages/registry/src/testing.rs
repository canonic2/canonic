//! Test support: catalog signing keys, an owned localhost TLS registry and
//! its fixtures.
//!
//! Consumers enable the `test-support` feature only for their own tests. The
//! library's unit tests use the signing key without the feature. The fixture
//! CA is public test data trusted only by fetchers built here; production
//! [`Fetcher::new`](crate::acquisition::Fetcher::new) always uses public roots.

#[cfg(feature = "test-support")]
mod server;
#[cfg(feature = "test-support")]
mod signed_registry;
mod signing;

#[cfg(feature = "test-support")]
pub use server::{Response, Server};
#[cfg(feature = "test-support")]
pub use signed_registry::SignedRegistry;
pub use signing::SigningKey;
