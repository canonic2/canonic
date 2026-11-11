//! Optional private consent has immutable bodies and a small postcommit root.
use super::{
    BTreeMap, Context, Error, PathBuf, State, Store, directory, key, objects, private_directory,
    private_files,
};
use crate::review::storage::sources::{self, Namespace};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const INDEX_LIMIT: usize = 2 * 1024 * 1024;
const RECORD_LIMIT: usize = 1024;
mod legacy;
mod partitions;
mod selection;
pub(super) use selection::matches_legacy;
use selection::{locks_for_source, merge_observations};
const META_FIELDS: &[&str] = &[
    "sources",
    "default",
    "installed",
    "shared_installed",
    "registry_trust",
    "disabled",
    "projection",
    "policy_lineage",
];
#[cfg(test)]
thread_local! {static FIXTURE_DIRECTORY:std::cell::RefCell<Option<PathBuf>>=const {std::cell::RefCell::new(None)};}
#[cfg(test)]
pub(in crate::review::storage) fn fixture_namespace(name: &str) {
    let key = crate::policy::fingerprint(
        format!("{}:{name}", std::env::current_exe().unwrap().display()).as_bytes(),
    );
    FIXTURE_DIRECTORY.with(|slot| {
        *slot.borrow_mut() = Some(
            std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("shield-consent-fixture-{key}")),
        );
    });
}
#[cfg(test)]
pub(in crate::review::storage) fn fixture_directory() -> Option<PathBuf> {
    FIXTURE_DIRECTORY.with(|slot| slot.borrow().clone())
}
#[cfg(test)]
struct FixtureDirectory(Option<PathBuf>);
#[cfg(test)]
impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        FIXTURE_DIRECTORY.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}
#[cfg(test)]
fn scoped_directory(path: PathBuf) -> FixtureDirectory {
    FixtureDirectory(FIXTURE_DIRECTORY.with(|slot| slot.replace(Some(path))))
}
#[cfg(test)]
pub(super) fn record(key: &str) -> Option<Value> {
    let namespace = partitions::select(key, false).ok()?;
    serde_json::to_value(read_index(&namespace).ok()?.candidates.get(key)?).ok()
}
#[cfg(test)]
pub(super) fn remove_record(key: &str) {
    let namespace = partitions::select(key, false).unwrap();
    let _guard = objects::Directory::enter(&namespace.directory).unwrap();
    let mut index = read_index(&namespace).unwrap();
    index.candidates.remove(key);
    private_files::write(
        &namespace.directory.join("state.json"),
        &serde_json::to_value(index).unwrap(),
        INDEX_LIMIT,
    )
    .unwrap();
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    schema_version: u8,
    candidates: BTreeMap<String, BTreeMap<String, Record>>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    source: Value,
    context: Value,
}

