//! Reuse reviewed policy across checkouts without transferring request authority.
use super::{Error, State, Store, bootstrap, history, objects, private_directory, private_files};
use crate::review::{Context, runtime};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

const LIMIT: usize = super::MAX_STATE;
const RELOCATION_ATTEMPTS: usize = 3;
pub(super) mod consent;
mod reactivation;
pub(super) use reactivation::reactivate;

pub(super) fn directory() -> Result<PathBuf, Error> {
    #[cfg(test)]
    return if let Some(directory) = consent::fixture_directory() {
        Ok(directory)
    } else {
        Ok(std::env::temp_dir()
            .canonicalize()?
            .join(format!("shield-reviewed-policies-{}", std::process::id())))
    };
    #[cfg(not(test))]
    {
        #[cfg(windows)]
        let base = std::env::var_os("LOCALAPPDATA").map(|home| PathBuf::from(home).join(".shield"));
        #[cfg(not(windows))]
        let base = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".shield"));
        let base = base
            .filter(|path| path.is_absolute())
            .ok_or(Error::Invalid("user policy storage unavailable"))?;
        Ok(base.join("reviewed-policies"))
    }
}

fn key(candidate: &Value) -> Result<String, Error> {
    let authored = BTreeMap::from([
        ("config", &candidate["config"]),
        ("local_config", &candidate["local_config"]),
        ("rules", &candidate["rules"]),
        ("local_rules", &candidate["local_rules"]),
    ]);
    crate::json::fingerprint(&("reviewed-policy-v1", authored))
        .map_err(|_| Error::Invalid("policy receipt encoding"))
}

fn authored(candidate: &Value) -> Value {
    json!({"config":candidate["config"],"local_config":candidate["local_config"],"rules":candidate["rules"],"local_rules":candidate["local_rules"]})
}

