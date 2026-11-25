//! Writes a reviewed edit to the policy files. Every CLI command and accepted
//! MCP proposal changes policy this way: check that the files still hold what
//! the edit was prepared from, write the files that change, remove those the
//! edit no longer produces, and bring the review state, the Codex prefix rules
//! and registered agents' MCP entries up to the written files.
use crate::{
    configuration::Evaluator,
    review::{
        Checkpoint, Context, Error,
        changes::{diff, project_files},
        integrations::{self, project_sources},
        model::{State, Texts},
        publication::{ProjectionLease, mcp_changed, projections},
        registry_trust,
        storage::Store,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

/// The policy files an edit was prepared from: each file's hash, or `None`
/// when it was absent.
pub(in crate::review) type Base = BTreeMap<String, Option<String>>;

/// The operator metadata a policy change produces. Everything else in review
/// state is current state's, never an older copy carried by the edit.
const POLICY_FIELDS: [&str; 3] = ["installed", "shared_installed", "disabled"];

/// A proposal writes exactly the files the person reviewed. Every other file
/// it recorded is an input the edit leaves unchanged.
fn renders_as_reviewed(
    files: &BTreeMap<String, String>,
    reviewed: &BTreeMap<String, Texts>,
) -> Result<(), Error> {
    let rendered_differs = files
        .iter()
        .filter(|(name, _)| project_sources::digest_named(name).is_none())
        .any(|(name, text)| reviewed.get(name).and_then(|t| t.after.as_ref()) != Some(text));
    let unrendered_changes = reviewed
        .iter()
        .any(|(name, t)| !files.contains_key(name) && t.after.is_some() && t.after != t.before);
    if rendered_differs || unrendered_changes {
        return Err(Error::StaleSnapshot);
    }
    Ok(())
}

/// The one concurrency check, under the writer lock: the files still hold
/// what the edit was prepared from, and every file a proposal recorded still
/// holds the text it was reviewed against.
fn still_as_reviewed(
    context: &Context,
    base: &Base,
    reviewed: Option<&BTreeMap<String, Texts>>,
) -> Result<(), Error> {
    if hashes(context, base.keys().cloned())? != *base {
        return Err(Error::StaleSnapshot);
    }
    let root = Path::new(&context.project_root);
    for (name, texts) in reviewed.into_iter().flatten() {
        if project_files::read_file(root, name)? != texts.before {
            return Err(Error::StaleSnapshot);
        }
    }
    Ok(())
}

/// Current operator metadata with the policy fields `meta` produces, and its
/// staged connection publication when this edit staged one.
fn with_policy_fields(mut next: Value, meta: &Value, staged: bool) -> Value {
    let publication = ["integration_pending", "integration_publication"];
    let fields = POLICY_FIELDS
        .into_iter()
        .chain(publication.into_iter().filter(|_| staged));
    for field in fields {
        match meta.get(field) {
            Some(value) => next[field] = value.clone(),
            None => {
                if let Some(fields) = next.as_object_mut() {
                    fields.remove(field);
                }
            }
        }
    }
    next
}

/// A reviewed change from `before`'s policy to `candidate`.
pub(in crate::review) struct Edit<'a> {
    pub(in crate::review) before: &'a State,
    pub(in crate::review) candidate: Value,
    pub(in crate::review) meta: Value,
    /// The files as the person reviewed them; see [`base`].
    pub(in crate::review) base: Base,
    /// The candidate's plan when the review already prepared it.
    pub(in crate::review) evaluator: Option<&'a Evaluator>,
    /// For a proposal, the file texts the person reviewed; the edit writes
    /// exactly these or nothing.
    pub(in crate::review) reviewed: Option<&'a BTreeMap<String, Texts>>,
}

/// The policy files an edit from `before` to `candidate` touches, each with
/// its text now and the text the edit writes. Digest-named pack artifacts
/// are left out and kept in the per-user store, which serves every checkout.
pub(in crate::review) fn texts(
    context: &Context,
    before: &State,
    candidate: &Value,
    meta: &Value,
) -> Result<BTreeMap<String, Texts>, Error> {
    for bundle in [&before.candidates["project"], candidate] {
        project_sources::keep_bundle_artifacts(bundle).map_err(super::error)?;
    }
    let after = projections::projections(candidate, meta)?;
    // Files the configurations name that Shield does not write, such as a
    // hand-written pack, are inputs the edit leaves as they are.
    let kept = project_sources::referenced(candidate).map_err(super::error)?;
    let mut names = projections::paths(&before.candidates["project"], &before.operator)?;
    names.extend(after.keys().cloned());
    names.extend(project_sources::referenced(&before.candidates["project"]).map_err(super::error)?);
    names.extend(kept.iter().cloned());
    let root = Path::new(&context.project_root);
    let mut texts = BTreeMap::new();
    for name in names {
        if project_sources::digest_named(&name).is_some() {
            continue;
        }
        let now = project_files::read_file(root, &name)?;
        let written = match after.get(&name) {
            Some(text) => Some(text.clone()),
            None if kept.contains(&name) => now.clone(),
            None => None,
        };
        texts.insert(
            name,
            Texts {
                before: now,
                after: written,
            },
        );
    }
    Ok(texts)
}

/// The candidate one side of `texts` describes: before, or after applying.
pub(in crate::review) fn bundle(
    root: &Path,
    texts: &BTreeMap<String, Texts>,
    after: bool,
) -> Result<Value, Error> {
    let side = |name: &str| {
        texts.get(name).and_then(|texts| {
            if after {
                texts.after.clone()
            } else {
                texts.before.clone()
            }
        })
    };
    let sources =
        project_sources::from_texts(&crate::layout::bundle(root), &side).map_err(super::error)?;
    Ok(json!(sources.bundle().map_err(super::error)?))
}

/// The hashes of every policy file an edit from `before` to `candidate`
/// reads or writes, as they are now. Capture this when the edit is prepared
/// and pass it to [`write()`].
pub(in crate::review) fn base(
    context: &Context,
    before: &State,
    candidate: &Value,
    meta: &Value,
) -> Result<Base, Error> {
    let mut names = projections::paths(&before.candidates["project"], &before.operator)?;
    names.extend(projections::paths(candidate, meta)?);
    hashes(context, names.into_iter())
}

fn hashes(context: &Context, names: impl Iterator<Item = String>) -> Result<Base, Error> {
    let names: BTreeMap<String, String> = names.map(|name| (name, String::new())).collect();
    let hashes = projections::projection_hashes(context, &names)?;
    Ok(names
        .into_keys()
        .map(|name| {
            let hash = hashes[&name].as_str().map(str::to_owned);
            (name, hash)
        })
        .collect())
}

/// Write `edit`. A proposal named by `proposal` is marked applied in the same
/// state transaction, with `mechanism` recording how the person confirmed it.
///
/// # Errors
/// [`Error::StaleSnapshot`] when a file differs from the edit's base, so the
/// caller prepares the edit again and asks again; a revoked pack, an invalid
/// candidate or a connection change confirmed without a form are refused
/// before anything is written.
pub(in crate::review) fn write(
    context: &Context,
    edit: Edit<'_>,
    proposal: Option<&str>,
    mechanism: &str,
) -> Result<Value, Error> {
    write_controlled(context, edit, proposal, mechanism, &|| Ok(()))
}

pub(in crate::review) fn write_controlled(
    context: &Context,
    edit: Edit<'_>,
    proposal: Option<&str>,
    mechanism: &str,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, Error> {
    let root = Path::new(&context.project_root);
    let lease = ProjectionLease::acquire(root)?;
    lease.check(context)?;
    let _trace = crate::diagnostics::trace::span("policy.write");
    let Edit {
        before,
        candidate,
        mut meta,
        base,
        evaluator,
        reviewed,
    } = edit;
    crate::review::changes::historical::validate(&before.candidates["project"], &candidate)?;
    let changes = diff::between(
        &before.operator,
        &before.candidates["project"],
        &meta,
        &candidate,
    );
    let owned;
    let evaluator = if let Some(evaluator) = evaluator {
        evaluator
    } else {
        let candidates = BTreeMap::from([("project".into(), candidate.clone())]);
        owned = Evaluator::new(crate::review::snapshots(&candidates)?)?;
        &owned
    };
    let revision = evaluator.revision().to_owned();
    // Connection settings need a form that shows them, even on hosts that
    // otherwise approve through the tool's visible arguments.
    if mechanism == "host-tool-permission"
        && mcp_changed(&before.candidates["project"], &candidate)?
    {
        return Err(Error::ApprovalRequired);
    }
    let guard = registry_trust::guard_effective(&meta, evaluator.effective(), checkpoint)?;
    guard.bind_modes(&mut meta)?;
    let files = projections::projections(&candidate, &meta)?;
    if let Some(reviewed) = reviewed {
        renders_as_reviewed(&files, reviewed)?;
    }
    let plan = connection_plan(root, before, &candidate, evaluator)?;
    if let Some(plan) = &plan {
        integrations::activation::stage(&mut meta, plan, proposal)?;
        integrations::activation::own(&mut meta, &revision);
    }
    let store = Store::open(root)?;
    let mut enabled = true;
    let mut written = revision.clone();
    let mut receipt = store.update_checkpoint(context, &|| Ok(()), |state| {
        lease.check(context)?;
        still_as_reviewed(context, &base, reviewed)?;
        if let Some(id) = proposal {
            applied(state, context, id, mechanism)?;
        }
        publish_files(context, &files, &base)?;
        let mut next = with_policy_fields(state.operator.clone(), &meta, plan.is_some());
        guard.bind_modes(&mut next)?;
        next["projection"] = json!(projections::digest(&files));
        next["projection_file_hashes"] = json!(projections::file_hashes(&files));
        state.candidates = BTreeMap::from([("project".into(), candidate.clone())]);
        state.operator = next;
        state.policy_revision.clone_from(&revision);
        // Follow the files as they now are, including ones this edit left
        // alone, such as a local layer holding only settings.
        state.followed_files = None;
        super::sync(context, state)?;
        // The files written are the policy; the candidate's text may differ
        // in form, such as JSON rendered as YAML, so its label may differ.
        written.clone_from(&state.policy_revision);
        if plan.is_some() {
            integrations::activation::own(&mut state.operator, &written);
        }
        meta = state.operator.clone();
        enabled = state.settings.enabled();
        let receipt = json!({"schema_version":1,"committed":true,"policy_revision":written,
            "mechanism":mechanism,"proposal_id":proposal});
        record_write(state, context, proposal, &written, mechanism);
        if let Some(id) = proposal
            && let Some(row) = state.proposals.iter_mut().find(|row| row.id == id)
        {
            row.receipt = receipt.clone();
        }
        Ok(receipt)
    })?;
    receipt["changes"] = changes;
    receipt["repair_needed"] = json!(false);
    if let Err(error) =
        integrations::sync_prefix_rules(root, &meta, evaluator.codex_prefixes(), enabled)
    {
        receipt["repair_needed"] = json!(true);
        receipt["setup_error"] = json!(error.to_string());
        receipt["repair_route"] = json!("shield mcp setup");
    }
    if plan.is_some() && meta["integration_publication"]["policy_revision"] == written.as_str() {
        crate::review::publication::resume_connections_guarded(
            context,
            proposal,
            &written,
            &mut receipt,
            &lease,
        );
    }
    Ok(receipt)
}

/// Write `candidate` as a person accepting it at once would: against the
/// proposal's recorded files, or the files as they are now.
#[cfg(test)]
pub(in crate::review) fn commit_fixture(
    context: &Context,
    before: &State,
    candidate: Value,
    meta: Value,
    proposal: Option<&str>,
) -> Result<Value, Error> {
    let row = proposal.and_then(|id| before.proposals.iter().find(|row| row.id == id));
    let base = match row.and_then(|row| row.reviewed_paths.clone()) {
        Some(base) => base,
        None => base(context, before, &candidate, &meta)?,
    };
    write(
        context,
        Edit {
            before,
            candidate,
            meta,
            base,
            evaluator: None,
            reviewed: row.map(|row| &row.files).filter(|files| !files.is_empty()),
        },
        proposal,
        "operator-cli",
    )
}

/// Write the files whose bytes change and remove the policy files the edit no
/// longer produces, such as a local layer it emptied.
fn publish_files(
    context: &Context,
    files: &BTreeMap<String, String>,
    base: &Base,
) -> Result<(), Error> {
    let changed: BTreeMap<String, String> = files
        .iter()
        .filter(|(name, bytes)| {
            base.get(*name).cloned().flatten() != Some(crate::policy::fingerprint(bytes.as_bytes()))
        })
        .map(|(name, bytes)| (name.clone(), bytes.clone()))
        .collect();
    projections::materialize(context, &changed)?;
    let root = Path::new(&context.project_root);
    for (name, hash) in base {
        if hash.is_some() && !files.contains_key(name) {
            crate::configuration::portable_path(name)?;
            match std::fs::remove_file(crate::layout::bundle_file(root, name)) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into());
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Record the write in history; requests asked before it were asked of an
/// earlier policy.
fn record_write(
    state: &mut State,
    context: &Context,
    proposal: Option<&str>,
    revision: &str,
    mechanism: &str,
) {
    let event = state.event(
        "policy_files_written",
        context,
        json!({"proposal_id":proposal}),
        json!({}),
        json!({"policy_revision":revision,"mechanism":mechanism}),
    );
    state.record(event);
}

/// Mark a pending proposal applied by the person who confirmed it.
fn applied(state: &mut State, context: &Context, id: &str, mechanism: &str) -> Result<(), Error> {
    state.check_operations()?;
    let row = state
        .proposals
        .iter_mut()
        .find(|row| row.id == id)
        .ok_or(Error::Missing)?;
    if context.surface == "mcp" && !crate::review::proposal_lifecycle::owns(row, context) {
        return Err(Error::ApprovalRequired);
    }
    if row.status != "pending" {
        return Err(Error::StaleSnapshot);
    }
    crate::review::proposal_lifecycle::applied(row);
    row.operator = Some(json!({"operator_id":null,"display_name":null,"mechanism":mechanism}));
    Ok(())
}

/// The rewrite of registered agents' MCP entries this edit's connection
/// changes need, when Shield is registered with any agent.
fn connection_plan(
    root: &Path,
    before: &State,
    candidate: &Value,
    evaluator: &Evaluator,
) -> Result<Option<integrations::Plan>, Error> {
    let lock = |candidate: &Value| candidate["lock"].get("mcp").cloned();
    if before.operator["integration"].is_null()
        || lock(&before.candidates["project"]) == lock(candidate)
    {
        return Ok(None);
    }
    let previous =
        serde_json::from_value(lock(&before.candidates["project"]).unwrap_or_else(|| json!({})))
            .map_err(|_| Error::Invalid("invalid retained MCP connection settings"))?;
    integrations::activation::prepare(
        root,
        &before.operator,
        &previous,
        evaluator.effective().mcp_connections(),
        evaluator.revision(),
    )
}

#[cfg(test)]
mod tests;
