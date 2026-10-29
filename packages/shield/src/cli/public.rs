//! Canonical operator command families; transports are hidden integration plumbing.
use crate::policy::Profile;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

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
    /// Compiled cache location for init, MCP setup or cache preparation.
    #[arg(long, global = true)]
    pub cache_dir: Option<PathBuf>,
    /// Operation to perform.
    #[command(subcommand)]
    pub command: PublicCommand,
}

/// Everyday operations, with one confirmation per mutation.
#[derive(Subcommand)]
pub enum PublicCommand {
    /// Create this project's Shield policy and optionally set up agents, after confirmation.
    Init {
        /// Agent to set up with project hooks and MCP; repeat for several. Omission asks.
        #[arg(long = "agent", value_parser = crate::agents::names())]
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
    /// Check configuration and installation prerequisites; changes nothing.
    Doctor,
    /// Set up agent hooks and MCP, or serve Shield's MCP tools.
    Mcp(Mcp),
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
    /// List or inspect agent request decisions and outcomes.
    History(History),
    /// Prepare disposable compiled data from the currently committed policy.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
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
            _ => None,
        }
    }
}

/// Maintenance of derived policy data; never activates pending source edits.
#[derive(Subcommand)]
pub enum CacheCommand {
    /// Prepare or repair the active generation without a policy approval prompt.
    Prepare,
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
    /// Agent to set up or remove; repeat for several. Omission asks.
    #[arg(long = "agent", value_parser = crate::agents::names())]
    pub agents: Vec<String>,
    /// Shield integrations are project scoped; user scope is refused.
    #[arg(long, value_enum, default_value = "project")]
    pub scope: Scope,
    /// Remove the selected agents' hooks, MCP server and permission settings.
    #[arg(long)]
    pub remove: bool,
    /// Show the review without writing anything.
    #[arg(long)]
    pub dry_run: bool,
}

/// Plan-engine selection; no selection authorizes tool execution.
#[derive(Clone, Copy)]
pub(crate) enum IntegrationChoice {
    /// Preserve existing registration, resuming any confirmed repair plan.
    Keep,
    /// Install the agents in this set, indexed by the immutable adapter registry.
    Agents(u64),
    /// Remove selected adapters while preserving the others; zero opens selection.
    RemoveAgents(u64),
}

#[cfg(test)]
impl IntegrationChoice {
    /// Install one registered agent.
    pub(crate) fn agent(id: &str) -> Self {
        Self::Agents(Self::bits(&[id]).expect("registered agent"))
    }
    /// Install every registered agent.
    pub(crate) fn all() -> Self {
        Self::Agents(Self::every())
    }
    /// Remove every Shield-owned agent registration.
    pub(crate) fn remove_all() -> Self {
        Self::RemoveAgents(Self::every())
    }
    fn every() -> u64 {
        let ids: Vec<_> = crate::agents::ADAPTERS
            .iter()
            .map(|a| a.spec().id)
            .collect();
        Self::bits(&ids).expect("registered agents")
    }
}

impl IntegrationChoice {
    pub(crate) fn contains(self, agent: &str) -> bool {
        match self {
            Self::Agents(bits) | Self::RemoveAgents(bits) => crate::agents::ADAPTERS
                .iter()
                .position(|a| a.spec().id == agent)
                .is_some_and(|i| i < u64::BITS as usize && bits & (1 << i) != 0),
            Self::Keep => false,
        }
    }

    /// The registry-indexed set of `agents`.
    pub(crate) fn bits(agents: &[impl AsRef<str>]) -> Result<u64, crate::review::Error> {
        use crate::review::Error;
        let mut bits = 0u64;
        for id in agents {
            let index = crate::agents::ADAPTERS
                .iter()
                .position(|a| a.spec().id == id.as_ref())
                .ok_or(Error::Invalid("Shield supports Claude and Codex only"))?;
            bits |= 1u64
                .checked_shl(
                    u32::try_from(index).map_err(|_| Error::Invalid("too many agent adapters"))?,
                )
                .ok_or(Error::Invalid("too many agent adapters"))?;
        }
        Ok(bits)
    }
}

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
    /// Show active policy, pending proposals and history settings.
    Show,
    /// Review pending proposals, edited files, projection repair or history settings in a terminal.
    Review,
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
    Remove { reference: String },
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
    Add,
    /// Edit the selected project rule through guided input, after confirmation.
    Edit { id: String },
    /// Remove a selected project custom rule, after confirmation.
    Remove { id: String },
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
    },
    /// Explain the request and its decision/approval/completion observations.
    Show { entry: String },
}

#[cfg(test)]
mod mcp_setup_tests {
    use super::*;
    use clap::Parser;

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
    fn setup_flags_select_agents_removal_or_the_guided_flow() {
        let choice = setup(&["--agent", "codex", "--agent", "claude"])
            .choice()
            .unwrap()
            .unwrap();
        assert!(matches!(choice, IntegrationChoice::Agents(_)));
        for adapter in crate::agents::ADAPTERS {
            assert!(choice.contains(adapter.spec().id));
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
