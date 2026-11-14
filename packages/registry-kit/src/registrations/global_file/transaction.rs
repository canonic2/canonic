//! One scoped generation journal. The active reference is the commit point;
//! pending intent contains only a caller-reviewed generation, never discovery.
use super::{GlobalFile, MAX_FILE_BYTES};
use crate::{
    digest,
    files::{self, Bounded, Commit},
    registrations::{RegistrationError as Error, Registrations, Scope},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

/// Maximum serialized product-owned operation receipt.
pub const MAX_RECEIPT_BYTES: u64 = 65_536;
const MAX_GENERATION: u64 = 2 * MAX_FILE_BYTES + MAX_RECEIPT_BYTES + 4096;
const MAX_ENTRIES: usize = 64;

/// Exact registered values, active generation and projection captured for review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    registrations: Registrations,
    generation: Option<String>,
    projection: Option<String>,
    ready: bool,
}
impl Snapshot {
    /// Committed values to present and modify in the caller's review.
    #[must_use]
    pub fn registrations(&self) -> &Registrations {
        &self.registrations
    }
    /// Selected immutable generation, absent for a legacy configuration.
    #[must_use]
    pub fn generation(&self) -> Option<&str> {
        self.generation.as_deref()
    }
    /// Whether materialized configuration agrees with committed values.
    #[must_use]
    pub fn projection_ready(&self) -> bool {
        self.ready
    }
}

/// A committed operation, including honest post-commit repair/durability state.
#[derive(Clone, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "durability, projection repair, replay and resumed intent are independent receipt facts"
)]
pub struct Publication {
    /// Immutable committed generation identity.
    pub generation: String,
    /// Caller-supplied logical mutation key.
    pub operation: String,
    /// Exact caller-owned operation receipt captured before publication.
    pub receipt: Value,
    /// This call confirmed required generation/reference/projection flushes.
    pub durable: bool,
    /// The projection still needs repair; conflicting manual bytes are preserved.
    pub repair_needed: bool,
    /// An already approved or committed operation was reconciled.
    pub replayed: bool,
    /// An unfinished approved journal intent was present during reconciliation.
    pub resumed: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    schema_version: u8,
    before: Snapshot,
    after: Registrations,
    operation: String,
    receipt: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    schema_version: u8,
    generation: String,
}

fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn directory(file: &GlobalFile) -> PathBuf {
    file.path.with_extension("state")
}
fn reference_path(file: &GlobalFile, name: &str) -> PathBuf {
    directory(file).join(format!("{name}.json"))
}
fn generation_path(file: &GlobalFile, id: &str) -> PathBuf {
    directory(file).join(format!("generation-{id}.json"))
}

fn path(file: &GlobalFile) -> Result<&Path, Error> {
    if file
        .path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["lock", "new", "lease", "state"].contains(&e.to_lowercase().as_str()))
    {
        return Err(Error::Path);
    }
    let parent = file.path.parent().ok_or(Error::Path)?;
    Ok(if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    })
}

fn state_directory(file: &GlobalFile, create: bool) -> Result<bool, Error> {
    path(file)?;
    match files::kind(&directory(file))? {
        None if create => files::create_private_dir(&directory(file))?,
        None => return Ok(false),
        Some(kind) if kind.is_symlink() => return Err(Error::Link),
        Some(kind) if !kind.is_dir() => return Err(Error::Malformed),
        Some(_) => {}
    }
    Ok(true)
}

fn lease(file: &GlobalFile) -> Result<File, Error> {
    let parent = path(file)?;
    match files::kind(parent)? {
        None => files::create_private_dir(parent)?,
        Some(kind) if kind.is_symlink() => return Err(Error::Link),
        Some(kind) if !kind.is_dir() => return Err(Error::Path),
        Some(_) => {}
    }
    let held = files::lease(
        &file.path.with_extension("lease"),
        &file.path.with_extension("lock"),
    )
    .map_err(|error| {
        if error.kind() == io::ErrorKind::WouldBlock {
            Error::Locked(file.path.with_extension("lock"))
        } else {
            Error::Io(error)
        }
    })?;
    state_directory(file, true)?;
    #[cfg(test)]
    tests::point("lease");
    Ok(held)
}

