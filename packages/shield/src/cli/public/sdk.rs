//! Explicit operator registration, separate from native agent setup.
use clap::{Args, Subcommand};

#[derive(Args)]
/// Operator registration for separately reviewed SDK mappings.
pub struct Sdk {
    /// SDK registration operation.
    #[command(subcommand)]
    pub command: SdkCommand,
}

#[derive(Subcommand)]
/// Explicit SDK registration operations.
pub enum SdkCommand {
    /// Review the exact binary and fixed read-only adapter mapping.
    Setup {
        /// Register the separate general-runtime Claude read-only consumer.
        #[arg(long, value_parser = ["claude-read-only"])]
        adapter: String,
    },
}
