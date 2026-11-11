//! Fresh enrollment replaces no authority until an exact human-reviewed commit.
use super::{Error, State, Store, Temporary, history, private, private_directory, sync};
use crate::{
    configuration, policy,
    review::{Context, bootstrap::Plan},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

pub(in crate::review) struct Previous {
    pub(in crate::review) root: Option<String>,
    pub(in crate::review) digest: Option<String>,
    bytes: Option<Vec<u8>>,
    history_pins: Vec<String>,
    checkpoint_pins: Vec<String>,
    launch_roots: Vec<String>,
    pub(in crate::review) confirmed_launch_roots: Vec<String>,
}

/// Read only the bounded checkpoint's location header. Source policy and saved
/// approvals are not decoded, prepared or accepted as authority here.
pub(in crate::review) fn probe(project: &Path) -> Result<(PathBuf, Option<String>), Error> {
    let store = Store::location(project)?;
    let previous = match store.checkpoint_bytes() {
        Ok(bytes) => Some(location_header(&store, &bytes)?),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::symlink_metadata(Path::new(&store.root).join(".shield/config.yml"))?;
            None
        }
        Err(error) => return Err(error),
    };
    Ok((PathBuf::from(store.root), previous))
}

fn location_header(store: &Store, bytes: &[u8]) -> Result<String, Error> {
    let path = store.directory.join("state.json");
    let raw = crate::json::parse(bytes).map_err(|_| Error::StateCorrupt { path: path.clone() })?;
    let Some(schema) = raw["schema_version"].as_u64() else {
        return Err(Error::StateCorrupt { path });
    };
    if schema != 1 {
        return Err(Error::StateFormat {
            path,
            observed: schema.to_string(),
        });
    }
    // Current checkpoints record no location: the folder is where it was found.
    // An earlier format's recorded root is only validated, never trusted.
    match &raw["project_root"] {
        Value::Null => Ok(store.root.clone()),
        recorded => recorded
            .as_str()
            .filter(|root| {
                root.len() <= 4096 && !root.contains('\0') && Path::new(root).is_absolute()
            })
            .map(|_| store.root.clone())
            .ok_or(Error::StateCorrupt { path }),
    }
}

pub(in crate::review) fn inspect(project: &Path) -> Result<Previous, Error> {
    let store = Store::location(project)?;
    let bytes = match store.checkpoint_bytes() {
        Ok(bytes) => bytes,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Previous {
                root: None,
                digest: None,
                bytes: None,
                history_pins: vec![],
                checkpoint_pins: vec![],
                launch_roots: vec![],
                confirmed_launch_roots: vec![],
            });
        }
        Err(error) => return Err(error),
    };
    let root = location_header(&store, &bytes)?;
    // Validate at the declared old location without authorizing that location or
    // publishing historical conversion. State/source files still come from here.
    let saved = Store {
        directory: store.directory.clone(),
        root: root.clone(),
        writer_owned: std::sync::Mutex::new(None),
    };
    let state = match saved.decode(&bytes) {
        Ok(state) => {
            Store::validate_checkpoint(&state)?;
            state
        }
        Err(_) => super::upgrades::inspect(&saved, &bytes)?,
    };
    let mut history_pins = Vec::new();
    if let Some(digest) = state
        .history_generation
        .strip_prefix("journal-")
        .filter(|value| digest(value))
    {
        history_pins.push(digest.to_owned());
    }
    for pin in pins(&state.operator["bootstrap"]["history_recovery"]) {
        if !history_pins.contains(&pin) && history_pins.len() < 2 {
            history_pins.push(pin);
        }
    }
    let launch_roots: Vec<_> = state.operator["bootstrap"]["launch_roots"]
        .as_array()
        .into_iter()
        .flatten()
        .take(2)
        .filter_map(Value::as_str)
        .filter(|path| path.len() <= 4096 && !path.contains('\0') && Path::new(path).is_absolute())
        .map(str::to_owned)
        .collect();
    let confirmed_launch_roots = if enrollment(&state, &state.operator["bootstrap"]) {
        launch_roots.clone()
    } else {
        Vec::new()
    };
    Ok(Previous {
        root: Some(root),
        digest: Some(policy::fingerprint(&bytes)),
        bytes: Some(bytes),
        history_pins,
        checkpoint_pins: pins(&state.operator["bootstrap"]["checkpoint_recovery"]),
        launch_roots,
        confirmed_launch_roots,
    })
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn pins(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|value| digest(value))
        .take(2)
        .map(str::to_owned)
        .collect()
}

