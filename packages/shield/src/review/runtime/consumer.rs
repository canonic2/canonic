//! Owns prepared active snapshots, exact request evidence and locked revision handoff.
use super::{Context, Error, Store};
use crate::review::Checkpoint;
use crate::{
    configuration::{CandidateEvaluation, Evaluator},
    policy,
};
use serde::Serialize;
#[cfg(test)]
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

/// Failure to load, evaluate or revalidate local active policy evidence.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// Filesystem failure with its original cause.
    #[error("runtime storage I/O failed: {0}")]
    Io(#[source] std::io::Error),
    /// Shared policy validation/evaluation failure.
    #[error(transparent)]
    Policy(policy::Error),
    /// Invalid project, request or retained state.
    #[error("invalid runtime input or state: {0}")]
    Invalid(&'static str),
    /// An owning writer prevented bounded lock acquisition.
    #[error("runtime state is locked; retry after the owning writer finishes")]
    Locked,
    /// Local enrollment was not found.
    #[error("runtime enrollment was not found")]
    Missing,
    /// The evaluated activation is no longer current, including same-policy reactivation.
    #[error("active snapshot changed; reload and evaluate the request again")]
    StaleSnapshot,
    /// Derived policy is unavailable; operator preparation restores committed data.
    #[error("compiled policy unavailable; run shield cache prepare in an operator terminal")]
    CacheUnavailable,
}
impl From<Error> for RuntimeError {
    fn from(error: Error) -> Self {
        match error {
            Error::Cancelled => Self::Policy(crate::policy::Error::Interrupted),
            Error::Io(error) => Self::Io(error),
            Error::Policy(error) => Self::Policy(error),
            Error::Invalid(reason) | Error::Ambiguous { reason, .. } => Self::Invalid(reason),
            Error::NotPortable(_) => Self::Invalid("rule names a folder on this machine"),
            Error::Dependency(_) => Self::Invalid("dependency closure is invalid"),
            Error::RequiredPack { .. } => Self::Invalid("pack is required by another selection"),
            Error::UnsupportedReleases(_) => {
                Self::Invalid("no pack release supports this Shield version")
            }
            Error::Locked | Error::Overloaded => Self::Locked,
            Error::Capacity { .. } => Self::Policy(crate::policy::Error::Limit),
            Error::Missing => Self::Missing,
            Error::StaleSnapshot => Self::StaleSnapshot,
            Error::CacheUnavailable => Self::CacheUnavailable,
            Error::ApprovalRequired => Self::Invalid("operator approval is required"),
            Error::McpSetupRequired => Self::Invalid("MCP setup requires terminal review"),
            Error::CandidateUnavailable | Error::CursorUnavailable => {
                Self::Invalid("diagnostic view is unavailable")
            }
        }
    }
}

/// Immutable identity of one complete local activation, not an authenticated grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SnapshotIdentity {
    project_id: String,
    project_root: String,
    activation_id: String,
    previous_activation_id: Option<String>,
    policy_revision: String,
    candidate_digest: String,
    #[serde(skip)]
    binding: String,
    #[serde(skip)]
    compiled_digest: String,
}
impl SnapshotIdentity {
    /// Enrolled project identity, retained across candidate changes.
    #[must_use]
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    /// Canonical owning project directory; request cwd does not change it.
    #[must_use]
    pub fn project_root(&self) -> &Path {
        Path::new(&self.project_root)
    }
    /// Exact activation identity, including same-policy reactivation.
    #[must_use]
    pub fn activation_id(&self) -> &str {
        &self.activation_id
    }
    /// Effective retained policy revision.
    #[must_use]
    pub fn policy_revision(&self) -> &str {
        &self.policy_revision
    }
    /// Digest of every captured scope's source bytes.
    #[must_use]
    pub fn candidate_digest(&self) -> &str {
        &self.candidate_digest
    }
}

/// A registered local project handle; it never silently follows reenrollment.
pub struct Project {
    context: Context,
}
impl Project {
    /// Bind an explicit existing local review enrollment to its canonical root and ID.
    /// No activation, authenticated scope enrollment or host registration occurs.
    ///
    /// # Errors
    /// Fails for missing/invalid enrollment, corrupt aggregate state or filesystem errors.
    pub fn open(root: &Path) -> Result<Self, RuntimeError> {
        Ok(Self {
            context: crate::review::open(root)?,
        })
    }