fn bytes(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, Error> {
    match files::kind(path)? {
        None => Ok(None),
        Some(kind) if kind.is_symlink() => Err(Error::Link),
        Some(_) => match files::read_bounded(path, limit)? {
            Bounded::Bytes(bytes) => Ok(Some(bytes)),
            _ => Err(Error::Malformed),
        },
    }
}
fn projection(file: &GlobalFile) -> Result<Option<Vec<u8>>, Error> {
    bytes(&file.path, MAX_FILE_BYTES)
}
fn hash(bytes: Option<&[u8]>) -> Option<String> {
    bytes.map(digest::sha256_hex)
}
fn registrations(file: &GlobalFile, bytes: Option<&[u8]>) -> Result<Registrations, Error> {
    let value: Registrations = bytes.map_or_else(
        || Ok(Registrations::default()),
        |b| serde_json::from_slice(b).map_err(|_| Error::Malformed),
    )?;
    value.validate(&file.builtin, Scope::Global)?;
    Ok(value)
}
fn encoded(value: &impl Serialize, limit: u64) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Malformed)?;
    if bytes.len() as u64 > limit {
        return Err(Error::TooLarge);
    }
    Ok(bytes)
}

fn reference(file: &GlobalFile, name: &str) -> Result<Option<String>, Error> {
    if !state_directory(file, false)? {
        return Ok(None);
    }
    bytes(&reference_path(file, name), 256)?
        .map(|bytes| {
            let value: Reference = serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)?;
            if value.schema_version != 1 || !valid_id(&value.generation) {
                return Err(Error::Malformed);
            }
            Ok(value.generation)
        })
        .transpose()
}
fn generation(file: &GlobalFile, id: &str) -> Result<Generation, Error> {
    if !valid_id(id) {
        return Err(Error::Malformed);
    }
    let bytes = bytes(&generation_path(file, id), MAX_GENERATION)?.ok_or(Error::Malformed)?;
    if digest::sha256_hex(&bytes) != id {
        return Err(Error::Malformed);
    }
    let value: Generation = serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)?;
    if value.schema_version != 1
        || !valid_id(&value.operation)
        || value
            .before
            .generation
            .as_deref()
            .is_some_and(|id| !valid_id(id))
        || value
            .before
            .projection
            .as_deref()
            .is_some_and(|id| !valid_id(id))
    {
        return Err(Error::Malformed);
    }
    value
        .before
        .registrations
        .validate(&file.builtin, Scope::Global)?;
    value.after.validate(&file.builtin, Scope::Global)?;
    encoded(&value.receipt, MAX_RECEIPT_BYTES)?;
    encoded(&value.before.registrations, MAX_FILE_BYTES)?;
    encoded(&value.after, MAX_FILE_BYTES)?;
    Ok(value)
}

pub(super) fn read_projection(file: &GlobalFile) -> Result<Option<Registrations>, Error> {
    projection(file)?
        .map(|bytes| registrations(file, Some(&bytes)))
        .transpose()
}

pub(super) fn snapshot(file: &GlobalFile) -> Result<Snapshot, Error> {
    let active = current(file)?;
    let raw = projection(file);
    if let Some((id, generation)) = active {
        let ready = raw.as_ref().is_ok_and(|raw| {
            raw.is_some()
                && registrations(file, raw.as_deref()).is_ok_and(|r| r == generation.after)
        });
        Ok(Snapshot {
            registrations: generation.after,
            generation: Some(id),
            projection: raw.ok().and_then(|r| hash(r.as_deref())),
            ready,
        })
    } else {
        let raw = raw?;
        Ok(Snapshot {
            registrations: registrations(file, raw.as_deref())?,
            generation: None,
            projection: hash(raw.as_deref()),
            ready: true,
        })
    }
}

fn publication(
    file: &GlobalFile,
    id: &str,
    value: &Generation,
    durable: bool,
    replayed: bool,
) -> Publication {
    let ready = projection(file).is_ok_and(|raw| {
        raw.is_some() && registrations(file, raw.as_deref()).is_ok_and(|r| r == value.after)
    });
    Publication {
        generation: id.into(),
        operation: value.operation.clone(),
        receipt: value.receipt.clone(),
        durable,
        repair_needed: !ready,
        replayed,
        resumed: false,
    }
}

pub(super) fn receipt(file: &GlobalFile, operation: &str) -> Result<Option<Publication>, Error> {
    let Some((id, value)) = current(file)? else {
        return Ok(None);
    };
    Ok((value.operation == operation).then(|| publication(file, &id, &value, false, true)))
}

fn current(file: &GlobalFile) -> Result<Option<(String, Generation)>, Error> {
    for _ in 0..3 {
        let Some(id) = reference(file, "active")? else {
            return Ok(None);
        };
        match generation(file, &id) {
            Ok(value) => return Ok(Some((id, value))),
            Err(error) if reference(file, "active")?.as_deref() == Some(&id) => return Err(error),
            Err(_) => {}
        }
    }
    Err(Error::Changed)
}

