//! The Workbench server: the front door to the Node server and the page index.
//!
//! Literal parsing and definition rules are pure. The index workflow coordinates
//! filesystem discovery, saved state and JavaScript evaluation. CLI transport and
//! response projection stay at the boundary; shared observations live in model/files.
mod agent_view;
mod build;
mod capture;
mod catalog;
mod cli;
mod compiler;
mod config;
mod definition;
mod diagnostics;
mod discovery;
mod docs;
mod evaluate;
mod export;
mod files;
mod handoff;
mod host;
mod index;
mod js;
mod literal;
mod model;
mod node;
mod order;
mod paths;
mod portable;
mod protocol;
mod proxies;
mod refused;
mod saved;
mod serve;
mod simulators;
mod space;
mod storybook;

/// Run the native CLI using process arguments and standard streams.
///
/// # Errors
/// Returns failures for invalid requests, discovery or evaluator execution.
pub fn run() -> anyhow::Result<()> {
    cli::run()
}
