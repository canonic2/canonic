//! The registered SDK text-read profile; general wire data appoints no consumer.
use crate::{
    policy::{
        self,
        normalized::{Context as RequestContext, Input},
    },
    review::{
        Checkpoint, Context, Error,
        read_approval::runtime as approval,
        runtime::consumer::{EvaluatedAuthority, ResourceRequest},
        sdk_read::RegisteredReadConsumer,
    },
    runtime::{EvaluatedRequest, Project, SnapshotIdentity},
    runtime_protocol::wire::{Observation, Provenance},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

pub(crate) struct ReadCall {
    pub(crate) input: Input,
    pub(crate) context: RequestContext,
    pub(crate) provenance: Provenance,
}

#[derive(Clone, Debug)]
pub(crate) struct ReadWindow {
    pub(crate) path: String,
    pub(crate) offset: Option<u64>,
    pub(crate) limit: Option<u64>,
}

impl ReadCall {
    /// Validate only preserved mapping evidence. Resource observation is owned
    /// separately by the text binding, never by completion observation.
    pub(crate) fn window(&self, project: &str) -> Result<ReadWindow, Error> {
        self.input.validate(&self.context, project)?;
        if self.context.environment.is_some() {
            return Err(Error::Invalid("SDK read environment is unavailable"));
        }
        let Input::File {
            action,
            sources,
            destination: None,
            recursive: false,
            original_input,
        } = &self.input
        else {
            return Err(Error::Invalid("unsupported SDK read input"));
        };
        if action != "read" || sources.len() != 1 || !Path::new(&sources[0]).is_absolute() {
            return Err(Error::Invalid("unsupported SDK read input"));
        }
        let fields = original_input
            .as_object()
            .ok_or(Error::Invalid("invalid SDK read tool input"))?;
        if fields
            .keys()
            .any(|key| !["file_path", "offset", "limit"].contains(&key.as_str()))
            || original_input["file_path"].as_str() != Some(sources[0].as_str())
        {
            return Err(Error::Invalid(
                "SDK normalized read differs from tool input",
            ));
        }
        let number = |name: &str| -> Result<Option<u64>, Error> {
            fields
                .get(name)
                .map(|value| {
                    value
                        .as_u64()
                        .filter(|value| (1..=1_000_000).contains(value))
                        .ok_or(Error::Invalid("invalid SDK read window"))
                })
                .transpose()
        };
        Ok(ReadWindow {
            path: sources[0].clone(),
            offset: number("offset")?,
            limit: number("limit")?,
        })
    }

    fn attributed(
        &self,
        consumer: &RegisteredReadConsumer,
        completion: bool,
    ) -> Result<Context, Error> {
        let valid = |value: &str| !value.is_empty() && value.len() <= 128 && !value.contains('\0');
        if self
            .provenance
            .session_id
            .as_deref()
            .is_some_and(|value| !valid(value))
            || self
                .provenance
                .tool_use_id
                .as_deref()
                .is_none_or(|value| !valid(value))
            || (completion
                && !["PostToolUse", "PostToolUseFailure"].contains(&self.provenance.event.as_str()))
            || (!completion && self.provenance.event != "PreToolUse")
        {
            return Err(Error::Invalid("invalid SDK read provenance"));
        }
        let mut context = consumer.context().clone();
        context.cwd = Some(self.context.cwd.clone());
        context.session_id.clone_from(&self.provenance.session_id);
        context.tool_use_id.clone_from(&self.provenance.tool_use_id);
        context.host_event_name = Some(self.provenance.event.clone());
        Ok(context)
    }
}

impl ReadWindow {
    fn request(&self, project: &str, cwd: &str) -> Result<policy::Request, Error> {
        let mut arguments = vec![self.path.clone()];
        if let Some(offset) = self.offset {
            arguments.extend(["--offset".into(), offset.to_string()]);
        }
        if let Some(limit) = self.limit {
            arguments.extend(["--limit".into(), limit.to_string()]);
        }
        let request = policy::Request {
            executable: "shield:read-text".into(),
            arguments,
            cwd: cwd.into(),
            project: project.into(),
        };
        request.validate()?;
        Ok(request)
    }
    fn operation<'a>(&'a self, consumer: &'a RegisteredReadConsumer) -> ReadOperation<'a> {
        ReadOperation {
            kind: "sdk-read-text-v1",
            path: &self.path,
            offset: self.offset,
            limit: self.limit,
            consumer_context: consumer.binding_digest(),
        }
    }
}

#[derive(Serialize)]
struct ReadOperation<'a> {
    kind: &'static str,
    path: &'a str,
    offset: Option<u64>,
    limit: Option<u64>,
    consumer_context: &'a str,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadDisposition {
    Allowed,
    ApprovalRequired,
    Blocked,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum HistoryStatus {
    Recorded,
    Disabled,
    Dropped,
}

#[derive(Serialize)]
pub(crate) struct HistoryReceipt {
    pub(crate) status: HistoryStatus,
    pub(crate) request_id: Option<String>,
    pub(crate) event_id: Option<String>,
}

/// The proof and resource stay owned through response encoding. The evidence is
/// immutable and does not authorize a later reconstructed operation.
pub(crate) struct ReadOutcome {
    consumer: RegisteredReadConsumer,
    binding: super::read_text::Binding,
    evidence: EvaluatedRequest,
    call: ReadCall,
    attribution: Context,
    authority: EvaluatedAuthority,
    cache_root: Option<std::path::PathBuf>,
    disposition: ReadDisposition,
    request_id: Option<String>,
    approval_request_id: Option<String>,
    approval_id: Option<String>,
    retry_ref: Option<String>,
    consumed: bool,
    history: HistoryReceipt,
}

impl ReadOutcome {
    pub(crate) fn evidence(&self) -> &EvaluatedRequest {
        &self.evidence
    }
    pub(crate) fn input(&self) -> &Input {
        &self.call.input
    }
    pub(crate) fn context(&self) -> &RequestContext {
        &self.call.context
    }
    pub(crate) fn policy_decision(&self) -> policy::Decision {
        self.evidence.evaluation().decision
    }
    pub(crate) fn disposition(&self) -> ReadDisposition {
        self.disposition
    }
    pub(crate) fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
    pub(crate) fn approval_request_id(&self) -> Option<&str> {
        self.approval_request_id.as_deref()
    }
    pub(crate) fn approval_id(&self) -> Option<&str> {
        self.approval_id.as_deref()
    }
    pub(crate) fn retry_ref(&self) -> Option<&str> {
        self.retry_ref.as_deref()
    }
    pub(crate) fn consumed(&self) -> bool {
        self.consumed
    }
    pub(crate) fn history(&self) -> &HistoryReceipt {
        &self.history
    }
    pub(crate) fn identity(&self) -> &SnapshotIdentity {
        self.evidence.identity()
    }
    /// Only a short in-memory handoff belongs inside this callback. Stdout and
    /// the host's later read occur after the project authority lock is released.
    pub(crate) fn finalize<T>(
        &self,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce() -> T,
    ) -> Result<T, Error> {
        approval::finalize(
            &self.consumer,
            &self.attribution,
            &self.evidence,
            &self.authority,
            checkpoint,
            || {
                super::protocol::mandatory_read_allowed(
                    &self.binding.path,
                    self.cache_root.as_deref(),
                ) && self.binding.verify().is_ok()
            },
            operation,
        )
    }
}

pub(crate) fn evaluate(
    consumer: RegisteredReadConsumer,
    call: ReadCall,
    retry_ref: Option<&str>,
    checkpoint: Checkpoint<'_>,
) -> Result<ReadOutcome, Error> {
    read(
        consumer,
        call,
        approval::Phase::Evaluate { retry_ref },
        checkpoint,
    )
}
pub(crate) fn consume(
    consumer: RegisteredReadConsumer,
    call: ReadCall,
    request_id: &str,
    retry_ref: &str,
    checkpoint: Checkpoint<'_>,
) -> Result<ReadOutcome, Error> {
    read(
        consumer,
        call,
        approval::Phase::Consume {
            request_id,
            retry_ref,
        },
        checkpoint,
    )
}

fn read(
    consumer: RegisteredReadConsumer,
    mut call: ReadCall,
    phase: approval::Phase<'_>,
    checkpoint: Checkpoint<'_>,
) -> Result<ReadOutcome, Error> {
    checkpoint()?;
    let window = call.window(&consumer.context().project_root)?;
    let reported_cwd = call.context.cwd.clone();
    let cwd = Path::new(&call.context.cwd).canonicalize()?;
    if !cwd.is_dir()
        || !super::protocol::owns_context(Path::new(&consumer.context().project_root), &cwd)
    {
        return Err(Error::Invalid("invalid SDK working directory"));
    }
    call.context.cwd = cwd
        .to_str()
        .ok_or(Error::Invalid("SDK working directory is not UTF-8"))?
        .into();
    let attribution = call.attributed(&consumer, false)?;
    let project = Project::open(Path::new(&attribution.project_root)).map_err(runtime_error)?;
    let snapshot = project
        .snapshot_controlled(checkpoint)
        .map_err(runtime_error)?;
    let cache_root = crate::review::compiled_cache_root(Path::new(&attribution.project_root))?;
    if snapshot.identity().project_id() != attribution.project_id {
        return Err(Error::StaleSnapshot);
    }
    checkpoint()?;
    let binding = super::read_text::Binding::open_scoped(
        Path::new(&attribution.project_root),
        &cwd,
        &window.path,
        snapshot.access_paths(),
    )
    .map_err(|()| Error::Invalid("unsupported SDK read input"))?;
    if !super::protocol::mandatory_read_allowed(&binding.path, cache_root.as_deref()) {
        return Err(Error::Invalid("SDK read targets private operator state"));
    }
    checkpoint()?;
    let request = window.request(&attribution.project_root, &call.context.cwd)?;
    let resource = ResourceRequest {
        input: call.input.clone(),
        context: call.context.clone(),
        mcp: None,
    };
    let evidence = snapshot
        .evaluate_resource_controlled(request, resource, checkpoint)
        .map_err(runtime_error)?;
    let guarded = approval::gate(
        &consumer,
        &attribution,
        &evidence,
        &binding.digest,
        phase,
        checkpoint,
        || binding.verify().is_ok(),
    )?;
    let receipt = guarded.receipt;
    let disposition = if receipt.allowed {
        ReadDisposition::Allowed
    } else if receipt.approval.status == "pending" {
        ReadDisposition::ApprovalRequired
    } else {
        ReadDisposition::Blocked
    };
    checkpoint()?;
    let history = record_decision(
        &consumer,
        &attribution,
        &window,
        &evidence,
        &receipt,
        disposition,
        &reported_cwd,
        checkpoint,
    );
    checkpoint()?;
    let request_id = receipt
        .approval
        .request_id
        .clone()
        .or_else(|| history.request_id.clone());
    Ok(ReadOutcome {
        consumer,
        binding,
        evidence,
        call,
        attribution,
        authority: guarded.authority,
        cache_root,
        disposition,
        request_id,
        approval_request_id: receipt.approval.request_id,
        approval_id: receipt.approval.id,
        retry_ref: receipt.retry_ref,
        consumed: receipt.allowed && receipt.code == "allowed-once",
        history,
    })
}

fn runtime_error(error: crate::runtime::Error) -> Error {
    use crate::runtime::Error as Runtime;
    match error {
        Runtime::Io(error) => Error::Io(error),
        Runtime::Policy(policy::Error::Interrupted) => Error::Cancelled,
        Runtime::Policy(error) => error.into(),
        Runtime::Invalid(reason) => Error::Invalid(reason),
        Runtime::Locked => Error::Locked,
        Runtime::Missing => Error::Missing,
        Runtime::StaleSnapshot => Error::StaleSnapshot,
        Runtime::CacheUnavailable => Error::CacheUnavailable,
        Runtime::RevokedPolicy { .. } => Error::Invalid("active policy is known revoked"),
    }
}

impl HistoryReceipt {
    fn dropped() -> Self {
        Self {
            status: HistoryStatus::Dropped,
            request_id: None,
            event_id: None,
        }
    }
    fn from_value(value: &Value, observation: bool) -> Self {
        let id = |field: &str| {
            value[field]
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 128 && !id.contains('\0'))
                .map(str::to_owned)
        };
        match value["status"].as_str() {
            Some("disabled") => Self {
                status: HistoryStatus::Disabled,
                request_id: None,
                event_id: None,
            },
            Some("recorded")
                if id("event_id").is_some() && (observation || id("request_id").is_some()) =>
            {
                Self {
                    status: HistoryStatus::Recorded,
                    request_id: id("request_id"),
                    event_id: id("event_id"),
                }
            }
            _ => Self::dropped(),
        }
    }
}

