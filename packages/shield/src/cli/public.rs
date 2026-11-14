//! Canonical operator command families; transports are hidden integration plumbing.
use crate::policy::Profile;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
mod sdk;
pub(crate) use sdk::{Sdk, SdkCommand};

/// Shield's project policy and approval interface.
#[derive(Parser)]
#[command(
    name = "shield",
    version,
    about = "Project policy and approval for AI agents",
    arg_required_else_help = true,
    after_help = "Start a project:  shield init\nConfigure agents: shield mcp setup\nInspect policy:   shield status  /  shield policy show  /  shield rule list\nFind packs:       shield pack search <query>  /  shield pack info <reference>\nReview requests:  shield request list  /  shield request approve <request>\nStructured data:  add --json to inspection commands\nTerminal color:   NO_COLOR disables color"
)]
pub struct Cli {
    /// Select a project; otherwise discover the nearest enrolled ancestor.
    #[arg(long, global = true)]
    pub project: Option<PathBuf>,
    /// Print one JSON result; commands that change policy refuse JSON mode.
    #[arg(long, global = true)]
    pub json: bool,
    /// Write private per-process diagnostics in this directory (also `SHIELD_DEBUG_LOG_DIR`).
    #[arg(long, global = true, conflicts_with = "no_debug_log")]
    pub debug_log_dir: Option<PathBuf>,
    /// Disable diagnostic logging, including automatic debug-build logs.
    #[arg(long, global = true)]
    pub no_debug_log: bool,
    /// Folder for this project's compiled policy plans, chosen at init or MCP setup.
    #[arg(long, global = true)]
    pub cache_dir: Option<PathBuf>,
    /// Shared configuration file; otherwise .shield/config.yml, then .config/shield/config.yml.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Operation to perform.
    #[command(subcommand)]
    pub command: PublicCommand,
}

/// Everyday operations, with one confirmation per mutation.
#[derive(Subcommand)]
pub enum PublicCommand {
    /// Create this project's Shield policy and optionally set up agents, after confirmation.
    Init {
        /// Agent to set up with project hooks and MCP, or box for Box; repeat for several. Omission asks.
        #[arg(long = "agent", value_parser = integration_names())]
        agents: Vec<String>,
        /// Set up no agents now.
        #[arg(long, conflicts_with = "agents")]
        no_setup: bool,
        /// Skip the default baseline addition; preserve configured packs and rules.
        #[arg(long, conflicts_with = "baseline_profile")]
        no_baseline: bool,
        /// Profile for the default baseline; omission offers Recommended during onboarding.
        #[arg(long = "profile", value_enum, value_name = "PROFILE")]
        baseline_profile: Option<Profile>,
    },
    /// Show policy, integrations and pending requests.
    Status,
    /// Resume this project's native Shield checks after confirmation.
    Enable,
    /// Pause this project's native Shield checks after confirmation, preserving policy and setup.
    Disable,
    /// Check configuration and installation prerequisites without changing policy.
    Doctor,
    /// Read and search the documentation bundled with this version, without enrollment.
    Docs(Docs),
    /// Set up agent hooks and MCP, or serve Shield's MCP tools.
    Mcp(Mcp),
    /// Register an explicitly reviewed read-only SDK mapping.
    Sdk(Sdk),
    /// Change whether agents may read or change protected settings and policy files.
    Protection(Protection),
    /// Show policy or review pending changes and history settings.
    Policy(Policy),
    /// Install, update, configure, remove, find or inspect permission packs.
    Pack(Pack),
    /// List, add, remove or choose registry sources.
    Registry(Registry),
    /// List, add, edit or remove project custom rules.
    Rule(Rule),
    /// List, inspect, approve or deny stored agent requests.
    Request(Request),
    /// Inspect or revoke temporary conversation grants, or approve a retained candidate.
    SessionGrant(SessionGrant),
    /// List or inspect agent request decisions and outcomes.
    History(History),
    /// Registered integration transports; not an operator workflow.
    #[command(name = "__internal", hide = true, subcommand)]
    Internal(Box<super::Command>),
}

