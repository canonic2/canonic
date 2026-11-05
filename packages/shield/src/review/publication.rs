//! Confirmed policy publication, proposal binding and recoverable projections.
#[cfg(test)]
use super::project::load;
use super::project::{metadata, root, status};
use crate::{
    configuration, policy,
    review::{self, Context, Error, model::State, storage::Store},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
pub(super) mod projections;
use projections::{
    digest, edited_projections, file_hashes, projection_changed, projection_fingerprint,
    projection_hashes, projections, repair_projections,
};

/// Host confirmation provenance supplied only after the interface validates its approval path.
/// This records the mechanism; it does not grant authority by itself.
#[derive(Clone, Copy)]
pub(super) enum HostConfirmation {
    Elicitation,
    ToolPermission,
}
/// Reviewed plan and optional verified durable source reference. CLI candidates
/// retain their source during publication; MCP references were verified at apply.
#[derive(Clone, Copy)]
pub(in crate::review) struct Prepared<'a> {
    pub(in crate::review) source: Option<&'a Value>,
    pub(in crate::review) evaluator: Option<&'a configuration::Evaluator>,
}

/// Publish the exact candidate whose plan survived the operator's review.
pub(super) fn commit_prepared(
    context: &Context,
    before: &State,
    candidate: Value,
    meta: Value,
    proposal: Option<&str>,
    manual_fingerprint: Option<&str>,
    evaluator: &configuration::Evaluator,
) -> Result<Value, Error> {
    commit_confirmed(
        context,
        before,
        candidate,
        meta,
        proposal,
        manual_fingerprint,
        "operator-cli",
        Some(Prepared {
            source: None,
            evaluator: Some(evaluator),
        }),
    )
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
        let Ok(recovered) = Store::open(project).and_then(|store| store.read_current()) else {
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

#[cfg(test)]
pub(super) fn commit(
    context: &Context,
    before: &State,
    candidate: Value,
    meta: Value,
    proposal: Option<&str>,
) -> Result<Value, Error> {
    commit_reviewed(context, before, candidate, meta, proposal, None)
}

#[cfg(test)]
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
        None,
    )
}

