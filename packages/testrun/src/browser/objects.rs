//! Driver object identity and parent lifetimes within one binding epoch.
//!
//! This transport graph confers no task/page authority. The dispatcher validates
//! initializers; workflows separately own documents, handles and selected pages.

use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Active graph capacity, including the synthetic Root object.
pub const MAX_OBJECTS: usize = 16_384;
const MAX_GUID_BYTES: usize = 1024;
const MAX_TYPE_BYTES: usize = 128;

/// A reference issued by this table carries its epoch and local serial and
/// cannot be deserialized from a public request. GUID reuse cannot revive it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ObjectRef {
    epoch: Uuid,
    serial: u64,
    guid: String,
}
impl ObjectRef {
    pub(crate) fn guid(&self) -> &str {
        &self.guid
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("Driver object graph was lost")]
    Lost,
    #[error("Object belongs to another driver epoch")]
    WrongEpoch,
    #[error("Driver object reference is stale")]
    Stale,
    #[error("Driver object GUID is unknown")]
    Unknown,
    #[error("Driver object has an unexpected type")]
    WrongType,
    #[error("Driver object identity or type exceeds its bounds")]
    InvalidIdentity,
    #[error("Driver object GUID is already live")]
    Duplicate,
    #[error("Driver object graph is at capacity")]
    Capacity,
    #[error("Driver object capacity must be between 1 and {MAX_OBJECTS}")]
    InvalidCapacity,
    #[error("Driver object serial space is exhausted")]
    Exhausted,
    #[error("The synthetic driver Root cannot be adopted or disposed")]
    Root,
    #[error("Driver object adoption would create a cycle")]
    Cycle,
}

struct Object {
    serial: u64,
    kind: String,
    parent: Option<String>,
    children: BTreeSet<String>,
}

/// One owner mutates the graph in wire-event order. Object creation may precede
/// a call result, so indexing is independent of pending-call completion.
pub struct ObjectTable {
    epoch: Uuid,
    next_serial: u64,
    objects: BTreeMap<String, Object>,
    capacity: usize,
    lost: bool,
}

impl Default for ObjectTable {
    fn default() -> Self {
        Self::with_capacity(MAX_OBJECTS).expect("Default object capacity is valid")
    }
}

impl ObjectTable {
    /// Read the type of an issued reference without accepting a wire GUID.
    pub fn kind(&self, reference: &ObjectRef) -> Result<&str, Error> {
        Ok(&self.object(reference)?.kind)
    }
    pub(crate) fn children(&self, parent: &ObjectRef, kind: &str) -> Result<Vec<ObjectRef>, Error> {
        self.object(parent)?
            .children
            .iter()
            .filter_map(|guid| {
                self.objects
                    .get(guid)
                    .filter(|object| object.kind == kind)
                    .map(|_| self.lookup(guid, kind))
            })
            .collect()
    }
    /// Prove channel ownership without inferring an execution frame from a URL.
    pub(crate) fn owned_by(&self, child: &ObjectRef, parent: &ObjectRef) -> Result<bool, Error> {
        self.object(child)?;
        self.object(parent)?;
        let mut cursor = Some(child.guid.as_str());
        for _ in 0..64 {
            let Some(guid) = cursor else {
                return Ok(false);
            };
            if guid == parent.guid {
                return Ok(true);
            }
            cursor = self
                .objects
                .get(guid)
                .ok_or(Error::Unknown)?
                .parent
                .as_deref();
        }
        Ok(false)
    }

    pub(crate) fn wire_kind(&self, guid: &str) -> Result<&str, Error> {
        self.live()?;
        Ok(&self.objects.get(guid).ok_or(Error::Unknown)?.kind)
    }

    pub fn with_capacity(capacity: usize) -> Result<Self, Error> {
        if !(1..=MAX_OBJECTS).contains(&capacity) {
            return Err(Error::InvalidCapacity);
        }
        let root = Object {
            serial: 0,
            kind: "Root".into(),
            parent: None,
            children: BTreeSet::new(),
        };
        Ok(Self {
            epoch: Uuid::new_v4(),
            next_serial: 1,
            objects: BTreeMap::from([(String::new(), root)]),
            capacity,
            lost: false,
        })
    }

    /// Resolve a private wire GUID and verify its expected protocol type.
    /// This binding operation is not a task-authorization check.
    pub fn lookup(&self, guid: &str, expected: &str) -> Result<ObjectRef, Error> {
        self.live()?;
        let object = self.objects.get(guid).ok_or(Error::Unknown)?;
        if object.kind != expected {
            return Err(Error::WrongType);
        }
        Ok(ObjectRef {
            epoch: self.epoch,
            serial: object.serial,
            guid: guid.to_owned(),
        })
    }

