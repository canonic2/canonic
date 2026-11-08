//! Historical checkpoints are validated, retained privately and atomically converted.
use super::{Error, State, Store, Temporary, private, sync};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
};

mod proposals;
#[cfg(test)]
mod retention_tests;
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod tests;

pub(super) fn checkpoint(bytes: &[u8]) -> Result<State, Error> {
    let state: State = serde_json::from_slice(bytes)
        .map_err(|_| Error::Invalid("corrupt review state; preserve it for recovery"))?;
    if state.schema_version != 1 {
        return Err(Error::Invalid("unsupported review schema"));
    }
    if state.operator.is_null() {
        return Err(Error::Invalid(
            "historical preview metadata needs conversion",
        ));
    }
    // Historical array journals need conversion. In a current-family checkpoint,
    // an unsupported optional history reference degrades history independently;
    // it does not invalidate otherwise verified policy and approval authority.
    if state.history_generation.len() == 64
        && state
            .history_generation
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        && !serde_json::from_slice::<Value>(bytes).is_ok_and(|raw| {
            [
                "hook_requests",
                "session_grants",
                "policy_drafts",
                "dismissed_reviews",
                "enforcement",
            ]
            .iter()
            .any(|key| raw.get(*key).is_some())
        })
    {
        return Err(Error::Invalid("historical array history needs conversion"));
    }
    Ok(state)
}

/// Only pre-required-cache records enter this codec. Newer capability fields
/// identify the strict current family, including when one of its fields is corrupt.
fn historical(bytes: &[u8]) -> Result<(Value, State), Error> {
    let raw =
        crate::json::parse(bytes).map_err(|_| Error::Invalid("corrupt historical review state"))?;
    let object = raw
        .as_object()
        .ok_or(Error::Invalid("invalid historical checkpoint"))?;
    if raw["schema_version"]
        .as_u64()
        .is_some_and(|version| version != 1)
        || object.keys().any(|key| {
            ![
                "schema_version",
                "history_generation",
                "operator",
                "project_id",
                "project_root",
                "sequence",
                "policy_revision",
                "candidates",
                "registry_checkpoints",
                "active",
                "read_requests",
                "run_requests",
                "proposals",
                "events",
                "authorization_events",
                "history_enabled",
                "history_limit",
                "hook_requests",
                "session_grants",
                "policy_drafts",
                "dismissed_reviews",
                "enforcement",
            ]
            .contains(&key.as_str())
        })
    {
        return Err(Error::Invalid(
            "unsupported review state format; original checkpoint preserved",
        ));
    }
    if raw["schema_version"] != 1
        || [
            "hook_requests",
            "session_grants",
            "policy_drafts",
            "dismissed_reviews",
            "enforcement",
        ]
        .iter()
        .any(|key| object.contains_key(*key))
    {
        return Err(Error::Invalid(
            "corrupt review state; preserve it for recovery",
        ));
    }
    let mut target = raw.clone();
    proposals::prepare(&mut target)?;
    // Empty legacy operator metadata was an explicit supported preview state.
    // It supplies no activation, registration or new source trust.
    if target["operator"].is_null() {
        target["operator"] = json!({"sources":{},"default":null,"installed":[],"projection":null});
    }
    target["active"] = Value::Null;
    if target["history_generation"].is_null() {
        target["history_generation"] = json!(format!(
            "journal-{}",
            raw["project_id"].as_str().unwrap_or("")
        ));
    }
    let object = target.as_object_mut().expect("checked object");
    for (name, default) in [
        ("registry_checkpoints", json!({})),
        ("read_requests", json!([])),
        ("run_requests", json!([])),
    ] {
        object.entry(name).or_insert(default);
    }
    let state = serde_json::from_value(target)
        .map_err(|_| Error::Invalid("invalid historical checkpoint fields"))?;
    Ok((raw, state))
}

pub(super) fn convert(store: &Store, original: &[u8]) -> Result<State, Error> {
    let held = *store
        .writer_owned
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        == Some(std::thread::current().id());
    let mut bytes = original.to_vec();
    loop {
        let (raw, mut state) = recognize(store, &bytes)?;
        store.validate_checkpoint(&state)?;
        crate::review::integrations::registration(&state.operator)?;
        crate::review::integrations::pending(&state.operator)?;
        let previous = recovery_sources(store, &state.operator)?;
        // Compilation and immutable derived-cache preparation do not require
        // authority ownership. Only publish after checking the same source again.
        crate::review::runtime::restore_historical(store, &mut state, &raw["active"])?;
        #[cfg(test)]
        super::tests::observe(super::tests::Stage::UpgradePrepared);
        let owned = (!held).then(|| store.lock(&|| Ok(()))).transpose()?;
        let latest = store.checkpoint_bytes()?;
        if let Ok(state) = checkpoint(&latest) {
            return Ok(state);
        }
        if latest != bytes {
            drop(owned);
            bytes = latest;
            continue;
        }
        let current = crate::policy::fingerprint(&bytes);
        let mut keep = previous.clone();
        if !keep.contains(&current) {
            keep.push(current.clone());
        }
        retain(store, &bytes, &keep)?;
        // Commit the recovery references atomically with the upgraded checkpoint.
        // Before that commit all previously pinned evidence remains retained.
        let mut committed = vec![current];
        for digest in previous {
            if committed.len() == 2 {
                break;
            }
            if !committed.contains(&digest) {
                committed.push(digest);
            }
        }
        state.operator["upgrade_recovery"] = json!(committed);
        super::history::upgrade(store, &raw, &mut state)?;
        store.validate_checkpoint(&state)?;
        store.write(&state)?;
        let _ = cleanup(store, &committed);
        if let Some(owned) = owned {
            owned.release()?;
        }
        return Ok(state);
    }
}

