//! Shared project inspection, owner-qualified rule lookup and non-activating proposal transitions.
use super::{Checkpoint, Context, Error, FOREIGN_WITHDRAWAL, Failure, text, views::Views};
use crate::review::storage::Store;
use serde_json::{Value, json};
use std::path::Path;
mod paging;

pub(super) fn rules_page(
    context: &Context,
    views: &mut Views,
    args: &Value,
) -> Result<Value, Error> {
    paging::rules(context, views, args)
}

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
        &captured.candidate,
        &captured.metadata,
        captured.revision(),
        selector,
    )?;
    let mut value = crate::review::inspection::captured_rules_with_resolution(
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
        let mut value = row.public();
        value["before_candidate_id"] = Value::Null;
        value["after_candidate_id"] = Value::Null;
        value["capture_availability"] = json!("historical_capture_unavailable");
        preview_events(&mut value, &state.authorization_events, id);
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
    let after_revision =
        crate::review::inspection::source::revision(&row.candidate, after_revision.as_deref())?;
    let display = crate::review::inspection::source::policy(&row.candidate, &after_revision)?;
    let mut value = row.public();
    value
        .as_object_mut()
        .map(|object| object.remove("apply_arguments"));
    if let Some(source) = display["rules"].as_str() {
        value["rules"] = serde_saphyr::from_str::<Value>(source).unwrap_or(Value::Null);
        let redacted = crate::review::model::redact(&mut value);
        value["redacted"] = json!(redacted || value["redacted"] == true);
    }
    value["capture_freshness"] = json!("unchecked; apply revalidates exact reviewed files");
    if let Some((arguments, summary)) = crate::review::model::presentation::read(&state, row) {
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
    preview_events(&mut value, &state.authorization_events, id);
    Ok(value)
}

fn preview_events(value: &mut Value, events: &[Value], id: &str) {
    let matching = events
        .iter()
        .filter(|event| event["links"]["proposal_id"] == id);
    let total = matching.clone().count();
    value["authorization_events"] = json!(matching.take(16).collect::<Vec<_>>());
    value["authorization_events_total"] = json!(total);
    value["authorization_events_list"] =
        json!({"tool":"policy_show","proposal_id":id,"section":"authorization_events"});
}
pub(super) fn policy(context: &Context, views: &mut Views, args: &Value) -> Result<Value, Error> {
    if let Some(id) = text(args, "proposal_id")? {
        if text(args, "section")? == Some("authorization_events") {
            let state = crate::review::project::checkpoint(context)?;
            if !state.proposals.iter().any(|row| row.id == id) {
                return Err(Error::Missing);
            }
            let events: Vec<_> = state
                .authorization_events
                .iter()
                .filter(|event| event["links"]["proposal_id"] == id)
                .collect();
            let page = crate::review::page::borrowed(
                context,
                "policy_show",
                args,
                events.iter().copied(),
                |event| {
                    event
                        .get("event_id")
                        .cloned()
                        .unwrap_or_else(|| event.clone())
                },
                Value::clone,
                "policy_show with proposal_id",
            )?;
            return Ok(
                json!({"proposal_id":id,"authorization_events":page.items,"authorization_events_total":events.len(),"next_cursor":page.next_cursor}),
            );
        }
        if text(args, "section")?.is_some() {
            return Err(Error::Invalid("invalid proposal section"));
        }
        if args.get("cursor").is_some() {
            return Err(Error::InvalidCursor);
        }
        let mut value = proposal(context, views, id)?;
        crate::review::page::preview(
            &mut value,
            "authorization_events",
            json!({"tool":"policy_show","proposal_id":id,"section":"authorization_events"}),
        );
        let candidate = value["after_candidate_id"].clone();
        let total = value["rules"]["rules"].as_array().map_or(0, Vec::len);
        let route = json!({"tool":"rule_list","candidate_id":candidate});
        crate::review::page::preview(&mut value["rules"], "rules", route.clone());
        value["rules_total"] = json!(total);
        value["rules_list"] = route;
        return Ok(value);
    }
    if text(args, "section")?.is_some() {
        return Err(Error::Invalid(
            "authorization_events section requires proposal_id",
        ));
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
    let state = crate::review::project::checkpoint(context)?;
    let mut value = json!({"policy_revision":state.policy_revision});
    let agent = text(args, "agent")?;
    let session = text(args, "session_id")?;
    let rows = state.proposals.iter().filter(|p| {
        p.status == status
            && agent.is_none_or(|a| p.provenance["agent"] == a)
            && session.is_none_or(|s| p.provenance["session_id"] == s)
    });
    let page = crate::review::page::borrowed(
        context,
        "policy_show",
        args,
        rows,
        |row| json!(row.id),
        crate::review::model::Proposal::summary,
        "policy_show with proposal_id",
    )?;
    value["proposals"] = json!(page.items);
    value["next_cursor"] = json!(page.next_cursor);
    value["capture_freshness"] = json!("unchecked; apply revalidates exact reviewed files");
    Ok(value)
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
    match crate::review::transition(context, id, "withdraw", checkpoint) {
        // The base revision changed: the proposal is saved as stale, not withdrawn.
        Ok(value) if value["error"]["code"] == "stale-proposal" => Err(Error::StaleSnapshot.into()),
        Ok(value) => Ok(value),
        // Ownership changed between the read and the locked update.
        Err(Error::ApprovalRequired) => Err(Failure::Refused(&FOREIGN_WITHDRAWAL)),
        Err(error) => Err(error.into()),
    }
}
