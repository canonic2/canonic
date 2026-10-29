//! Project-owned proposals and observational history, shared by CLI and MCP.
mod approvals;
mod cache;
mod changes;
pub(crate) mod cli;
mod clock;
mod history;
mod inspection;
pub(crate) mod integrations;
pub(crate) mod mcp;
mod model;
pub(crate) mod observations;
pub(crate) mod operator;
mod packs;
pub(crate) mod path_access;
mod project;
mod publication;
pub(crate) mod read_approval;
mod registries;
pub(crate) mod run_approval;
pub(crate) mod runtime;
mod storage;
#[cfg(test)]
mod tests;

use crate::{configuration, policy};
#[cfg(target_os = "macos")]
pub(crate) use cli::context as cli_context;
pub(crate) use model::{Context, EventInput};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use storage::Store;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("cancelled; no change confirmed")]
    Cancelled,
    #[error("review storage I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Policy(#[from] policy::Error),
    #[error("dependency resolution refused: {}", dependency_refusal(.0))]
    Dependency(#[from] canonic_registry::resolution::Error),
    #[error("pack {pack} is required by {dependents:?}")]
    RequiredPack {
        pack: String,
        dependents: Vec<String>,
    },
    #[error("no release of {0} supports this Shield version")]
    UnsupportedReleases(String),
    #[error("invalid review input or state: {0}")]
    Invalid(&'static str),
    /// A reference names several packs; `alternatives` holds at most
    /// [`MAX_ALTERNATIVES`] of the `total` references that name exactly one.
    #[error("{reason}; use one of {}", alternatives_text(.alternatives, *.total))]
    Ambiguous {
        reason: &'static str,
        alternatives: Vec<String>,
        total: usize,
    },
    /// A rule names this machine's folders; the message names the path and
    /// the portable form.
    #[error("{0}")]
    NotPortable(String),
    #[error("review state is locked; retry after the owning writer finishes")]
    Locked,
    #[error("review object was not found")]
    Missing,
    #[error("proposal requires operator approval")]
    ApprovalRequired,
    #[error(
        "MCP connection settings or resolved defaults changed; review with shield policy review in a terminal, then run shield mcp setup"
    )]
    McpSetupRequired,
    #[error("active snapshot changed; reload and evaluate the request again")]
    StaleSnapshot,
    #[error("diagnostic candidate is unavailable in this connection")]
    CandidateUnavailable,
    #[error("cursor is unavailable or belongs to another query")]
    CursorUnavailable,
    #[error("connection capacity reached; reconnect to continue")]
    Overloaded,
    #[error("compiled policy unavailable; run shield cache prepare in an operator terminal")]
    CacheUnavailable,
    /// Live review capacity is full. The counts say how many objects wait for
    /// the operator and which bound was reached.
    #[error(
        "{} capacity reached: {pending} pending, {retained} retained, limit {limit}; {}",
        kind.noun(),
        kind.recovery()
    )]
    Capacity {
        kind: CapacityKind,
        pending: usize,
        retained: usize,
        limit: usize,
    },
}

/// The review store whose capacity was reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapacityKind {
    /// Pending policy proposals.
    Proposals,
    /// Policy proposals retained in every status. Terminal proposals are not
    /// reclaimed, so review does not free this bound.
    RetainedProposals,
    /// Text-read action requests of one consumer kind.
    ReadRequests,
    /// Checked-run action requests of one consumer kind.
    RunRequests,
}

impl CapacityKind {
    pub(crate) fn noun(self) -> &'static str {
        match self {
            Self::Proposals | Self::RetainedProposals => "policy proposal",
            Self::ReadRequests => "read request",
            Self::RunRequests => "run request",
        }
    }
    /// What the operator does to free capacity.
    pub(crate) fn recovery(self) -> &'static str {
        match self {
            Self::Proposals => {
                "review or reject pending proposals with shield policy review, then retry"
            }
            Self::RetainedProposals => {
                "retained proposal storage is full and cannot be cleared yet, so new proposals cannot be saved"
            }
            Self::ReadRequests | Self::RunRequests => {
                "review pending requests with shield request list or deny them with shield request deny, then retry"
            }
        }
    }
    /// The operator command that frees capacity, or `None` when no command can.
    pub(crate) fn operator_route(self) -> Option<&'static str> {
        match self {
            Self::Proposals => Some("shield policy review"),
            Self::RetainedProposals => None,
            Self::ReadRequests | Self::RunRequests => Some("shield request list"),
        }
    }
}