#[derive(Serialize)]
struct Explanation<'a> {
    decision: policy::Decision,
    revision: &'a str,
    reason: &'static str,
    matches: &'a [policy::Match],
    requirement_checks: &'a [policy::rules::Check],
    guidance: &'a [policy::diagnostics::GuidanceRecord],
}
#[derive(Serialize)]
struct Evaluation<'a> {
    decision: policy::Decision,
    revision: &'a str,
    #[serde(rename = "evaluation")]
    explanation: Explanation<'a>,
}
#[derive(Serialize)]
struct Approval<'a> {
    request_id: Option<&'a str>,
    status: &'a str,
    #[serde(rename = "approval_id")]
    id: Option<&'a str>,
}
#[derive(Serialize)]
struct Details<'a> {
    operation: ReadOperation<'a>,
    request: &'a policy::Request,
    evaluation: Evaluation<'a>,
    policy_revision: &'a str,
    activation_id: &'a str,
    candidate_digest: &'a str,
    decision: policy::Decision,
    host_decision: &'static str,
    disposition: ReadDisposition,
    approval: Approval<'a>,
    code: &'a str,
    execution: &'static str,
    reported_cwd: &'a str,
    missing: Vec<&'static str>,
    omitted: Omitted,
}
#[derive(Serialize)]
struct Omitted {
    matches: usize,
    requirement_checks: usize,
    guidance: usize,
}
#[expect(
    clippy::too_many_arguments,
    reason = "one borrowed optional history view shares the exact bound call without serializing a policy bundle"
)]
fn record_decision(
    consumer: &RegisteredReadConsumer,
    context: &Context,
    window: &ReadWindow,
    evidence: &EvaluatedRequest,
    receipt: &approval::Receipt,
    disposition: ReadDisposition,
    reported_cwd: &str,
    checkpoint: Checkpoint<'_>,
) -> HistoryReceipt {
    let evaluation = evidence.evaluation().evaluation();
    let omitted = Omitted {
        matches: evaluation.matches.len().saturating_sub(8),
        requirement_checks: evaluation.requirement_checks.len().saturating_sub(32),
        guidance: evaluation.guidance.len().saturating_sub(3),
    };
    let missing = [
        ("matched_rule_diagnostics", omitted.matches),
        ("requirement_diagnostics", omitted.requirement_checks),
        ("guidance_diagnostics", omitted.guidance),
    ]
    .into_iter()
    .filter_map(|(name, count)| (count > 0).then_some(name))
    .collect();
    let identity = evidence.identity();
    let details = Details {
        operation: window.operation(consumer),
        request: evidence.request(),
        evaluation: Evaluation {
            decision: evaluation.decision,
            revision: identity.policy_revision(),
            explanation: Explanation {
                decision: evaluation.decision,
                revision: &evaluation.revision,
                reason: evaluation.reason,
                matches: &evaluation.matches[..evaluation.matches.len().min(8)],
                requirement_checks: &evaluation.requirement_checks
                    [..evaluation.requirement_checks.len().min(32)],
                guidance: &evaluation.guidance[..evaluation.guidance.len().min(3)],
            },
        },
        policy_revision: identity.policy_revision(),
        activation_id: identity.activation_id(),
        candidate_digest: identity.candidate_digest(),
        decision: evaluation.decision,
        host_decision: if receipt.allowed { "allow" } else { "deny" },
        disposition,
        approval: Approval {
            request_id: receipt.approval.request_id.as_deref(),
            status: &receipt.approval.status,
            id: receipt.approval.id.as_deref(),
        },
        code: &receipt.code,
        execution: "unobserved",
        reported_cwd,
        missing,
        omitted,
    };
    let projected = crate::evidence::projection::encode_controlled_with_sources(
        &details,
        8192,
        &|| {
            checkpoint()
                .map_err(|_| serde_json::Error::io(std::io::Error::other("SDK history deadline")))
        },
        &[],
    );
    let Ok((value, _)) = projected else {
        return HistoryReceipt::dropped();
    };
    crate::review::observations::decision(context, value).map_or_else(
        |_| HistoryReceipt::dropped(),
        |value| HistoryReceipt::from_value(&value, false),
    )
}

