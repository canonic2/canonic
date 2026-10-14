//! Behavior through the public API with owned TLS registries and temporary folders.
//!
//! Requires the `test-support` feature.

mod hosted;
mod signed;
mod workflow;

use canonic_registry::{acquisition::Fetcher, testing::Server};
use std::time::Duration;

/// A fetcher trusting the owned fixture CA, for signed and local sources alike.
fn signed_fetcher() -> Fetcher {
    Server::fetcher(Duration::from_secs(2))
}
