//! Shared project inspection, owner-qualified rule lookup and non-activating proposal transitions.
use super::{
    Checkpoint, Context, Error, FAILED_PROPOSAL, FOREIGN_WITHDRAWAL, Failure, required, text,
    views::Views,
};
use crate::review::{project, storage::Store};
use serde_json::{Value, json};
use std::path::Path;

/// Resolve rules against an explicit retained capture or the active generation.
pub(super) fn rules(
    context: &Context,
    views: &mut Views,
    selector: Option<&Value>,
    candidate: Option<&str>,
) -> Result<Value, Error> {
    let id = match candidate {
        Some(id) => id.to_owned(),
        None => views.active(context)?,
    };
    let captured = views.get(&id)?;
    let mut value = crate::review::inspection::captured_rules(
        context,
        &captured.source()?,
        &captured.metadata,
        captured.revision()?,
        selector,
    )?;
    if candidate.is_some() {
        value["candidate_id"] = json!(id);
        value["policy_revision"] = json!(captured.revision()?);
    }
    Ok(value)
}
pub(super) fn policy(context: &Context, views: &mut Views, args: &Value) -> Result<Value, Error> {
    if let Some(id) = text(args, "proposal_id")? {
        let mut value = crate::review::proposals(context, Some(id), false)?;
        proposal_handles(context, views, &mut value)?;
        return Ok(value);
    }
    let status = text(args, "status")?.unwrap_or("pending");
    if ![
        "pending",
        "applied",
        "rejected",
        "withdrawn",
        "stale",
        "failed",
    ]
    .contains(&status)
    {
        return Err(Error::Invalid("invalid proposal status"));
    }
    let mut value = project::policy(context)?;
    let mut proposals = crate::review::proposals(context, None, false)?;
    let rows = proposals["proposals"]
        .as_array_mut()
        .ok_or(Error::Invalid("proposals missing"))?;
    let agent = text(args, "agent")?;
    let session = text(args, "session_id")?;
    rows.retain(|p| {
        p["status"] == status
            && agent.is_none_or(|a| crate::review::history::agent_matches(p, a))
            && session.is_none_or(|s| p["provenance"]["session_id"] == s)
    });
    value
        .as_object_mut()
        .ok_or(Error::Invalid("invalid policy view"))?
        .remove("pending_configuration");
    value["proposals"] = json!(rows);
    let active_id = views.active(context)?;
    if value["policy_revision"] != views.get(&active_id)?.revision()? {
        return Err(Error::StaleSnapshot);
    }
    value["active_candidate_id"] = json!(active_id);
    views.page("policy_show", args, value, "proposals")
}
pub(super) fn proposal_handles(
    context: &Context,
    views: &mut Views,
    value: &mut Value,
) -> Result<(), Error> {
    let (before, after) = views.proposal(context, required(value, "proposal_id")?)?;
    value["before_candidate_id"] = json!(before);
    value["after_candidate_id"] = json!(after);
    Ok(())
}
pub(super) fn transition(
    context: &Context,
    name: &str,
    id: &str,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Failure> {
    let store = Store::open(Path::new(&context.project_root))?;
    let state = if name == "policy_receipt" {
        store.read_checkpoint()?
    } else {
        store.read()?
    };
    state.check(context)?;
    let row = state
        .proposals
        .iter()
        .find(|p| p.id == id)
        .ok_or(Error::Missing)?;
    if name == "policy_receipt" {
        return match row.status.as_str() {
            // Rejected and withdrawn proposals are terminal; review cannot revive them.
            "rejected" | "withdrawn" => Err(Error::Invalid(
                "proposal is terminal and cannot be applied; submit a new proposal",
            )
            .into()),
            "stale" => Err(Error::StaleSnapshot.into()),
            "failed" => Err(Failure::Refused(&FAILED_PROPOSAL)),
            "applied" if row.receipt.is_null() => Err(Error::Missing.into()),
            "applied" => Ok(
                json!({"proposal_id":id,"status":row.status,"policy_revision":row.receipt["policy_revision"],"receipt":row.receipt,"provenance":row.provenance,"operator":row.operator}),
            ),
            // A pending proposal whose base changed cannot apply, so its
            // receipt routes to a new proposal rather than to apply.
            "pending"
                if row.policy_revision != state.policy_revision
                    || crate::review::publication::validate_proposal_base(&state, id).is_err() =>
            {
                Err(Error::StaleSnapshot.into())
            }
            _ => Err(Error::ApprovalRequired.into()),
        };
    }
    if row.retry_owner != context.retry_owner() {
        return Err(Failure::Refused(&FOREIGN_WITHDRAWAL));
    }
    match row.status.as_str() {
        "withdrawn" => return Ok(row.public(false)),
        "stale" => return Err(Error::StaleSnapshot.into()),
        _ => {}
    }
    drop(state);
    match crate::review::transition(context, id, "withdraw", None, checkpoint) {
        // The base revision changed: the proposal is saved as stale, not withdrawn.
        Ok(value) if value["error"]["code"] == "stale-proposal" => Err(Error::StaleSnapshot.into()),
        Ok(value) => Ok(value),
        // Ownership changed between the read and the locked update.
        Err(Error::ApprovalRequired) => Err(Failure::Refused(&FOREIGN_WITHDRAWAL)),
        Err(error) => Err(error.into()),
    }
}
