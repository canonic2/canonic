//! Native Workbench page indexing.
//!
//! Literal parsing and definition rules are pure. The index workflow coordinates
//! filesystem discovery, saved state and JavaScript evaluation. CLI transport and
//! response projection stay at the boundary; shared observations live in model/files.
mod cli;
mod definition;
mod discovery;
mod evaluate;
mod files;
mod index;
mod literal;
mod model;
mod paths;
mod protocol;
mod saved;

/// Run the native CLI using process arguments and standard streams.
///
/// # Errors
/// Returns failures for invalid requests, discovery or evaluator execution.
pub fn run() -> anyhow::Result<()> {
    cli::run()
}