fn namespace(create: bool) -> Result<Namespace, Error> {
    let directory = directory()?;
    if create {
        if let Some(parent) = directory.parent()
            && !parent.exists()
        {
            private_directory(parent)?;
        }
        private_files::directory(&directory, true)?;
    }
    Namespace::private(&directory)
}
fn read_index(namespace: &Namespace) -> Result<Index, Error> {
    let value = match private_files::read(&namespace.directory.join("state.json"), INDEX_LIMIT) {
        Ok(value) => value,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Index {
                schema_version: 3,
                ..Index::default()
            });
        }
        Err(error) => return Err(error),
    };
    // The first compact index had one record per authoring key. Preserve it
    // as one exact variant instead of silently replacing its reviewed bytes.
    let value = if value["schema_version"] == 2 {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct LegacyIndex {
            schema_version: u8,
            candidates: BTreeMap<String, Record>,
        }
        let old: LegacyIndex = serde_json::from_value(value)
            .map_err(|_| Error::Invalid("private consent index invalid"))?;
        if old.schema_version != 2 {
            return Err(Error::Invalid("private consent index invalid"));
        }
        serde_json::to_value(Index {
            schema_version: 3,
            candidates: old
                .candidates
                .into_iter()
                .map(|(key, record)| {
                    (
                        key,
                        BTreeMap::from([(sources::candidate_identity(&record.source), record)]),
                    )
                })
                .collect(),
        })
        .map_err(|_| Error::Invalid("private consent index encoding"))?
    } else {
        value
    };
    let index: Index = serde_json::from_value(value)
        .map_err(|_| Error::Invalid("private consent index invalid"))?;
    if index.schema_version != 3
        || index.candidates.values().map(BTreeMap::len).sum::<usize>() > RECORD_LIMIT
        || index.candidates.values().any(|variants| {
            variants.is_empty()
                || variants
                    .iter()
                    .any(|(id, record)| id != &sources::candidate_identity(&record.source))
        })
        || index
            .candidates
            .keys()
            .any(|key| key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(Error::Invalid("private consent index invalid"));
    }
    Ok(index)
}
fn source_key(namespace: &Namespace, reference: &Value) -> Result<String, Error> {
    let mut authored = BTreeMap::new();
    for field in ["config", "local_config", "rules", "local_rules"] {
        authored.insert(field, namespace.field(reference, field, None)?);
    }
    crate::json::fingerprint(&("reviewed-policy-v1", authored))
        .map_err(|_| Error::Invalid("policy consent key encoding"))
}
fn context(state: &State) -> Value {
    let mut metadata = serde_json::Map::new();
    for field in META_FIELDS {
        metadata.insert((*field).into(), state.operator[*field].clone());
    }
    metadata.insert("policy_lineage".into(), json!(state.policy_lineage()));
    json!({"metadata":metadata,"registry_checkpoints":state.registry_checkpoints})
}
pub(super) fn metadata(value: &Value) -> Result<Value, Error> {
    let object = value
        .as_object()
        .ok_or(Error::Invalid("private consent metadata invalid"))?;
    if object
        .keys()
        .any(|key| !META_FIELDS.contains(&key.as_str()))
    {
        return Err(Error::Invalid("unsupported private consent metadata"));
    }
    Ok(value.clone())
}
fn matches_context(state: &State, captured: &Value) -> bool {
    captured["registry_checkpoints"]
        .as_object()
        .is_some_and(|checkpoints| {
            checkpoints.len() == state.registry_checkpoints.len()
                && state
                    .registry_checkpoints
                    .iter()
                    .all(|(key, value)| checkpoints.get(key) == Some(value))
        })
        && META_FIELDS.iter().all(|field| {
            if *field == "policy_lineage" {
                captured["metadata"][*field] == state.policy_lineage()
            } else {
                captured["metadata"][*field] == state.operator[*field]
            }
        })
}

