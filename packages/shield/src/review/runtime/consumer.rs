//! Owns prepared policy-file snapshots, exact request evidence and locked revision handoff.
use super::{Context, Error};
use crate::review::{Checkpoint, storage::Store};
use crate::{
    configuration::{CandidateEvaluation, Evaluator},
    policy,
};
use serde::Serialize;
#[cfg(test)]
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
mod authorization;
pub(crate) use authorization::EvaluatedAuthority;

/// Failure to load, evaluate or revalidate local active policy evidence.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// An authenticated, previously observed exact artifact revocation prevents
    /// use of the affected policy until its source is removed or updated.
    #[error(
        "policy artifact {pack}@{version} from {source_identity} is known revoked; remove or update the affected pack"
    )]
    RevokedPolicy {
        source_identity: String,
        pack: String,
        version: String,
    },
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
    /// The policy files changed since the request was evaluated.
    #[error("policy files changed; reload and evaluate the request again")]
    StaleSnapshot,
    /// Derived policy is unavailable; operator preparation restores committed data.
    #[error("compiled policy folder unusable; run shield doctor to find the cause")]
    CacheUnavailable,
    /// The project's policy files are invalid, so there is no policy to apply.
    #[error("the policy files in .shield/ are invalid: {0}")]
    PolicyFiles(String),
}
impl From<Error> for RuntimeError {
    fn from(error: Error) -> Self {
        match error {
            Error::RegistryRevoked {
                source_identity,
                pack,
                version,
                ..
            } => Self::RevokedPolicy {
                source_identity,
                pack,
                version,
            },
            Error::Cancelled => Self::Policy(crate::policy::Error::Interrupted),
            Error::Io(error) => Self::Io(error),
            Error::Policy(error) => Self::Policy(error),
            Error::Invalid(reason)
            | Error::Ambiguous { reason, .. }
            | Error::Grouped { reason, .. } => Self::Invalid(reason),
            Error::ApprovalFormTooLong { .. } => Self::Invalid("approval form is too long"),
            Error::NotPortable(_) => Self::Invalid("rule names a folder on this machine"),
            Error::StateFormat { .. } => {
                Self::Invalid("unsupported Shield checkpoint format; original bytes preserved")
            }
            Error::StateCorrupt { .. } => {
                Self::Invalid("corrupt review state; preserve it for recovery")
            }
            Error::Dependency(_) => Self::Invalid("dependency closure is invalid"),
            Error::RequiredPack { .. } => Self::Invalid("pack is required by another selection"),
            Error::UnsupportedReleases(_) => {
                Self::Invalid("no pack release supports this Shield version")
            }
            Error::Locked | Error::Overloaded => Self::Locked,
            Error::Capacity { .. } => Self::Policy(crate::policy::Error::Limit),
            Error::Missing => Self::Missing,
            Error::StaleSnapshot => Self::StaleSnapshot,
            Error::StateRecreated => Self::Invalid("project state was recreated; reconnect Shield"),
            Error::CacheUnavailable => Self::CacheUnavailable,
            Error::PolicyFiles(message) => Self::PolicyFiles(message),
            Error::ApprovalRequired => Self::Invalid("operator approval is required"),
            Error::AgentNotConnected => {
                Self::Invalid("this agent is not connected to Shield in this project")
            }
            Error::CandidateUnavailable | Error::CursorUnavailable => {
                Self::Invalid("diagnostic view is unavailable")
            }
        }
    }
}

