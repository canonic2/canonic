//! Shared project inspection, owner-qualified rule lookup and non-activating proposal transitions.
use super::{Checkpoint, Context, Error, FOREIGN_WITHDRAWAL, Failure, text, views::Views};
use crate::review::storage::Store;
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
        let store = Store::open(Path::new(&context.project_root))?;
        let mut state = store.read_checkpoint()?;
        state.check(context)?;
        let row = state
            .proposals
            .iter_mut()
            .find(|row| row.id == id)
            .ok_or(Error::Missing)?;
        store.hydrate_proposal(row)?;
        let mut value = row.public(false);
        super::apply::present_proposal(context, row, &mut value);
        value["before_candidate_id"] =
            json!(views.retain(&row.base_candidate, &row.base_metadata)?);
        value["after_candidate_id"] = json!(views.retain(&row.candidate, &row.candidate_metadata)?);
        value["authorization_events"] = json!(
            state
                .authorization_events
                .iter()
                .filter(|event| event["links"]["proposal_id"] == id)
                .collect::<Vec<_>>()
        );
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
    let state = crate::review::project::captured(context)?;
    let active_id = views.active(context)?;
    let captured = views.get(&active_id)?;
    if crate::review::storage::sources::candidate_identity(&state.candidates["project"])
        != crate::review::storage::sources::candidate_identity(&captured.candidate)
    {
        return Err(Error::StaleSnapshot);
    }
    let mut value = crate::review::inspect_prepared(context, &state, captured.evaluator()?)?;
    let mut rows: Vec<_> = state
        .proposals
        .iter()
        .map(crate::review::model::Proposal::summary)
        .collect();
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
    value["active_candidate_id"] = json!(active_id);
    views.page("policy_show", args, value, "proposals")
}
pub(super) fn transition(
    context: &Context,
    name: &str,
    id: &str,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Failure> {
    let store = Store::open(Path::new(&context.project_root))?;
    let state = store.read_checkpoint()?;
    state.check(context)?;
    let row = state
        .proposals
        .iter()
        .find(|p| p.id == id)
        .ok_or(Error::Missing)?;
    if name == "policy_receipt" {
        if row.status != "pending" {
            if row.status == "applied" && row.receipt.is_null() {
                return Err(Error::Missing.into());
            }
            return Ok(
                json!({"proposal_id":row.id,"status":row.status,"closed_reason":row.closed_reason,
                "receipt":row.receipt,"operator":row.operator,"provenance":row.provenance}),
            );
        }
        if !crate::review::proposal_lifecycle::owns(row, context) {
            return Err(Error::Invalid(
                "review belongs to another interaction; inspect its outcome or prepare fresh",
            )
            .into());
        }
        return Err(Error::ApprovalRequired.into());
    }
    if !crate::review::proposal_lifecycle::owns(row, context) {
        return Err(Failure::Refused(&FOREIGN_WITHDRAWAL));
    }
    if row.status != "pending" {
        return Ok(row.summary());
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