    /// Load and prepare the latest complete active snapshot for repeated evaluation.
    /// Old snapshots remain immutable; candidate changes do not mutate them.
    ///
    /// # Errors
    /// Fails for inactive, changed project identity, invalid state or filesystem errors.
    pub fn snapshot(&self) -> Result<Snapshot, RuntimeError> {
        Ok(load(&self.context, &|| Ok(()))?)
    }

    /// Revalidate exact request evidence and hold the writer lock through a short callback.
    /// The callback receives the owned original request and its original evaluation.
    /// It must not wait for command completion or acquire another review/runtime writer lock.
    /// This revision guard does not authorize execution, consume an ask grant, resolve
    /// an executable, or block agent access to operator/state paths. Any decision may
    /// reach the callback; enforcement consumers still own allow/ask/deny handling.
    /// [`crate::enforcement::handoff`] adds decision handling and mandatory classified
    /// access restrictions for a trusted host that independently verifies operations.
    ///
    /// # Errors
    /// Refuses another project or a stale activation before invoking the callback.
    /// Invalid state, filesystem and bounded lock/release failures are reported.
    /// A lock release failure can occur after the callback has run; it cannot undo it.
    pub fn with_current<T>(
        &self,
        evaluated: &EvaluatedRequest,
        operation: impl FnOnce(&policy::Request, &CandidateEvaluation) -> T,
    ) -> Result<T, RuntimeError> {
        let identity = &evaluated.identity;
        if identity.project_id != self.context.project_id
            || identity.project_root != self.context.project_root
        {
            return Err(RuntimeError::Invalid(
                "request evidence belongs to another project",
            ));
        }
        Ok(
            Store::open(Path::new(&self.context.project_root))?.inspect_checkpoint(
                &self.context,
                &|| Ok(()),
                |state| {
                    let active = state.active.as_ref().ok_or(Error::StaleSnapshot)?;
                    if active.activation_id != identity.activation_id
                        || active.binding != identity.binding
                        || active.compiled.digest != identity.compiled_digest
                    {
                        return Err(Error::StaleSnapshot);
                    }
                    if evaluated.time_sensitive {
                        let current = if let Some(resource) = &evaluated.resource {
                            evaluated.evaluator.evaluate_resource(
                                &evaluated.request,
                                &resource.input,
                                &resource.context,
                                &|| Ok(()),
                            )?
                        } else {
                            evaluated
                                .evaluator
                                .evaluate_controlled(&evaluated.request, &|| Ok(()))?
                        };
                        if current.decision != evaluated.evaluation.decision {
                            return Err(Error::StaleSnapshot);
                        }
                    }
                    Ok(operation(&evaluated.request, &evaluated.evaluation))
                },
            )?,
        )
    }
}

