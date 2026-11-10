//! Exact checked-run claim digests and terminal capture; replay never owns a handoff.
use super::{Context, Error, EvaluatedRequest, Origin, State, Value, hash, json};
use crate::review::authorization_receipts as terminal;

fn claim<'a>(
    id: &'a str,
    project: &'a str,
    origin: &'a Origin,
    binding: &'a str,
) -> terminal::Claim<'a> {
    terminal::Claim {
        kind: terminal::Kind::Execution,
        id,
        project,
        agent: origin.agent.as_deref(),
        session: origin.session_id.as_deref(),
        consumer: &origin.consumer_id,
        binding,
    }
}

pub(super) fn capture(
    state: &mut State,
    index: usize,
    status: terminal::Status,
    time: u64,
    reason: Option<&str>,
) -> Result<(), Error> {
    let row = &state.run_requests[index];
    let binding = hash(&json!([
        row.project_id,
        row.activation_id,
        row.policy_revision,
        row.candidate_digest,
        row.request,
        row.resource_digest,
        row.origin
    ]));
    state.terminal_receipts.capture(terminal::Capture {
        claim: claim(&row.request_id, &row.project_id, &row.origin, &binding),
        decision: if status == terminal::Status::Consumed {
            terminal::Decision::Approve
        } else {
            terminal::Decision::Deny
        },
        status,
        grant: row.grant.as_ref().map(|grant| terminal::Granted {
            id: &grant.approval_id,
            issued: grant.issued_at_ms,
            expires: grant.expires_at_ms,
        }),
        reason,
        at: time,
    })
}

pub(super) fn replay(
    state: &State,
    origin: &Origin,
    evidence: &EvaluatedRequest,
    resource: &Value,
    id: &str,
    time: u64,
) -> Option<Value> {
    let identity = evidence.identity();
    let binding = hash(&json!([
        identity.project_id(),
        identity.activation_id(),
        identity.policy_revision(),
        identity.candidate_digest(),
        evidence.request(),
        hash(resource),
        origin
    ]));
    state
        .terminal_receipts
        .replay_consumer(claim(id, identity.project_id(), origin, &binding), time)
}

pub(super) fn operator(
    context: &Context,
    id: &str,
    decision: terminal::Decision,
    ttl_seconds: Option<u64>,
    reason: Option<&str>,
    at: u64,
) -> Result<Option<Value>, Error> {
    let state = crate::review::project::checkpoint(context)?;
    state.check_operations()?;
    state
        .terminal_receipts
        .replay_operator(terminal::OperatorReplay {
            context,
            id,
            decision,
            ttl_seconds,
            reason,
            at,
        })
}
