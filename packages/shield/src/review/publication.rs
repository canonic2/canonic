//! What policy changes share besides writing the files: the lease that orders
//! writers of `.shield/` and agent files, the projections of a candidate into
//! files, the first review state `shield init` creates, connection publication
//! retries and the review of a candidate against the current policy.
use crate::{
    configuration,
    review::{self, Context, Error, model::State, storage::Store},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
mod lease;
pub(super) mod projections;
pub(in crate::review) use lease::ProjectionLease;

/// Host confirmation provenance supplied only after the interface validates its approval path.
/// This records the mechanism; it does not grant authority by itself.
#[derive(Clone, Copy)]
pub(super) enum HostConfirmation {
    Elicitation,
    ToolPermission,
}
impl HostConfirmation {
    pub(super) fn mechanism(self) -> &'static str {
        match self {
            Self::Elicitation => "host-elicitation",
            Self::ToolPermission => "host-tool-permission",
        }
    }
}

/// Enroll a confirmed initial candidate, recovering only the exact committed state.
pub(super) fn initialize(
    project: &Path,
    candidates: &BTreeMap<String, Value>,
    revision: &str,
    meta: &Value,
    checkpoints: BTreeMap<String, Value>,
) -> Result<(), Error> {
    if let Some(candidate) = candidates.get("project") {
        super::changes::historical::validate(&Value::Null, candidate)?;
    }
    let initialization = Store::initialize_verified(
        project,
        candidates.clone(),
        revision,
        meta.clone(),
        checkpoints,
    );
    if let Err(error) = initialization {
        // Recovery only confirms a commit that already happened; when no state
        // was written, the initialization failure is the cause to report.
        let Ok(recovered) = Store::open(project).and_then(|store| store.read_current()) else {
            return Err(error);
        };
        if recovered.operator != *meta || recovered.candidates != *candidates {
            return Err(error);
        }
    }
    Ok(())
}

/// Retry the connection publication staged by an applied change.
pub(in crate::review) fn resume_connections(
    context: &Context,
    proposal: Option<&str>,
    owner: &str,
    receipt: &mut Value,
) {
    resume_connections_with(context, proposal, owner, receipt, None);
}

/// [`resume_connections`] for a writer that already holds the lease.
pub(in crate::review) fn resume_connections_guarded(
    context: &Context,
    proposal: Option<&str>,
    owner: &str,
    receipt: &mut Value,
    publication: &ProjectionLease,
) {
    resume_connections_with(context, proposal, owner, receipt, Some(publication));
}

fn resume_connections_with(
    context: &Context,
    proposal: Option<&str>,
    owner: &str,
    receipt: &mut Value,
    publication: Option<&ProjectionLease>,
) {
    let result = match publication {
        Some(guard) => {
            super::integrations::activation::resume_guarded(context, proposal, owner, guard)
        }
        None => super::integrations::activation::resume(context, proposal, owner),
    };
    match result {
        Ok(published) => {
            receipt["connection_publication"] =
                json!(if published { "published" } else { "superseded" });
            if !published {
                receipt["next_action"] = json!(
                    "Another policy change or setup superseded this publication. Inspect status for its current registered bindings."
                );
            }
        }
        Err(error) => {
            receipt["connection_publication"] = json!("pending");
            receipt["setup_error"] = json!(error.to_string());
            receipt["repair_route"] = json!("mcp_setup");
            receipt["repair_arguments"] = json!({"pending":"review"});
            receipt["next_action"] = json!(
                "Confirmed connection publication will resume automatically. Conflicting owned settings can be reviewed or cancelled in this session through mcp_setup."
            );
        }
    }
}

/// Connection changes need a form showing private settings, even on hosts that
/// normally approve policy through the tool's model-visible arguments.
pub(in crate::review) fn mcp_changed(before: &Value, candidate: &Value) -> Result<bool, Error> {
    let old = configuration::layers::merged(before)?;
    let new = configuration::layers::merged(candidate)?;
    if ["mcp", "local_mcp"]
        .iter()
        .any(|key| old[*key] != new[*key])
    {
        return Ok(true);
    }
    let connections = |candidate: &Value| -> Result<crate::mcp_connections::Connections, Error> {
        serde_json::from_value(
            candidate["lock"]
                .get("mcp")
                .cloned()
                .unwrap_or_else(|| json!({})),
        )
        .map_err(|_| Error::Invalid("invalid retained MCP connection settings"))
    };
    let old = connections(before)?;
    let new = connections(candidate)?;
    Ok(old.len() != new.len()
        || old.iter().any(|(id, c)| {
            new.get(id).is_none_or(|n| {
                c.enabled != n.enabled
                    || c.local != n.local
                    || c.transport != n.transport
                    || c.agents != n.agents
                    || c.packs != n.packs
            })
        }))
}

/// What a candidate changes against the current policy, with its permission comparison.
#[cfg(test)]
pub(super) fn change_review(
    context: &Context,
    before: &State,
    candidate: &Value,
    meta: &Value,
) -> Result<Value, Error> {
    prepare_review(context, before, candidate, meta, None).map(|(view, _)| view)
}

pub(super) fn prepare_review(
    context: &Context,
    before: &State,
    candidate: &Value,
    meta: &Value,
    base: Option<&configuration::Evaluator>,
) -> Result<(Value, configuration::Evaluator), Error> {
    if base.is_some_and(|plan| plan.revision() != before.policy_revision) {
        return Err(Error::StaleSnapshot);
    }
    let mut candidates = BTreeMap::new();
    candidates.insert("project".into(), candidate.clone());
    let (mut comparison, plan) = review::compare_prepared(&before.candidates, &candidates, base)?;
    if let Some(lines) = review::formatting_changes(&before.candidates["project"], candidate) {
        comparison["formatting"] = lines;
    }
    Ok((
        json!({"operation":"change policy","project_id":context.project_id,
        "changes":super::changes::diff::between(&before.operator, &before.candidates["project"], meta, candidate),
        "packs":crate::review::packs::summaries(meta),"candidate":candidate,"comparison":comparison}),
        plan,
    ))
}