    /// Validate a previously issued reference before converting it to wire form.
    pub fn guid<'a>(&'a self, reference: &ObjectRef, expected: &str) -> Result<&'a str, Error> {
        let object = self.object(reference)?;
        if object.kind != expected {
            return Err(Error::WrongType);
        }
        // Return the table's owned key, never an unchecked reference string.
        Ok(self
            .objects
            .get_key_value(&reference.guid)
            .expect("Validated object")
            .0)
    }

    /// Apply a validated __create__ event. Unsupported type semantics and
    /// initializer validation belong to the versioned dispatcher, not this graph.
    pub fn create(
        &mut self,
        parent: &ObjectRef,
        guid: &str,
        kind: &str,
    ) -> Result<ObjectRef, Error> {
        self.object(parent)?;
        if guid.is_empty()
            || guid.len() > MAX_GUID_BYTES
            || kind.is_empty()
            || kind.len() > MAX_TYPE_BYTES
            || kind == "Root"
        {
            return Err(Error::InvalidIdentity);
        }
        if self.objects.contains_key(guid) {
            return Err(Error::Duplicate);
        }
        if self.objects.len() >= self.capacity {
            return Err(Error::Capacity);
        }
        let serial = self.next_serial;
        let next = serial.checked_add(1).ok_or(Error::Exhausted)?;
        let object = Object {
            serial,
            kind: kind.into(),
            parent: Some(parent.guid.clone()),
            children: BTreeSet::new(),
        };
        self.objects.insert(guid.into(), object);
        self.objects
            .get_mut(&parent.guid)
            .expect("Validated parent")
            .children
            .insert(guid.into());
        self.next_serial = next;
        Ok(ObjectRef {
            epoch: self.epoch,
            serial,
            guid: guid.into(),
        })
    }

    /// Apply __adopt__ without changing the object's reference or descendants.
    /// Invalid adoption leaves the complete graph unchanged.
    pub fn adopt(&mut self, child: &ObjectRef, parent: &ObjectRef) -> Result<(), Error> {
        self.object(child)?;
        self.object(parent)?;
        let previous = self.objects[&child.guid]
            .parent
            .clone()
            .ok_or(Error::Root)?;
        let mut ancestor = Some(parent.guid.as_str());
        while let Some(guid) = ancestor {
            if guid == child.guid {
                return Err(Error::Cycle);
            }
            ancestor = self.objects[guid].parent.as_deref();
        }
        self.objects
            .get_mut(&previous)
            .expect("Owned parent")
            .children
            .remove(&child.guid);
        self.objects
            .get_mut(&parent.guid)
            .expect("Validated parent")
            .children
            .insert(child.guid.clone());
        self.objects
            .get_mut(&child.guid)
            .expect("Validated child")
            .parent = Some(parent.guid.clone());
        Ok(())
    }

    /// Dispose a subtree iteratively; deep driver graphs cannot exhaust the
    /// stack. Freed GUIDs may be reused, but receive a distinct local serial.
    pub fn dispose(&mut self, reference: &ObjectRef) -> Result<usize, Error> {
        let parent = self.object(reference)?.parent.clone().ok_or(Error::Root)?;
        let mut pending = vec![reference.guid.clone()];
        let mut removed = 0;
        while let Some(guid) = pending.pop() {
            let object = self.objects.remove(&guid).expect("Owned subtree");
            pending.extend(object.children);
            removed += 1;
        }
        self.objects
            .get_mut(&parent)
            .expect("Surviving parent")
            .children
            .remove(&reference.guid);
        Ok(removed)
    }

    /// Driver loss invalidates every reference and stops further graph mutation.
    /// A new explicit connection must construct a new table/epoch.
    pub fn mark_lost(&mut self) {
        self.lost = true;
        self.objects.clear();
    }

    fn live(&self) -> Result<(), Error> {
        if self.lost { Err(Error::Lost) } else { Ok(()) }
    }

    fn object(&self, reference: &ObjectRef) -> Result<&Object, Error> {
        self.live()?;
        if reference.epoch != self.epoch {
            return Err(Error::WrongEpoch);
        }
        let object = self.objects.get(&reference.guid).ok_or(Error::Stale)?;
        if object.serial != reference.serial {
            return Err(Error::Stale);
        }
        Ok(object)
    }
}

#[cfg(test)]
mod tests;