impl PublicCommand {
    /// The MCP server arguments when this command serves MCP.
    #[must_use]
    pub fn mcp_serve(&self) -> Option<&super::McpServe> {
        match self {
            Self::Mcp(Mcp {
                command: McpCommand::Serve(input),
            }) => Some(input),
            Self::Internal(command) => match command.as_ref() {
                super::Command::Mcp(input) => Some(input),
                _ => None,
            },
            _ => None,
        }
    }
}

/// Reusable, private authorization for one native agent conversation.
#[derive(Args)]
pub struct SessionGrant {
    /// Session-grant operation.
    #[command(subcommand)]
    pub command: SessionGrantCommand,
}

/// Inspection is read-only; mutations require an operator terminal.
#[derive(Subcommand)]
pub enum SessionGrantCommand {
    /// Prepare an inactive conversation-grant candidate from a JSON object.
    Propose { input: PathBuf },
    /// List this project's retained grants and pending candidates.
    List {
        /// Select one hook-issued session reference.
        #[arg(long)]
        session_ref: Option<String>,
        /// Continue the selected immutable page.
        #[arg(long)]
        cursor: Option<String>,
        /// Maximum rows, from 1 to 64.
        #[arg(long, default_value_t = 16)]
        limit: u64,
    },
    /// Inspect a proposal or grant.
    Show {
        /// Retained proposal or grant ID.
        id: String,
    },
    /// Review and confirm a retained session-grant candidate.
    Approve {
        /// Retained proposal ID.
        proposal_id: String,
    },
    /// Revoke a grant or withdraw a pending candidate.
    Revoke {
        /// Retained proposal or grant ID.
        id: String,
    },
}

/// Where an integration or configuration applies.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Scope {
    /// This project.
    Project,
    /// Every project of the current user.
    User,
}

/// Agent integration and the project MCP server.
#[derive(Args)]
pub struct Mcp {
    /// MCP operation.
    #[command(subcommand)]
    pub command: McpCommand,
}

/// Shield's own MCP server and its agent registration.
#[derive(Subcommand)]
pub enum McpCommand {
    /// Install, repair or with --remove uninstall project hooks and Shield MCP, after confirmation.
    Setup(Setup),
    /// Serve this project's Shield MCP tools on standard input and output.
    Serve(super::McpServe),
}

/// Agent integration selection.
#[derive(Args)]
pub struct Setup {
    /// Review conflicting pending setup or cancel that interrupted publication.
    #[arg(long, value_enum, conflicts_with_all = ["agents", "remove", "reset_tool_permissions"])]
    pub pending: Option<PendingSetup>,
    /// Agent to set up or remove, or box for a Box MCP definition; repeat for several. Omission asks.
    #[arg(long = "agent", value_parser = integration_names())]
    pub agents: Vec<String>,
    /// Shield integrations are project scoped; user scope is refused.
    #[arg(long, value_enum, default_value = "project")]
    pub scope: Scope,
    /// Remove the selected agents' hooks, MCP server and permission settings.
    #[arg(long)]
    pub remove: bool,
    /// Start from current tool permission defaults instead of the saved selection.
    #[arg(long, conflicts_with = "remove")]
    pub reset_tool_permissions: bool,
    /// Show the review without writing anything.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum PendingSetup {
    Review,
    Cancel,
}

/// The integrations `--agent` accepts: Box first, then the native agent adapters.
pub(crate) fn integration_names() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(
        std::iter::once(crate::review::integrations::box_definition::ID)
            .chain(crate::agents::ADAPTERS.iter().map(|a| a.spec().id)),
    )
}

pub(crate) use crate::review::integrations::IntegrationChoice;