fn write(file: &GlobalFile, target: &Path, bytes: &[u8]) -> Result<Commit, Error> {
    Ok(files::replace_private(
        &files::temporary_path(&directory(file), "global-state"),
        target,
        bytes,
    )?)
}
fn write_reference(file: &GlobalFile, name: &str, id: &str) -> Result<Commit, Error> {
    write(
        file,
        &reference_path(file, name),
        &encoded(
            &Reference {
                schema_version: 1,
                generation: id.into(),
            },
            256,
        )?,
    )
}
fn durable(commit: Commit) -> Result<(), Error> {
    match commit {
        Commit::Durable => Ok(()),
        Commit::NotDurable(error) => Err(Error::Io(error)),
    }
}

fn synchronize(file: &GlobalFile, id: &str) -> bool {
    [generation_path(file, id), reference_path(file, "active")]
        .iter()
        .all(|path| {
            files::sync_existing(path)
                .and_then(|()| files::sync_directory(path))
                .is_ok()
        })
}

fn materialize(file: &GlobalFile, value: &Generation) -> Result<Commit, Error> {
    let raw = projection(file)?;
    if raw.is_some() && registrations(file, raw.as_deref()).is_ok_and(|r| r == value.after) {
        files::sync_existing(&file.path)?;
        return Ok(match files::sync_directory(&file.path) {
            Ok(()) => Commit::Durable,
            Err(e) => Commit::NotDurable(e),
        });
    }
    // A missing projection holds no manual bytes to preserve.
    if raw.is_some() && hash(raw.as_deref()) != value.before.projection {
        return Err(Error::Changed);
    }
    let temporary = file.path.with_extension("new");
    if let Err(error) = fs::remove_file(&temporary)
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error.into());
    }
    Ok(files::replace_private(
        &temporary,
        &file.path,
        &encoded(&value.after, MAX_FILE_BYTES)?,
    )?)
}

fn finish(
    file: &GlobalFile,
    id: &str,
    value: &Generation,
    mut flushed: bool,
    replayed: bool,
) -> Publication {
    let projected = materialize(file, value);
    flushed &= matches!(projected, Ok(Commit::Durable));
    if flushed {
        let pending = reference_path(file, "pending");
        if fs::remove_file(&pending).is_err() || files::sync_directory(&pending).is_err() {
            flushed = false;
        }
    }
    let mut result = publication(file, id, value, flushed, replayed);
    result.resumed = replayed;
    result
}

