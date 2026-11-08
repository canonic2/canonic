//! Owned native MCP registration, review, discovery and configuration codecs.
mod agents;
mod detection;
pub mod document;
mod inputs;
mod preflight;
pub mod storage;
mod workflow;

use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub use agents::{Format, catalog};
pub use detection::defaults_on_path as discovery_defaults;
pub use preflight::check as check_catalog;
pub use workflow::{
    AgentStatus, Plan, PreparationSession, human, prepare, prepare_with_policy, run,
    run_with_policy,
};

/// Authorization boundary selected by the embedding product.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Authorization {
    /// Terminal confirmation or explicit named-agent `yes`.
    #[default]
    ConfirmOrExplicitYes,
    /// Every applying operation requires terminal confirmation.
    TerminalOnly,
}

/// Product choices layered over shared registration mechanics.
#[derive(Clone, Copy, Debug, Default)]
pub struct SetupPolicy {
    pub authorization: Authorization,
    /// Set the project root as cwd only for adapters with a supported cwd field.
    pub project_directory: bool,
    /// Refuse Box and native agents together for install/repair; allow removal.
    pub exclusive_routes: bool,
    /// Payload-free trace observer. The returned guard lives through the bounded
    /// catalog check; the observer cannot replace execution or its validation.
    pub catalog_observer: Option<CatalogObserver>,
}

/// Metadata observer returning a guard retained through catalog execution.
pub type CatalogObserver = fn(&Path) -> Box<dyn std::any::Any>;

/// Host configuration destination; project is always the default.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    #[default]
    Project,
    User,
}

/// Common options; writes require terminal confirmation or explicit `yes`.
#[derive(Args, Clone, Debug, Default)]
pub struct Options {
    /// Agent to install or repair; repeat for multiple agents. Omit for a picker.
    #[arg(long = "agent")]
    pub agents: Vec<String>,
    /// Configure this project, or explicitly configure this OS user's agents.
    #[arg(long, value_enum, default_value = "project")]
    pub scope: Scope,
    /// Remove only the selected owned registrations.
    #[arg(long)]
    pub remove: bool,
    /// Start from current tool permission defaults instead of the saved selection.
    #[arg(long, conflicts_with = "remove")]
    pub reset_tool_permissions: bool,
    /// Inspect the selected changes without writes or confirmation (also with --json).
    #[arg(long)]
    pub dry_run: bool,
    /// Apply the selected changes without a terminal confirmation; requires --agent.
    #[arg(long, conflicts_with = "dry_run", requires = "agents")]
    pub yes: bool,
    /// Explicit tool approvals; `None` reuses configured approvals or current defaults.
    #[arg(skip)]
    pub approved: Option<Vec<String>>,
}

/// One MCP tool offered on the permissions page.
#[derive(Clone, Debug)]
pub struct Tool {
    pub name: String,
    pub description: String,
    /// Whether the permissions page marks it for approval without asking.
    pub approved: bool,
}

impl Tool {
    pub fn new(name: impl Into<String>, description: impl Into<String>, approved: bool) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            approved,
        }
    }
}

/// Product-owned launch context, with explicit roots for isolated tests.
#[derive(Clone, Debug)]
pub struct Context {
    pub product: String,
    pub program: PathBuf,
    pub arguments: Vec<String>,
    /// Explicit launch command override; otherwise project scope uses the
    /// product's PATH name and user scope uses the installed executable.
    pub launch_command: Option<String>,
    /// Arguments appended to the trusted executable's MCP launch for probing.
    pub check_arguments: Vec<String>,
    pub root: PathBuf,
    pub home: PathBuf,
    pub state: PathBuf,
    /// Explicit agent-home overrides captured at the process boundary.
    pub agent_homes: BTreeMap<String, PathBuf>,
    /// The product's MCP tools, for the permissions page and approval settings.
    pub tools: Vec<Tool>,
}

impl Context {
    /// Resolve native user configuration and the installed executable.
    ///
    /// # Errors
    /// Returns an error if the project path cannot be resolved, the native home
    /// or configuration directory is unavailable, or executable lookup fails.
    pub fn local(product: &str, root: &Path, arguments: Vec<String>) -> Result<Self> {
        Self::local_resolved(product, project_root(root)?, arguments)
    }