/// Diagnostics validate the same retained authority without publishing any files.
pub(super) fn inspect(store: &Store, bytes: &[u8]) -> Result<State, Error> {
    let (raw, mut state) = recognize(store, bytes)?;
    store.validate_checkpoint(&state)?;
    crate::review::integrations::registration(&state.operator)?;
    crate::review::integrations::pending(&state.operator)?;
    recovery_sources(store, &state.operator)?;
    crate::review::runtime::inspect_historical(store, &mut state, &raw["active"])?;
    super::history::inspect_legacy(store, &raw, &mut state);
    store.validate_checkpoint(&state)?;
    Ok(state)
}

fn recognize(store: &Store, bytes: &[u8]) -> Result<(Value, State), Error> {
    historical(bytes).map_err(|error| {
        if matches!(
            error,
            Error::Invalid("unsupported review state format; original checkpoint preserved")
        ) {
            let observed = crate::json::parse(bytes)
                .ok()
                .and_then(|raw| raw["schema_version"].as_u64())
                .map_or_else(|| "invalid".into(), |version| version.to_string());
            Error::StateFormat {
                path: store.directory.join("state.json"),
                observed,
            }
        } else if matches!(error, Error::Invalid(_)) {
            Error::StateCorrupt {
                path: store.directory.join("state.json"),
            }
        } else {
            error
        }
    })
}

fn source_name(bytes: &[u8]) -> String {
    format!("upgrade-{}.json", crate::policy::fingerprint(bytes))
}

fn retained(store: &Store, name: &str) -> Result<Vec<u8>, Error> {
    let path = store.directory.join(name);
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > super::MAX_STATE as u64
    {
        return Err(Error::Invalid(
            "upgrade recovery source must be a bounded regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
            || metadata.uid() != fs::metadata(&store.directory)?.uid()
        {
            return Err(Error::Invalid("upgrade recovery source must be private"));
        }
    }
    #[cfg(windows)]
    crate::windows_security::check_private(&path)?;
    #[cfg(windows)]
    let file = crate::windows_security::read_regular(&path)?;
    #[cfg(not(windows))]
    let file = fs::File::open(&path)?;
    let mut bytes = vec![];
    file.take((super::MAX_STATE + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > super::MAX_STATE {
        return Err(crate::policy::Error::Limit.into());
    }
    Ok(bytes)
}

fn recovery_sources(store: &Store, operator: &Value) -> Result<Vec<String>, Error> {
    let Some(value) = operator.get("upgrade_recovery") else {
        return Ok(Vec::new());
    };
    let sources = value
        .as_array()
        .filter(|sources| sources.len() <= 2)
        .ok_or(Error::Invalid("invalid upgrade recovery references"))?;
    let mut validated: Vec<String> = Vec::new();
    for source in sources {
        let digest = source
            .as_str()
            .filter(|digest| {
                digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .filter(|digest| !validated.iter().any(|previous| previous == *digest))
            .ok_or(Error::Invalid("invalid upgrade recovery references"))?;
        let bytes = retained(store, &format!("upgrade-{digest}.json"))?;
        if crate::policy::fingerprint(&bytes) != digest {
            return Err(Error::Invalid("upgrade recovery reference differs"));
        }
        validated.push(digest.to_owned());
    }
    Ok(validated)
}

fn retain(store: &Store, bytes: &[u8], keep: &[String]) -> Result<(), Error> {
    clear_temporaries(store)?;
    let name = source_name(bytes);
    let target = store.directory.join(&name);
    if fs::symlink_metadata(&target).is_ok() {
        if retained(store, &name)? != bytes {
            return Err(Error::Invalid("upgrade recovery source differs"));
        }
        cleanup(store, keep)?;
        return Ok(());
    }
    // The old checkpoint is still authoritative and all its committed pins
    // remain kept. Failed-attempt backups do not accumulate as old writers
    // advance that checkpoint between unsuccessful migration attempts.
    cleanup(store, keep)?;
    let temporary = Temporary {
        path: store
            .directory
            .join(format!(".upgrade-{}.tmp", std::process::id())),
    };
    if fs::symlink_metadata(&temporary.path).is_ok() {
        retained(
            store,
            temporary
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or(Error::Invalid("upgrade temporary name"))?,
        )?;
        fs::remove_file(&temporary.path)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    let mut file = options.open(&temporary.path)?;
    file.write_all(bytes)?;
    sync(&file)?;
    drop(file);
    super::replace(&temporary.path, &target)?;
    #[cfg(unix)]
    sync(&fs::File::open(&store.directory)?)?;
    Ok(())
}

fn clear_temporaries(store: &Store) -> Result<(), Error> {
    for entry in fs::read_dir(&store.directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name
            .strip_prefix(".upgrade-")
            .and_then(|s| s.strip_suffix(".tmp"))
            .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()))
        {
            retained(store, name)?;
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// Delete only private, exact-hash owned sources outside the pinned set.
/// At most two committed sources and one pending conversion source are kept.
fn cleanup(store: &Store, keep: &[String]) -> Result<(), Error> {
    // Stream names with constant memory. Unrelated directory entries cannot
    // force an otherwise valid upgrade into a permanent capacity refusal.
    for entry in fs::read_dir(&store.directory)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(digest) = name
            .strip_prefix("upgrade-")
            .and_then(|s| s.strip_suffix(".json"))
        else {
            continue;
        };
        if keep.iter().any(|kept| kept == digest)
            || digest.len() != 64
            || !digest.bytes().all(|c| c.is_ascii_hexdigit())
        {
            continue;
        }
        if retained(store, &name).is_ok_and(|old| crate::policy::fingerprint(&old) == digest) {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