#[derive(Serialize)]
pub(crate) struct ObservationReceipt {
    status: HistoryStatus,
    event_id: Option<String>,
    correlation: &'static str,
    reported_request_id: Option<String>,
}
pub(crate) struct ObservationOutcome {
    consumer: RegisteredReadConsumer,
    receipt: ObservationReceipt,
}
impl ObservationOutcome {
    pub(crate) fn project_id(&self) -> &str {
        &self.consumer.context().project_id
    }
    pub(crate) fn receipt(&self) -> &ObservationReceipt {
        &self.receipt
    }
}

pub(crate) fn observe(
    consumer: RegisteredReadConsumer,
    call: &ReadCall,
    delivery_id: &str,
    reported_request_id: Option<&str>,
    observation: Observation,
    checkpoint: Checkpoint<'_>,
) -> Result<ObservationOutcome, Error> {
    checkpoint()?;
    let window = call.window(&consumer.context().project_root)?;
    let context = call.attributed(&consumer, true)?;
    let matches_event = observation.kind == "completed" && call.provenance.event == "PostToolUse"
        || matches!(observation.kind.as_str(), "failed" | "cancelled")
            && call.provenance.event == "PostToolUseFailure";
    if !matches_event
        || observation.occurred_at.is_some()
        || observation.run_id.is_some()
        || observation.exit_code.is_some()
        || observation.signal.is_some()
        || observation.host_approval.is_some()
    {
        return Err(Error::Invalid("unsupported SDK observation evidence"));
    }
    let input = crate::review::EventInput {
        delivery_id: delivery_id.into(),
        request_id: reported_request_id.map(str::to_owned),
        status: observation.kind,
    };
    input.validate()?;
    checkpoint()?;
    let history = crate::review::observations::completion(
        &context,
        &input,
        &json!(window.operation(&consumer)),
    )
    .map_or_else(
        |_| HistoryReceipt::dropped(),
        |result| HistoryReceipt::from_value(&result, true),
    );
    Ok(ObservationOutcome {
        consumer,
        receipt: ObservationReceipt {
            status: history.status,
            event_id: history.event_id,
            correlation: "deferred",
            reported_request_id: input.request_id,
        },
    })
}
