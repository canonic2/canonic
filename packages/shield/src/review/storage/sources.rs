//! Immutable durable source objects shared by review, active and proposal references.
use super::{Error, Store, private, sync};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
/// One private immutable-object namespace with an atomic durable root index.
/// Project checkpoints and per-user consent use the same codec, not authority.
pub(in crate::review) struct Namespace {
    pub(super) directory: PathBuf,
    user: bool,
}
impl Namespace {
    pub(in crate::review) fn project(store: &Store) -> Self {
        Self {
            directory: store.directory.clone(),
            user: false,
        }
    }
    pub(in crate::review) fn private(directory: &Path) -> Result<Self, Error> {
        super::private_files::directory(directory, false)?;
        Ok(Self {
            directory: directory.into(),
            user: true,
        })
    }
    fn external_usage(&self) -> Result<(u64, usize), Error> {
        if !self.user {
            return Ok((0, 0));
        }
        let entries = fs::read_dir(&self.directory)?
            .take(4097)
            .collect::<Result<Vec<_>, _>>()?;
        if entries.len() > 4096 {
            return Err(Error::CleanupPending);
        }
        // The index and its one atomic pending file reserve their full bounds.
        // Every preserved legacy/unknown top-level entry is charged by metadata.
        let mut bytes = 0u64;
        let mut files = 0usize;
        let mut index_bytes = 0u64;
        for entry in entries {
            let name = entry.file_name();
            if name == "sources" {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            if name == "state.json" || name == "state.pending" {
                index_bytes = index_bytes
                    .checked_add(metadata.len())
                    .ok_or(crate::policy::Error::Limit)?;
                continue;
            }
            bytes = bytes
                .checked_add(metadata.len())
                .ok_or(crate::policy::Error::Limit)?;
            files += 1;
        }
        Ok((
            bytes.saturating_add(index_bytes.max(4 * 1024 * 1024)),
            files + 2,
        ))
    }
    pub(in crate::review) fn retain_owned(
        &self,
        mut value: Value,
    ) -> Result<(Value, super::objects::Pins), Error> {
        let pins = compact_namespace(self, &mut value, true)?;
        Ok((value, pins))
    }
    pub(in crate::review) fn retain_map(
        &self,
        candidates: &std::collections::BTreeMap<String, Value>,
    ) -> Result<
        (
            std::collections::BTreeMap<String, Value>,
            super::objects::Pins,
        ),
        Error,
    > {
        let roots: serde_json::Map<_, _> = candidates
            .iter()
            .map(|(name, value)| {
                (
                    name.clone(),
                    if value["kind"] == "candidate-bundle" {
                        Value::Null
                    } else {
                        value.clone()
                    },
                )
            })
            .collect();
        let mut batch = retention::Batch::new(&Value::Object(roots), true)?;
        let mut retained = std::collections::BTreeMap::new();
        for (name, value) in candidates {
            let reference = if value["kind"] == "candidate-bundle" {
                manifest::retain(self, value, &mut batch)?
            } else {
                if value["kind"] != KIND && value["kind"] != manifest::KIND {
                    return Err(Error::Invalid("unsupported retained candidate"));
                }
                check(value)?;
                value.clone()
            };
            retained.insert(name.clone(), reference);
        }
        Ok((retained, batch.finish(self)?))
    }
    pub(in crate::review) fn captured(&self, reference: &Value) -> Result<Value, Error> {
        let mut value = reference.clone();
        hydrate_namespace(self, &mut value)?;
        Ok(value)
    }
    pub(in crate::review) fn field(
        &self,
        reference: &Value,
        field: &str,
        index: Option<usize>,
    ) -> Result<Value, Error> {
        if reference["kind"] == manifest::KIND {
            manifest::field(self, reference, field, index)
        } else {
            let value = self.captured(reference)?;
            Ok(match index {
                Some(index) => value[field].get(index).ok_or(Error::Missing)?.clone(),
                None => value[field].clone(),
            })
        }
    }
    pub(in crate::review) fn pin(&self, reference: &Value) -> Result<super::objects::Pins, Error> {
        retention::pin(self, reference)
    }
    pub(in crate::review) fn import(
        &self,
        source: &Self,
        reference: &Value,
    ) -> Result<(Value, super::objects::Pins), Error> {
        manifest::import(self, source, reference)
    }
    pub(in crate::review) fn retain_json(
        &self,
        value: &Value,
    ) -> Result<(Value, super::objects::Pins), Error> {
        manifest::retain_json(self, value)
    }
    pub(in crate::review) fn read_json(&self, reference: &Value) -> Result<Value, Error> {
        manifest::read_json(self, reference)
    }
}
#[cfg(test)]
pub(in crate::review) fn captured_field(
    store: &Store,
    reference: &Value,
    field: &str,
    index: Option<usize>,
) -> Result<Value, Error> {
    Namespace::project(store).field(reference, field, index)
}

const KIND: &str = "retained-source-v1";
mod manifest;
#[cfg(test)]
mod manifest_tests;
mod retention;
#[cfg(test)]
pub(in crate::review) use retention::tests::activate as activate_fixture;
pub(in crate::review) fn pin(
    store: &Store,
    candidate: &Value,
) -> Result<super::objects::Pins, Error> {
    Namespace::project(store).pin(candidate)
}
pub(in crate::review) fn pin_inspection(
    store: &Store,
    candidate: &Value,
) -> Result<super::objects::Pins, Error> {
    retention::pin_inspection(&Namespace::project(store), candidate)
}

pub(in crate::review) fn map_digest(
    candidates: &std::collections::BTreeMap<String, Value>,
) -> String {
    if candidates.len() == 1
        && let Some(value) = candidates.get("project")
        && (value["kind"] == KIND || value["kind"] == manifest::KIND)
        && check(value).is_ok()
        && let Some(digest) = value["candidate_digest"].as_str()
    {
        return digest.to_owned();
    }
    crate::json::fingerprint(candidates).expect("candidate JSON serializes")
}

pub(super) fn checkpoint(
    store: &Store,
    value: &mut Value,
    hydrate_source: bool,
) -> Result<super::objects::Pins, Error> {
    let namespace = Namespace::project(store);
    let mut batch = retention::Batch::new(value, false)?;
    let mut apply = |value: &mut Value| {
        if hydrate_source {
            hydrate(store, value)
        } else {
            compact_inner(&namespace, value, &mut batch)
        }
    };
    for name in ["candidates", "active"] {
        if !value[name].is_null() {
            if name == "active" {
                apply(&mut value[name]["candidates"])?;
            } else {
                apply(&mut value[name])?;
            }
        }
    }
    if let Some(proposals) = value["proposals"].as_array_mut() {
        for row in proposals {
            for name in ["candidate", "base_candidate"] {
                apply(&mut row[name])?;
            }
        }
    }
    batch.finish(&namespace)
}
#[cfg(test)]
pub(super) fn compact(store: &Store, value: &mut Value) -> Result<super::objects::Pins, Error> {
    compact_namespace(&Namespace::project(store), value, false)
}
fn compact_namespace(
    store: &Namespace,
    value: &mut Value,
    activating: bool,
) -> Result<super::objects::Pins, Error> {
    let mut batch = retention::Batch::new(value, activating)?;
    compact_inner(store, value, &mut batch)?;
    batch.finish(store)
}
fn compact_inner(
    store: &Namespace,
    value: &mut Value,
    batch: &mut retention::Batch,
) -> Result<(), Error> {
    if value["kind"] == "candidate-bundle" {
        *value = manifest::retain(store, value, batch)?;
    } else if value["kind"] == KIND || value["kind"] == manifest::KIND {
        check(value)?;
    } else {
        match value {
            Value::Object(map) => {
                for child in map.values_mut() {
                    compact_inner(store, child, batch)?;
                }
            }
            Value::Array(rows) => {
                for child in rows {
                    compact_inner(store, child, batch)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
/// The writer PID of a `<digest>.<pid>-<nanoseconds>.tmp` source temporary.
fn temporary_writer(name: &str) -> Option<u32> {
    let (digest, stamp) = name.strip_suffix(".tmp")?.split_once('.')?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    super::stamp_pid(stamp, 2)
}
pub(in crate::review) fn candidate_identity(value: &Value) -> String {
    if (value["kind"] == KIND || value["kind"] == manifest::KIND) && check(value).is_ok() {
        return value["digest"].as_str().unwrap_or_default().to_owned();
    }
    let _trace = crate::diagnostics::trace::span("source.identity");
    crate::diagnostics::trace::count("source.identity_encodings", 1);
    crate::json::fingerprint(value).expect("candidate JSON serializes")
}

fn check(value: &Value) -> Result<(), Error> {
    if value["kind"] == manifest::KIND {
        return manifest::check(value);
    }
    if value.as_object().is_none_or(|fields| fields.len() != 4)
        || ["digest", "candidate_digest"].iter().any(|key| {
            value[key]
                .as_str()
                .is_none_or(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        })
        || value["length"]
            .as_u64()
            .is_none_or(|n| n > crate::configuration::MAX_BUNDLE_BYTES as u64)
    {
        return Err(Error::Invalid("invalid retained source reference"));
    }
    Ok(())
}

pub(super) fn hydrate(store: &Store, value: &mut Value) -> Result<(), Error> {
    hydrate_namespace(&Namespace::project(store), value)
}
fn hydrate_namespace(store: &Namespace, value: &mut Value) -> Result<(), Error> {
    let _trace = crate::diagnostics::trace::span("source.hydrate");
    if value["kind"] == manifest::KIND {
        *value = manifest::hydrate(store, value)?;
    } else if value["kind"] == KIND {
        check(value)?;
        let digest = value["digest"]
            .as_str()
            .ok_or(Error::Invalid("source digest missing"))?;
        let path = store
            .directory
            .join("sources")
            .join(format!("{digest}.json"));
        let directory = fs::symlink_metadata(store.directory.join("sources"))?;
        let metadata = fs::symlink_metadata(&path)?;
        if directory.file_type().is_symlink()
            || !directory.is_dir()
            || metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() != value["length"].as_u64().unwrap_or_default()
        {
            return Err(Error::Invalid(
                "retained source must be a bounded regular file",
            ));
        }
        let mut bytes = Vec::new();
        super::objects::read_file(&path)?
            .take((crate::configuration::MAX_BUNDLE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if crate::policy::fingerprint(&bytes) != digest {
            return Err(Error::Invalid("retained source digest differs"));
        }
        crate::diagnostics::trace::count("source.hydrated", 1);
        crate::diagnostics::trace::count("source.read_bytes", bytes.len() as u64);
        let candidate: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Invalid("corrupt retained source"))?;
        if candidate["kind"] != "candidate-bundle"
            || crate::json::fingerprint(&std::collections::BTreeMap::from([(
                "project", &candidate,
            )]))
            .map_err(|_| Error::Invalid("source encoding"))?
                != value["candidate_digest"]
        {
            return Err(Error::Invalid("retained candidate binding differs"));
        }
        *value = candidate;
    } else {
        match value {
            Value::Object(map) => {
                for child in map.values_mut() {
                    hydrate_namespace(store, child)?;
                }
            }
            Value::Array(rows) => {
                for child in rows {
                    hydrate_namespace(store, child)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
