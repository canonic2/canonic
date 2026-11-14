//! Explicit root delegation review and offline known-revocation authority guards.
use super::{
    Checkpoint, Context, Error,
    model::State,
    registries,
    storage::{
        Store,
        registry_trust::{Namespace, Retained},
    },
};
use canonic_registry_kit::{
    acquisition::{Fetcher, VerifiedUpdate, VersionedRequest},
    catalog::{Catalog, Entry},
    checkpoint::Checkpoints,
    source::Source,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::Path,
    time::SystemTime,
};
#[cfg(test)]
mod tests;

pub(in crate::review) struct Guard {
    pub(in crate::review) revision: String,
    bindings: BTreeMap<String, Value>,
    _lease: Option<File>,
    _retained: Vec<Retained>,
}
impl Guard {
    pub(in crate::review) fn bind_modes(&self, metadata: &mut Value) -> Result<(), Error> {
        if self.bindings.is_empty() {
            return Ok(());
        }
        if metadata["registry_trust"].is_null() {
            metadata["registry_trust"] = json!({});
        }
        let modes = metadata["registry_trust"]
            .as_object_mut()
            .ok_or(Error::Invalid("invalid retained registry trust modes"))?;
        for (source, reference) in &self.bindings {
            modes.insert(source.clone(), reference.clone());
        }
        Ok(())
    }
}