impl Setup {
    /// The plan selection: explicit agents, removal, or `None` for the guided flow.
    pub(crate) fn choice(&self) -> Result<Option<IntegrationChoice>, crate::review::Error> {
        if self.scope != Scope::Project {
            return Err(crate::review::Error::Invalid(
                "Shield integrations are project scoped; user scope is not supported",
            ));
        }
        let bits = IntegrationChoice::bits(&self.agents)?;
        Ok(if self.remove {
            Some(IntegrationChoice::RemoveAgents(bits))
        } else if bits == 0 {
            None
        } else {
            Some(IntegrationChoice::Agents(bits))
        })
    }
}

/// Protected file access settings.
#[derive(Args)]
pub struct Protection {
    /// Protection operation.
    #[command(subcommand)]
    pub command: ProtectionCommand,
}

/// Protected file access operations.
#[derive(Subcommand)]
pub enum ProtectionCommand {
    /// Choose agent access to protected settings and policy files, after confirmation.
    Set,
}

/// Project policy inspection and review.
#[derive(Args)]
pub struct Policy {
    /// Policy operation.
    #[command(subcommand)]
    pub command: PolicyCommand,
}

/// Project policy operations.
#[derive(Subcommand)]
pub enum PolicyCommand {
    /// Show the policy, live reviews and history settings.
    Show,
    /// Review a live change or history settings in a terminal.
    Review,
    /// Reject a live proposal after confirmation.
    Reject {
        proposal_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Preview a normalized request from a JSON input/context object; never executes it.
    Evaluate {
        input: PathBuf,
        /// The current policy by default; select a proposal's instead.
        #[arg(long)]
        proposal_id: Option<String>,
    },
    /// Compare the current policy with a proposal's, or a proposal's two sides, without changing anything.
    Compare {
        /// active, proposal:ID, or proposal:ID:before.
        #[arg(long, default_value = "active")]
        before: String,
        /// active, proposal:ID, or proposal:ID:before.
        #[arg(long)]
        after: String,
        #[arg(long, default_value = "literal_argv", value_parser = ["sample", "literal_argv", "supported_semantic"])]
        domain: String,
        /// JSON input/context object, required for the sample domain.
        #[arg(long)]
        input: Option<PathBuf>,
    },
}

/// Embedded documentation, independent of project enrollment.
#[derive(Args)]
pub struct Docs {
    #[command(subcommand)]
    pub command: DocsCommand,
}

/// The same selectors and bounds as the documentation MCP tools.
#[derive(Subcommand)]
pub enum DocsCommand {
    /// List bundled page paths, titles and line counts.
    List {
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(long, default_value_t = 50)]
        limit: u64,
    },
    /// Search for case-insensitive literal text.
    Search {
        query: String,
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(long, default_value_t = 50)]
        limit: u64,
    },
    /// Read a page's lines.
    Read {
        path: String,
        #[arg(long, default_value_t = 1)]
        start_line: u64,
        #[arg(long, default_value_t = 100)]
        limit: u64,
    },
}

/// Permission pack operations.
#[derive(Args)]
pub struct Pack {
    /// Pack operation.
    #[command(subcommand)]
    pub command: PackCommand,
}