/// A config-only clone can reuse this developer's exact reviewed policy. A
/// teammate's checked-in files cannot manufacture a private user receipt.
pub(super) fn restore(store: &Store) -> Result<(), Error> {
    if store.checkpoint_present()? {
        return Ok(());
    }
    let config = PathBuf::from(&store.root).join(".shield/config.yml");
    if !config.try_exists()? {
        return Ok(());
    }
    let directory = directory()?;
    if !directory.try_exists()? {
        return Ok(());
    }
    private_files::directory(&directory, false)?;
    let Ok(candidate) = crate::review::integrations::project_sources::authored(&config) else {
        // Without a local checkpoint, malformed source stays an unreviewed
        // first-use candidate. Status can still explain that state.
        return Ok(());
    };
    if let Some((candidates, evaluator, metadata, checkpoints, _pins)) =
        consent::restore_record(store, &candidate)?
    {
        return match Store::initialize_with(
            std::path::Path::new(&store.root),
            candidates,
            evaluator.revision(),
            metadata,
            Some(super::InitialActivation::Reused(&evaluator)),
            checkpoints,
        ) {
            Ok(_) | Err(Error::Invalid("review project already enrolled")) => Ok(()),
            Err(error) => Err(error),
        };
    }
    let path = directory.join(format!("{}.json", key(&candidate)?));
    let receipt = match private_files::read(&path, LIMIT) {
        Ok(value) => value,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if receipt["schema_version"] != 1
        || authored(&receipt["candidate"]) != candidate
        || !receipt["metadata"].is_object()
    {
        return Err(Error::Invalid("invalid reviewed policy receipt"));
    }
    if !crate::review::integrations::project_sources::matches_artifacts(
        &config,
        &receipt["candidate"],
    )
    .map_err(|_| Error::Invalid("cannot validate project artifacts against reviewed policy"))?
    {
        return Ok(());
    }
    if !consent::matches_legacy(&config, &receipt["candidate"], &receipt["metadata"])? {
        return Ok(());
    }
    let candidates = BTreeMap::from([("project".into(), receipt["candidate"].clone())]);
    let evaluator = crate::configuration::Evaluator::new(crate::review::snapshots(&candidates)?)?;
    let registry_checkpoints = receipt
        .get("registry_checkpoints")
        .map(|value| serde_json::from_value::<BTreeMap<String, Value>>(value.clone()))
        .transpose()
        .map_err(|_| Error::Invalid("invalid portable registry checkpoints"))?
        .unwrap_or_default();
    // Initialization serializes competing adopters with the destination writer.
    match Store::initialize_with(
        std::path::Path::new(&store.root),
        candidates,
        evaluator.revision(),
        consent::metadata(&receipt["metadata"])?,
        Some(super::InitialActivation::Reused(&evaluator)),
        registry_checkpoints,
    ) {
        Ok(_) | Err(Error::Invalid("review project already enrolled")) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Old absolute roots are provenance, never a reason to demand another human
/// policy review. Validate the saved authority before deriving a fresh runtime.
pub(super) fn relocate(store: &Store, mut state: State) -> Result<State, Error> {
    let observed = super::incarnation::observe(store, true).map_err(|_| {
        Error::Invalid("operation continuity unavailable; retained checkpoint unchanged")
    })?;
    if super::incarnation::matches(&state, &observed) {
        state = super::administration::seed(store, state)?;
    }
    if state.project_root == store.root && super::incarnation::matches(&state, &observed) {
        return Ok(state);
    }
    let owned = *store
        .writer_owned
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        == Some(std::thread::current().id());
    if owned {
        // Projection owners acquire before authority. An outer runtime startup
        // can normalize this copied epoch without reversing that lock order.
        return Err(Error::StaleSnapshot);
    }
    let _projection =
        crate::review::publication::ProjectionLease::acquire(std::path::Path::new(&store.root))?;
    for _ in 0..RELOCATION_ATTEMPTS {
        let bytes = store.checkpoint_bytes()?;
        let current = super::upgrades::checkpoint(&bytes)?;
        if current.project_root == store.root && super::incarnation::matches(&current, &observed) {
            return Ok(current);
        }
        // Source validation and disposable cache preparation do not hold the
        // authority writer. Recheck the exact checkpoint before publishing.
        let mut prepared = prepare_relocation(store, current, &bytes, &observed)?;
        if let Some(proof) = prepared.administration_copy.take() {
            prepared.administration_copy = Some(proof.bind(&prepared));
        }
        let registry_guard = if prepared.active.is_some() {
            let effective = crate::configuration::recorded_effective(
                &prepared.candidates["project"],
                &prepared.policy_revision,
            )?;
            match crate::review::registry_trust::guard_effective(&prepared.operator, &effective) {
                Ok(guard) => {
                    guard.bind_modes(&mut prepared.operator)?;
                    Some(guard)
                }
                // Discard the unpublished activation. The next preparation sees
                // this authority refusal and records reviewed inactive consent.
                Err(_) => continue,
            }
        } else {
            None
        };
        let (retained, _source_pins) = store.retain_candidates(&prepared.candidates)?;
        prepared.candidates = retained;
        if let Some(active) = &mut prepared.active {
            active.candidates = prepared.candidates.clone();
        }
        let prepared_consent = prepared
            .active
            .as_ref()
            .and_then(|active| active.project_candidate().ok())
            .and_then(|reference| store.prepare_policy_consent(reference));
        super::stamp_policy_consent(&mut prepared.operator, prepared_consent.as_ref());
        #[cfg(test)]
        super::tests::observe(super::tests::Stage::RelocationPrepared);
        let lock = (!owned).then(|| store.lock(&|| Ok(()))).transpose()?;
        if store.checkpoint_bytes()? != bytes {
            drop(lock);
            continue;
        }
        if super::incarnation::observe(store, false).ok().as_deref() != Some(observed.as_str()) {
            return Err(Error::StaleSnapshot);
        }
        // Reader slots are disposable local runtime state, never imported
        // authority. Repair copied directory permissions without policy review.
        super::private_directory(&store.directory)?;
        let _ = history::prepare_readers(store);
        bootstrap::retain(store, &bytes)?;
        match store.write(&mut prepared) {
            Err(Error::StaleSnapshot) => {
                drop(lock);
                continue;
            }
            result => result?,
        }
        drop(registry_guard);
        let recovery = prepared.operator["bootstrap"]["checkpoint_recovery"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        bootstrap::cleanup(store, &recovery);
        if let Some(lock) = lock {
            lock.release()?;
        }
        store.finish_policy_consent(
            prepared_consent,
            prepared.active.as_ref().map(runtime::Active::activation_id),
        );
        return Ok(prepared);
    }
    Err(Error::StaleSnapshot)
}

#[expect(
    clippy::too_many_lines,
    reason = "one validated relocation derives the new epoch while preserving exact inactive/active consent and recovery evidence"
)]
pub(super) fn prepare_relocation(
    store: &Store,
    mut current: State,
    bytes: &[u8],
    incarnation: &str,
) -> Result<State, Error> {
    // Validate against its recorded root before changing any bindings.
    Store::validate_checkpoint(&current)?;
    let mut reviewed = current
        .active
        .as_ref()
        .map(|active| {
            Ok::<_, Error>((
                store.captured_candidate(active.project_candidate()?)?,
                active.revision().to_owned(),
            ))
        })
        .transpose()?;
    if reviewed.is_none() && crate::review::registry_trust::inactive_reviewed(&current) {
        let candidate = store.captured_candidate(
            current
                .candidates
                .get("project")
                .ok_or(Error::CandidateUnavailable)?,
        )?;
        reviewed = Some((candidate, current.policy_revision.clone()));
    }
    // Only a proven native-directory rename carries local administrative
    // intent. A new directory/host must prove exact retained policy from the
    // user's independent private store before any compilation or activation.
    if !super::incarnation::matches(&current, incarnation) {
        let administration = super::administration::copied(&current);
        if let Some((candidate, revision)) = reviewed.take() {
            reviewed = Some((adopt_private_retained(&mut current, &candidate)?, revision));
        } else {
            // An inactive preview is not a source of portable policy or admin
            // authority. Preserve authored candidates for explicit review.
            current.operator = unreviewed_metadata();
        }
        reset_unproven_administration(&mut current)?;
        if let Some(proof) = administration {
            current.administration_copy = Some(proof.restore(&mut current));
        }
    }
    let old_root = current.project_root.clone();
    if current.operator.get("policy_lineage").is_none() {
        current.operator["policy_lineage"] = json!(current.project_id);
    }
    if current.history_owner.is_none() {
        current.history_owner = Some(current.project_id.clone());
    }
    let history_recovery = predecessors(
        current.history_generation.strip_prefix("journal-"),
        &current.operator["bootstrap"]["history_recovery"],
    );
    let checkpoint_recovery = predecessors(
        Some(&crate::policy::fingerprint(bytes)),
        &current.operator["bootstrap"]["checkpoint_recovery"],
    );
    current.project_root.clone_from(&store.root);
    current.project_id = crate::policy::fingerprint(
        history::identifier("project", &crate::policy::fingerprint(b"portable"))?.as_bytes(),
    );
    current.history_available = true;
    if let Some((candidate, revision)) = &reviewed {
        current.candidates = BTreeMap::from([("project".into(), candidate.clone())]);
        current.policy_revision.clone_from(revision);
    }
    current.active = None;
    clear_transient_authority(&mut current);
    current
        .operator
        .as_object_mut()
        .ok_or(Error::Invalid("operator metadata"))?
        .remove("runtime_incarnation");
    super::incarnation::stamp(&mut current, incarnation);
    let mut launch_roots = vec![json!(old_root)];
    for path in current.operator["portable_launch_roots"]
        .as_array()
        .into_iter()
        .flatten()
        .take(1)
    {
        if !launch_roots.contains(path) {
            launch_roots.push(path.clone());
        }
    }
    current.operator["portable_launch_roots"] = json!(launch_roots);
    for field in [
        "bootstrap",
        "integration_review",
        "integration_pending",
        "integration_publication",
        "compiled_cache_root",
        "portable_policy",
        "policy_reuse",
        "policy_consent",
        "upgrade_recovery",
    ] {
        current
            .operator
            .as_object_mut()
            .ok_or(Error::Invalid("operator metadata"))?
            .remove(field);
    }
    current.operator["bootstrap"] =
        json!({"history_recovery":history_recovery,"checkpoint_recovery":checkpoint_recovery});
    let context = Context::new(current.project_id.clone(), store.root.clone(), "cli");
    if reviewed.is_none() {
        // A preview has no policy consent to reuse or activate. Only its local
        // operation epoch changes; authored candidates remain inactive.
        let event = current.event("runtime_incarnation_reset", &context, json!({}),json!({}),
            json!({"previous_project_root":old_root,"previous_checkpoint":crate::policy::fingerprint(bytes)}));
        current.record(event);
        Store::validate_checkpoint(&current)?;
        return Ok(current);
    }
    let evaluator = crate::configuration::Evaluator::new_for_revision(
        crate::review::snapshots(&current.candidates)?,
        &current.policy_revision,
    )?;
    match crate::review::registry_trust::guard_effective(&current.operator, evaluator.effective()) {
        Ok(guard) => guard.bind_modes(&mut current.operator)?,
        Err(error) => {
            crate::review::registry_trust::record_inactive_reuse(&mut current, &context, &error);
            Store::validate_checkpoint(&current)?;
            return Ok(current);
        }
    }
    let described = crate::review::cache::describe_candidate(
        &context,
        &current.candidates,
        &current.policy_revision,
        &current.operator,
        &evaluator,
    )?;
    let compiled = match crate::review::cache::build_candidate(
        &context,
        &current.candidates,
        &current.policy_revision,
        &current.operator,
        &evaluator,
    ) {
        Ok(compiled) => compiled,
        Err(Error::Io(_) | Error::CacheUnavailable) => described,
        Err(error) => return Err(error),
    };
    runtime::publish_confirmed(
        &mut current,
        &context,
        None,
        compiled,
        "reviewed-policy-reuse",
    );
    let mut event = current.event("project_relocated", &context, json!({}), json!({}), json!({"previous_project_root":old_root,"previous_checkpoint":crate::policy::fingerprint(bytes)}));
    event["operator"] = json!({"mechanism":"reviewed-policy-reuse"});
    current.record(event);
    Store::validate_checkpoint(&current)?;
    Ok(current)
}

fn adopt_private_retained(state: &mut State, candidate: &Value) -> Result<Value, Error> {
    let mut record = consent::retained_record(candidate)?.ok_or(Error::Invalid(
        "retained policy has no private reviewed-content proof; checkpoint unchanged",
    ))?;
    if record.metadata["policy_lineage"] != state.policy_lineage() {
        // Index3 has one context per source variant, not one per independent
        // origin. Recognized content must not attach an earlier project's copy
        // to the latest identical-policy initialization's unrelated lineage.
        record.metadata["policy_lineage"] = json!(crate::policy::fingerprint(
            history::identifier(
                "lineage",
                &crate::policy::fingerprint(b"private-source-reuse")
            )?
            .as_bytes()
        ));
        record.metadata["portable_lineage_unverified"] = json!(true);
    }
    state.operator = record.metadata;
    state.registry_checkpoints = record.checkpoints;
    Ok(record.candidate)
}

fn reset_unproven_administration(state: &mut State) -> Result<(), Error> {
    state.enforcement = crate::review::mode::Mode::default();
    state.operator["portable_admin_unverified"] = json!(true);
    // These copied ownership/review claims are not current publisher authority.
    for field in [
        "integration",
        "path_access",
        "mcp_rebind_pending",
        "portable_registration_epoch",
        "current_executable",
    ] {
        state
            .operator
            .as_object_mut()
            .ok_or(Error::Invalid("operator metadata"))?
            .remove(field);
    }
    Ok(())
}

fn unreviewed_metadata() -> Value {
    json!({"sources":{},"default":null,"installed":[],"projection":null})
}

/// Historical public codecs need the same origin proof before source conversion
/// or disposable cache preparation, not only after their checkpoint is written.
pub(super) fn authenticate_historical(
    store: &Store,
    state: &mut State,
    active: &Value,
    observed: &str,
) -> Result<(), Error> {
    if super::incarnation::matches(state, observed) {
        return Ok(());
    }
    if active.is_null() {
        state.operator = unreviewed_metadata();
    } else {
        if active["candidates"]
            .as_object()
            .is_none_or(|values| values.len() != 1)
        {
            return Err(Error::Invalid(
                "private reviewed-content proof does not cover historical scopes",
            ));
        }
        let source = active["candidates"]
            .get("project")
            .ok_or(Error::CandidateUnavailable)?;
        let candidate = store.captured_candidate(source)?;
        adopt_private_retained(state, &candidate)?;
    }
    reset_unproven_administration(state)
}

fn predecessors(current: Option<&str>, previous: &Value) -> Vec<String> {
    let mut retained = Vec::new();
    for value in current.into_iter().chain(
        previous
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str),
    ) {
        if retained.len() == 2 {
            break;
        }
        if !retained.iter().any(|saved| saved == value) {
            retained.push(value.to_owned());
        }
    }
    retained
}

fn clear_transient_authority(state: &mut State) {
    // Tokens are local capabilities, unlike reviewed semantic connection IDs.
    // Keep exact owned host-entry evidence for a future equivalent rebind.
    let mut rebind = false;
    if let Some(connections) = state
        .operator
        .get_mut("integration")
        .and_then(|registration| registration.get_mut("connections"))
        .and_then(Value::as_object_mut)
    {
        for owned in connections.values_mut() {
            rebind |= owned["review"].as_array().into_iter().flatten().any(|row| {
                row["source"] == "local"
                    && row["enabled"] == true
                    && row["projection"] == "private Shield launcher"
            });
            if let Some(object) = owned.as_object_mut() {
                object.insert("launches".into(), json!({}));
            }
        }
    }
    if rebind {
        state.operator["mcp_rebind_pending"] = json!(true);
    }
    state.read_requests.clear();
    state.run_requests.clear();
    state.terminal_receipts = crate::review::authorization_receipts::Registry::default();
    state.hook_requests.clear();
    state.session_grants = crate::review::session_grants::Registry::default();
    state.proposals.clear();
    state.policy_drafts.clear();
    state.dismissed_reviews.clear();
    state.events.clear();
    state.authorization_events.clear();
    state.pending_history.clear();
    state.history_rewrite = None;
    state.history_control = false;
    state.history_evicted = false;
}

/// Upgrade an existing installation on first use. The marker belongs only to
/// private runtime metadata; it neither changes policy nor grants authority.
pub(super) fn seed(store: &Store, state: State) -> Result<State, Error> {
    let Some(active) = &state.active else {
        return Ok(state);
    };
    // Seeding cannot manufacture origin authority from a self-consistent
    // copied checkpoint. Only this already-proven native namespace may export
    // a legacy activation to the developer-private consent store.
    let observed = super::incarnation::observe(store, false).map_err(|_| {
        Error::Invalid("operation continuity unavailable; retained checkpoint unchanged")
    })?;
    if !super::incarnation::matches(&state, &observed) {
        return Err(Error::Invalid(
            "private policy consent requires proven native continuity",
        ));
    }
    if state.operator["portable_policy"] == active.activation_id()
        && state.operator.get("policy_lineage").is_some()
        && state.operator["policy_consent_version"] == 2
    {
        return Ok(state);
    }
    let owned = *store
        .writer_owned
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        == Some(std::thread::current().id());
    if owned {
        return Ok(state);
    }
    let prepared_consent = store.prepare_policy_consent(active.project_candidate()?);
    let lock = (!owned).then(|| store.lock(&|| Ok(()))).transpose()?;
    let mut current = super::upgrades::checkpoint(&store.checkpoint_bytes()?)?;
    Store::validate_checkpoint(&current)?;
    if !super::incarnation::matches(&current, &observed)
        || super::incarnation::observe(store, false).ok().as_deref() != Some(observed.as_str())
    {
        return Err(Error::StaleSnapshot);
    }
    if current.active.as_ref().is_some_and(|active| {
        current.operator["portable_policy"] != active.activation_id()
            || current.operator.get("policy_lineage").is_none()
            || current.operator["policy_consent_version"] != 2
    }) {
        current.remember_policy = true;
        if current.operator.get("policy_lineage").is_none() {
            current.operator["policy_lineage"] = json!(current.project_id);
        }
        super::stamp_policy_consent(&mut current.operator, prepared_consent.as_ref());
        store.write(&mut current)?;
    }
    if let Some(lock) = lock {
        lock.release()?;
    }
    store.finish_policy_consent(
        prepared_consent,
        current.active.as_ref().map(runtime::Active::activation_id),
    );
    Ok(current)
}

#[cfg(test)]
mod tests;
