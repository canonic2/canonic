//! Reuse reviewed policy in any folder with the same files, and convert
//! checkpoints written by the earlier identity-bound format once.
use super::{Error, State, Store, authority, bootstrap, objects, private_directory, private_files};
use crate::review::{Context, runtime};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

const LIMIT: usize = super::MAX_STATE;
pub(super) mod consent;
#[cfg(test)]
mod tests;

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

/// Fields only the earlier identity-bound format wrote. Their presence marks a
/// checkpoint that still needs its one conversion.
const LEGACY_FIELDS: &[&str] = &[
    "runtime_incarnation",
    "administration_origin",
    "portable_admin_unverified",
    "portable_lineage_unverified",
];

pub(super) fn legacy(state: &State) -> bool {
    state.recorded_root.is_some()
        || LEGACY_FIELDS
            .iter()
            .any(|field| state.operator.get(*field).is_some())
}

/// What the earlier format's own evidence proves. A pre-continuity record at
/// its recorded root or a matching identity stamp shows this is the
/// installation it was written for, upgraded in place. A committed
/// administration receipt for exactly these settings proves only those
/// settings and the registration, never the policy. Proven authority becomes
/// this user's content-keyed consent once.
fn proof(store: &Store, state: &State) -> authority::Legacy {
    if !legacy(state) {
        authority::Legacy::Unproven
    } else if installation(store, state) {
        authority::Legacy::Installation
    } else if super::administration::committed(state) {
        authority::Legacy::Administration
    } else {
        authority::Legacy::Unproven
    }
}

fn installation(store: &Store, state: &State) -> bool {
    let continuity_free = [
        "runtime_incarnation",
        "administration_origin",
        "policy_consent_key",
    ]
    .iter()
    .all(|field| state.operator.get(*field).is_none());
    (continuity_free
        && state.recorded_root.as_deref() == Some(store.root.as_str())
        && private_files::directory(&store.directory, false).is_ok())
        || super::incarnation::recorded_here(store, state)
}

fn strip(state: &mut State) -> Result<(), Error> {
    if state.recorded_root.is_some() {
        // Re-bind while the root that bound it is still known.
        runtime::Active::rebind_without_root(state)?;
    }
    state.recorded_root = None;
    if let Some(fields) = state.operator.as_object_mut() {
        for field in LEGACY_FIELDS {
            fields.remove(*field);
        }
    }
    Ok(())
}

/// Admit a checkpoint's authority in memory. A legacy checkpoint is
/// recognized, stripped of identity fields and, when `seed` is set and its
/// evidence proves it, recorded as this user's consent.
pub(super) fn admit(store: &Store, mut state: State, seed: bool) -> Result<State, Error> {
    let legacy = proof(store, &state);
    strip(&mut state)?;
    crate::review::read_approval::retain_local(&mut state);
    crate::review::run_approval::retain_local(&mut state);
    authority::admit(store, &mut state, &authority::Admission { legacy, seed })?;
    Ok(state)
}

/// Convert a legacy checkpoint once under the writer: keep a private recovery
/// copy, record proven authority as consent, then publish the checkpoint
/// without the folder's path or identity. A reader inside an existing writer
/// gets [`Error::StaleSnapshot`], so the writer releases and retries after
/// this conversion.
pub(super) fn convert(store: &Store) -> Result<State, Error> {
    let owned = *store
        .writer_owned
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        == Some(std::thread::current().id());
    if owned {
        // Conversion keeps its recovery copy and event under its own lock;
        // an outer writer releases and retries after it.
        return Err(Error::StaleSnapshot);
    }
    let lock = store.lock(&|| Ok(()))?;
    let bytes = store.checkpoint_bytes()?;
    let current = store.decode(&bytes)?;
    if !legacy(&current) {
        lock.release()?;
        return admit(store, current, false);
    }
    let proven = proof(store, &current);
    let mut converted = admit(store, current, true)?;
    Store::validate_checkpoint(&converted)?;
    bootstrap::retain(store, &bytes)?;
    let context = Context::new(converted.project_id.clone(), store.root.clone(), "cli");
    let event = converted.event(
        "checkpoint_converted",
        &context,
        json!({}),
        json!({}),
        json!({"previous_checkpoint":crate::policy::fingerprint(&bytes),"authority":match proven {
            authority::Legacy::Installation => "recorded",
            authority::Legacy::Administration => "settings",
            authority::Legacy::Unproven => "defaults",
        }}),
    );
    converted.record(event);
    store.write(&mut converted)?;
    lock.release()?;
    // Only a proven installation's policy is exported for reuse.
    if proven == authority::Legacy::Installation {
        return seed(store, converted);
    }
    Ok(converted)
}

/// Export a converted installation's active policy to the reusable
/// per-user policy store, so clones of the same files reuse it.
fn seed(store: &Store, state: State) -> Result<State, Error> {
    let Some(active) = &state.active else {
        return Ok(state);
    };
    if state.operator["portable_policy"] == active.activation_id()
        && state.operator.get("policy_lineage").is_some()
        && state.operator["policy_consent_version"] == 2
    {
        return Ok(state);
    }
    let prepared_consent = store.prepare_policy_consent(active.project_candidate()?);
    let lock = store.lock(&|| Ok(()))?;
    // Admitted like any read, so this write never records consent for content
    // that changed on disk after the conversion.
    let mut current = admit(store, store.decode(&store.checkpoint_bytes()?)?, false)?;
    Store::validate_checkpoint(&current)?;
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
    lock.release()?;
    store.finish_policy_consent(
        prepared_consent,
        current.active.as_ref().map(runtime::Active::activation_id),
    );
    Ok(current)
}