#[expect(
    clippy::too_many_lines,
    reason = "the enrollment transaction keeps exact source preparation, native projection ownership and durable recovery handoff in one ordered commit"
)]
pub(in crate::review) fn commit(
    plan: &Plan,
    mechanism: &str,
    checkpoint: crate::review::Checkpoint<'_>,
) -> Result<Context, Error> {
    checkpoint()?;
    let surface = if mechanism == "operator-cli" {
        "cli"
    } else {
        "mcp"
    };
    let store = Store::location(&plan.root)?;
    private_directory(&store.directory)?;
    let publication = crate::review::publication::ProjectionLease::acquire(&plan.root)?;
    if let Some(mut context) = committed(plan)? {
        context.surface = surface.into();
        return Ok(context);
    }
    checkpoint()?;
    let current_digest = match store.checkpoint_bytes() {
        Ok(bytes) => Some(policy::fingerprint(&bytes)),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if current_digest != plan.previous.digest {
        return Err(Error::StaleSnapshot);
    }
    let current = crate::review::bootstrap::prepare_options(
        &plan.root,
        plan.requested_launch.as_deref(),
        plan.requested_cache.as_deref(),
    )?;
    if current.fingerprint != plan.fingerprint {
        return Err(Error::StaleSnapshot);
    }
    checkpoint()?;
    let mut pending = plan.previous.checkpoint_pins.clone();
    pending.extend(plan.previous.digest.iter().cloned());
    let mut checkpoint_pins = Vec::new();
    if let Some(bytes) = &plan.previous.bytes {
        checkpoint_pins.push(policy::fingerprint(bytes));
    }
    for pin in &plan.previous.checkpoint_pins {
        if checkpoint_pins.len() == 2 {
            break;
        }
        if !checkpoint_pins.contains(pin) {
            checkpoint_pins.push(pin.clone());
        }
    }
    checkpoint()?;
    let mut state = fresh(plan)?;
    // The receipt names no folder: the enrollment is wherever these files are.
    let receipt = json!({"review_fingerprint":plan.fingerprint,"project_id":state.project_id,
        "previous_checkpoint":plan.previous.digest,"history_recovery":plan.previous.history_pins,
        "checkpoint_recovery":checkpoint_pins,"cache_dir":plan.requested_cache});
    state.operator["bootstrap"] = receipt.clone();
    let context = Context::new(
        state.project_id.clone(),
        state.project_root.clone(),
        surface,
    );
    let evaluator = configuration::Evaluator::new(crate::review::snapshots(&state.candidates)?)?;
    let registry_guard =
        crate::review::registry_trust::guard_effective(&state.operator, evaluator.effective())?;
    registry_guard.bind_modes(&mut state.operator)?;
    let compiled = crate::review::cache::build_candidate(
        &context,
        &state.candidates,
        &state.policy_revision,
        &state.operator,
        &evaluator,
    )?;
    crate::review::runtime::publish_confirmed(&mut state, &context, None, compiled, mechanism);
    let (retained, _source_pins) = store.retain_candidates(&state.candidates)?;
    state.candidates = retained;
    if let Some(active) = &mut state.active {
        active.candidates = state.candidates.clone();
    }
    let prepared_consent = state
        .active
        .as_ref()
        .and_then(|active| active.project_candidate().ok())
        .and_then(|reference| store.prepare_policy_consent(reference));
    super::stamp_policy_consent(&mut state.operator, prepared_consent.as_ref());
    let mut event = state.event(
        "project_enrolled",
        &context,
        json!({}),
        json!({}),
        json!({"bootstrap":receipt}),
    );
    event["operator"] = json!({"operator_id":null,"display_name":null,"mechanism":mechanism});
    state.record(event);
    Store::validate_checkpoint(&state)?;
    store.ignore_local_state();
    // Keep observational evidence of the previous identity as recovery data.
    // The new history generation does not read or import those observations.
    checkpoint()?;
    publication.check(&context)?;
    let lock = store.lock(checkpoint)?;
    let latest = match store.checkpoint_bytes() {
        Ok(bytes) => Some(policy::fingerprint(&bytes)),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if latest != plan.previous.digest
        || crate::review::bootstrap::prepare_options(
            &plan.root,
            plan.requested_launch.as_deref(),
            plan.requested_cache.as_deref(),
        )?
        .fingerprint
            != plan.fingerprint
    {
        return Err(Error::StaleSnapshot);
    }
    clear_temporaries(&store);
    cleanup(&store, &pending);
    if let Some(bytes) = &plan.previous.bytes {
        retain(&store, bytes)?;
    }
    let result = store.write(&mut state);
    if result.is_err() {
        let committed = store
            .checkpoint_bytes()
            .ok()
            .and_then(|bytes| store.decode(&bytes).ok())
            .is_some_and(|saved| {
                Store::validate_checkpoint(&saved).is_ok()
                    && saved.project_id == state.project_id
                    && enrollment(&saved, &receipt)
            });
        if !committed {
            result?;
        }
    }
    cleanup(&store, &checkpoint_pins);
    lock.release()?;
    crate::review::configuration_changes::export(&mut state);
    store.finish_policy_consent(
        prepared_consent,
        state
            .active
            .as_ref()
            .map(crate::review::runtime::Active::activation_id),
    );
    Ok(context)
}

fn fresh(plan: &Plan) -> Result<State, Error> {
    let project_id = history::identifier("project", &policy::fingerprint(b"shield-project"))?;
    let project_id = policy::fingerprint(project_id.as_bytes());
    Ok(State {
        legacy_preview: None,
        remember_policy: false,
        history_generation: history::generation()?,
        history_owner: None,
        history_available: true,
        pending_history: vec![],
        history_rewrite: None,
        history_control: false,
        history_evicted: false,
        operator: plan.metadata.clone(),
        schema_version: 1,
        project_id,
        project_root: plan
            .root
            .to_str()
            .ok_or(Error::Invalid("project must be UTF-8"))?
            .into(),
        recorded_root: None,
        sequence: 0,
        policy_revision: plan.revision.clone(),
        candidates: BTreeMap::from([("project".into(), plan.candidate.clone())]),
        registry_checkpoints: BTreeMap::default(),
        active: None,
        read_requests: vec![],
        run_requests: vec![],
        hook_requests: vec![],
        terminal_receipts: crate::review::authorization_receipts::Registry::default(),
        session_grants: crate::review::session_grants::Registry::default(),
        proposals: vec![],
        policy_drafts: vec![],
        dismissed_reviews: vec![],
        events: vec![],
        configuration_changes: crate::review::configuration_changes::Log::default(),
        configuration_changes_dirty: false,
        authorization_events: vec![],
        history_enabled: true,
        enforcement: crate::review::mode::Mode::default(),
        history_limit: 5000,
    })
}

/// Inspect current authority metadata without source preparation, historical
/// conversion, optional observations or filesystem publication.
pub(in crate::review) fn current_enrollment(project: &Path) -> Result<State, Error> {
    let store = Store::open(project)?;
    let state = store.decode(&store.checkpoint_bytes()?)?;
    Store::validate_checkpoint(&state)?;
    Ok(state)
}

pub(in crate::review) fn enrollment(state: &State, receipt: &Value) -> bool {
    receipt.is_object()
        && receipt["project_id"] == state.project_id
        && state.authorization_events.iter().any(|event| {
            event["kind"] == "project_enrolled"
                && event["provenance"]["project_id"] == state.project_id
                && event["details"]["bootstrap"] == *receipt
                && ["host-elicitation", "operator-cli"]
                    .contains(&event["operator"]["mechanism"].as_str().unwrap_or(""))
        })
}

pub(super) fn retain(store: &Store, bytes: &[u8]) -> Result<(), Error> {
    let target = store
        .directory
        .join(format!("bootstrap-{}.json", policy::fingerprint(bytes)));
    match fs::symlink_metadata(&target) {
        Ok(_) => {
            if recovery_bytes(&target)? != bytes {
                return Err(Error::Invalid("enrollment recovery source differs"));
            }
            super::history::inline::retain(store, bytes, "bootstrap");
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let temporary = Temporary {
        path: store
            .directory
            .join(format!(".bootstrap-{}.tmp", std::process::id())),
    };
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    let mut file = options.open(&temporary.path)?;
    #[cfg(test)]
    super::tests::observe(super::tests::Stage::Created);
    file.write_all(bytes)?;
    #[cfg(test)]
    super::tests::observe(super::tests::Stage::Written);
    sync(&file)?;
    #[cfg(test)]
    super::tests::observe(super::tests::Stage::Synced);
    drop(file);
    super::replace(&temporary.path, &target)?;
    super::history::inline::retain(store, bytes, "bootstrap");
    #[cfg(test)]
    super::tests::observe(super::tests::Stage::Renamed);
    #[cfg(unix)]
    sync(&fs::File::open(&store.directory)?)?;
    #[cfg(test)]
    super::tests::observe(super::tests::Stage::Complete);
    Ok(())
}

fn clear_temporaries(store: &Store) {
    let Ok(entries) = fs::read_dir(&store.directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|name| name.strip_prefix(".bootstrap-"))
            .and_then(|name| name.strip_suffix(".tmp"))
            .and_then(|stamp| super::stamp_pid(stamp, 1))
            .filter(|pid| *pid != 0)
        else {
            continue;
        };
        // This authority lock excludes every bootstrap writer. A same-PID
        // temporary therefore predates this writer, including after PID reuse.
        if (pid == std::process::id() || !super::writer_running(pid))
            && recovery_bytes(&entry.path()).is_ok()
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub(in crate::review) fn committed(plan: &Plan) -> Result<Option<Context>, Error> {
    let store = Store::location(&plan.root)?;
    let bytes = match store.checkpoint_bytes() {
        Ok(bytes) => bytes,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let Ok(state) = store.decode(&bytes) else {
        return Ok(None);
    };
    if Store::validate_checkpoint(&state).is_err()
        || state.operator["bootstrap"]["review_fingerprint"] != plan.fingerprint
        || !enrollment(&state, &state.operator["bootstrap"])
    {
        return Ok(None);
    }
    Ok(Some(Context::new(
        state.project_id,
        state.project_root,
        "mcp",
    )))
}

pub(in crate::review) fn launch_matches_metadata(project: &Path, requested: &Path) -> bool {
    inspect(project).is_ok_and(|previous| {
        previous
            .root
            .as_deref()
            .is_some_and(|root| Path::new(root) == requested)
            || previous
                .launch_roots
                .iter()
                .any(|root| Path::new(root) == requested)
    })
}

fn recovery_bytes(path: &Path) -> Result<Vec<u8>, Error> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > super::MAX_STATE as u64
    {
        return Err(Error::Invalid(
            "enrollment recovery must be a bounded private regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
            || metadata.uid()
                != fs::metadata(path.parent().ok_or(Error::Invalid("recovery parent"))?)?.uid()
        {
            return Err(Error::Invalid("enrollment recovery must be private"));
        }
    }
    #[cfg(windows)]
    crate::windows_security::check_private(path)?;
    #[cfg(windows)]
    let file = crate::windows_security::read_regular(path)?;
    #[cfg(not(windows))]
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take((super::MAX_STATE + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > super::MAX_STATE {
        return Err(policy::Error::Limit.into());
    }
    Ok(bytes)
}

pub(super) fn cleanup(store: &Store, keep: &[String]) {
    let Ok(entries) = fs::read_dir(&store.directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pin) = name
            .to_str()
            .and_then(|name| name.strip_prefix("bootstrap-"))
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        if !digest(pin) || keep.iter().any(|kept| kept == pin) {
            continue;
        }
        if recovery_bytes(&entry.path()).is_ok_and(|bytes| policy::fingerprint(&bytes) == pin)
            && fs::remove_file(entry.path()).is_ok()
        {
            super::history::inline::remove(store, pin, "bootstrap");
        }
    }
}