/// These pins bridge preparation, durable activation and consent-root publication.
/// No candidate body is read or written by a project authority writer.
pub(in crate::review) struct PreparedConsent {
    user: Namespace,
    key: String,
    source: Value,
    project_pin: objects::Pins,
    user_pin: objects::Pins,
    refresh: bool,
}
impl PreparedConsent {
    pub(in crate::review) fn prepare(store: &Store, reference: &Value) -> Result<Self, Error> {
        match Self::prepare_at(store, reference, false) {
            Err(Error::Capacity { .. } | Error::CleanupPending) => {
                Self::prepare_at(store, reference, true)
            }
            result => result,
        }
    }
    fn prepare_at(store: &Store, reference: &Value, isolated: bool) -> Result<Self, Error> {
        let project = Namespace::project(store);
        let project_pin = project.pin(reference)?;
        let key = source_key(&project, reference)?;
        let user = if isolated {
            partitions::create(&key)?
        } else {
            partitions::select(&key, true)?
        };
        let root = objects::Directory::enter(&user.directory)?;
        let index = read_index(&user)?;
        let existing = index
            .candidates
            .get(&key)
            .and_then(|variants| variants.get(&sources::candidate_identity(reference)))
            .filter(|record| {
                sources::candidate_identity(&record.source)
                    == sources::candidate_identity(reference)
                    && sources::map_digest(&BTreeMap::from([(
                        "project".into(),
                        record.source.clone(),
                    )])) == sources::map_digest(&BTreeMap::from([(
                        "project".into(),
                        reference.clone(),
                    )]))
            });
        let (source, user_pin) = if let Some(existing) = existing {
            // Verify the small manifest binding. Full artifact verification
            // remains mandatory on restore; no new consent is inferred here.
            user.field(&existing.source, "kind", None)?;
            (existing.source.clone(), user.pin(&existing.source)?)
        } else {
            user.import(&project, reference)?
        };
        drop(root);
        Ok(Self {
            user,
            key,
            source,
            project_pin,
            user_pin,
            refresh: false,
        })
    }
    pub(in crate::review) fn key(&self) -> &str {
        &self.key
    }
    pub(in crate::review) fn publish(
        mut self,
        store: &Store,
        activation: &str,
    ) -> Result<(), Error> {
        let state = store.read_checkpoint()?;
        verify_activation(&state, activation, &self.source)?;
        let captured = context(&state);
        let mut body = captured.clone();
        if !self.refresh {
            body["locks"] =
                locks_for_source(&Namespace::project(store), &self.source, &state.operator)?;
        }
        // Current trust floors were bound by the final activation guard. Only
        // this bounded metadata blob is new after commit, outside authority.
        let root = objects::Directory::enter(&self.user.directory)?;
        let mut index = read_index(&self.user)?;
        // A writer prepared before overflow publication must follow the new
        // root too; otherwise it could publish newer trust into a hidden index.
        let target = partitions::select(&self.key, false)?;
        if target.directory != self.user.directory && self.user.directory != directory()? {
            return Err(Error::StaleSnapshot);
        }
        if target.directory != self.user.directory
            || (self.user.directory == directory()? && partitions::full(&index))
        {
            drop(root);
            let target = if target.directory == self.user.directory {
                partitions::create(&self.key)?
            } else {
                target
            };
            let (source, pin) = target.import(&self.user, &self.source)?;
            self.user = target;
            self.source = source;
            self.user_pin = pin;
            return self.publish(store, activation);
        }
        let _pins = (&self.project_pin, &self.user_pin);
        let identity = sources::candidate_identity(&self.source);
        if let Some(previous) = index
            .candidates
            .get(&self.key)
            .and_then(|rows| rows.get(&identity))
        {
            let previous = self.user.read_json(&previous.context)?;
            if self.refresh {
                body["locks"] = previous["locks"].clone();
            }
            merge_observations(&mut body, &previous)?;
        } else if self.refresh {
            return Err(Error::Missing);
        }
        let (context_reference, context_pin) = self.user.retain_json(&body)?;
        let context_identity =
            Context::new(state.project_id.clone(), state.project_root.clone(), "cli");
        store.inspect_checkpoint(&context_identity, &|| Ok(()), |current| {
            verify_activation(current, activation, &self.source)?;
            if !matches_context(current, &captured) {
                return Err(Error::StaleSnapshot);
            }
            Ok(())
        })?;
        let count = index.candidates.values().map(BTreeMap::len).sum::<usize>();
        if !index
            .candidates
            .get(&self.key)
            .is_some_and(|rows| rows.contains_key(&identity))
            && count >= RECORD_LIMIT
        {
            return Err(Error::Capacity {
                kind: crate::review::CapacityKind::StoredEntries,
                pending: 0,
                retained: count + 1,
                limit: RECORD_LIMIT,
            });
        }
        let record = Record {
            source: self.source.clone(),
            context: context_reference,
        };
        if index
            .candidates
            .get(&self.key)
            .and_then(|rows| rows.get(&identity))
            != Some(&record)
        {
            index
                .candidates
                .entry(self.key.clone())
                .or_default()
                .insert(identity, record);
            let wire = serde_json::to_value(&index)
                .map_err(|_| Error::Invalid("policy consent index encoding"))?;
            private_files::write(&self.user.directory.join("state.json"), &wire, INDEX_LIMIT)?;
        }
        drop(context_pin);
        crate::diagnostics::trace::count("policy.consent_published", 1);
        Ok(())
    }
}
fn verify_activation(state: &State, activation: &str, source: &Value) -> Result<(), Error> {
    let active = state
        .active
        .as_ref()
        .filter(|active| active.activation_id() == activation)
        .ok_or(Error::StaleSnapshot)?;
    active.validate_binding(state)?;
    let reference = active.project_candidate()?;
    if sources::candidate_identity(reference) != sources::candidate_identity(source)
        || sources::map_digest(&BTreeMap::from([("project".into(), reference.clone())]))
            != sources::map_digest(&BTreeMap::from([("project".into(), source.clone())]))
    {
        return Err(Error::StaleSnapshot);
    }
    Ok(())
}
pub(in crate::review::storage) fn prepare(
    store: &Store,
    reference: &Value,
) -> Option<PreparedConsent> {
    match PreparedConsent::prepare(store, reference) {
        Ok(prepared) => Some(prepared),
        Err(error) => {
            unavailable(&error);
            None
        }
    }
}
pub(in crate::review::storage) fn stamp(metadata: &mut Value, prepared: Option<&PreparedConsent>) {
    if let Some(prepared) = prepared {
        metadata["policy_consent_key"] = json!(prepared.key());
    } else if let Some(metadata) = metadata.as_object_mut() {
        metadata.remove("policy_consent_key");
    }
}
pub(in crate::review::storage) fn finish(
    store: &Store,
    prepared: Option<PreparedConsent>,
    activation: Option<&str>,
) -> bool {
    let result = match (prepared, activation) {
        (Some(prepared), Some(activation)) => prepared.publish(store, activation),
        _ => return false,
    };
    if let Err(error) = &result {
        unavailable(error);
    }
    result.is_ok()
}

