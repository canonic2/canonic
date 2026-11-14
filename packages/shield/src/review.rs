//! Project-owned proposals and observational history, shared by CLI and MCP.
pub(crate) mod admin;
pub(crate) mod approvals;
mod authorization_receipts;
pub(crate) mod bootstrap;
mod cache;
mod changes;
pub(crate) mod cli;
mod clock;
mod files;
mod history;
pub(crate) mod hook_approval;
mod inspection;
pub(crate) mod integrations;
pub(crate) mod mcp;
pub(crate) mod mode;
mod model;
pub(crate) mod observations;
pub(crate) mod operator;
pub(crate) mod pack_updates;
mod packs;
pub(crate) mod path_access;
pub(crate) mod preparation_tasks;
pub(crate) mod progress;
mod project;
pub(crate) mod proposal_lifecycle;
mod publication;
pub(crate) mod read_approval;
mod registries;
pub(in crate::review) mod registry_trust;
pub(crate) mod reports;
pub(crate) mod run_approval;
pub(crate) mod runtime;
pub(crate) mod sdk_read;
pub(crate) mod session_grants;
mod settings;
mod storage;
#[cfg(test)]
mod tests;

use crate::{configuration, policy};
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
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
    Dependency(#[from] canonic_registry_kit::resolution::Error),
    #[error("pack {pack} is required by {dependents:?}")]
    RequiredPack {
        pack: String,
        dependents: Vec<String>,
    },
    #[error("no release of {0} supports this Shield version")]
    UnsupportedReleases(String),
    #[error("invalid review input or state: {0}")]
    Invalid(&'static str),
    #[error(
        "policy artifact {pack}@{version} from {source_identity} is known revoked; remove or update the affected pack"
    )]
    RegistryRevoked {
        source_identity: String,
        pack: String,
        version: String,
        digest: String,
    },
    #[error(
        "unsupported Shield checkpoint at {path:?}: observed schema {observed}; supported schema 1 with retained historical codecs. Original bytes are unchanged. Use a Shield version that supports this format; do not delete or re-enroll the project"
    )]
    StateFormat {
        path: std::path::PathBuf,
        observed: String,
    },
    #[error("corrupt review state at {path:?}; preserve it for recovery")]
    StateCorrupt { path: std::path::PathBuf },
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
    /// One request of an in-session approval group cannot be approved, so
    /// none is. `request` is a Shield-issued ID or the position of an unknown
    /// one; `recovery` is the next step for that reason.
    #[error(
        "request {request} cannot be approved together with the others: {reason}. Nothing was approved. {recovery}"
    )]
    Grouped {
        request: String,
        reason: &'static str,
        recovery: &'static str,
    },
    /// A validated group's form exceeds the display bound. The suggested
    /// prefix fits; the remaining IDs still need their own review.
    #[error(
        "these requests are too long to show together. Nothing was approved. Call request_approve with {{\"request_ids\":{retry:?}}}; remaining request_ids: {remaining:?}. Approve only requests still needed; every call requires confirmation"
    )]
    ApprovalFormTooLong {
        retry: Vec<String>,
        remaining: Vec<String>,
    },
    #[error("review state is locked; retry after the owning writer finishes")]
    Locked,
    #[error("review object was not found")]
    Missing,
    /// The connection's agent made the request, but the project records no
    /// integration for that agent, so this connection cannot ask the person.
    #[error("this agent is not connected to Shield in this project")]
    AgentNotConnected,
    #[error("proposal requires operator approval")]
    ApprovalRequired,
    #[error("policy or review state changed; reload and evaluate the request again")]
    StaleSnapshot,
    /// The folder's review state was recreated after this context was opened.
    #[error("this project's Shield state was recreated; reconnect Shield")]
    StateRecreated,
    #[error("diagnostic candidate is unavailable in this connection")]
    CandidateUnavailable,
    #[error("cursor is unavailable or belongs to another query")]
    CursorUnavailable,
    #[error("connection capacity reached; reconnect to continue")]
    Overloaded,
    #[error("compiled policy folder unusable; run shield doctor to find the cause")]
    CacheUnavailable,
    /// The project's policy files cannot be read, parsed or resolved, so there
    /// is no policy to apply. The message names the problem.
    #[error("the policy files in .shield/ are invalid: {0}")]
    PolicyFiles(String),
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
                "finish or withdraw your live review in this session, or wait for other live interactions to finish or expire"
            }
            Self::RetainedProposals => {
                "the retained outcome recovery window is full; retry after older closed outcomes leave their 24-hour recovery window"
            }
            Self::ReadRequests | Self::RunRequests => {
                "review pending requests with shield request list or deny them with shield request deny, then retry"
            }
        }
    }
    /// The operator command that frees capacity, or `None` when no command can.
    pub(crate) fn operator_route(self) -> Option<&'static str> {
        match self {
            Self::Proposals | Self::RetainedProposals => None,
            Self::ReadRequests | Self::RunRequests => Some("shield request list"),
        }
    }
}

