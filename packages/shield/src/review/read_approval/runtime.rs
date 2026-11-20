//! Separate general-runtime capture and private-capability consumption.
use super::{
    Context, Decision, Error, EvaluatedRequest, Origin, ReadRequest, State, Store, Value,
    capture_context, consume, json, now, result,
};
use crate::review::{
    Checkpoint, runtime::consumer::EvaluatedAuthority, sdk_read::RegisteredReadConsumer,
};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Copy)]
pub(crate) enum Phase<'a> {
    Evaluate {
        retry_ref: Option<&'a str>,
    },
    Consume {
        request_id: &'a str,
        retry_ref: &'a str,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Approval {
    pub(crate) request_id: Option<String>,
    pub(crate) status: String,
    #[serde(rename = "approval_id")]
    pub(crate) id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub(crate) code: String,
    pub(crate) allowed: bool,
    pub(crate) approval: Approval,
    #[serde(skip)]
    pub(crate) retry_ref: Option<String>,
}
pub(crate) struct Guarded {
    pub(crate) receipt: Receipt,
    pub(crate) authority: EvaluatedAuthority,
}

pub(crate) fn gate(
    consumer: &RegisteredReadConsumer,
    context: &Context,
    evidence: &EvaluatedRequest,
    resource: &str,
    phase: Phase<'_>,
    checkpoint: Checkpoint<'_>,
    verify: impl FnOnce() -> bool,
) -> Result<Guarded, Error> {
    checkpoint()?;
    if evidence.identity().project_id() != context.project_id
        || evidence.request().project != context.project_root
        || evidence.resource().is_none()
        || !crate::enforcement::read_text::valid_request(evidence.request())
    {
        return Err(Error::Invalid("unsupported SDK read binding"));
    }
    let origin = Origin {
        agent: context.agent.clone(),
        session_id: context.session_id.clone(),
        consumer_id: consumer.consumer_id().into(),
    };
    let authority = evidence.authorization(context, checkpoint)?;
    let mut receipt = None;
    Store::open(Path::new(&context.project_root))?.update_checkpoint(
        context,
        checkpoint,
        |state| {
            consumer.check_state(state)?;
            authority.check_locked(state, evidence, checkpoint)?;
            if !verify() {
                return Err(Error::StaleSnapshot);
            }
            // Native resource I/O is not a hard deadline. Never mutate authority
            // after a verifier returns past this invocation's captured deadline.
            checkpoint()?;
            let time = now()?;
            let value = match phase {
                Phase::Evaluate { retry_ref: None } => match evidence.evaluation().decision {
                    Decision::Allow => result("allowed", true, None, "none", None),
                    Decision::Deny => result("policy-deny", false, None, "none", None),
                    Decision::Ask => capture_context(
                        state,
                        context,
                        &origin,
                        evidence,
                        resource,
                        time,
                        Some(consumer.binding_digest()),
                    )?,
                },
                Phase::Evaluate {
                    retry_ref: Some(reference),
                } => {
                    let row =
                        find_reference(state, consumer, &origin, evidence, resource, reference)?;
                    inspect(row, time)
                }
                Phase::Consume {
                    request_id,
                    retry_ref,
                } => {
                    let row = state
                        .read_requests
                        .iter()
                        .find(|row| row.request_id == request_id)
                        .ok_or(Error::Missing)?;
                    verify_row(row, consumer, &origin, evidence, resource, retry_ref)?;
                    if evidence.evaluation().decision != Decision::Ask {
                        return Err(Error::StaleSnapshot);
                    }
                    consume(
                        state,
                        context,
                        &origin,
                        evidence,
                        resource,
                        request_id,
                        time,
                        Some(consumer.binding_digest()),
                    )?
                }
            };
            let mut captured: Receipt = serde_json::from_value(value)
                .map_err(|_| Error::Invalid("invalid SDK gate receipt"))?;
            if let Some(id) = &captured.approval.request_id {
                let row = state
                    .read_requests
                    .iter()
                    .find(|row| &row.request_id == id)
                    .ok_or(Error::Missing)?;
                captured.retry_ref = Some(consumer.retry_reference(id, &row.binding)?);
            }
            checkpoint()?;
            receipt = Some(captured);
            Ok(json!({}))
        },
    )?;
    Ok(Guarded {
        receipt: receipt.ok_or(Error::Invalid("SDK gate receipt missing"))?,
        authority,
    })
}

fn verify_row(
    row: &ReadRequest,
    consumer: &RegisteredReadConsumer,
    origin: &Origin,
    evidence: &EvaluatedRequest,
    resource: &str,
    reference: &str,
) -> Result<(), Error> {
    if row.runtime_context.as_deref() != Some(consumer.binding_digest())
        || !row.matches(evidence, resource, origin)
    {
        return Err(Error::StaleSnapshot);
    }
    consumer.verify_retry_reference(&row.request_id, &row.binding, reference)
}
fn find_reference<'a>(
    state: &'a State,
    consumer: &RegisteredReadConsumer,
    origin: &Origin,
    evidence: &EvaluatedRequest,
    resource: &str,
    reference: &str,
) -> Result<&'a ReadRequest, Error> {
    state
        .read_requests
        .iter()
        .find(|row| verify_row(row, consumer, origin, evidence, resource, reference).is_ok())
        .ok_or(Error::Missing)
}
fn inspect(row: &ReadRequest, time: u64) -> Value {
    let (code, status) = if row.status == "consumed" {
        ("approval-consumed", "consumed")
    } else if row.status == "revoked" {
        ("approval-revoked", "revoked")
    } else if row
        .grant
        .as_ref()
        .is_some_and(|grant| time < grant.issued_at_ms || time >= grant.expires_at_ms)
    {
        ("approval-expired", "expired")
    } else {
        ("approval-required", "pending")
    };
    result(code, false, Some(&row.request_id), status, None)
}

pub(crate) fn finalize<T>(
    consumer: &RegisteredReadConsumer,
    context: &Context,
    evidence: &EvaluatedRequest,
    authority: &EvaluatedAuthority,
    checkpoint: Checkpoint<'_>,
    verify: impl FnOnce() -> bool,
    operation: impl FnOnce() -> T,
) -> Result<T, Error> {
    Store::open(Path::new(&context.project_root))?.inspect_checkpoint(
        context,
        checkpoint,
        |state| {
            consumer.check_state(state)?;
            authority.check_locked(state, evidence, checkpoint)?;
            if !verify() {
                return Err(Error::StaleSnapshot);
            }
            checkpoint()?;
            Ok(operation())
        },
    )
}
