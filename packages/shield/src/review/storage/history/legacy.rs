//! Prepare observational history before an outer checkpoint upgrade commits.
use super::{
    Error, SEGMENT_COUNT, SEGMENT_LIMIT, STORE_LIMIT, State, Store, bound_event, capture,
    error_path, finish, line, lock, mark_failure, regular, segments, write_segment,
};
#[cfg(test)]
use super::{private, read, replace};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Read,
};
#[cfg(test)]
use std::{fs::OpenOptions, io::Write, path::Path};

const LEGACY_BYTES: usize = 32 * 1024 * 1024;
const LEGACY_EVENTS: usize = 40_000;

#[cfg(test)]
pub(in crate::review::storage) fn inspect(store: &Store, raw: &Value, state: &mut State) {
    let generation = raw["history_generation"].as_str();
    if generation.is_some_and(|generation| generation.starts_with("journal-"))
        || (generation.is_none() && segments(store, state).map_or(true, |rows| !rows.is_empty()))
    {
        let view = super::lock_diagnostics(store).and_then(|_guard| capture(store, state));
        finish(view, state);
        if inline_history(raw) {
            state.history_available = false;
        }
    } else if let Ok(events) = legacy_events(store, raw) {
        state.events = super::correlation::view(events, state.settings.history_limit);
        state.trim();
    } else {
        state.events.clear();
        state.history_available = false;
    }
}

/// The caller holds authority ownership and retains the exact old checkpoint.
/// Legacy sources stay untouched; only this conversion's deterministic journal
/// namespace may be replaced when retrying an interrupted preparation.
pub(in crate::review::storage) fn upgrade(
    store: &Store,
    raw: &Value,
    state: &mut State,
) -> Result<(), Error> {
    let generation = raw["history_generation"].as_str();
    let journal = generation.filter(|generation| generation.starts_with("journal-"));
    state.history_generation =
        journal.map_or_else(|| format!("journal-{}", state.project_id), str::to_owned);
    // Missing generation in the first journal implementation meant exactly
    // journal-<project_id>, not a fresh random namespace.
    let existing = if journal.is_some() || raw["history_generation"].is_null() {
        segments(store, state)
    } else {
        Ok(Vec::new())
    };
    if journal.is_some() || existing.as_ref().map_or(true, |rows| !rows.is_empty()) {
        if let Ok(_guard) = lock(store) {
            finish(capture(store, state), state);
            if !state.history_available {
                mark_failure(store, state);
            }
        } else {
            state.history_available = false;
            mark_failure(store, state);
        }
        // The archived checkpoint preserves these records, but selecting the
        // journal does not establish that they were published into it.
        if inline_history(raw) {
            state.history_available = false;
            mark_failure(store, state);
        }
        return Ok(());
    }
    // Include only retained observational inputs in the name. It is stable
    // across process crashes, unrelated authorization changes and retries.
    state.history_generation = format!(
        "journal-{}",
        crate::json::fingerprint(&json!([
            "legacy-history-upgrade-v1",
            state.project_id,
            raw["history_generation"],
            raw["events"]
        ]))
        .map_err(|_| Error::Invalid("legacy history identity encoding"))?
    );
    let outcome = (|| {
        let _guard = lock(store)?;
        let events = legacy_events(store, raw)?;
        prepare(store, state, &events)?;
        let failure = error_path(store, state)?;
        match fs::symlink_metadata(&failure) {
            Ok(_) => {
                regular(&failure)?;
                fs::remove_file(failure)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        state.events = events;
        state.history_available = true;
        Ok::<_, Error>(())
    })();
    if outcome.is_err() || existing.is_err() {
        // Optional history never prevents valid policy/approval migration.
        // The old checkpoint and any old generation remain available as exact
        // recovery evidence. A marker keeps the loss visible after restart.
        state.history_available = false;
        mark_failure(store, state);
    }
    Ok(())
}

fn inline_history(raw: &Value) -> bool {
    raw["events"]
        .as_array()
        .is_some_and(|events| !events.is_empty())
}

fn legacy_events(store: &Store, raw: &Value) -> Result<Vec<Value>, Error> {
    let events = if let Some(generation) = raw["history_generation"].as_str() {
        if generation.len() != 64 || !generation.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::Invalid("legacy history generation invalid"));
        }
        let path = store.directory.join(format!("history-{generation}.json"));
        regular(&path)?;
        let mut bytes = Vec::new();
        File::open(path)?
            .take((LEGACY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > LEGACY_BYTES || crate::policy::fingerprint(&bytes) != generation {
            return Err(Error::Invalid("legacy history generation differs"));
        }
        crate::json::parse(&bytes)
            .map_err(|_| Error::Invalid("legacy history generation is invalid JSON"))?
    } else {
        let events = raw["events"].clone();
        if !crate::json::fits(&events, LEGACY_BYTES) {
            return Err(Error::Invalid("legacy inline history exceeds limit"));
        }
        events
    };
    let Value::Array(events) = events else {
        return Err(Error::Invalid("legacy history must be an array"));
    };
    if events.len() > LEGACY_EVENTS
        || events.iter().any(|event| {
            !event.is_object()
                || (!crate::json::fits(event, 8192)
                    && !event["details"].is_null()
                    && !event["details"].is_object())
        })
    {
        return Err(Error::Invalid("legacy history events invalid"));
    }
    Ok(events.into_iter().map(bound_event).collect())
}

/// Preserve every bounded legacy observation, without applying another
/// retention window during format conversion. Normal readers retain their
/// configured view; the original full array remains untouched.
fn prepare(store: &Store, state: &State, events: &[Value]) -> Result<(), Error> {
    let rows = events.iter().map(line).collect::<Result<Vec<_>, _>>()?;
    let bytes = rows.iter().map(Vec::len).sum::<usize>();
    if bytes > STORE_LIMIT - SEGMENT_COUNT * 1024 {
        return Err(Error::Invalid("legacy history exceeds journal limit"));
    }
    // This namespace is derived from the exact old checkpoint and cannot be
    // the journal it selected. Partial files from an earlier attempt can be
    // recreated without touching the committed legacy generation.
    for segment in segments(store, state)? {
        fs::remove_file(segment.path)?;
    }
    let (mut sequence, mut chunk, mut requests) = (0, Vec::new(), BTreeSet::new());
    for (event, row) in events.iter().zip(rows) {
        if !chunk.is_empty() && chunk.len() + row.len() + 1024 > SEGMENT_LIMIT {
            write_segment(store, state, sequence, Some(requests.len() as u64), &chunk)?;
            (sequence, chunk, requests) = (sequence + 1, Vec::new(), BTreeSet::new());
        }
        if event["kind"] == "decision"
            && let Some(request) = event["links"]["request_id"].as_str()
        {
            requests.insert(request.to_owned());
        }
        chunk.extend(row);
    }
    write_segment(store, state, sequence, None, &chunk)?;
    Ok(())
}

#[cfg(test)]
mod tests;