impl Error {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Io(_) => "io",
            Self::Policy(policy::Error::Interrupted) => "interrupted",
            Self::Policy(policy::Error::Limit)
            | Self::Dependency(canonic_registry::resolution::Error::Limit) => "resource-limit",
            Self::Dependency(_)
            | Self::Policy(_)
            | Self::Invalid(_)
            | Self::Ambiguous { .. }
            | Self::NotPortable(_)
            | Self::UnsupportedReleases(_)
            | Self::RequiredPack { .. } => "invalid-review",
            Self::Locked => "review-locked",
            Self::Missing => "not-found",
            Self::ApprovalRequired => "approval-required",
            Self::McpSetupRequired => "mcp_setup_required",
            Self::StaleSnapshot => "stale-snapshot",
            Self::CandidateUnavailable => "candidate_unavailable",
            Self::CursorUnavailable => "cursor_unavailable",
            Self::Overloaded => "overloaded",
            Self::CacheUnavailable => "cache-unavailable",
            Self::Capacity { .. } => "capacity-reached",
        }
    }
}

/// The most alternatives an ambiguity error lists.
pub(crate) const MAX_ALTERNATIVES: usize = 8;

impl Error {
    /// An ambiguity refusal listing the first [`MAX_ALTERNATIVES`] references.
    pub(crate) fn ambiguous(reason: &'static str, alternatives: Vec<String>) -> Self {
        let total = alternatives.len();
        Self::Ambiguous {
            reason,
            alternatives: alternatives.into_iter().take(MAX_ALTERNATIVES).collect(),
            total,
        }
    }
}

fn alternatives_text(alternatives: &[String], total: usize) -> String {
    let mut text = alternatives.join(", ");
    if total > alternatives.len() {
        use std::fmt::Write;
        let _ = write!(text, " and {} more", total - alternatives.len());
    }
    text
}

/// Resolver refusals in Shield's terms: dependency levels are protection profiles.
fn dependency_refusal(error: &canonic_registry::resolution::Error) -> String {
    match error {
        canonic_registry::resolution::Error::LevelConflict(instance) => format!(
            "explicit profile conflicts with dependency requirements: Instance {{ registry: {:?}, pack: \"{}\" }}",
            instance.registry, instance.pack
        ),
        other => other.to_string(),
    }
}

pub(crate) type Checkpoint<'a> = &'a dyn Fn() -> Result<(), policy::Error>;

#[cfg(test)]
pub(crate) fn initialize(
    project: &Path,
    candidates: BTreeMap<String, Value>,
) -> Result<Value, Error> {
    let snapshots = snapshots(&candidates)?;
    let evaluator = configuration::Evaluator::new(snapshots)?;
    Store::initialize(project, candidates, evaluator.revision())
}

fn snapshots(
    candidates: &BTreeMap<String, Value>,
) -> Result<Vec<(policy::Scope, configuration::Snapshot)>, Error> {
    Ok(vec![(
        policy::Scope::Project,
        configuration::Bundle::snapshot_from_json(&project_bundle_bytes(candidates)?)?,
    )])
}

