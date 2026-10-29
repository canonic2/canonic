//! Owns argument parsing for the Shield executable.

pub(crate) mod public;
pub use public::{Cli, PublicCommand};

use clap::{Args, Subcommand};
use std::path::PathBuf;

/// Registered transports and registry validation utilities.
#[derive(Subcommand)]
pub enum Command {
    /// Validate declarative pack or custom-rule documents.
    Validate(Selection),
    /// Check a pack's declarative decision fixtures without execution.
    TestPack(PackTest),
    /// Evaluate one normalized host operation from stdin.
    EnforceRequest(EnforceRequest),
    /// Translate a registered native host event into the core protocol.
    NativeHook(NativeHook),
    /// Launch an exact reviewed local stdio connection without exposing its settings.
    McpConnect(McpConnect),
    /// Run a supported native command against active policy.
    Run(Run),
    /// Print the history reader's verified, correlated event view for fixtures.
    HistoryEvents(HistoryEvents),
}

/// Read-only history event view of one enrolled project.
#[derive(Args)]
pub struct HistoryEvents {
    /// Owning enrolled project directory.
    #[arg(long)]
    pub project: PathBuf,
}

/// Bounded foreground native execution with fixed environment and no shell.
#[derive(Args)]
pub struct Run {
    /// Owning enrolled project and supplied attribution.
    #[command(flatten)]
    pub context: ReviewContext,
    /// Canonical cwd inside the enrolled project; defaults to its root.
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Deadline including preparation; cleanup has a separate short grace period.
    #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..=3_600_000))]
    pub timeout_ms: u64,
    /// Stored native request selector; only an existing exact operator grant can allow Ask.
    #[arg(long)]
    pub approval_request: Option<String>,
    /// Exact absolute executable and literal arguments; no PATH or interpreter lookup.
    #[arg(last = true, required = true, num_args = 1..)]
    pub argv: Vec<String>,
}

/// Explicit registration for the normalized enforcement handoff transport.
#[derive(Args)]
pub struct EnforceRequest {
    /// Owning enrolled project fixed by the trusted host launcher.
    #[arg(long)]
    pub project: PathBuf,
}

/// Project-bound native command hook; vendor data cannot choose project authority.
#[derive(Args)]
pub struct NativeHook {
    /// Project fixed by confirmed installation.
    #[arg(long)]
    pub project: PathBuf,
    /// Agent protocol fixed by confirmed installation.
    #[arg(long, value_parser = crate::agents::names())]
    pub agent: String,
    /// Event fixed by confirmed installation, including input-read failures.
    #[arg(long)]
    pub event: String,
}

#[derive(Args)]
pub struct McpConnect {
    #[arg(long)]
    pub project: PathBuf,
    #[arg(long)]
    pub token: String,
}

/// Registered project and optional caller attribution for local review.
#[derive(Args)]
pub struct ReviewContext {
    /// Owning enrolled project directory.
    #[arg(long, global = true, default_value = ".")]
    pub project: PathBuf,
    /// Optional agent label, recorded as caller attribution.
    #[arg(long, global = true)]
    pub agent: Option<String>,
    /// Optional host session ID supplied by the launcher.
    #[arg(long, global = true)]
    pub session_id: Option<String>,
    /// Optional host tool-call ID.
    #[arg(long, global = true)]
    pub tool_use_id: Option<String>,
}

/// Project MCP server; the enrolled project comes from `--project` or the working directory.
#[derive(Args)]
pub struct McpServe {
    /// Agent label, recorded as caller attribution.
    #[arg(long)]
    pub agent: Option<String>,
    /// Host session ID supplied by the launcher.
    #[arg(long)]
    pub session_id: Option<String>,
    /// Host tool-call ID.
    #[arg(long)]
    pub tool_use_id: Option<String>,
    /// Cooperative work deadline including queue and encoding.
    #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..=60_000))]
    pub request_timeout_ms: u64,
}

/// Registry fixture inputs for one candidate pack.
#[derive(Args)]
pub struct PackTest {
    /// Include matched rules, requirement facts and guidance in fixture results.
    #[arg(long)]
    pub details: bool,
    /// Local permission pack to validate and test.
    #[arg(long)]
    pub pack: PathBuf,
    /// Companion packs composed under the same fixture profile; repeat as needed.
    #[arg(long)]
    pub with_pack: Vec<PathBuf>,
    /// Versioned YAML or JSON decision fixtures.
    #[arg(long)]
    pub cases: PathBuf,
}

/// Explicit candidate documents, without activation or implicit discovery.
#[derive(Args)]
pub struct Selection {
    /// Local permission pack; repeat for multiple packs.
    #[arg(long)]
    pub pack: Vec<PathBuf>,
    /// Local project custom-rule document.
    #[arg(long)]
    pub rules: Option<PathBuf>,
}

#[cfg(test)]
mod properties {
    use super::*;
    use clap::Parser;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn unknown_commands_with_arbitrary_arguments_are_refused(
            name in "unsupported-[a-z]{1,32}",
            arguments in prop::collection::vec(any::<String>(), 0..16),
        ) {
            let mut argv = vec!["shield".to_owned(), name];
            argv.extend(arguments);
            let result = Cli::try_parse_from(argv);
            prop_assert!(matches!(result, Err(error) if error.kind() == clap::error::ErrorKind::InvalidSubcommand));
        }
    }
}
