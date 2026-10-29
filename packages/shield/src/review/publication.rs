//! Confirmed policy publication, proposal binding and recoverable projections.
#[cfg(test)]
use super::project::load;
use super::project::{metadata, root, status};
use crate::{
    configuration, policy,
    review::{self, Context, Error, model::State, storage::Store},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
pub(super) mod projections;
use projections::{
    digest, file_hashes, projection_changed, projection_fingerprint, projection_hashes,
    projections, repair_projections,
};

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
        let Ok(recovered) = Store::open(project).and_then(|store| store.read()) else {
            return Err(error);
        };
        if recovered.operator != *meta
            || recovered.candidates != *candidates
            || recovered.active.is_none()
        {
            return Err(error);
        }
    }
    Ok(())
}

pub(super) fn commit(
    context: &Context,
    before: &State,
    candidate: Value,
    meta: Value,
    proposal: Option<&str>,
) -> Result<Value, Error> {
    commit_reviewed(context, before, candidate, meta, proposal, None)
}

pub(super) fn commit_reviewed(
    context: &Context,
    before: &State,
    candidate: Value,
    meta: Value,
    proposal: Option<&str>,
    manual_fingerprint: Option<&str>,
) -> Result<Value, Error> {
    commit_confirmed(
        context,
        before,
        candidate,
        meta,
        proposal,
        manual_fingerprint,
        "operator-cli",
    )
}

pub(in crate::review) fn commit_mcp(
    context: &Context,
    before: &State,
    candidate: Value,
    meta: Value,
    proposal: &str,
    confirmation: HostConfirmation,
) -> Result<Value, Error> {
    commit_confirmed(
        context,
        before,
        candidate,
        meta,
        Some(proposal),
        None,
        confirmation.mechanism(),
    )
}

fn commit_confirmed(
    context: &Context,
    before: &State,
    candidate: Value,
    mut meta: Value,
    proposal: Option<&str>,
    manual_fingerprint: Option<&str>,
    mechanism: &str,
) -> Result<Value, Error> {
    if let Some(id) = proposal {
        validate_proposal_base(before, id)?;
    }
    let changes = super::changes::diff::between(
        &before.operator,
        &before.candidates["project"],
        &meta,
        &candidate,
    );
    let mut candidates = BTreeMap::new();
    candidates.insert("project".into(), candidate);
    let evaluator = configuration::Evaluator::new(review::snapshots(&candidates)?)?;
    if mechanism != "operator-cli" {
        validate_mcp_change(before, &candidates["project"])?;
    }
    let files = projections(&candidates["project"], &meta)?;
    meta["projection"] = json!(digest(&files));
    meta["projection_file_hashes"] = json!(file_hashes(&files));
    let expected = publication_identity(before);
    let store = Store::open(Path::new(&context.project_root))?;
    let compiled =
        review::cache::build_candidate(context, &candidates, evaluator.revision(), &meta)?;
    let candidates = store.retain_candidates(&candidates)?;
    let candidate_identity = review::storage::sources::candidate_identity(&candidates["project"]);
    let mut attempted_activation = None;
    let result = store.update_checkpoint(context, &|| Ok(()), |state| {
        if manual_fingerprint.map_or_else(
            || projection_changed(context, before),
            |fingerprint| Ok(projection_fingerprint(context, before)? != fingerprint),
        )? {
            return Err(Error::Invalid(
                "project files changed during confirmation; review again",
            ));
        }
        if expected != publication_identity(state) {
            return Err(Error::Invalid("project changed; review again"));
        }
        if let Some(id) = proposal {
            let row = state
                .proposals
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or(Error::Missing)?;
            if row.status != "pending"
                || row.policy_revision != state.policy_revision
                || review::storage::sources::candidate_identity(&row.candidate)
                    != candidate_identity
            {
                return Err(Error::Invalid("proposal changed or is stale"));
            }
            row.status = "applied".into();
            row.operator =
                Some(json!({"operator_id":null,"display_name":null,"mechanism":mechanism}));
        }
        if metadata(before)["projection"] != meta["projection"] {
            meta["projection_previous"] = projection_hashes(context, &files)?;
        }
        keep_path_access(&mut meta, &state.operator);
        state.candidates = candidates;
        state.policy_revision = evaluator.revision().into();
        state.operator = meta.clone();
        let receipt =
            review::runtime::publish_confirmed(state, context, proposal, compiled, mechanism);
        if let Some(id) = proposal {
            let row = state
                .proposals
                .iter_mut()
                .find(|row| row.id == id)
                .ok_or(Error::Missing)?;
            row.receipt = receipt.clone();
        }
        attempted_activation = receipt["activation_id"].as_str().map(str::to_owned);
        Ok(receipt)
    });
    let receipt = match result {
        Ok(value) => value,
        Err(error) => {
            let recovered = store.read()?;
            if attempted_activation.as_deref().is_none_or(|id| {
                recovered
                    .active
                    .as_ref()
                    .is_none_or(|a| a.activation_id() != id)
            }) || recovered.operator != meta
                || recovered.policy_revision != evaluator.revision()
            {
                return Err(error);
            }
            json!({"committed":true,"policy_revision":recovered.policy_revision,"repair_needed":true})
        }
    };
    let mut receipt = receipt;
    repair_after_commit(context, &store, &evaluator, &files, &meta, &mut receipt);
    receipt["changes"] = changes;
    Ok(receipt)
}