fn unavailable(error: &Error) {
    crate::diagnostics::trace::count("policy.consent_unavailable", 1);
    if matches!(
        error,
        Error::Capacity { .. } | Error::CleanupPending | Error::Policy(crate::policy::Error::Limit)
    ) {
        eprintln!("Shield: private policy reuse storage is full; local policy remains active.");
    }
}
type RestoredConsent = (
    BTreeMap<String, Value>,
    crate::configuration::Evaluator,
    Value,
    BTreeMap<String, Value>,
    objects::Pins,
);

/// Authenticate retained content against the developer-private origin, rather
/// than selecting by the destination's possibly dirty authored projections.
/// The returned metadata and lineage belong to that private record; copied
/// public activation hashes are never proof of a human review.
pub(super) struct RetainedConsent {
    pub(super) candidate: Value,
    pub(super) metadata: Value,
}
pub(super) fn retained_record(candidate: &Value) -> Result<Option<RetainedConsent>, Error> {
    let user = match partitions::select(&key(candidate)?, false) {
        Ok(namespace) => namespace,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let root = objects::Directory::enter(&user.directory)?;
    let index = read_index(&user)?;
    let identity = sources::candidate_identity(candidate);
    let Some(record) = index
        .candidates
        .get(&key(candidate)?)
        .and_then(|variants| variants.get(&identity))
    else {
        drop(root);
        return legacy::retained(candidate);
    };
    let record_wire = serde_json::to_value(record)
        .map_err(|_| Error::Invalid("private consent record encoding"))?;
    let _pins = user.pin(&record_wire)?;
    drop(root);
    let raw = user.captured(&record.source)?;
    if raw != *candidate {
        return Ok(None);
    }
    let body = user.read_json(&record.context)?;
    let metadata = metadata(&body["metadata"])?;
    if metadata["policy_lineage"]
        .as_str()
        .is_none_or(str::is_empty)
    {
        return Err(Error::Invalid("private consent lineage unavailable"));
    }
    serde_json::from_value::<BTreeMap<String, Value>>(body["registry_checkpoints"].clone())
        .map_err(|_| Error::Invalid("private consent checkpoints invalid"))?;
    Ok(Some(RetainedConsent {
        candidate: raw,
        metadata,
    }))
}

pub(super) fn restore_record(
    store: &Store,
    authored: &Value,
) -> Result<Option<RestoredConsent>, Error> {
    let user = match partitions::select(&key(authored)?, false) {
        Ok(namespace) => namespace,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let root = objects::Directory::enter(&user.directory)?;
    let index = read_index(&user)?;
    let Some(variants) = index.candidates.get(&key(authored)?) else {
        return Ok(None);
    };
    let config = PathBuf::from(&store.root).join(".shield/config.yml");
    let mut selected = None;
    for record in variants.values() {
        let body = user.read_json(&record.context)?;
        let expected = if body["locks"].is_null() {
            locks_for_source(&user, &record.source, &body["metadata"])?
        } else {
            body["locks"].clone()
        };
        if selection::matches(&config, &user, &record.source, authored, &expected)? {
            if selected.is_some() {
                return Err(Error::Invalid(
                    "reviewed policy variants are ambiguous; restore exact portable locks or review project files",
                ));
            }
            selected = Some(record);
        }
    }
    let Some(record) = selected else {
        return Ok(None);
    };
    let record_wire = serde_json::to_value(record)
        .map_err(|_| Error::Invalid("private consent record encoding"))?;
    let _user_pins = user.pin(&record_wire)?;
    drop(root);
    let body = user.read_json(&record.context)?;
    let metadata = metadata(&body["metadata"])?;
    let raw = user.captured(&record.source)?;
    if super::authored(&raw) != *authored
        || !crate::review::integrations::project_sources::matches_artifacts(
            &PathBuf::from(&store.root).join(".shield/config.yml"),
            &raw,
        )
        .map_err(|_| Error::Invalid("cannot validate project artifacts against reviewed policy"))?
    {
        return Ok(None);
    }
    let evaluator = crate::configuration::Evaluator::new(crate::review::snapshots(
        &BTreeMap::from([("project".into(), raw)]),
    )?)?;
    super::super::private_directory(&store.directory)?;
    let (reference, pins) = Namespace::project(store).import(&user, &record.source)?;
    let checkpoints: BTreeMap<String, Value> =
        serde_json::from_value(body["registry_checkpoints"].clone())
            .map_err(|_| Error::Invalid("private consent checkpoints invalid"))?;
    Ok(Some((
        BTreeMap::from([("project".into(), reference)]),
        evaluator,
        metadata,
        checkpoints,
        pins,
    )))
}
/// Whether this user reviewed this exact retained policy content before.
pub(in crate::review::storage) fn presence_for(candidate: &Value) -> bool {
    retained_record(candidate).is_ok_and(|record| {
        record.is_some_and(|record| record.candidate == *candidate && record.metadata.is_object())
    })
}
pub(in crate::review) fn presence(state: &State) -> Value {
    let Some(key) = state.operator["policy_consent_key"].as_str() else {
        return json!({"present":false,"integrity":"unchecked"});
    };
    let present = partitions::select(key, false)
        .and_then(|namespace| read_index(&namespace))
        .is_ok_and(|index| {
            index.candidates.get(key).is_some_and(|variants| {
                variants.values().any(|record| {
                    state.candidates.get("project").is_some_and(|source| {
                        sources::candidate_identity(source)
                            == sources::candidate_identity(&record.source)
                    })
                })
            })
        });
    json!({"present":present,"integrity":"unchecked"})
}
pub(in crate::review::storage) fn refresh(store: &Store) -> bool {
    let result = (|| {
        let state = store.read_checkpoint()?;
        let active = state.active.as_ref().ok_or(Error::Missing)?;
        let key = state.operator["policy_consent_key"]
            .as_str()
            .ok_or(Error::Missing)?
            .to_owned();
        let user = partitions::select(&key, false)?;
        let root = objects::Directory::enter(&user.directory)?;
        let index = read_index(&user)?;
        let source = active.project_candidate()?;
        let record = index
            .candidates
            .get(&key)
            .and_then(|rows| rows.get(&sources::candidate_identity(source)))
            .ok_or(Error::Missing)?;
        verify_activation(&state, active.activation_id(), &record.source)?;
        let user_pin = user.pin(&record.source)?;
        let project_pin = Namespace::project(store).pin(source)?;
        let source = record.source.clone();
        drop(root);
        PreparedConsent {
            user,
            key,
            source,
            project_pin,
            user_pin,
            refresh: true,
        }
        .publish(store, active.activation_id())
    })();
    if result.is_err() {
        crate::diagnostics::trace::count("policy.consent_unavailable", 1);
    }
    result.is_ok()
}
#[cfg(test)]
mod tests;