/// Removes an uncommitted pending reference. Only the lease holder calls this,
/// and only before the intent reached the commit point.
fn abandon(file: &GlobalFile) -> Result<(), Error> {
    let pending = reference_path(file, "pending");
    match fs::remove_file(&pending) {
        Ok(()) => files::sync_directory(&pending)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn recover_locked(file: &GlobalFile) -> Result<Option<Publication>, Error> {
    let active = reference(file, "active")?;
    if let Some(id) = reference(file, "pending")? {
        let value = generation(file, &id)?;
        if active.as_deref() == Some(&id) {
            let flushed = synchronize(file, &id);
            return Ok(Some(finish(file, &id, &value, flushed, true)));
        }
        if active == value.before.generation
            && hash(projection(file)?.as_deref()) == value.before.projection
        {
            files::sync_existing(&generation_path(file, &id))?;
            files::sync_directory(&generation_path(file, &id))?;
            let flushed = matches!(write_reference(file, "active", &id)?, Commit::Durable);
            return Ok(Some(finish(file, &id, &value, flushed, true)));
        }
        // The intent never reached the commit point and its reviewed base is
        // gone, so it can never commit. Discard it; the active generation stays
        // authoritative and `receipt` reports nothing for its operation.
        abandon(file)?;
    }
    active
        .map(|id| {
            let value = generation(file, &id)?;
            if projection(file)?.is_none() {
                materialize(file, &value)?;
            }
            let mut result = publication(file, &id, &value, false, true);
            result.durable = !result.repair_needed
                && synchronize(file, &id)
                && files::sync_existing(&file.path)
                    .and_then(|()| files::sync_directory(&file.path))
                    .is_ok();
            Ok(result)
        })
        .transpose()
}

pub(super) fn recover(file: &GlobalFile) -> Result<Option<Publication>, Error> {
    if reference(file, "active")?.is_none() && reference(file, "pending")?.is_none() {
        return Ok(None);
    }
    let _lease = lease(file)?;
    recover_locked(file)
}

fn cleanup(file: &GlobalFile, retain: &[Option<String>]) -> Result<(), Error> {
    for (index, entry) in fs::read_dir(directory(file))?.enumerate() {
        if index >= MAX_ENTRIES {
            return Err(Error::Limit);
        }
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let generation = name
            .strip_prefix("generation-")
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|id| valid_id(id));
        let obsolete = generation.is_some_and(|id| !retain.iter().flatten().any(|keep| keep == id));
        // This is an exact owned logical name, not a case-insensitive extension.
        let temporary = name.starts_with(".global-state.") && name.strip_suffix(".tmp").is_some();
        if obsolete || temporary {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// Bounds and validates the caller's proposed values before taking the lease.
fn admissible(
    file: &GlobalFile,
    after: &Registrations,
    operation: &str,
    receipt: &Value,
) -> Result<(), Error> {
    after.validate(&file.builtin, Scope::Global)?;
    encoded(after, MAX_FILE_BYTES)?;
    encoded(receipt, MAX_RECEIPT_BYTES)?;
    if !valid_id(operation) {
        return Err(Error::Malformed);
    }
    Ok(())
}

pub(super) fn publish(
    file: &GlobalFile,
    before: &Snapshot,
    after: &Registrations,
    operation: &str,
    receipt: &Value,
) -> Result<Publication, Error> {
    admissible(file, after, operation, receipt)?;
    let _lease = lease(file)?;
    let recovered = recover_locked(file)?;
    let current = snapshot(file)?;
    publish_locked(file, recovered, &current, before, after, operation, receipt)
}

/// The value-only conditional replacement, recovering and publishing under one
/// lease so interrupted intent is reconciled once, before the base is captured.
pub(super) fn replace(
    file: &GlobalFile,
    before: &Registrations,
    after: &Registrations,
) -> Result<Publication, Error> {
    let operation =
        digest::sha256_hex(&serde_json::to_vec(&(before, after)).map_err(|_| Error::Malformed)?);
    admissible(file, after, &operation, &Value::Null)?;
    let _lease = lease(file)?;
    let recovered = recover_locked(file)?;
    let captured = snapshot(file)?;
    // A value-only review never showed the manual bytes, so it cannot replace them.
    if !captured.ready {
        return Err(Error::ProjectionConflict(file.path.clone()));
    }
    if captured.registrations() != before {
        return Err(Error::Changed);
    }
    publish_locked(
        file,
        recovered,
        &captured,
        &captured,
        after,
        &operation,
        &Value::Null,
    )
}

/// Publishes under the held lease. `current` is the snapshot taken under it.
/// A reviewed snapshot whose projection disagreed with its generation binds
/// those exact bytes, so publishing replaces only the edit the caller saw.
fn publish_locked(
    file: &GlobalFile,
    recovered: Option<Publication>,
    current: &Snapshot,
    before: &Snapshot,
    after: &Registrations,
    operation: &str,
    receipt: &Value,
) -> Result<Publication, Error> {
    // Refuse an unreadable projection (link, non-file, oversized) before any write.
    projection(file)?;
    let value = Generation {
        schema_version: 1,
        before: before.clone(),
        after: after.clone(),
        operation: operation.into(),
        receipt: receipt.clone(),
    };
    let encoded = encoded(&value, MAX_GENERATION)?;
    let id = digest::sha256_hex(&encoded);
    if let Some(mut previous) = recovered
        && previous.generation == id
    {
        previous.replayed = true;
        return Ok(previous);
    }
    if current != before {
        return Err(Error::Changed);
    }
    cleanup(file, std::slice::from_ref(&before.generation))?;
    let target = generation_path(file, &id);
    match bytes(&target, MAX_GENERATION)? {
        Some(existing) if existing != encoded => return Err(Error::Malformed),
        Some(_) => {}
        None => durable(write(file, &target, &encoded)?)?,
    }
    durable(write_reference(file, "pending", &id)?)?;
    #[cfg(test)]
    tests::point("intent");
    if reference(file, "active")? != before.generation
        || hash(projection(file)?.as_deref()) != before.projection
    {
        // Nothing committed; leave no intent that a later writer must refuse.
        abandon(file)?;
        return Err(Error::Changed);
    }
    let flushed = matches!(write_reference(file, "active", &id)?, Commit::Durable);
    #[cfg(test)]
    tests::point("reference");
    let result = finish(file, &id, &value, flushed, false);
    #[cfg(test)]
    tests::point("projection");
    Ok(result)
}

#[cfg(test)]
mod tests;