/// Installed and available permission packs.
#[derive(Subcommand)]
pub enum PackCommand {
    /// List installed packs.
    List,
    /// Show an installed selection, its rules and source.
    Show { reference: String },
    /// Install a permission pack or change its profile, after confirmation.
    Add {
        /// Pack name, registry/name, publisher/name, or exact @version.
        reference: String,
        /// Select a profile; otherwise preserve the selection or ask, preselecting recommended.
        #[arg(long, value_enum)]
        profile: Option<Profile>,
        /// Bind a pack variable instead of the guided walk; repeatable.
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
        /// Add the pack to your uncommitted local configuration only.
        #[arg(long)]
        local: bool,
    },
    /// Update one selection or all tracking selections, after confirmation.
    Update { reference: Option<String> },
    /// Remove an installed selection after reviewing its effects.
    Remove {
        reference: String,
        /// Disable shared selections or remove local selections in your local configuration.
        #[arg(long)]
        local: bool,
    },
    /// Change a selection's variable values and rule overrides, after confirmation.
    Configure {
        /// Installed pack name or `pack:<uuid>` selector.
        reference: String,
        /// Bind a variable; a value starting with [ is a JSON list. Repeatable.
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
        /// Remove a variable value. Repeatable.
        #[arg(long = "unset", value_name = "NAME")]
        unset: Vec<String>,
        /// Override a rule: `off`, `allow`, `ask`, `deny`, `on_unmet:ask` or `on_unmet:deny`. Repeatable.
        #[arg(long = "override", value_name = "ID=VALUE")]
        overrides: Vec<String>,
        /// Remove a rule override. Repeatable.
        #[arg(long = "clear-override", value_name = "ID")]
        clear: Vec<String>,
        /// Change your uncommitted local configuration instead of the shared one.
        #[arg(long)]
        local: bool,
    },
    /// Search the default or named source without installing policy.
    Search { query: String },
    /// Show a pack's available releases and profiles from its registry.
    Info { reference: String },
}

/// Registry source arguments.
#[derive(Args)]
pub struct Registry {
    /// Affect this project's sources or the user's sources for every project.
    #[arg(long, value_enum, global = true, default_value = "project")]
    pub scope: Scope,
    /// Source operation.
    #[command(subcommand)]
    pub command: RegistryCommand,
}

/// Registry source operations.
#[derive(Subcommand)]
pub enum RegistryCommand {
    /// Review delegated root roles and signed revocations for a custom HTTPS source.
    Trust { name: String },
    /// List effective registry sources.
    List,
    /// Enroll a local directory or signed HTTPS registry, after confirmation.
    Add { name: String, location: String },
    /// Remove a registration, preserving installed policy, after confirmation.
    Remove { name: String },
    /// Choose the source for future unqualified search and add, after confirmation.
    Default { name: String },
}

/// Project custom rules.
#[derive(Args)]
pub struct Rule {
    /// Rule operation.
    #[command(subcommand)]
    pub command: RuleCommand,
}

/// Project custom rule operations.
#[derive(Subcommand)]
pub enum RuleCommand {
    /// List pack and custom rules.
    List,
    /// Create a project rule through guided input, after confirmation.
    Add {
        #[arg(long)]
        local: bool,
    },
    /// Edit the selected project rule through guided input, after confirmation.
    Edit {
        id: String,
        #[arg(long)]
        local: bool,
    },
    /// Remove a selected project custom rule, after confirmation.
    Remove {
        id: String,
        #[arg(long)]
        local: bool,
    },
}

/// Stored agent requests that wait for, or hold, an operator decision.
#[derive(Args)]
pub struct Request {
    /// Request operation.
    #[command(subcommand)]
    pub command: RequestCommand,
}

/// Agent request operations.
#[derive(Subcommand)]
pub enum RequestCommand {
    /// List pending and approved requests.
    List,
    /// Explain a stored request.
    Show { request: String },
    /// Authorize one stored Ask request once, after confirmation.
    Approve { request: String },
    /// Deny a pending request or revoke an unused grant, after confirmation.
    Deny { request: String },
}

/// Request history.
#[derive(Args)]
pub struct History {
    /// History operation.
    #[command(subcommand)]
    pub command: HistoryCommand,
}