/// An owned prepared evaluator over one independently captured local activation.
pub struct Snapshot {
    identity: SnapshotIdentity,
    evaluator: Arc<Evaluator>,
    working_files: Option<crate::location::WorkingFiles>,
}
/// Codex skips its sandbox only when every part of a command chain matches an
/// allow prefix. Its parts are the submitted script's outermost commands:
/// commands inside another command's span (a wrapper's target) or in an
/// embedded script (`bash -c`) are not parts. Operations sharing one span are
/// one part, covered when any of them is. Operations without an argv request
/// are skipped, which errs toward reporting coverage.
pub(crate) fn codex_covers(
    prefixes: &[policy::agent_outputs::CodexPrefix],
    operations: &serde_json::Value,
) -> bool {
    let span = |operation: &serde_json::Value| {
        Some((
            operation["span"]["start"].as_u64()?,
            operation["span"]["end"].as_u64()?,
        ))
    };
    let submitted: Vec<_> = operations
        .as_array()
        .into_iter()
        .flatten()
        .filter(|operation| operation["source_unit"] == 0)
        .filter_map(|operation| Some((span(operation)?, operation)))
        .collect();
    let mut parts = std::collections::BTreeMap::<(u64, u64), bool>::new();
    for &((start, end), operation) in &submitted {
        let nested = submitted
            .iter()
            .any(|&((s, e), _)| s <= start && end <= e && (s, e) != (start, end));
        let request = &operation["request"];
        let Some(executable) = request["executable"].as_str() else {
            continue;
        };
        if nested {
            continue;
        }
        let arguments: Vec<&str> = request["arguments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .collect();
        let covered = prefixes.iter().any(|p| p.covers(executable, &arguments));
        *parts.entry((start, end)).or_default() |= covered;
    }
    !parts.is_empty() && parts.values().all(|covered| *covered)
}

impl Snapshot {
    pub(crate) fn mcp_connections(&self) -> &crate::mcp_connections::Connections {
        self.evaluator.effective().mcp_connections()
    }
    pub(crate) fn mcp_connection(
        &self,
        agent: &str,
        alias: &str,
    ) -> Option<(&str, &crate::mcp_connections::Connection)> {
        self.evaluator
            .effective()
            .mcp_connections()
            .iter()
            .find(|(_, c)| c.enabled && c.agents.get(agent).is_some_and(|a| a == alias))
            .map(|(id, c)| (id.as_str(), c))
    }
    pub(crate) fn observe_working_files(&mut self, root: Option<&crate::location::WorkingFiles>) {
        self.working_files = root.cloned();
    }
    pub(crate) fn access_paths(&self) -> &crate::configuration::AccessPaths {
        self.evaluator.effective().access_paths()
    }
    pub(crate) fn time_sensitive(&self) -> bool {
        self.evaluator.time_sensitive()
    }
    /// Codex prefixes of this policy's active rules that allow.
    pub(crate) fn codex_prefixes(&self) -> &[policy::agent_outputs::CodexPrefix] {
        self.evaluator.codex_prefixes()
    }
    /// Whether Codex would run the analyzed command outside its sandbox
    /// without asking: every top-level command matches one of its prefixes.
    pub(crate) fn codex_covers(&self, operations: &serde_json::Value) -> bool {
        codex_covers(self.codex_prefixes(), operations)
    }

    pub(crate) fn analyze_shell(
        &self,
        source: &str,
        dialect: crate::shell::Dialect,
        cwd: &str,
        original: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, RuntimeError> {
        let input = policy::normalized::Input::Shell {
            source: source.into(),
            dialect: serde_json::json!(dialect)
                .as_str()
                .expect("dialect string")
                .into(),
            dialect_version: None,
            original_input: original.cloned().unwrap_or_else(|| serde_json::json!({})),
        };
        let context = policy::normalized::Context {
            cwd: cwd.into(),
            environment: None,
            working_files: self.working_files.clone(),
        };
        input
            .validate(&context, &self.identity.project_root)
            .map_err(RuntimeError::Policy)?;
        self.evaluator
            .evaluate_normalized(
                &input,
                &context,
                &self.identity.project_root,
                None,
                &crate::evidence::now(),
                &|| Ok(()),
            )
            .map_err(RuntimeError::Policy)
    }
    /// Identity of this retained activation; it does not assert that it is still current.
    #[must_use]
    pub fn identity(&self) -> &SnapshotIdentity {
        &self.identity
    }

    /// Evaluate and retain the exact literal request through the shared policy core.
    /// No files are reread; a newer activation requires a fresh project snapshot.
    /// This evidence alone is not execution permission.
    ///
    /// # Errors
    /// Rejects another owning project, malformed requests or shared evaluator failures.
    pub fn evaluate(&self, request: policy::Request) -> Result<EvaluatedRequest, RuntimeError> {
        Ok(self.evaluate_controlled(request, &|| Ok(()))?)
    }

    pub(crate) fn evaluate_resource(
        &self,
        request: policy::Request,
        mut resource: ResourceRequest,
    ) -> Result<EvaluatedRequest, RuntimeError> {
        if request.project != self.identity.project_root || request.cwd != resource.context.cwd {
            return Err(RuntimeError::Invalid(
                "resource context differs from runtime request",
            ));
        }
        resource
            .context
            .working_files
            .clone_from(&self.working_files);
        let evaluation = self
            .evaluator
            .evaluate_resource(&request, &resource.input, &resource.context, &|| Ok(()))
            .map_err(RuntimeError::Policy)?;
        Ok(EvaluatedRequest {
            identity: self.identity.clone(),
            request,
            evaluation,
            time_sensitive: self.evaluator.time_sensitive(),
            evaluator: Arc::clone(&self.evaluator),
            resource: Some(resource),
        })
    }

    pub(super) fn evaluate_controlled(
        &self,
        request: policy::Request,
        checkpoint: Checkpoint<'_>,
    ) -> Result<EvaluatedRequest, Error> {
        if request.project != self.identity.project_root {
            return Err(Error::Invalid("runtime request project differs"));
        }
        let evaluation = self.evaluator.evaluate_controlled(&request, checkpoint)?;
        Ok(EvaluatedRequest {
            identity: self.identity.clone(),
            request,
            evaluation,
            time_sensitive: self.evaluator.time_sensitive(),
            evaluator: Arc::clone(&self.evaluator),
            resource: None,
        })
    }

    #[cfg(test)]
    pub(super) fn metadata(&self) -> Value {
        let identity = &self.identity;
        json!({"activation_id":identity.activation_id,"previous_activation_id":identity.previous_activation_id,
            "policy_revision":identity.policy_revision,"candidate_digest":identity.candidate_digest})
    }
}

/// Immutable exact request and evaluation, bound to a local activation.
/// It cannot be deserialized from caller-provided authorization claims.
///
/// ```compile_fail
/// let evidence: shield::runtime::EvaluatedRequest = serde_json::from_str("{}").unwrap();
/// ```
pub struct EvaluatedRequest {
    identity: SnapshotIdentity,
    request: policy::Request,
    evaluation: CandidateEvaluation,
    time_sensitive: bool,
    evaluator: Arc<Evaluator>,
    resource: Option<ResourceRequest>,
}
#[derive(Clone, serde::Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResourceRequest {
    pub(crate) input: policy::normalized::Input,
    pub(crate) context: policy::normalized::Context,
}
impl EvaluatedRequest {
    pub(crate) fn resource(&self) -> Option<&ResourceRequest> {
        self.resource.as_ref()
    }
    /// Activation that produced this evidence.
    #[must_use]
    pub fn identity(&self) -> &SnapshotIdentity {
        &self.identity
    }
    /// Original literal request. Access is intentional and unredacted.
    #[must_use]
    pub fn request(&self) -> &policy::Request {
        &self.request
    }
    /// Original core evaluation. Access is intentional and unredacted.
    #[must_use]
    pub fn evaluation(&self) -> &CandidateEvaluation {
        &self.evaluation
    }
}

pub(in crate::review) fn load(
    context: &Context,
    checkpoint: Checkpoint<'_>,
) -> Result<Snapshot, Error> {
    load_plan(context, checkpoint, true)
}

/// Inspect an existing plan without preparing or publishing missing cache data.
pub(in crate::review) fn load_cached(
    context: &Context,
    checkpoint: Checkpoint<'_>,
) -> Result<Snapshot, Error> {
    load_plan(context, checkpoint, false)
}

fn load_plan(
    context: &Context,
    checkpoint: Checkpoint<'_>,
    recover: bool,
) -> Result<Snapshot, Error> {
    checkpoint()?;
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    let active = state.active.as_ref().ok_or(Error::Invalid(
        "no active snapshot; initialize the project with shield init",
    ))?;
    let loaded = active.compiled.load(&state.project_root, &state.project_id);
    let evaluator = match loaded {
        Ok(evaluator) => evaluator,
        Err(_) if recover => {
            super::super::cache::recover(context, checkpoint)?;
            return load_plan(context, checkpoint, false);
        }
        Err(error) => return Err(error),
    };
    let compiled = &active.compiled;
    if evaluator.revision() != active.policy_revision {
        return Err(Error::StaleSnapshot);
    }
    checkpoint()?;
    Ok(Snapshot {
        working_files: None,
        identity: SnapshotIdentity {
            project_id: state.project_id,
            project_root: state.project_root,
            activation_id: active.activation_id.clone(),
            previous_activation_id: active.previous_activation_id.clone(),
            policy_revision: active.policy_revision.clone(),
            candidate_digest: active.candidate_digest.clone(),
            binding: active.binding.clone(),
            compiled_digest: compiled.digest.clone(),
        },
        evaluator: Arc::new(evaluator),
    })
}

#[cfg(test)]
mod tests;