/// Codex prefixes a candidate adds or removes against the current policy, or
/// None when they are unchanged. A new prefix lets Codex run matching commands
/// outside its sandbox without asking, so every review shows these.
pub(crate) fn codex_prefix_changes(
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
) -> Result<Option<Value>, Error> {
    let prefixes = |candidates| -> Result<std::collections::BTreeSet<_>, Error> {
        let evaluator = configuration::Evaluator::new(snapshots(candidates)?)?;
        Ok(evaluator.codex_prefixes().iter().cloned().collect())
    };
    // A current policy that no longer prepares has no prefixes to keep.
    let old = prefixes(before).unwrap_or_default();
    let new = prefixes(after)?;
    let added: Vec<_> = new.difference(&old).collect();
    let removed: Vec<_> = old.difference(&new).collect();
    Ok(
        (!added.is_empty() || !removed.is_empty())
            .then(|| json!({"added":added,"removed":removed})),
    )
}

/// Review lines for project files a change rewrites in full or whose comments
/// it drops, or None when every file keeps its formatting.
pub(crate) fn formatting_changes(before: &Value, after: &Value) -> Option<Value> {
    let local_rules = configuration::layers::merged(after)
        .ok()
        .and_then(|config| config["local_rules"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "rules.local.yml".into());
    let lines: Vec<String> = [
        ("config.yml", "config"),
        ("config.local.yml", "local_config"),
        ("rules.yml", "rules"),
        (local_rules.as_str(), "local_rules"),
    ]
    .into_iter()
    .filter_map(|(name, field)| {
        crate::policy_files::notice(name, before[field].as_str(), after[field].as_str())
    })
    .collect();
    (!lines.is_empty()).then(|| json!(lines))
}

fn project_bundle(candidates: &BTreeMap<String, Value>) -> Result<configuration::Bundle, Error> {
    Ok(configuration::Bundle::from_json(&project_bundle_bytes(
        candidates,
    )?)?)
}

fn project_bundle_bytes(candidates: &BTreeMap<String, Value>) -> Result<Vec<u8>, Error> {
    let candidate = candidates
        .get("project")
        .filter(|_| candidates.len() == 1)
        .ok_or(Error::Invalid("project candidate is required"))?;
    serde_json::to_vec(candidate).map_err(|_| Error::Invalid("candidate encoding"))
}

pub(crate) fn open(project: &Path) -> Result<Context, Error> {
    let store = Store::open(project)?;
    let state = store.read_checkpoint()?;
    Ok(Context::new(state.project_id, state.project_root, "cli"))
}

/// The history reader's event view: verified records with reader-derived
/// completion links, duplicate deliveries dropped and retention applied.
/// `shield history list` joins this same view into request entries.
pub(crate) fn history_events(project: &Path) -> Result<Value, Error> {
    let context = open(project)?;
    let state = Store::open(project)?.read_history()?;
    state.check(&context)?;
    Ok(
        json!({"schema_version":1,"available":state.history_available,"evicted":state.history_evicted,"events":state.events}),
    )
}

pub(crate) fn compiled_cache_root(project: &Path) -> Result<Option<std::path::PathBuf>, Error> {
    let state = Store::open(project)?.read_checkpoint()?;
    Ok(state.active.map(|active| active.compiled.directory))
}

pub(crate) fn inspect(context: &Context) -> Result<Value, Error> {
    let state = Store::open(Path::new(&context.project_root))?.read()?;
    state.check(context)?;
    let bundle = project_bundle(&state.candidates)?;
    let evaluator =
        configuration::Evaluator::new(vec![(policy::Scope::Project, bundle.snapshot()?)])?;
    let rules = match bundle.rules_source() {
        Some(source) => inspection::project_rules(source, &context.project_root)?,
        None => {
            json!({"schema_version":1,"rules":[],"kind":"custom","capabilities":["structured-rules-v1"]})
        }
    };
    let mut value = json!({"schema_version":1,"mode":"local-review","enforcement":"not-installed","project_id":state.project_id,
        "project_root":state.project_root,"policy_revision":state.policy_revision,"effective":evaluator.effective(),"project_rules":rules});
    let redacted = model::redact(&mut value);
    value["redacted"] = json!(redacted);
    Ok(value)
}

pub(crate) fn proposals(context: &Context, id: Option<&str>, reveal: bool) -> Result<Value, Error> {
    let store = Store::open(Path::new(&context.project_root))?;
    let mut state = if reveal {
        store.read_unprepared()?
    } else {
        store.read_checkpoint()?
    };
    state.check(context)?;
    if let Some(id) = id {
        let proposal = state
            .proposals
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(Error::Missing)?;
        // The checkpoint keeps candidates as retained-source references. The
        // approval arguments bind and describe the captured candidate itself,
        // exactly as the proposal reply and apply do.
        proposal.candidate = store.captured_candidate(&proposal.candidate)?;
        proposal.base_candidate = store.captured_candidate(&proposal.base_candidate)?;
        let mut result = proposal.public(reveal);
        result["authorization_events"] = json!(
            state
                .authorization_events
                .iter()
                .filter(|event| event["links"]["proposal_id"] == id)
                .collect::<Vec<_>>()
        );
        return Ok(result);
    }
    Ok(
        json!({"schema_version":1,"project_id":state.project_id,"proposals":state.proposals.iter().map(model::Proposal::summary).collect::<Vec<_>>()}),
    )
}

pub(crate) fn transition(
    context: &Context,
    id: &str,
    action: &str,
    reason: Option<&str>,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    if reason.is_some_and(|reason| reason.len() > 4096) {
        return Err(policy::Error::Limit.into());
    }
    let store = Store::open(Path::new(&context.project_root))?;
    if !matches!(action, "reject" | "withdraw") {
        return Err(Error::Invalid("invalid proposal transition"));
    }
    if action == "reject" && context.surface != "cli" {
        return Err(Error::ApprovalRequired);
    }
    store.update(context, checkpoint, |state| {
        let index = state.proposals.iter().position(|p| p.id == id).ok_or(Error::Missing)?;
        let proposal = &state.proposals[index];
        if action == "withdraw" && context.surface == "mcp" {
            if proposal.retry_owner != context.retry_owner() { return Err(Error::ApprovalRequired); }
            if proposal.status == "withdrawn" { return Ok(proposal.public(false)); }
        }
        if proposal.status != "pending" { return Err(Error::Invalid("proposal is terminal or stale")); }
        if proposal.policy_revision != state.policy_revision {
            state.proposals[index].status = "stale".into();
            let event = state.event("proposal_stale", context, json!({"proposal_id":id}), json!({}), json!({"policy_revision":state.policy_revision}));
            state.record(event);
            return Ok(json!({"schema_version":1,"error":{"code":"stale-proposal"},"proposal":state.proposals[index].public(false)}));
        }
        let (status, kind) = if action == "reject" {
            ("rejected", "proposal_rejected")
        } else {
            ("withdrawn", "proposal_withdrawn")
        };
        state.proposals[index].status = status.into();
        let operator = (action == "reject").then(|| json!({"operator_id":null,"display_name":null,"mechanism":"operator-cli"}));
        state.proposals[index].operator.clone_from(&operator);
        let mut event = state.event(kind, context, json!({"proposal_id":id}), json!({"operator_reason":reason}),
            json!({"policy_revision":state.policy_revision}));
        event["operator"] = json!(operator);
        state.record(event);
        Ok(state.proposals[index].public(false))
    })
}

pub(crate) fn history_settings(
    context: &Context,
    enabled: Option<bool>,
    limit: Option<usize>,
    purge: bool,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    if limit.is_some_and(|v| !(1..=10000).contains(&v)) {
        return Err(Error::Invalid("history limit must be 1..10000"));
    }
    Store::open(Path::new(&context.project_root))?.update_checkpoint(context, checkpoint, |state| {
        if let Some(enabled) = enabled { state.history_enabled = enabled; }
        if let Some(limit) = limit { state.history_limit = limit; }
        if purge { state.events.clear(); }
        state.trim();
        state.history_control = true;
        if purge || limit.is_some() {
            // A new generation prevents stale writers and late callbacks from reviving pruned data.
            state.history_generation = storage::history::generation()?;
            state.history_rewrite = Some(purge);
        }
        Ok(json!({"schema_version":1,"enabled":state.history_enabled,"limit":state.history_limit,"purged":purge}))
    })
}