/// Request history operations.
#[derive(Subcommand)]
pub enum HistoryCommand {
    /// Review changes to history collection, retention or clearing.
    Configure {
        #[arg(long)]
        enabled: Option<bool>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        purge: bool,
    },
    /// List request decisions, newest first.
    List {
        /// Show requests denied by policy or an operator.
        #[arg(long)]
        denied: bool,
        /// Show requests explicitly approved by an operator.
        #[arg(long)]
        approved: bool,
        /// Filter the requesting agent.
        #[arg(long)]
        agent: Option<String>,
        /// Show requests authorized by a reusable session grant.
        #[arg(long, value_parser=["session_grant"])]
        authorization_kind: Option<String>,
        /// Filter uses of one reusable grant.
        #[arg(long)]
        grant_id: Option<String>,
    },
    /// Explain the request and its decision/approval/completion observations.
    Show { entry: String },
}

#[cfg(test)]
mod mcp_setup_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn sdk_setup_requires_the_fixed_adapter_and_has_no_confirmation_bypass() {
        let cli = Cli::try_parse_from(["shield", "sdk", "setup", "--adapter", "claude-read-only"])
            .unwrap();
        assert!(matches!(cli.command, PublicCommand::Sdk(Sdk {
            command: SdkCommand::Setup { adapter }
        }) if adapter == "claude-read-only"));
        for args in [
            vec!["shield", "sdk", "setup"],
            vec!["shield", "sdk", "setup", "--adapter", "claude"],
            vec![
                "shield",
                "sdk",
                "setup",
                "--adapter",
                "claude-read-only",
                "--yes",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn pack_removal_accepts_the_local_layer_flag() {
        let cli =
            Cli::try_parse_from(["shield", "pack", "remove", "acme/tools", "--local"]).unwrap();
        assert!(matches!(
            cli.command,
            PublicCommand::Pack(Pack {
                command: PackCommand::Remove { local: true, .. }
            })
        ));
    }

    #[test]
    fn shield_mode_commands_select_a_project_without_auto_confirmation_flags() {
        for name in ["enable", "disable"] {
            assert!(Cli::try_parse_from(["shield", name]).is_ok());
            assert!(Cli::try_parse_from(["shield", name, "--project", "/path/to/acme"]).is_ok());
            assert!(Cli::try_parse_from(["shield", name, "--yes"]).is_err());
        }
    }

    #[test]
    fn session_grant_approval_has_one_cli_name() {
        assert!(Cli::try_parse_from(["shield", "session-grant", "approve", "acme"]).is_ok());
        assert!(Cli::try_parse_from(["shield", "session-grant", "apply", "acme"]).is_err());
    }

    fn setup(args: &[&str]) -> Setup {
        let cli = Cli::try_parse_from(["shield", "mcp", "setup"].iter().chain(args)).unwrap();
        let PublicCommand::Mcp(Mcp {
            command: McpCommand::Setup(setup),
        }) = cli.command
        else {
            panic!("Expected setup")
        };
        setup
    }

    #[test]
    fn reset_tool_permissions_flags_allow_preview_and_refuse_removal() {
        assert!(!setup(&[]).reset_tool_permissions);
        assert!(setup(&["--reset-tool-permissions", "--dry-run"]).reset_tool_permissions);
        assert!(
            Cli::try_parse_from([
                "shield",
                "mcp",
                "setup",
                "--reset-tool-permissions",
                "--remove"
            ])
            .is_err()
        );
    }

    #[test]
    fn setup_pending_flags_preserve_project_and_confirmation_scope() {
        assert!(matches!(
            setup(&["--pending", "review"]).pending,
            Some(PendingSetup::Review)
        ));
        assert!(matches!(
            setup(&["--pending", "cancel", "--dry-run"]).pending,
            Some(PendingSetup::Cancel)
        ));
        assert!(
            setup(&["--pending", "cancel", "--scope", "user"])
                .choice()
                .is_err()
        );
        for conflict in ["--remove", "--reset-tool-permissions"] {
            assert!(
                Cli::try_parse_from(["shield", "mcp", "setup", "--pending", "cancel", conflict])
                    .is_err()
            );
        }
        assert!(
            Cli::try_parse_from([
                "shield",
                "mcp",
                "setup",
                "--pending",
                "review",
                "--agent",
                "claude"
            ])
            .is_err()
        );
    }

    #[test]
    fn setup_flags_select_agents_removal_or_the_guided_flow() {
        let choice = setup(&["--agent", "codex", "--agent", "claude"])
            .choice()
            .unwrap()
            .unwrap();
        assert!(matches!(choice, IntegrationChoice::Agents(_)));
        for adapter in crate::agents::ADAPTERS {
            assert_eq!(
                choice.contains(adapter.spec().id),
                ["claude", "codex"].contains(&adapter.spec().id)
            );
        }
        assert!(!choice.contains("unregistered-agent"));
        assert!(setup(&[]).choice().unwrap().is_none());
        let removal = setup(&["--remove", "--agent", "codex"])
            .choice()
            .unwrap()
            .unwrap();
        assert!(matches!(removal, IntegrationChoice::RemoveAgents(_)));
        assert!(removal.contains("codex") && !removal.contains("claude"));
        assert!(matches!(
            setup(&["--remove"]).choice().unwrap(),
            Some(IntegrationChoice::RemoveAgents(0))
        ));
        assert!(setup(&["--scope", "user"]).choice().is_err());
        assert!(setup(&["--dry-run"]).dry_run);
        let boxed = setup(&["--agent", "box"]).choice().unwrap().unwrap();
        assert!(boxed.contains("box"));
        for adapter in crate::agents::ADAPTERS {
            assert!(!boxed.contains(adapter.spec().id));
            assert!(!choice.contains("box"));
        }
        let removal = setup(&["--remove", "--agent", "box"])
            .choice()
            .unwrap()
            .unwrap();
        assert!(removal.contains("box") && !removal.contains("claude"));
        // Box is an integration target, never a native hook protocol.
        assert!(
            Cli::try_parse_from([
                "shield",
                "__internal",
                "native-hook",
                "--project",
                "/srv/acme",
                "--agent",
                "box",
                "--event",
                "PreToolUse"
            ])
            .is_err()
        );
    }

    #[test]
    fn setup_and_init_accept_only_registered_agent_ids() {
        for args in [
            vec!["shield", "mcp", "setup", "--agent", "kimi"],
            vec!["shield", "mcp", "setup", "all"],
            vec!["shield", "init", "--agent", "kimi"],
            vec!["shield", "init", "--agent", "codex", "--no-setup"],
            vec!["shield", "setup"],
        ] {
            assert!(Cli::try_parse_from(&args).is_err(), "{args:?}");
        }
        let cli = Cli::try_parse_from(["shield", "init", "--agent", "codex", "--agent", "claude"])
            .unwrap();
        let PublicCommand::Init { agents, .. } = cli.command else {
            panic!("Expected init")
        };
        assert_eq!(agents, ["codex", "claude"]);
        assert!(Cli::try_parse_from(["shield", "init", "--no-setup"]).is_ok());
    }

    #[test]
    fn listings_are_noun_verb_commands_and_plural_nouns_are_refused() {
        for args in [
            ["pack", "list"],
            ["registry", "list"],
            ["rule", "list"],
            ["request", "list"],
            ["history", "list"],
            ["policy", "show"],
        ] {
            assert!(
                Cli::try_parse_from(["shield"].iter().chain(&args)).is_ok(),
                "{args:?}"
            );
        }
        for old in [
            "packs",
            "registries",
            "rules",
            "approvals",
            "approve",
            "deny",
            "add",
            "update",
            "remove",
            "search",
            "info",
        ] {
            assert!(
                Cli::try_parse_from(["shield", old, "acme"]).is_err(),
                "{old}"
            );
        }
        let cli = Cli::try_parse_from(["shield", "registry", "list", "--scope", "user"]).unwrap();
        let PublicCommand::Registry(registry) = cli.command else {
            panic!("Expected registry")
        };
        assert!(registry.scope == Scope::User);
    }
}
