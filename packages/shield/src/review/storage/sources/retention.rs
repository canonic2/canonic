//! Durable references and live reader leases protect immutable source objects.
#[cfg(test)]
use super::fs;
use super::{BTreeSet, Error, KIND, Store, Value, check, temporary_writer};
use crate::review::storage::objects::{Directory, Pins};
#[cfg(test)]
use std::path::Path;

pub(super) fn collect(value: &Value, keep: &mut BTreeSet<String>) -> Result<(), Error> {
    if value["kind"] == "candidate-bundle" {
        return Ok(());
    } else if value["kind"] == KIND {
        check(value)?;
        keep.insert(format!(
            "{}.json",
            value["digest"]
                .as_str()
                .ok_or(Error::Invalid("source digest"))?
        ));
    } else {
        match value {
            Value::Object(map) => {
                for child in map.values() {
                    collect(child, keep)?;
                }
            }
            Value::Array(rows) => {
                for child in rows {
                    collect(child, keep)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn checkpoint_refs(value: &Value, keep: &mut BTreeSet<String>) -> Result<(), Error> {
    collect(&value["candidates"], keep)?;
    collect(&value["active"]["candidates"], keep)?;
    for proposal in value["proposals"].as_array().into_iter().flatten() {
        collect(&proposal["candidate"], keep)?;
        collect(&proposal["base_candidate"], keep)?;
    }
    Ok(())
}

#[derive(Default)]
struct Committed {
    main: crate::review::storage::objects::Checkpoint,
    recovery: std::collections::BTreeMap<String, crate::review::storage::objects::Checkpoint>,
}
impl Committed {
    fn read(&mut self, store: &Store, keep: &mut BTreeSet<String>) -> Result<Option<Value>, Error> {
        let Some(state) = self.main.read(&store.directory.join("state.json"), None)? else {
            return Ok(None);
        };
        checkpoint_refs(state, keep)?;
        for (prefix, pins) in [
            ("upgrade", &state["operator"]["upgrade_recovery"]),
            (
                "bootstrap",
                &state["operator"]["bootstrap"]["checkpoint_recovery"],
            ),
        ] {
            for digest in pins.as_array().into_iter().flatten() {
                let digest = digest
                    .as_str()
                    .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or(Error::Invalid("source recovery reference"))?;
                let name = format!("{prefix}-{digest}.json");
                let recovery = self
                    .recovery
                    .entry(name.clone())
                    .or_default()
                    .read(&store.directory.join(name), Some(digest))?
                    .ok_or(Error::Missing)?;
                checkpoint_refs(recovery, keep)?;
            }
        }
        Ok(Some(state["active"]["activation_id"].clone()))
    }
}
pub(super) struct Batch {
    keep: BTreeSet<String>,
    pin_names: BTreeSet<String>,
    directory: Option<Directory>,
    inventoried: bool,
    activation: Value,
    activating: bool,
}
impl Batch {
    pub(super) fn new(value: &Value, activating: bool) -> Result<Self, Error> {
        let mut keep = BTreeSet::new();
        if value.get("candidates").is_some() {
            checkpoint_refs(value, &mut keep)?;
        } else {
            collect(value, &mut keep)?;
        }
        Ok(Self {
            pin_names: if activating {
                keep.clone()
            } else {
                BTreeSet::new()
            },
            keep,
            directory: None,
            inventoried: false,
            activation: value["active"]["activation_id"].clone(),
            activating,
        })
    }
    pub(super) fn protect(&mut self, digest: &str) {
        self.keep.insert(format!("{digest}.json"));
        self.pin_names.insert(format!("{digest}.json"));
    }
    pub(super) fn lock(&mut self, store: &Store) -> Result<(), Error> {
        if self.directory.is_none() {
            self.directory = Some(Directory::enter(&store.directory.join("sources"))?);
        }
        Ok(())
    }
    pub(super) fn admit(&mut self, store: &Store, digest: &str, bytes: u64) -> Result<(), Error> {
        self.lock(store)?;
        if !self.inventoried {
            let mut committed = Committed::default();
            let current = committed.read(store, &mut self.keep)?;
            self.activating |= current
                .as_ref()
                .is_none_or(|current| current != &self.activation && !self.activation.is_null());
            self.directory
                .as_mut()
                .ok_or(Error::Locked)?
                .sweep_refresh(
                    &self.keep,
                    |name| {
                        name.strip_suffix(".json").is_some_and(|s| {
                            s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
                        }) || temporary_writer(name)
                            .is_some_and(|pid| !super::super::writer_running(pid))
                    },
                    |keep| committed.read(store, keep).map(|_| ()),
                )?;
            self.inventoried = true;
        }
        self.keep.insert(format!("{digest}.json"));
        self.directory
            .as_ref()
            .ok_or(Error::Locked)?
            .admit(bytes, 1, self.activating)
    }
    pub(super) fn added(&mut self, bytes: u64) {
        if let Some(directory) = &mut self.directory {
            directory.added(bytes);
        }
    }
    pub(super) fn finish(self, store: &Store) -> Result<Pins, Error> {
        if self.pin_names.is_empty() {
            return Ok(std::sync::Arc::new(Vec::new()));
        }
        match self.directory {
            Some(directory) => directory.pins(&self.pin_names),
            None => Directory::enter(&store.directory.join("sources"))?.pins(&self.pin_names),
        }
    }
}

pub(in crate::review) fn pin(store: &Store, candidate: &Value) -> Result<Pins, Error> {
    let mut names = BTreeSet::new();
    collect(candidate, &mut names)?;
    if names.is_empty() {
        return Ok(std::sync::Arc::new(Vec::new()));
    }
    Directory::enter(&store.directory.join("sources"))?.pins(&names)
}

#[cfg(test)]
pub(super) mod tests;