impl Error {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::RegistryRevoked { .. } => "revoked-policy",
            Self::Io(_) => "io",
            Self::Policy(policy::Error::Interrupted) => "interrupted",
            Self::Policy(policy::Error::Limit)
            | Self::Dependency(canonic_registry_kit::resolution::Error::Limit) => "resource-limit",
            Self::Dependency(_)
            | Self::Policy(_)
            | Self::Invalid(_)
            | Self::StateFormat { .. }
            | Self::StateCorrupt { .. }
            | Self::Ambiguous { .. }
            | Self::NotPortable(_)
            | Self::Grouped { .. }
            | Self::ApprovalFormTooLong { .. }
            | Self::UnsupportedReleases(_)
            | Self::RequiredPack { .. } => "invalid-review",
            Self::Locked => "review-locked",
            Self::Missing => "not-found",
            Self::AgentNotConnected => "agent-not-connected",
            Self::ApprovalRequired => "approval-required",
            Self::StaleSnapshot => "stale-snapshot",
            Self::StateRecreated => "state-recreated",
            Self::CandidateUnavailable => "candidate_unavailable",
            Self::CursorUnavailable => "cursor_unavailable",
            Self::Overloaded => "overloaded",
            Self::CacheUnavailable => "cache-unavailable",
            Self::PolicyFiles(_) => "policy-files-invalid",
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
fn dependency_refusal(error: &canonic_registry_kit::resolution::Error) -> String {
    match error {
        canonic_registry_kit::resolution::Error::LevelConflict(instance) => format!(
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

/// The identity of a captured candidate: the digest of its canonical JSON.
pub(crate) fn candidate_identity(candidate: &Value) -> String {
    let _trace = crate::diagnostics::trace::span("source.identity");
    crate::diagnostics::trace::count("source.identity_encodings", 1);
    crate::json::fingerprint(candidate).expect("candidate JSON serializes")
}

/// The identity of a set of captured candidates.
pub(crate) fn map_digest(candidates: &BTreeMap<String, Value>) -> String {
    crate::json::fingerprint(candidates).expect("candidate JSON serializes")
}

/// Compare captured sources and retain the next plan for publication. Prefix
/// changes use those same plans, including an already validated base when supplied.
pub(in crate::review) fn compare_prepared(
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
    prepared_base: Option<&configuration::Evaluator>,
) -> Result<(Value, configuration::Evaluator), Error> {
    changes::historical::validate(&before["project"], &after["project"])?;
    let before = snapshots(before)?;
    let after = snapshots(after)?;
    let mut comparison = json!(configuration::compare_policy(&before, &after)?);
    let owned_base;
    let base_plan = if let Some(plan) = prepared_base {
        plan
    } else {
        owned_base = configuration::Evaluator::new(before)?;
        &owned_base
    };
    let next_plan = configuration::Evaluator::new(after)?;
    changes::compare_prefixes(base_plan, &next_plan, &mut comparison);
    Ok((comparison, next_plan))
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

fn project_bundle_bytes(candidates: &BTreeMap<String, Value>) -> Result<Vec<u8>, Error> {
    let candidate = candidates
        .get("project")
        .filter(|_| candidates.len() == 1)
        .ok_or(Error::Invalid("project candidate is required"))?;
    serde_json::to_vec(candidate).map_err(|_| Error::Invalid("candidate encoding"))
}

/// Remove a batch of this user's records earlier releases kept and this one
/// never reads. The CLI and MCP server call it; hooks never do.
pub(crate) fn retire_user_records() {
    storage::retired::retire_user_records();
}

pub(crate) fn open(project: &Path) -> Result<Context, Error> {
    let store = Store::open(project)?;
    let state = store.read_checkpoint()?;
    let context = Context::new(state.project_id.clone(), state.project_root.clone(), "cli");
    integrations::rebind_launchers(&context, &state);
    Ok(context)
}

pub(crate) fn open_diagnostics(project: &Path) -> Result<Context, Error> {
    let store = Store::inspect(project)?;
    let (state, _) = store.read_diagnostic_checkpoint()?;
    Ok(Context::new(
        state.project_id,
        store.root().to_owned(),
        "cli",
    ))
}

/// A connection needs a current checkout epoch before it can own operations.
/// Supported historical formats otherwise stay inspectable until actual use.
pub(crate) fn open_connection(project: &Path) -> Result<Context, Error> {
    let store = Store::open(project)?;
    let (state, _) = store.read_diagnostic_checkpoint()?;
    if state.project_root != store.root() {
        return open(project);
    }
    Ok(Context::new(state.project_id, state.project_root, "cli"))
}

/// A proposal's recorded status, closing reason, receipt and origin, read from
/// review state as diagnostics read it.
#[cfg(test)]
pub(crate) fn proposal_outcome(project: &Path, id: &str) -> Value {
    let (state, _) = Store::inspect(project)
        .and_then(|store| store.read_diagnostic_checkpoint())
        .expect("review state");
    let row = state
        .proposals
        .iter()
        .find(|row| row.id == id)
        .expect("proposal");
    json!({"proposal_id":row.id,"status":row.status,"closed_reason":row.closed_reason,
        "receipt":row.receipt,"operator":row.operator,"provenance":row.provenance})
}

#[cfg(test)]
pub(crate) fn update_operator_fixture(
    context: &Context,
    update: impl FnOnce(&mut Value),
) -> Result<Value, Error> {
    Store::open(Path::new(&context.project_root))?.update(context, &|| Ok(()), |state| {
        update(&mut state.operator);
        Ok(json!({}))
    })
}

/// Derive caller context and pause guards from one validated checkpoint.
/// Later authority handoff still performs its locked freshness check.
pub(crate) fn open_native(project: &Path) -> Result<(Context, Option<mode::Paused>), Error> {
    let store = Store::open(project)?;
    let state = store.read_checkpoint()?;
    let context = Context::new(state.project_id.clone(), state.project_root.clone(), "cli");
    integrations::rebind_launchers(&context, &state);
    Ok((context, mode::paused(&state)))
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

/// Where compiled plans of the policy files are cached in tests.
#[cfg(test)]
pub(crate) fn plan_cache_directory() -> std::path::PathBuf {
    files::plans(&cache::default_directory().expect("test cache directory"))
}

/// The cache folder the policy loader reads this project's plans from, which
/// native path guards protect.
pub(crate) fn compiled_cache_root(project: &Path) -> Result<Option<std::path::PathBuf>, Error> {
    let state = Store::open(project)?.read_checkpoint()?;
    files::cache_for(&state.operator).map(Some)
}

#[cfg(test)]
pub(crate) fn inspect(context: &Context) -> Result<Value, Error> {
    let (state, evaluator) =
        Store::open(Path::new(&context.project_root))?.read_current_evaluated()?;
    state.check(context)?;
    inspect_prepared(context, &state, &evaluator)
}

pub(in crate::review) fn inspect_prepared(
    context: &Context,
    state: &model::State,
    evaluator: &configuration::Evaluator,
) -> Result<Value, Error> {
    if evaluator.revision() != state.policy_revision {
        return Err(Error::StaleSnapshot);
    }
    inspect_metadata(context, state, evaluator.effective())
}

pub(in crate::review) fn inspect_recorded(
    context: &Context,
    state: &model::State,
) -> Result<Value, Error> {
    let candidate = state
        .candidates
        .get("project")
        .filter(|_| state.candidates.len() == 1)
        .ok_or(Error::Invalid("project candidate is required"))?;
    let effective = configuration::recorded_effective(candidate, &state.policy_revision)?;
    inspect_metadata(context, state, &effective)
}

fn inspect_metadata(
    _context: &Context,
    state: &model::State,
    effective: &configuration::Effective,
) -> Result<Value, Error> {
    // The caller bound the displayed source fields to the matching canonical
    // lock/revision; omitted source fields need not have been hydrated.
    // Rendering records never reconstructs a Snapshot or prepares matchers.
    let candidate = state
        .candidates
        .get("project")
        .filter(|_| state.candidates.len() == 1)
        .ok_or(Error::Invalid("project candidate is required"))?;
    let rules = match candidate["rules"].as_str() {
        Some(source) => policy::inspect_retained_custom(source)?,
        None => {
            json!({"schema_version":1,"rules":[],"kind":"custom","capabilities":["structured-rules-v1"]})
        }
    };
    let mut value = json!({"schema_version":1,"mode":"local-review","enforcement":"not-checked","project_id":state.project_id,
        "project_root":state.project_root,"policy_revision":state.policy_revision,"effective":effective,"project_rules":rules});
    let redacted = model::redact(&mut value);
    value["redacted"] = json!(redacted);
    Ok(value)
}

#[cfg(test)]
pub(crate) fn proposals(context: &Context, id: Option<&str>, reveal: bool) -> Result<Value, Error> {
    let store = Store::open(Path::new(&context.project_root))?;
    let mut state = store.read_checkpoint()?;
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
        store.hydrate_proposal(proposal)?;
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
    if action == "reject" {
        return admin::policy::Reject::new(context, id, reason)?.commit(context, checkpoint);
    }
    store.update_checkpoint(context, checkpoint, |state| {
        let index = state.proposals.iter().position(|p| p.id == id).ok_or(Error::Missing)?;
        store.hydrate_proposal(&mut state.proposals[index])?;
        let proposal = &state.proposals[index];
        if action == "withdraw" && context.surface == "mcp" {
            if !proposal_lifecycle::owns(proposal, context) { return Err(Error::ApprovalRequired); }
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
    admin::policy::History::new(context, enabled, limit, purge)?.commit(context, checkpoint)
}