pub(in crate::review) fn commit_mcp(
    context: &Context,
    before: &State,
    candidate: Value,
    meta: Value,
    proposal: &str,
    confirmation: HostConfirmation,
    prepared: Prepared<'_>,
) -> Result<Value, Error> {
    commit_confirmed(
        context,
        before,
        candidate,
        meta,
        Some(proposal),
        None,
        confirmation.mechanism(),
        Some(prepared),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "Publication keeps the reviewed base, approval provenance and verified source/plan explicit"
)]
fn commit_confirmed(
    context: &Context,
    before: &State,
    candidate: Value,
    mut meta: Value,
    proposal: Option<&str>,
    manual_fingerprint: Option<&str>,
    mechanism: &str,
    prepared: Option<Prepared<'_>>,
) -> Result<Value, Error> {
    let _trace = crate::diagnostics::trace::span("publication");
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
    let owned;
    let evaluator = if let Some(evaluator) = prepared.and_then(|prepared| prepared.evaluator) {
        evaluator
    } else {
        owned = configuration::Evaluator::new(review::snapshots(&candidates)?)?;
        &owned
    };
    if mechanism != "operator-cli" {
        validate_mcp_change(before, &candidates["project"])?;
    }
    let files = projections(&candidates["project"], &meta)?;
    meta["projection"] = json!(digest(&files));
    meta["projection_file_hashes"] = json!(file_hashes(&files));
    let expected = publication_identity(before);
    let store = Store::open(Path::new(&context.project_root))?;
    let source = prepared
        .and_then(|prepared| prepared.source)
        .unwrap_or(&candidates["project"]);
    let sources = BTreeMap::from([("project".into(), source.clone())]);
    let compiled =
        review::cache::build_candidate(context, &sources, evaluator.revision(), &meta, evaluator)?;
    let candidate_identity = review::storage::sources::candidate_identity(source);
    let candidate_digest = review::storage::sources::map_digest(&sources);
    // A selected proposal already owns a durable, verified source object. Publish
    // that exact reference under the lock instead of serializing and retaining it again.
    let retained = if proposal.is_none() {
        Some(store.retain_candidates(&candidates)?)
    } else {
        None
    };
    let mut attempted_activation = None;
    let mut preserved = BTreeSet::new();
    let result = store.update_checkpoint(context, &|| Ok(()), |state| {
        preserved =
            verify_confirmation_files(context, before, manual_fingerprint, &files, proposal)?;
        if expected != publication_identity(state) {
            return Err(Error::Invalid("project changed; review again"));
        }
        let candidates = if let Some(id) = proposal {
            let row = state
                .proposals
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or(Error::Missing)?;
            mark_applied(
                row,
                &state.policy_revision,
                prepared.and_then(|prepared| prepared.source),
                &candidate_identity,
                &candidate_digest,
                mechanism,
            )?;
            BTreeMap::from([("project".into(), row.candidate.clone())])
        } else {
            retained.ok_or(Error::CandidateUnavailable)?
        };
        prepare_projection_repair(context, before, &files, &mut meta, &preserved)?;
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
        Err(error) => recover_activation(
            error,
            &store,
            attempted_activation.as_deref(),
            &meta,
            evaluator.revision(),
        )?,
    };
    let mut receipt = receipt;
    let mut written = files;
    written.retain(|name, _| !preserved.contains(name));
    repair_after_commit(context, &store, evaluator, &written, &meta, &mut receipt);
    receipt["changes"] = changes;
    Ok(receipt)
}

/// Retain repair evidence without treating preserved edits as generated output.
fn prepare_projection_repair(
    context: &Context,
    before: &State,
    files: &BTreeMap<String, String>,
    meta: &mut Value,
    preserved: &BTreeSet<String>,
) -> Result<(), Error> {
    if metadata(before)["projection"] != meta["projection"] {
        meta["projection_previous"] = projection_hashes(context, files)?;
    }
    // A later repair may replace only what Shield wrote, never a kept edit.
    if let Some(previous) = meta["projection_previous"].as_object_mut() {
        for name in preserved {
            previous.insert(
                name.clone(),
                before.operator["projection_file_hashes"][name].clone(),
            );
        }
    }
    // Missing output is ordinarily recoverable after an interrupted write.
    // Remember kept edits so repair can distinguish a preserved deletion.
    if preserved.is_empty() {
        if let Some(meta) = meta.as_object_mut() {
            meta.remove("projection_preserved");
        }
    } else {
        meta["projection_preserved"] = json!(preserved);
    }
    Ok(())
}

/// Refuse a commit that would overwrite project files edited by hand, and
/// return the edited files it leaves unchanged. Those stay on disk, pending the
/// operator's review. Ordinary proposals use retained policy; adoption and
/// manual reviews bind the exact files captured for their review.
fn verify_confirmation_files(
    context: &Context,
    before: &State,
    manual_fingerprint: Option<&str>,
    files: &BTreeMap<String, String>,
    proposal: Option<&str>,
) -> Result<BTreeSet<String>, Error> {
    const CHANGED: Error =
        Error::Invalid("project files changed during confirmation; review again");
    if let Some(row) = proposal
        .and_then(|id| before.proposals.iter().find(|row| row.id == id))
        .filter(|row| row.reviewed_files.is_some())
    {
        if Some(super::changes::project_files::fingerprint(
            context,
            before,
            &row.candidate,
            &row.candidate_metadata,
        )?) != row.reviewed_files
        {
            return Err(Error::StaleSnapshot);
        }
        return Ok(BTreeSet::new());
    }
    if let Some(fingerprint) = manual_fingerprint {
        if projection_fingerprint(context, before)? != fingerprint {
            return Err(CHANGED);
        }
        return Ok(BTreeSet::new());
    }
    let Some(edited) = edited_projections(context, before)? else {
        if projection_changed(context, before)? {
            return Err(CHANGED);
        }
        return Ok(BTreeSet::new());
    };
    let recorded = &before.operator["projection_file_hashes"];
    if edited.iter().any(|name| {
        files
            .get(name)
            .map(|bytes| json!(policy::fingerprint(bytes.as_bytes())))
            != recorded.get(name).cloned()
    }) {
        return Err(Error::Invalid(
            "this change would overwrite unreviewed edits to .shield files; use policy_propose with change {kind: project_files_adopt} alone and policy_apply to review those edits, then propose the change again",
        ));
    }
    Ok(edited)
}

fn mark_applied(
    row: &mut super::model::Proposal,
    revision: &str,
    source: Option<&Value>,
    candidate_identity: &str,
    candidate_digest: &str,
    mechanism: &str,
) -> Result<(), Error> {
    if row.status != "pending"
        || row.policy_revision != revision
        || source.is_some_and(|source| row.candidate != *source)
        || review::storage::sources::candidate_identity(&row.candidate) != candidate_identity
        || review::storage::sources::map_digest(&BTreeMap::from([(
            "project".into(),
            row.candidate.clone(),
        )])) != candidate_digest
    {
        return Err(Error::Invalid("proposal changed or is stale"));
    }
    row.status = "applied".into();
    row.operator = Some(json!({"operator_id":null,"display_name":null,"mechanism":mechanism}));
    Ok(())
}

fn recover_activation(
    error: Error,
    store: &Store,
    attempted_activation: Option<&str>,
    metadata: &Value,
    revision: &str,
) -> Result<Value, Error> {
    let recovered = store.read_current()?;
    if attempted_activation.is_none_or(|id| {
        recovered
            .active
            .as_ref()
            .is_none_or(|active| active.activation_id() != id)
    }) || recovered.operator != *metadata
        || recovered.policy_revision != revision
    {
        return Err(error);
    }
    Ok(json!({"committed":true,"policy_revision":recovered.policy_revision,"repair_needed":true}))
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
    let _trace = crate::diagnostics::trace::span("publication.repair");
    let mut prefix_error = None;
    let repair = store.inspect_checkpoint(context, &|| Ok(()), |current| {
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