/// Relevant active selections only. Diagnostics never call this guard; policy
/// removal/repair remains available when old bytes have been revoked.
#[cfg(test)]
fn guard(metadata: &Value) -> Result<Guard, Error> {
    let rows = metadata["installed"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            Ok((
                row["source_identity"]
                    .as_str()
                    .ok_or(Error::Invalid("installed source missing"))?,
                row["id"]
                    .as_str()
                    .ok_or(Error::Invalid("installed pack missing"))?,
                row["version"]
                    .as_str()
                    .ok_or(Error::Invalid("installed version missing"))?,
                row["digest"]
                    .as_str()
                    .ok_or(Error::Invalid("installed digest missing"))?,
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    guard_rows(metadata, &rows)
}

/// Runtime authority uses the exact compiled artifact lock even when an older
/// state's operator rows omitted installed descriptions. Qualified source IDs
/// come from that lock; unqualified project files have no inferred registry trust.
pub(in crate::review) fn guard_effective(
    metadata: &Value,
    effective: &crate::configuration::Effective,
) -> Result<Guard, Error> {
    guard_rows(metadata, &effective_rows(metadata, effective))
}

/// Actual runtime use can finish an exact previously confirmed adoption before
/// taking its final shared guard. Inspection and guard construction stay pure.
pub(in crate::review) fn recover_effective(
    metadata: &Value,
    effective: &crate::configuration::Effective,
) -> Result<(), Error> {
    let namespace = Namespace::user()?;
    let sources: BTreeSet<_> = effective_rows(metadata, effective)
        .into_iter()
        .map(|row| row.0)
        .collect();
    for source in sources {
        namespace.recover(source)?;
    }
    Ok(())
}

fn effective_rows<'a>(
    metadata: &'a Value,
    effective: &'a crate::configuration::Effective,
) -> Vec<(&'a str, &'a str, &'a str, &'a str)> {
    let installed = metadata["installed"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let mut rows = Vec::new();
    for (id, version, digest) in effective.pack_artifacts() {
        let qualified = id
            .rsplit_once('/')
            .and_then(|(prefix, _)| prefix.rsplit_once('/'))
            .map(|(source, _)| (source, &id[source.len() + 1..]));
        let selected = qualified.or_else(|| {
            installed
                .iter()
                .find(|row| row["id"] == id && row["version"] == version && row["digest"] == digest)
                .and_then(|row| row["source_identity"].as_str())
                .map(|source| (source, id))
        });
        if let Some((source, pack)) = selected {
            rows.push((source, pack, version, digest));
        }
    }
    rows
}

fn guard_rows(metadata: &Value, rows: &[(&str, &str, &str, &str)]) -> Result<Guard, Error> {
    let sources: BTreeSet<_> = rows.iter().map(|row| row.0).collect();
    if sources.is_empty() {
        return Ok(Guard {
            revision: crate::policy::fingerprint(b"no-registry-trust"),
            bindings: BTreeMap::new(),
            _lease: None,
            _retained: Vec::new(),
        });
    }
    let namespace = Namespace::user()?;
    if !metadata["registry_trust"].is_null() && !metadata["registry_trust"].is_object() {
        return Err(Error::Invalid("invalid retained registry trust modes"));
    }
    let lease = namespace.guard()?;
    let mut revisions = BTreeMap::new();
    let mut bindings = BTreeMap::new();
    let mut retained = Vec::new();
    for source in sources {
        if let Some(head) = namespace.read_guarded(source)? {
            check_reference(metadata, source, &head)?;
            for row in rows.iter().filter(|row| row.0 == source) {
                check_row(&head, row.1, row.2, row.3)?;
            }
            revisions.insert(source, format!("{}:{}", head.generation, head.object));
            bindings.insert(source.to_owned(), reference(&head));
            retained.push(head);
        } else {
            if !metadata["registry_trust"][source].is_null() {
                return Err(Error::Invalid(
                    "adopted registry trust metadata unavailable; restore user trust storage before using this policy",
                ));
            }
            revisions.insert(source, "legacy-pinned".into());
        }
    }
    Ok(Guard {
        revision: crate::json::fingerprint(&revisions)
            .map_err(|_| Error::Invalid("registry trust encoding"))?,
        bindings,
        _lease: Some(lease),
        _retained: retained,
    })
}

fn reference(head: &Retained) -> Value {
    json!({"mode":"versioned-v1","generation":head.generation,"head_revision":head.revision,"object":head.object,"root_version":head.root.version(),"root_revision":head.root_revision,"revocation_sequence":head.revocations.sequence(),"revocation_revision":head.revocation_revision})
}

fn check_reference(metadata: &Value, source: &str, head: &Retained) -> Result<(), Error> {
    if !metadata["registry_trust"].is_null() && !metadata["registry_trust"].is_object() {
        return Err(Error::Invalid("invalid retained registry trust modes"));
    }
    let saved = &metadata["registry_trust"][source];
    if saved.is_null() {
        return Ok(());
    }
    let fields = saved
        .as_object()
        .ok_or(Error::Invalid("invalid retained registry trust reference"))?;
    if fields.keys().any(|field| {
        ![
            "mode",
            "generation",
            "head_revision",
            "object",
            "root_version",
            "root_revision",
            "revocation_sequence",
            "revocation_revision",
        ]
        .contains(&field.as_str())
    }) || saved["mode"] != "versioned-v1"
    {
        return Err(Error::Invalid("invalid retained registry trust reference"));
    }
    let generation = saved["generation"]
        .as_u64()
        .filter(|generation| *generation > 0)
        .ok_or(Error::Invalid("invalid retained registry trust generation"))?;
    let hash = |field: &str| {
        saved[field]
            .as_str()
            .filter(|value| canonic_registry_kit::digest::is_sha256_hex(value))
            .ok_or(Error::Invalid("invalid retained registry trust digest"))
    };
    let object = hash("object")?;
    let revision = hash("head_revision")?;
    if head.generation < generation
        || (head.generation == generation && (head.object != object || head.revision != revision))
    {
        return Err(Error::Invalid(
            "registry trust head rolled back or conflicts with retained policy knowledge",
        ));
    }
    for (version, digest, actual_version, actual_digest) in [
        (
            "root_version",
            "root_revision",
            head.root.version(),
            head.root_revision.as_str(),
        ),
        (
            "revocation_sequence",
            "revocation_revision",
            head.revocations.sequence(),
            head.revocation_revision.as_str(),
        ),
    ] {
        if saved[version].is_null() && saved[digest].is_null() {
            continue;
        }
        let observed = saved[version]
            .as_u64()
            .filter(|value| *value > 0)
            .ok_or(Error::Invalid("invalid retained registry trust floor"))?;
        let digest = hash(digest)?;
        if actual_version < observed || (actual_version == observed && actual_digest != digest) {
            return Err(Error::Invalid(
                "registry trust metadata is older than retained policy knowledge",
            ));
        }
    }
    Ok(())
}

fn check_row(head: &Retained, id: &str, version: &str, digest: &str) -> Result<(), Error> {
    let id = id
        .parse()
        .map_err(|_| Error::Invalid("installed pack identity invalid"))?;
    let version = version
        .parse()
        .map_err(|_| Error::Invalid("installed pack version invalid"))?;
    head.revocations
        .check_release(head.root.source().identity(), &id, &version, digest)
        .map_err(|error| match error {
            canonic_registry_kit::trust::TrustError::Revoked => Error::RegistryRevoked {
                source_identity: head.root.source().identity().into(),
                pack: id.to_string(),
                version: version.to_string(),
                digest: digest.into(),
            },
            _ => Error::Invalid("registry trust artifact binding invalid"),
        })
}

pub(in crate::review) fn require_mode(metadata: &Value, source: &str) -> Result<(), Error> {
    match Namespace::user()?.read(source)? {
        Some(head) => check_reference(metadata, source, &head),
        None if !metadata["registry_trust"].is_null()
            && !metadata["registry_trust"].is_object() =>
        {
            Err(Error::Invalid("invalid retained registry trust modes"))
        }
        None if !metadata["registry_trust"][source].is_null() => Err(Error::Invalid(
            "adopted registry trust metadata unavailable; no legacy fallback",
        )),
        None => Ok(()),
    }
}

/// A frozen first-adoption plan: exact captured origin/anchor, signed root roles,
/// revocations, catalog checkpoint and prior global head. Nothing is rediscovered
/// after the person confirms these bytes.
pub(in crate::review) struct Prepared {
    source: Source,
    name: String,
    root: String,
    project_id: String,
    before: Value,
    /// The project's registries, a setting of the policy files.
    registries: canonic_registry_kit::registrations::Registrations,
    global_generation: Option<String>,
    previous: Option<Retained>,
    update: VerifiedUpdate,
    project_checkpoints: BTreeMap<String, Value>,
}
impl Prepared {
    pub(in crate::review) fn new(
        context: &Context,
        name: &str,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Self, Error> {
        let state = super::project::checkpoint(context)?;
        let metadata = super::project::metadata(&state);
        if !metadata["registry_trust"].is_null() && !metadata["registry_trust"].is_object() {
            return Err(Error::Invalid("invalid retained registry trust modes"));
        }
        let snapshot = registries::global_file()?
            .snapshot()
            .map_err(registries::registration_error)?;
        let effective = registries::merge(
            snapshot.registrations(),
            Some(&registries::project(&state.settings)),
        )?;
        let source = effective
            .get(name)
            .ok_or(Error::Invalid("registry source not enrolled"))?
            .clone();
        if source.identity() == registries::BUILTIN {
            return Err(Error::Invalid(
                "main registry versioned trust awaits an explicitly activated product release",
            ));
        }
        if !source.is_signed() {
            return Err(Error::Invalid(
                "versioned trust requires an enrolled HTTPS registry",
            ));
        }
        let previous = Namespace::user()?.read(source.identity())?;
        if let Some(previous) = &previous {
            check_reference(&metadata, source.identity(), previous)?;
        }
        if previous.is_none() && !metadata["registry_trust"][source.identity()].is_null() {
            return Err(Error::Invalid(
                "adopted registry trust metadata unavailable",
            ));
        }
        let update = fetch(
            &context.fetcher,
            &source,
            previous.as_ref(),
            &metadata,
            checkpoint,
        )?;
        let project_checkpoints = project_checkpoints(&state, &update, &metadata)?;
        Ok(Self {
            source,
            name: name.into(),
            root: context.project_root.clone(),
            project_id: context.project_id.clone(),
            before: metadata,
            registries: state.settings.registries.clone(),
            global_generation: snapshot.generation().map(str::to_owned),
            previous,
            update,
            project_checkpoints,
        })
    }
    pub(in crate::review) fn review(&self) -> Value {
        let anchor = self
            .source
            .key()
            .and_then(|key| canonic_registry_kit::digest::decode(key, 32))
            .map(|key| crate::policy::fingerprint(&key));
        json!({"operation":"registry versioned trust","change":"adopt-versioned-trust","alias":self.name,"source":self.source,
            "trust":{"basis":"Explicitly trust the authenticated root roles delegated by this enrolled origin/key. Future root transitions require old and new root thresholds; this is not independent publisher identity proof.","canonical_location":self.source.location(),"source_identity":self.source.identity(),"registry_identity":self.source.registry_id(),"signing_key_fingerprint_sha256":anchor},
            "previous_root":self.previous.as_ref().map(|head|head.root.summary()),"root":self.update.root().summary(),"head_revision":self.update.head_revision(),"revocation_sequence":self.update.revocations().sequence(),
            "scope":"user","scope_effect":"Security metadata and known revocations apply to this source in every workspace owned by this user, including copies and restored checkouts.","installed_policy":"preserved; known revoked artifacts are blocked pending repair","policy_effect":"This enrolls no pack permissions; policy installation still requires its ordinary review."})
    }
    pub(in crate::review) fn commit(
        self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Value, Error> {
        checkpoint()?;
        if context.project_root != self.root || context.project_id != self.project_id {
            return Err(Error::StaleSnapshot);
        }
        let global = registries::global_file()?
            .snapshot()
            .map_err(registries::registration_error)?;
        if global.generation().map(str::to_owned) != self.global_generation {
            return Err(Error::StaleSnapshot);
        }
        let state = super::project::checkpoint(context)?;
        if super::project::metadata(&state) != self.before
            || state.settings.registries != self.registries
        {
            return Err(Error::StaleSnapshot);
        }
        self.update
            .root()
            .check_fresh(SystemTime::now())
            .map_err(super::storage::registry_trust::trust_error)?;
        self.update
            .revocations()
            .check_fresh(SystemTime::now())
            .map_err(super::storage::registry_trust::trust_error)?;
        let publication = Namespace::user()?.publish(&self.update, self.previous.as_ref())?;
        let mut receipt = self.review();
        receipt["committed"] = json!(true);
        receipt["replayed"] = json!(publication.replayed);
        receipt["durability"] = json!(if publication.durable {
            "confirmed"
        } else {
            "unconfirmed"
        });
        receipt["repair_needed"] = json!(publication.repair_needed);
        let adopted = json!({"mode":"versioned-v1","generation":publication.generation,"head_revision":publication.revision,"object":publication.object,"root_version":self.update.root().version(),"root_revision":self.update.root().revision(),"revocation_sequence":self.update.revocations().sequence(),"revocation_revision":self.update.revocations().revision()});
        let store = Store::open(Path::new(&context.project_root))?;
        let result = store.update_checkpoint(context, checkpoint, |state| {
            if super::project::metadata(state) != self.before
                || state.settings.registries != self.registries
            {
                return Err(Error::StaleSnapshot);
            }
            if !state.operator["registry_trust"].is_object() {
                state.operator["registry_trust"] = json!({});
            }
            state.operator["registry_trust"][self.source.identity()] = adopted.clone();
            state.registry_checkpoints = self.project_checkpoints.clone();
            Ok(json!({}))
        });
        if result.is_err() {
            receipt["repair_needed"] = json!(true);
            receipt["project_reference_updated"] = json!(false);
        } else {
            receipt["project_reference_updated"] = json!(true);
        }
        Ok(receipt)
    }
}

fn protected(metadata: &Value, source: &str) -> Result<BTreeMap<String, String>, Error> {
    let mut protected = BTreeMap::new();
    for row in ["installed", "disabled", "shared_installed"]
        .iter()
        .flat_map(|field| metadata[*field].as_array().into_iter().flatten())
        .filter(|row| row["source_identity"] == source)
    {
        let id = row["id"]
            .as_str()
            .ok_or(Error::Invalid("installed pack identity missing"))?;
        let version = row["version"]
            .as_str()
            .ok_or(Error::Invalid("installed pack version missing"))?;
        let digest = row["digest"]
            .as_str()
            .ok_or(Error::Invalid("installed pack digest missing"))?;
        if protected
            .insert(format!("{id}@{version}"), digest.to_owned())
            .is_some_and(|old| old != digest)
        {
            return Err(Error::Invalid("retained registry release digest differs"));
        }
    }
    Ok(protected)
}

fn fetch(
    fetcher: &Fetcher,
    source: &Source,
    previous: Option<&Retained>,
    metadata: &Value,
    checkpoint: Checkpoint<'_>,
) -> Result<VerifiedUpdate, Error> {
    let empty = Checkpoints::default();
    let protected = protected(metadata, source.identity())?;
    let interrupted = std::cell::RefCell::new(None);
    let result = fetcher.versioned_catalog(
        VersionedRequest {
            source,
            previous: previous.map(|head| (&head.root, &head.revocations)),
            checkpoints: previous.map_or(&empty, |head| &head.checkpoints),
            protected: &protected,
            now: SystemTime::now(),
        },
        &|| {
            checkpoint().map_err(|error| {
                *interrupted.borrow_mut() = Some(error);
                canonic_registry_kit::acquisition::AcquisitionError::Cancelled
            })
        },
    );
    if let Some(error) = interrupted.into_inner() {
        return Err(if matches!(error, crate::policy::Error::Interrupted) {
            Error::Cancelled
        } else {
            error.into()
        });
    }
    result.map_err(|_| {
        Error::Invalid("versioned registry metadata acquisition refused; no legacy fallback")
    })
}

fn project_checkpoints(
    state: &State,
    update: &VerifiedUpdate,
    metadata: &Value,
) -> Result<BTreeMap<String, Value>, Error> {
    let mut checkpoints: Checkpoints = serde_json::from_value(json!(state.registry_checkpoints))
        .map_err(|_| Error::Invalid("invalid registry checkpoints"))?;
    checkpoints
        .admit_retaining(
            update.catalog(),
            &protected(metadata, update.root().source().identity())?,
        )
        .map_err(|error| registries::checkpoint_error(&error))?;
    registries::retained(&checkpoints)
}

/// Registered versioned sources refresh through their highest user-private head.
/// Legacy sources remain on the old path until explicit review adopts the mode.
pub(in crate::review) fn catalog(
    fetcher: &Fetcher,
    source: &Source,
) -> Result<Option<Catalog>, Error> {
    let namespace = Namespace::user()?;
    namespace.recover(source.identity())?;
    let Some(previous) = namespace.read(source.identity())? else {
        return Ok(None);
    };
    if previous.root.source() != source {
        return Err(Error::Invalid("registry trust anchor changed"));
    }
    let update = fetch(fetcher, source, Some(&previous), &json!({}), &|| Ok(()))?;
    let catalog = update.catalog().clone();
    namespace.publish(&update, Some(&previous))?;
    Ok(Some(catalog))
}

/// Contextual acquisition preserves active and disabled release immutability
/// before global publication, then CASes the project's small mode/checkpoint ref.
pub(in crate::review) fn checked_catalog(
    context: &Context,
    source: &Source,
    state: &State,
) -> Result<Option<Catalog>, Error> {
    let namespace = Namespace::user()?;
    namespace.recover(source.identity())?;
    let Some(previous) = namespace.read(source.identity())? else {
        require_mode(&state.operator, source.identity())?;
        return Ok(None);
    };
    if previous.root.source() != source {
        return Err(Error::Invalid("registry trust anchor changed"));
    }
    let metadata = super::project::metadata(state);
    check_reference(&metadata, source.identity(), &previous)?;
    let update = fetch(
        &context.fetcher,
        source,
        Some(&previous),
        &metadata,
        &|| Ok(()),
    )?;
    let next = project_checkpoints(state, &update, &metadata)?;
    let catalog = update.catalog().clone();
    let published = namespace.publish(&update, Some(&previous))?;
    let store = Store::open(Path::new(&context.project_root))?;
    store.update_checkpoint(context,&||Ok(()),|current|{
        if super::project::metadata(current)!=metadata||current.settings.registries!=state.settings.registries||current.registry_checkpoints!=state.registry_checkpoints{return Err(Error::StaleSnapshot);}
        current.registry_checkpoints=next.clone();
        if !current.operator["registry_trust"].is_object(){current.operator["registry_trust"]=json!({});}
        current.operator["registry_trust"][source.identity()]=json!({"mode":"versioned-v1","generation":published.generation,"head_revision":published.revision,"object":published.object,"root_version":update.root().version(),"root_revision":update.root().revision(),"revocation_sequence":update.revocations().sequence(),"revocation_revision":update.revocations().revision()});
        Ok(json!({}))
    })?;
    Ok(Some(catalog))
}

/// Applies the current known revocation set before any network/cache artifact use.
pub(in crate::review) fn check_entry(
    source: &Source,
    entry: &Entry,
    acquiring: bool,
) -> Result<(), Error> {
    let namespace = Namespace::user()?;
    if acquiring {
        namespace.recover(source.identity())?;
    }
    let _guard = namespace.guard()?;
    if let Some(head) = namespace.read_guarded(source.identity())? {
        if head.root.source() != source {
            return Err(Error::Invalid("registry trust anchor changed"));
        }
        if acquiring {
            head.root
                .check_fresh(SystemTime::now())
                .map_err(super::storage::registry_trust::trust_error)?;
            head.revocations
                .check_fresh(SystemTime::now())
                .map_err(super::storage::registry_trust::trust_error)?;
        }
        head.revocations
            .check_release(source.identity(), &entry.id, &entry.version, &entry.sha256)
            .map_err(|error| match error {
                canonic_registry_kit::trust::TrustError::Revoked => Error::RegistryRevoked {
                    source_identity: source.identity().into(),
                    pack: entry.id.to_string(),
                    version: entry.version.to_string(),
                    digest: entry.sha256.clone(),
                },
                _ => Error::Invalid("registry trust artifact binding invalid"),
            })?;
    }
    Ok(())
}
