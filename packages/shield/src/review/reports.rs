//! Read-only report projections from retained proposal evidence, never policy preparation.
use super::{Checkpoint, Context, Error, model, storage::Store};
use serde_json::{Value, json};
use std::path::Path;

pub(crate) const TEMPLATE: &str =
    "shield-review://{project_id}/{proposal_id}/{candidate_digest}/{report}";

/// Add discoverable addresses without reopening authority or hydrating source.
pub(super) fn attach(context: &Context, tool: &str, data: &mut Value) {
    if !matches!(
        tool,
        "policy_propose"
            | "policy_show"
            | "policy_apply"
            | "policy_withdraw"
            | "policy_receipt"
            | "policy_reject"
            | "rule_add"
            | "rule_edit"
            | "rule_remove"
            | "pack_add"
            | "pack_update"
            | "pack_remove"
            | "pack_configure"
    ) {
        return;
    }
    if let (Some(id), Some(digest)) = (
        data["proposal_id"].as_str(),
        data["candidate_digest"].as_str(),
    ) {
        data["report_resources"] = json!(["comparison", "evidence"].map(|kind| {
            json!({"uri":format!("shield-review://{}/{id}/{digest}/{kind}", context.project_id),
                "name":format!("Policy review {kind}"),"mimeType":"application/json"})
        }));
    }
    if let Some(rows) = data.get_mut("proposals").and_then(Value::as_array_mut) {
        for row in rows {
            attach(context, tool, row);
        }
    }
}

#[cfg(test)]
mod tests;

/// Resolve only the selected enrollment and exact retained candidate identity.
pub(crate) fn read(
    context: &Context,
    uri: &str,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    checkpoint()?;
    let segments = uri.strip_prefix("shield-review://").ok_or(Error::Missing)?;
    let mut segments = segments.split('/');
    let project = segments.next().ok_or(Error::Missing)?;
    let id = segments.next().ok_or(Error::Missing)?;
    let digest = segments.next().ok_or(Error::Missing)?;
    let kind = segments.next().ok_or(Error::Missing)?;
    if project != context.project_id
        || !matches!(kind, "comparison" | "evidence")
        || segments.next().is_some()
    {
        return Err(Error::Missing);
    }
    let store = Store::open(Path::new(&context.project_root))?;
    let state = store.read_checkpoint()?;
    state.check(context)?;
    checkpoint()?;
    let proposal = state
        .proposals
        .iter()
        .find(|row| row.id == id)
        .ok_or(Error::Missing)?;
    if super::storage::sources::candidate_identity(&proposal.candidate) != digest {
        return Err(Error::Missing);
    }
    let mut data = if kind == "comparison" {
        proposal.comparison.clone()
    } else {
        let mut value = proposal.summary();
        value["base_candidate_digest"] = json!(super::storage::sources::candidate_identity(
            &proposal.base_candidate
        ));
        value["receipt"] = proposal.receipt.clone();
        value["authorization_events"] = json!(
            state
                .authorization_events
                .iter()
                .filter(|event| event["links"]["proposal_id"] == id)
                .collect::<Vec<_>>()
        );
        value
    };
    let private = model::redact_comparison(&mut data);
    let redacted = model::redact(&mut data) || private || data["redacted"] == true;
    checkpoint()?;
    Ok(
        json!({"schema_version":1,"project_id":context.project_id,"proposal_id":id,
        "candidate_digest":digest,"policy_revision":proposal.policy_revision,"report":kind,
        "data":data,"redacted":redacted,"authority":"Inspection only; no activation or execution permission."}),
    )
}