/// Immutable identity of the policy files a request was decided by, not an
/// authenticated grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SnapshotIdentity {
    project_id: String,
    project_root: String,
    activation_id: String,
    previous_activation_id: Option<String>,
    policy_revision: String,
    candidate_digest: String,
    registry_revision: String,
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
    /// The policy files' revision; kept under this name for request records.
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
    /// A folder with policy files and no review state gets its runtime state;
    /// no authenticated scope enrollment or host registration occurs.
    ///
    /// # Errors
    /// Fails for missing/invalid enrollment, corrupt aggregate state or filesystem errors.
    pub fn open(root: &Path) -> Result<Self, RuntimeError> {
        Ok(Self {
            context: crate::review::open(root)?,
        })
    }

    /// Load and prepare the policy files' plan for repeated evaluation.
    /// Old snapshots remain immutable; candidate changes do not mutate them.
    ///
    /// # Errors
    /// Fails for inactive, changed project identity, invalid state or filesystem errors.
    pub fn snapshot(&self) -> Result<Snapshot, RuntimeError> {
        self.snapshot_controlled(&|| Ok(()))
    }

    pub(crate) fn snapshot_controlled(
        &self,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Snapshot, RuntimeError> {
        Ok(load(&self.context, checkpoint)?)
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
    /// Refuses another project or changed policy files before invoking the callback.
    /// Invalid state, filesystem and bounded lock/release failures are reported.
    /// A lock release failure can occur after the callback has run; it cannot undo it.
    pub fn with_current<T>(
        &self,
        evaluated: &EvaluatedRequest,
        operation: impl FnOnce(&policy::Request, &CandidateEvaluation) -> T,
    ) -> Result<T, RuntimeError> {
        let authority = evaluated.authorization(&self.context, &|| Ok(()))?;
        Ok(
            Store::open(Path::new(&self.context.project_root))?.inspect_checkpoint(
                &self.context,
                &|| Ok(()),
                |state| {
                    authority.check_locked(state, evaluated, &|| Ok(()))?;
                    Ok(operation(&evaluated.request, &evaluated.evaluation))
                },
            )?,
        )
    }
}

/// An owned prepared evaluator over one read of the policy files.
pub struct Snapshot {
    identity: SnapshotIdentity,
    evaluator: Arc<Evaluator>,
    working_files: Option<crate::location::WorkingFiles>,
    input_status: serde_json::Value,
}

/// Exact authenticated source retained across the native adapter's final
/// override boundary. JSON receipts and optional installed descriptions cannot
/// construct this proof or replace the prepared effective artifact lock.
pub(crate) struct NativeSource {
    identity: SnapshotIdentity,
    evaluator: Arc<Evaluator>,
}

/// Keeps acknowledged registry authority stable until vendor disposition is
/// finalized. It does not hold project authority or cover external host work.
pub(crate) struct NativeAuthority {
    identity: SnapshotIdentity,
    _registry: crate::review::registry_trust::Guard,
}
impl NativeAuthority {
    pub(crate) fn same_source(&self, other: &Self) -> bool {
        self.identity == other.identity
    }
}
impl NativeSource {
    /// The policy is the files; a request evaluated against them stays bound to
    /// its project only.
    fn check(&self, state: &super::State) -> Result<(), Error> {
        if state.project_id != self.identity.project_id
            || state.project_root != self.identity.project_root
        {
            return Err(Error::StaleSnapshot);
        }
        Ok(())
    }
    /// Acquire shared trust outside project authority, then confirm the exact
    /// current policy revision and mode references under the short authority reader.
    pub(crate) fn acquire(&self, root: &Path) -> Result<NativeAuthority, Error> {
        let root = root.canonicalize()?;
        if root != Path::new(&self.identity.project_root) {
            return Err(Error::StaleSnapshot);
        }
        let store = Store::open(&root)?;
        let state = store.read_checkpoint()?;
        self.check(&state)?;
        let registry = crate::review::registry_trust::guard_effective(
            &state.operator,
            self.evaluator.effective(),
        )?;
        if registry.revision != self.identity.registry_revision {
            return Err(Error::StaleSnapshot);
        }
        let context = Context::new(state.project_id.clone(), state.project_root.clone(), "hook");
        store.inspect_checkpoint(&context, &|| Ok(()), |current| {
            self.check(current)?;
            if current.operator["registry_trust"] != state.operator["registry_trust"]
                || current.operator["installed"] != state.operator["installed"]
            {
                return Err(Error::StaleSnapshot);
            }
            Ok(())
        })?;
        Ok(NativeAuthority {
            identity: self.identity.clone(),
            _registry: registry,
        })
    }
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
    pub(crate) fn native_source(&self) -> NativeSource {
        NativeSource {
            identity: self.identity.clone(),
            evaluator: Arc::clone(&self.evaluator),
        }
    }
    /// Metadata-only observation of editable inputs at runtime loading. A match
    /// is a hint, never content validation or permission to use edited source.
    #[must_use]
    pub fn input_status(&self) -> &serde_json::Value {
        &self.input_status
    }
    pub(crate) fn has_mcp_integrations(&self) -> bool {
        self.evaluator.has_mcp_integrations()
    }
    pub(crate) fn resolve_mcp(
        &self,
        server: &str,
        connections: &crate::mcp_connections::Connections,
        source: &str,
    ) -> Result<policy::compiled::ResolvedMcp, RuntimeError> {
        let mut resolved = self
            .evaluator
            .resolve_mcp(server, connections, source)
            .map_err(RuntimeError::Policy)?;
        resolved.project_id = Some(self.identity.project_id.clone());
        Ok(resolved)
    }
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
        dialect_version: Option<&str>,
        cwd: &str,
        original: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, RuntimeError> {
        let input = policy::normalized::Input::Shell {
            source: source.into(),
            dialect: serde_json::json!(dialect)
                .as_str()
                .expect("dialect string")
                .into(),
            dialect_version: dialect_version.map(str::to_owned),
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
    /// Identity of the files this snapshot read; it does not assert they are unchanged.
    #[must_use]
    pub fn identity(&self) -> &SnapshotIdentity {
        &self.identity
    }

    /// Evaluate and retain the exact literal request through the shared policy core.
    /// No files are reread; changed policy files require a fresh project snapshot.
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
        resource: ResourceRequest,
    ) -> Result<EvaluatedRequest, RuntimeError> {
        self.evaluate_resource_controlled(request, resource, &|| Ok(()))
    }

    pub(crate) fn evaluate_resource_controlled(
        &self,
        request: policy::Request,
        mut resource: ResourceRequest,
        checkpoint: Checkpoint<'_>,
    ) -> Result<EvaluatedRequest, RuntimeError> {
        checkpoint().map_err(RuntimeError::Policy)?;
        if request.project != self.identity.project_root || request.cwd != resource.context.cwd {
            return Err(RuntimeError::Invalid(
                "resource context differs from runtime request",
            ));
        }
        if resource
            .mcp
            .as_ref()
            .is_some_and(|mcp| mcp.project_id.as_deref() != Some(self.identity.project_id.as_str()))
        {
            return Err(RuntimeError::Invalid(
                "MCP observation belongs to another project",
            ));
        }
        resource
            .context
            .working_files
            .clone_from(&self.working_files);
        let evaluation = self
            .evaluator
            .evaluate_resource_mcp(
                &request,
                &resource.input,
                &resource.context,
                resource.mcp.as_deref(),
                checkpoint,
            )
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

/// Immutable exact request and evaluation, bound to the policy files' revision.
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
    /// Adapter-derived matching authority, never decoded from a request or grant.
    #[serde(skip)]
    pub(crate) mcp: Option<Arc<policy::compiled::ResolvedMcp>>,
}
impl EvaluatedRequest {
    pub(crate) fn resource(&self) -> Option<&ResourceRequest> {
        self.resource.as_ref()
    }
    /// Policy files that produced this evidence.
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

/// Load the policy the project's files say now. The review state supplies only
/// the project's identity and registry trust; it holds no policy.
pub(in crate::review) fn load(
    context: &Context,
    checkpoint: Checkpoint<'_>,
) -> Result<Snapshot, Error> {
    checkpoint()?;
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    let cache = crate::review::files::cache_for(&state.operator).ok();
    let policy = crate::review::files::load_in(Path::new(&context.project_root), cache.as_deref())
        .map_err(|error| Error::PolicyFiles(error.to_string()))?;
    crate::review::registry_trust::recover_effective(
        &state.operator,
        policy.evaluator.effective(),
    )?;
    let registry_guard = crate::review::registry_trust::guard_effective(
        &state.operator,
        policy.evaluator.effective(),
    )?;
    let revision = policy.evaluator.revision().to_owned();
    checkpoint()?;
    Ok(Snapshot {
        working_files: None,
        input_status: serde_json::json!({"status":"current","authorizes":false}),
        identity: SnapshotIdentity {
            project_id: state.project_id.clone(),
            project_root: state.project_root.clone(),
            activation_id: revision.clone(),
            previous_activation_id: None,
            policy_revision: revision.clone(),
            // The files' key leaves out settings, so a settings change edits
            // a policy file but leaves pending approvals valid.
            candidate_digest: policy.digest,
            registry_revision: registry_guard.revision.clone(),
            binding: revision.clone(),
            compiled_digest: revision,
        },
        evaluator: Arc::new(policy.evaluator),
    })
}

#[cfg(test)]
mod tests;