/// Host confirmation cannot activate connection or resolved-setting changes.
pub(in crate::review) fn validate_mcp_change(
    before: &State,
    candidate: &Value,
) -> Result<(), Error> {
    let old = configuration::layers::merged(&before.candidates["project"])?;
    let new = configuration::layers::merged(candidate)?;
    if ["mcp", "local_mcp"]
        .iter()
        .any(|key| old[*key] != new[*key])
    {
        return Err(Error::McpSetupRequired);
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
    let old = connections(&before.candidates["project"])?;
    let new = connections(candidate)?;
    if old.len() != new.len()
        || old.iter().any(|(id, c)| {
            new.get(id).is_none_or(|n| {
                c.enabled != n.enabled
                    || c.local != n.local
                    || c.transport != n.transport
                    || c.agents != n.agents
                    || c.packs != n.packs
            })
        })
    {
        return Err(Error::McpSetupRequired);
    }
    Ok(())
}

/// Bring project files and agent prefix rules in line with the policy just
/// committed, unless another writer has replaced it since. The receipt names a
/// prefix rules failure, because repairing project files does not fix it.
fn repair_after_commit(
    context: &Context,
    store: &Store,
    evaluator: &configuration::Evaluator,
    files: &BTreeMap<String, String>,
    meta: &Value,
    receipt: &mut Value,
) {
    let mut prefix_error = None;
    let repair = store.inspect_locked(context, &|| Ok(()), |current| {
        if current.policy_revision != evaluator.revision()
            || metadata(current)["projection"] != meta["projection"]
        {
            return projection_changed(context, current);
        }
        repair_projections(context, files, meta)?;
        prefix_error = super::integrations::sync_prefix_rules(
            root(context).as_path(),
            &metadata(current),
            evaluator.codex_prefixes(),
        )
        .err();
        projection_changed(context, current)
    });
    receipt["repair_needed"] = json!(repair.unwrap_or(true) || prefix_error.is_some());
    if let Some(error) = prefix_error {
        receipt["setup_error"] = json!(error.to_string());
        receipt["repair_route"] = json!("shield mcp setup");
    }
}

/// A retained proposal cannot restore operator metadata changed since review.
/// Path protection has its own confirmed owner and is preserved independently.
pub(in crate::review) fn validate_proposal_base(state: &State, id: &str) -> Result<(), Error> {
    let proposal = state
        .proposals
        .iter()
        .find(|row| row.id == id)
        .ok_or(Error::Missing)?;
    let mut base = proposal.base_metadata.clone();
    keep_path_access(&mut base, &state.operator);
    if base != state.operator {
        return Err(Error::StaleSnapshot);
    }
    Ok(())
}

/// Protected file access changes only through its own confirmed command, so
/// metadata captured with an older proposal must not restore a stale choice.
fn keep_path_access(meta: &mut Value, current: &Value) {
    match current.get("path_access") {
        Some(access) => meta["path_access"] = access.clone(),
        None => {
            if let Some(fields) = meta.as_object_mut() {
                fields.remove("path_access");
            }
        }
    }
}

fn publication_identity(state: &State) -> String {
    policy::fingerprint(
        json!([
            state.policy_revision,
            state.operator,
            state.active.as_ref().map(review::runtime::Active::metadata)
        ])
        .to_string()
        .as_bytes(),
    )
}

/// What a candidate changes against the active policy, with its permission comparison.
pub(super) fn change_review(
    context: &Context,
    before: &State,
    candidate: &Value,
    meta: &Value,
) -> Result<Value, Error> {
    let mut candidates = BTreeMap::new();
    candidates.insert("project".into(), candidate.clone());
    let mut comparison = json!(configuration::compare_policy(
        &review::snapshots(&before.candidates)?,
        &review::snapshots(&candidates)?,
    )?);
    if let Some(prefixes) = review::codex_prefix_changes(&before.candidates, &candidates)? {
        comparison["codex_prefixes"] = prefixes;
    }
    if let Some(lines) = review::formatting_changes(&before.candidates["project"], candidate) {
        comparison["formatting"] = lines;
    }
    Ok(
        json!({"operation":"change policy","project_id":context.project_id,
        "changes":super::changes::diff::between(&before.operator, &before.candidates["project"], meta, candidate),
        "packs":crate::review::packs::summaries(meta),"candidate":candidate,"comparison":comparison}),
    )
}

pub(super) fn reconcile(context: &Context, expected: &State) -> Result<Value, Error> {
    Store::open(Path::new(&context.project_root))?.inspect_locked(
        context,
        &|| Ok(()),
        |state| {
            if state.policy_revision != expected.policy_revision
                || state.operator != expected.operator
                || json!(state.active) != json!(expected.active)
            {
                return Err(Error::Invalid(
                    "project changed during confirmation; review again",
                ));
            }
            let meta = metadata(state);
            repair_projections(
                context,
                &projections(&state.candidates["project"], &meta)?,
                &meta,
            )
        },
    )?;
    status(context)
}

#[cfg(test)]
mod tests;
