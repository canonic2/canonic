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
        None => views.active_readonly(context)?,
    };
    let captured = views.get(&id)?;
    let source = crate::review::inspection::source::rules(
        context,
        &captured.candidate,
        &captured.metadata,
        captured.revision(),
        selector,
    )?;
    let mut value = crate::review::inspection::captured_rules_with_resolution(
        context,
        &source,
        &captured.metadata,
        captured.revision(),
        selector,
        |index, artifact| views.resolution(&id, &source, index, artifact),
    )?;
    if candidate.is_some() {
        value["candidate_id"] = json!(id);
        value["policy_revision"] = json!(captured.revision());
    }
    Ok(value)
}
fn proposal(context: &Context, views: &mut Views, id: &str) -> Result<Value, Error> {
    let store = Store::inspect(Path::new(&context.project_root))?;
    let (mut state, _) = store.read_diagnostic_checkpoint()?;
    state.check_inspection(context)?;
    let index = state
        .proposals
        .iter()
        .position(|row| row.id == id)
        .ok_or(Error::Missing)?;
    // A proposal an earlier release prepared stores no file texts to rebuild
    // its candidates from; texts that no longer rebuild them refuse.
    let hydrated = match store.hydrate_proposal(&mut state.proposals[index]) {
        Ok(()) => true,
        Err(Error::CandidateUnavailable) => false,
        Err(error) => return Err(error),
    };
    let row = &state.proposals[index];
    if !hydrated || (row.status != "pending" && row.reasons["historical_capture"].is_object()) {
        let mut value = row.public(false);
        value["before_candidate_id"] = Value::Null;
        value["after_candidate_id"] = Value::Null;
        value["capture_availability"] = json!("historical_capture_unavailable");
        value["authorization_events"] = json!(
            state
                .authorization_events
                .iter()
                .filter(|event| event["links"]["proposal_id"] == id)
                .collect::<Vec<_>>()
        );
        return Ok(value);
    }
    let after_revision =
        serde_json::from_value::<crate::configuration::Effective>(row.comparison["after"].clone())
            .ok()
            .map(|metadata| metadata.revision())
            .transpose()?
            .or_else(|| {
                (row.candidate_digest == row.base_digest).then(|| row.policy_revision.clone())
            });
    let after_revision = crate::review::inspection::source::revision(
        context,
        &row.candidate,
        after_revision.as_deref(),
    )?;
    let display =
        crate::review::inspection::source::policy(context, &row.candidate, &after_revision)?;
    let mut value = row.public(false);
    value
        .as_object_mut()
        .map(|object| object.remove("apply_arguments"));
    if let Some(source) = display["rules"].as_str() {
        value["rules"] = serde_saphyr::from_str::<Value>(source).unwrap_or(Value::Null);
        let redacted = crate::review::model::redact(&mut value);
        value["redacted"] = json!(redacted || value["redacted"] == true);
    }
    value["capture_freshness"] = json!("unchecked; apply revalidates exact reviewed files");
    if let Some((arguments, summary)) =
        crate::review::model::presentation::read(context, &state, row)
    {
        value["presentation_availability"] = json!("available");
        super::apply::present_cached_proposal(context, row, &mut value, &arguments, &summary);
    } else {
        value["presentation_availability"] = json!("unavailable");
        value["presentation_next_action"] = json!(
            "Use the exact apply arguments from the original policy_propose result, or prepare a fresh change in this interaction; explicit application strictly revalidates the complete source."
        );
    }
    let (before, after) = views.retain_recorded_pair(
        context,
        (&row.base_candidate, &row.base_metadata),
        (&row.candidate, &row.candidate_metadata),
        &row.policy_revision,
        Some(&after_revision),
    )?;
    value["before_candidate_id"] = json!(before);
    value["after_candidate_id"] = json!(after);
    value["authorization_events"] = json!(
        state
            .authorization_events
            .iter()
            .filter(|event| event["links"]["proposal_id"] == id)
            .collect::<Vec<_>>()
    );
    Ok(value)
}
pub(super) fn policy(context: &Context, views: &mut Views, args: &Value) -> Result<Value, Error> {
    if let Some(id) = text(args, "proposal_id")? {
        return proposal(context, views, id);
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
    let mut state = crate::review::project::captured_inspection(context)?;
    let active_id = views.active_readonly(context)?;
    let captured = views.get(&active_id)?;
    if crate::review::candidate_identity(&state.candidates["project"])
        != crate::review::candidate_identity(&captured.candidate)
    {
        return Err(Error::StaleSnapshot);
    }
    state.candidates.insert(
        "project".into(),
        crate::review::inspection::source::policy(
            context,
            &state.candidates["project"],
            &state.policy_revision,
        )?,
    );
    let mut value = crate::review::inspect_recorded(context, &state)?;
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
    value["capture_freshness"] = json!("unchecked; apply revalidates exact reviewed files");
    value["policy_candidate_id"] = json!(active_id);
    views.page("policy_show", args, value, "proposals")
}
/// Withdraw this interaction's live review; a closed one returns its summary.
pub(super) fn withdraw(
    context: &Context,
    id: &str,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Failure> {
    let state = Store::open(Path::new(&context.project_root))?.read_checkpoint()?;
    state.check(context)?;
    state.check_operations()?;
    let row = state
        .proposals
        .iter()
        .find(|p| p.id == id)
        .ok_or(Error::Missing)?;
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