    /// Use an explicitly selected root without discovering a containing project.
    ///
    /// # Errors
    /// Returns an error when resolving the root, native user directories or executable fails.
    pub fn local_at_root(product: &str, root: &Path, arguments: Vec<String>) -> Result<Self> {
        Self::local_resolved(product, root.canonicalize()?, arguments)
    }

    fn local_resolved(product: &str, root: PathBuf, arguments: Vec<String>) -> Result<Self> {
        let native_home =
            dirs::home_dir().ok_or_else(|| Error::invalid("Home directory is unavailable"))?;
        let home = native_home.canonicalize()?;
        let native_config = dirs::config_dir()
            .ok_or_else(|| Error::invalid("User configuration directory is unavailable"))?;
        // Resolve an OS alias such as /var before validating owned descendants;
        // linked agent files and directories are still refused by storage.
        let config = native_config
            .strip_prefix(&native_home)
            .map_or_else(|_| native_config.clone(), |suffix| home.join(suffix));
        Ok(Self {
            product: product.into(),
            program: std::env::current_exe()?,
            arguments,
            launch_command: None,
            check_arguments: vec!["--check".into()],
            root,
            home,
            state: config.join("Canonic/agent-setup"),
            agent_homes: [
                ("codex", "CODEX_HOME"),
                ("kimi", "KIMI_CODE_HOME"),
                ("copilot-cli", "COPILOT_HOME"),
                ("cline-cli", "CLINE_MCP_SETTINGS_PATH"),
                ("xdg", "XDG_CONFIG_HOME"),
                ("roaming", "APPDATA"),
            ]
            .into_iter()
            .filter_map(|(agent, key)| {
                std::env::var_os(key).map(|value| (agent.into(), PathBuf::from(value)))
            })
            .collect(),
            tools: Vec::new(),
        })
    }

    /// Offer these tools on the permissions page.
    #[must_use]
    pub fn with_tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = tools;
        self
    }
}

/// Find the closest checkout/workspace boundary without invoking Git or Gate.
/// A Gate workspace's repository metadata entries (`.gate/repos/<id>`) hold a
/// `.git` but are not projects, so resolution continues to their workspace.
///
/// # Errors
/// Returns an error if `start` cannot be canonicalized.
pub fn project_root(start: &Path) -> Result<PathBuf> {
    let start = start.canonicalize()?;
    for directory in start.ancestors() {
        let repos = directory.parent();
        if repos.and_then(Path::file_name) == Some("repos".as_ref())
            && repos.and_then(Path::parent).and_then(Path::file_name) == Some(".gate".as_ref())
        {
            continue;
        }
        if directory.join(".git").exists() || directory.join("gate.yml").is_file() {
            return Ok(directory.to_path_buf());
        }
    }
    Ok(start)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    /// An unknown agent, or one this scope cannot configure automatically.
    #[error("{0}")]
    InvalidSelection(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    ConfirmationRequired(String),
    #[error("{0}")]
    CatalogCheck(String),
    #[error(
        "Setup stopped: {source}. Completed: {completed:?}. Pending: {pending:?}. Retry setup for the selected agents to repair pending changes; review externally edited entries manually."
    )]
    Partial {
        completed: Vec<String>,
        pending: Vec<String>,
        #[source]
        source: Box<Error>,
    },
    #[error("{0}")]
    Io(#[from] std::io::Error),
}
impl Error {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
    pub fn selection(message: impl Into<String>) -> Self {
        Self::InvalidSelection(message.into())
    }

    /// Stable classification for product error adapters.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Conflict(_) => "agent_setup_conflict",
            Self::ConfirmationRequired(_) => "agent_setup_confirmation_required",
            Self::CatalogCheck(_) => "agent_setup_catalog_check",
            Self::Partial { .. } => "agent_setup_partial",
            Self::Invalid(_) => "agent_setup",
            Self::InvalidSelection(_) => "agent_setup_invalid_selection",
            Self::Io(_) => "agent_setup_io",
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;
