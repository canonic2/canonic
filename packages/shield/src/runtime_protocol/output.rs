//! Borrow one captured read outcome into bounded, redacted evidence.
use super::Outcome;
use crate::{policy, review};
use serde::Serialize;

const MAX_OUTPUT: usize = 4 * 1024 * 1024;

#[derive(Serialize)]
struct Response<'a, T> {
    protocol_version: u8,
    correlation_id: Option<&'a str>,
    operation: Option<&'a str>,
    project_id: Option<&'a str>,
    request_id: Option<&'a str>,
    policy_revision: Option<&'a str>,
    result: Option<T>,
    error: Option<Failure<'a>>,
}

#[derive(Serialize)]
struct Failure<'a> {
    code: &'a str,
    message: String,
    operator_route: Option<&'a str>,
}

#[derive(Serialize)]
struct Normalized<'a> {
    input: &'a policy::normalized::Input,
    context: &'a policy::normalized::Context,
}

#[derive(Serialize)]
struct Coverage {
    model: &'static str,
    complete: bool,
    unknown: &'static str,
}

#[derive(Serialize)]
struct ReadResult<'a> {
    mode: &'static str,
    decision: policy::Decision,
    disposition: crate::enforcement::read_consumer::ReadDisposition,
    normalized_request: Normalized<'a>,
    explanation: &'a policy::Evaluation,
    analysis_coverage: Coverage,
    approval_request_id: Option<&'a str>,
    requirement_checks: &'a [policy::rules::Check],
    guidance: &'a [policy::diagnostics::GuidanceRecord],
    retry_ref: Option<&'a str>,
    approval_id: Option<&'a str>,
    consumed: bool,
    history: &'a crate::enforcement::read_consumer::HistoryReceipt,
    input_digest: &'a str,
}

pub(super) fn encode(
    outcome: &Outcome,
    correlation: &str,
    operation: &str,
    digest: &str,
    checkpoint: review::Checkpoint<'_>,
) -> Result<Vec<u8>, review::Error> {
    match outcome {
        Outcome::Read(read) => {
            let explanation = read.evidence().evaluation().evaluation();
            project(
                &Response {
                    protocol_version: 1,
                    correlation_id: Some(correlation),
                    operation: Some(operation),
                    project_id: Some(read.identity().project_id()),
                    request_id: read.request_id(),
                    policy_revision: Some(read.identity().policy_revision()),
                    result: Some(ReadResult {
                        mode: "runtime",
                        decision: read.policy_decision(),
                        disposition: read.disposition(),
                        normalized_request: Normalized {
                            input: read.input(),
                            context: read.context(),
                        },
                        explanation,
                        analysis_coverage: Coverage {
                            model: "bounded-sdk-text-read-v1",
                            complete: false,
                            unknown: "Host performs the read after the callback.",
                        },
                        approval_request_id: read.approval_request_id(),
                        requirement_checks: &explanation.requirement_checks,
                        guidance: &explanation.guidance,
                        retry_ref: read.retry_ref(),
                        approval_id: read.approval_id(),
                        consumed: read.consumed(),
                        history: read.history(),
                        input_digest: digest,
                    }),
                    error: None,
                },
                checkpoint,
            )
        }
        Outcome::Observe(observation) => project(
            &Response {
                protocol_version: 1,
                correlation_id: Some(correlation),
                operation: Some(operation),
                project_id: Some(observation.project_id()),
                request_id: None,
                policy_revision: None,
                result: Some(observation.receipt()),
                error: None,
            },
            checkpoint,
        ),
    }
}

fn project(
    value: &impl Serialize,
    checkpoint: review::Checkpoint<'_>,
) -> Result<Vec<u8>, review::Error> {
    checkpoint()?;
    let projected = crate::evidence::projection::encode_controlled_with_sources(
        value,
        MAX_OUTPUT,
        &|| {
            checkpoint()
                .map_err(|_| serde_json::Error::io(std::io::Error::other("runtime deadline")))
        },
        &[],
    )
    .map_err(|_| review::Error::Invalid("runtime response exceeded its budget"))?
    .0;
    checkpoint()?;
    let encoded = serde_json::to_vec(&projected)
        .map_err(|_| review::Error::Invalid("runtime response encoding"))?;
    checkpoint()?;
    Ok(encoded)
}

pub(super) fn failure(
    correlation: Option<&str>,
    operation: Option<&str>,
    project: Option<&str>,
    code: &str,
) -> Vec<u8> {
    serde_json::to_vec(&Response::<()> {
        protocol_version: 1,
        correlation_id: correlation,
        operation,
        project_id: project,
        request_id: None,
        policy_revision: None,
        result: None,
        error: Some(Failure {
            code,
            message: format!("Shield runtime request failed: {code}."),
            operator_route: None,
        }),
    })
    .expect("fixed bounded runtime failure")
}

pub(super) fn code(error: &review::Error) -> &'static str {
    match error {
        review::Error::Locked | review::Error::Overloaded => "overloaded",
        review::Error::Cancelled => "cancelled",
        review::Error::Missing => "project_unavailable",
        review::Error::StaleSnapshot => "stale_revision",
        review::Error::Io(_) => "io_failure",
        review::Error::ApprovalRequired => "approval_required",
        review::Error::Capacity { .. } | review::Error::Policy(policy::Error::Limit) => {
            "budget_exceeded"
        }
        review::Error::Invalid("unsupported SDK read input" | "unsupported runtime operation") => {
            "unsupported_operation"
        }
        _ => "state_invalid",
    }
}
