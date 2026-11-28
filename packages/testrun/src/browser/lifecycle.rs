//! Pure browser authority, resource generations and operation outcomes.

use super::{
    contract::{ContextOptions, Engine, LaunchOptions, MAX_OPERATIONS, MAX_RESOURCES, Reference},
    objects::ObjectRef,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use uuid::Uuid;

mod frame_owners;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Starting,
    Ready,
    Closing,
    CleanupPending,
    Lost,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Browser,
    Context,
    Page,
    Frame,
    FrameOwner,
    Handle,
    Worker,
    Dialog,
    Route,
    Binding,
}

#[derive(Clone)]
pub(crate) struct Resource {
    pub reference: Reference,
    pub kind: Kind,
    pub wire: ObjectRef,
    pub parent: Option<Reference>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: Uuid,
    pub outcome: Outcome,
    pub settled: bool,
    pub result: Option<Value>,
    pub code: Option<String>,
    pub cancel_requested: bool,
    #[serde(skip)]
    pub(crate) fingerprint: String,
    /// Submission order, so the oldest settled operation makes room first.
    #[serde(skip)]
    pub(crate) sequence: u64,
}

impl Operation {
    /// Publish the outcome and return any journal failure that fences the session.
    pub(crate) fn settle(
        &mut self,
        outcome: Outcome,
        mut result: Option<Value>,
        code: Option<String>,
        mut journal_failure: Option<&'static str>,
    ) -> Option<&'static str> {
        // A verified disconnect is authoritative even when the journal cannot append.
        if self.fingerprint == "disconnect"
            && outcome == Outcome::Succeeded
            && let Some(receipt) = &mut result
            && let Some(failure) = journal_failure.take()
        {
            receipt["cleanup"]["journalFailure"] = json!(failure);
        }
        self.settled = true;
        self.result = result;
        (self.outcome, self.code) = match journal_failure {
            Some(failure) => (Outcome::Unknown, Some(failure.into())),
            None => (outcome, code),
        };
        journal_failure
    }

    fn summary(&self) -> Value {
        json!({
            "id": self.id,
            "outcome": self.outcome,
            "settled": self.settled,
            "result": null,
            "code": self.code,
            "cancelRequested": self.cancel_requested,
        })
    }
}

pub(crate) struct State {
    pub connection: Uuid,
    pub owner: Uuid,
    pub owner_generation: Uuid,
    pub engine: Engine,
    pub launch_proxy: Option<super::contract::ContextProxy>,
    pub phase: Phase,
    pub selected_context: Option<Reference>,
    pub selected_page: Option<Reference>,
    pub resources: BTreeMap<Uuid, Resource>,
    pub retired: BTreeMap<Reference, Kind>,
    closed_workers: HashSet<ObjectRef>,
    pub closing: BTreeSet<Reference>,
    pub operations: BTreeMap<Uuid, Operation>,
    submitted: u64,
    pub failure: Option<String>,
    pub identity: Option<Value>,
    pub context_options: BTreeMap<Reference, ContextOptions>,
    /// Contexts whose virtual WebAuthn authenticator is installed; the driver
    /// does not report it.
    pub authenticators: BTreeSet<Reference>,
}

impl State {
    #[cfg(test)]
    pub fn new(owner: Uuid, generation: Uuid, options: &LaunchOptions) -> Self {
        Self::connection(Uuid::new_v4(), owner, generation, options)
    }
    /// The state of the browser connection `connection`, which composition
    /// names before launch so a session can address several browsers.
    pub fn connection(
        connection: Uuid,
        owner: Uuid,
        generation: Uuid,
        options: &LaunchOptions,
    ) -> Self {
        Self {
            connection,
            owner,
            owner_generation: generation,
            engine: options.engine,
            launch_proxy: options.proxy.clone(),
            phase: Phase::Starting,
            selected_context: None,
            selected_page: None,
            resources: BTreeMap::new(),
            retired: BTreeMap::new(),
            closed_workers: HashSet::new(),
            closing: BTreeSet::new(),
            operations: BTreeMap::new(),
            submitted: 0,
            failure: None,
            identity: None,
            context_options: BTreeMap::new(),
            authenticators: BTreeSet::new(),
        }
    }
    pub fn issue(
        &mut self,
        kind: Kind,
        wire: ObjectRef,
        parent: Option<Reference>,
    ) -> Result<Reference, &'static str> {
        if kind == Kind::Worker && self.closed_workers.contains(&wire) {
            return Err("STALE_REFERENCE");
        }
        // A frame owner shares its frame's wire object as a separate resource.
        if let Some(existing) = self
            .resources
            .values()
            .find(|r| r.wire == wire && (r.kind == Kind::FrameOwner) == (kind == Kind::FrameOwner))
        {
            if existing.kind != kind || existing.parent != parent {
                return Err("RESOURCE_SCOPE_CONFLICT");
            }
            return Ok(existing.reference);
        }
        if self.resources.len() >= MAX_RESOURCES {
            return Err("RESOURCE_CAPACITY");
        }
        if let Some(parent) = parent {
            // Allocation replies/events must still be accounted while their
            // parent is closing. Public access checks the closing fence later.
            if parent.connection_id != self.connection {
                return Err("FOREIGN_REFERENCE");
            }
            if self
                .resources
                .get(&parent.id)
                .is_none_or(|resource| resource.reference != parent)
            {
                return Err("STALE_REFERENCE");
            }
        }
        let reference = Reference {
            connection_id: self.connection,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        self.resources.insert(
            reference.id,
            Resource {
                reference,
                kind,
                wire,
                parent,
            },
        );
        Ok(reference)
    }
    pub fn resource(
        &self,
        reference: Reference,
        kind: Option<Kind>,
    ) -> Result<&Resource, &'static str> {
        if reference.connection_id != self.connection {
            return Err("FOREIGN_REFERENCE");
        }
        let resource = self.resources.get(&reference.id).ok_or("STALE_REFERENCE")?;
        if resource.reference != reference {
            return Err("STALE_REFERENCE");
        }
        if kind.is_some_and(|kind| kind != resource.kind) {
            return Err("INVALID_SCOPE");
        }
        let mut ancestor = Some(reference);
        while let Some(reference) = ancestor {
            if self.closing.contains(&reference) {
                return Err("SCOPE_CLOSING");
            }
            ancestor = self.resources.get(&reference.id).and_then(|r| r.parent);
        }
        Ok(resource)
    }
    pub fn by_wire(&self, wire: &ObjectRef) -> Option<Resource> {
        self.resources
            .values()
            .find(|r| &r.wire == wire && r.kind != Kind::FrameOwner)
            .cloned()
    }
    pub fn subtree(&self, reference: Reference) -> Vec<Resource> {
        let mut pending = vec![reference];
        let mut result = Vec::new();
        while let Some(reference) = pending.pop() {
            if let Some(resource) = self.resources.get(&reference.id)
                && resource.reference == reference
            {
                pending.extend(
                    self.resources
                        .values()
                        .filter(|child| child.parent == Some(reference))
                        .map(|child| child.reference),
                );
                result.push(resource.clone());
            }
        }
        result
    }
    pub fn ancestor(&self, reference: Reference, kind: Kind) -> Option<Reference> {
        let mut current = Some(reference);
        while let Some(reference) = current {
            let resource = self.resource(reference, None).ok()?;
            if resource.kind == kind {
                return Some(reference);
            }
            current = resource.parent;
        }
        None
    }
    /// Observation metadata retains exact ownership through a closing fence.
    /// New work must continue to use resource/ancestor and their live checks.
    pub(crate) fn retained_ancestor(&self, reference: Reference, kind: Kind) -> Option<Reference> {
        let mut current = Some(reference);
        while let Some(reference) = current {
            let resource = self.resources.get(&reference.id).filter(|resource| {
                resource.reference == reference && reference.connection_id == self.connection
            })?;
            if resource.kind == kind {
                return Some(reference);
            }
            current = resource.parent;
        }
        None
    }
    pub fn retire(&mut self, reference: Reference) {
        let mut pending = vec![reference];
        while let Some(reference) = pending.pop() {
            // Actual frame retirement ends every owner retained for it.
            if let Some(frame) = self.resources.get(&reference.id)
                && frame.kind == Kind::Frame
            {
                pending.extend(
                    self.resources
                        .values()
                        .filter(|owner| owner.kind == Kind::FrameOwner && owner.wire == frame.wire)
                        .map(|owner| owner.reference),
                );
            }
            pending.extend(
                self.resources
                    .values()
                    .filter(|r| r.parent == Some(reference))
                    .map(|r| r.reference),
            );
            let removed = self.resources.remove(&reference.id);
            self.context_options.remove(&reference);
            self.authenticators.remove(&reference);
            self.closing.remove(&reference);
            if self.selected_context == Some(reference) {
                self.selected_context = None;
            }
            if self.selected_page == Some(reference) {
                self.selected_page = None;
            }
            if let Some(resource) = removed {
                self.retired.insert(reference, resource.kind);
            }
        }
        while self.retired.len() > MAX_RESOURCES {
            self.retired.pop_first();
        }
    }
    /// Keep a close witness until the native channel is disposed. Enumeration
    /// or late observations cannot issue a fresh authority for that channel.
    pub fn close_worker(&mut self, reference: Reference) -> Result<(), &'static str> {
        let wire = self.resource(reference, Some(Kind::Worker))?.wire.clone();
        self.retire(reference);
        if self.closed_workers.len() >= super::objects::MAX_OBJECTS {
            return Err("RESOURCE_CAPACITY");
        }
        self.closed_workers.insert(wire);
        Ok(())
    }
    pub fn worker_disposed(&mut self, wire: &ObjectRef) {
        self.closed_workers.remove(wire);
    }
    pub fn navigate(&mut self, wire: &ObjectRef) {
        if let Some(frame) = self.by_wire(wire) {
            let children: Vec<_> = self
                .resources
                .values()
                .filter(|r| r.parent == Some(frame.reference))
                .map(|r| r.reference)
                .collect();
            for child in children {
                self.retire(child);
            }
            if let Some(resource) = self.resources.get_mut(&frame.reference.id) {
                resource.reference.generation = Uuid::new_v4();
            }
        }
    }
    pub fn begin(&mut self, id: Uuid, fingerprint: String) -> Result<bool, &'static str> {
        if let Some(previous) = self.operations.get(&id) {
            return if previous.fingerprint == fingerprint {
                Ok(false)
            } else {
                Err("OPERATION_CONFLICT")
            };
        }
        // A full history forgets its oldest settled operation; only
        // unsettled operations can exhaust it.
        if self.operations.len() >= MAX_OPERATIONS {
            let oldest = self
                .operations
                .values()
                .filter(|operation| operation.settled)
                .min_by_key(|operation| operation.sequence)
                .map(|operation| operation.id)
                .ok_or("OPERATION_CAPACITY")?;
            self.operations.remove(&oldest);
        }
        self.submitted += 1;
        self.operations.insert(
            id,
            Operation {
                id,
                fingerprint,
                outcome: Outcome::Queued,
                settled: false,
                result: None,
                code: None,
                cancel_requested: false,
                sequence: self.submitted,
            },
        );
        Ok(true)
    }
    pub fn snapshot(&self) -> Value {
        let mut snapshot = self.status_fields();
        let resources: Vec<_> = self
            .resources
            .values()
            .map(|r| json!({"reference":r.reference,"kind":r.kind,"parent":r.parent}))
            .collect();
        snapshot["resources"] = json!(resources);
        snapshot["operations"] = Value::Array(self.operation_summaries(self.operations.len()));
        snapshot
    }

    pub(crate) fn compact_status(&self, limit: usize) -> Value {
        let mut snapshot = self.status_fields();
        snapshot["operationsTotal"] = json!(self.operations.len());
        snapshot["operations"] = Value::Array(self.operation_summaries(limit));
        snapshot["detailsOmitted"] = json!(true);
        snapshot
    }

    fn operation_summaries(&self, limit: usize) -> Vec<Value> {
        let mut operations: Vec<_> = self.operations.values().collect();
        operations.sort_unstable_by_key(|operation| {
            let sequence = if operation.settled {
                u64::MAX - operation.sequence
            } else {
                operation.sequence
            };
            (operation.settled, sequence)
        });
        operations
            .into_iter()
            .take(limit)
            .map(Operation::summary)
            .collect()
    }

    pub(crate) fn pending_operation(&self) -> Option<Uuid> {
        self.operations
            .values()
            .filter(|operation| !operation.settled)
            .min_by_key(|operation| operation.sequence)
            .map(|operation| operation.id)
    }

    fn status_fields(&self) -> Value {
        json!({"connectionId":self.connection,"sessionId":self.owner,"sessionGeneration":self.owner_generation,
            "backend":self.engine.backend(),"engine":self.engine,"phase":self.phase,"identity":self.identity,
            "selectedContext":self.selected_context,"selectedPage":self.selected_page,
            "failure":self.failure})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::{
        contract::{ContextOptions, Mode},
        objects::ObjectTable,
    };
    fn state() -> State {
        State::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            &LaunchOptions {
                media_fixtures: Default::default(),
                proxy: None,
                engine: Engine::Firefox,
                mode: Mode::Headless,
                context: ContextOptions::default(),
            },
        )
    }
    #[test]
    fn a_full_operation_history_forgets_its_oldest_settled_operation() {
        let mut state = state();
        let ids: Vec<Uuid> = (0..MAX_OPERATIONS).map(|_| Uuid::new_v4()).collect();
        for id in &ids {
            assert!(state.begin(*id, "acme".into()).unwrap());
        }
        // Only unsettled operations can exhaust the history.
        assert_eq!(
            state.begin(Uuid::new_v4(), "acme".into()).unwrap_err(),
            "OPERATION_CAPACITY"
        );
        for id in [ids[3], ids[1], ids[2]] {
            state.operations.get_mut(&id).unwrap().settled = true;
        }
        let next = Uuid::new_v4();
        assert!(state.begin(next, "acme".into()).unwrap());
        assert!(!state.operations.contains_key(&ids[1]));
        assert!(state.operations.contains_key(&ids[2]));
        assert!(state.operations.contains_key(&ids[3]));
        assert!(state.operations.contains_key(&ids[0]));
        assert_eq!(state.operations.len(), MAX_OPERATIONS);
    }
    #[test]
    fn worker_close_witness_blocks_reissue_until_native_disposal_without_retaining_history() {
        let mut state = state();
        let mut objects = ObjectTable::default();
        let root = objects.lookup("", "Root").unwrap();
        let page = state
            .issue(
                Kind::Page,
                objects.create(&root, "page", "Page").unwrap(),
                None,
            )
            .unwrap();
        let worker_wire = objects.create(&root, "worker", "Worker").unwrap();
        let worker = state
            .issue(Kind::Worker, worker_wire.clone(), Some(page))
            .unwrap();
        let handle = state
            .issue(
                Kind::Handle,
                objects.create(&worker_wire, "value", "JSHandle").unwrap(),
                Some(worker),
            )
            .unwrap();
        state.close_worker(worker).unwrap();
        assert!(state.resource(worker, None).is_err());
        assert!(state.resource(handle, None).is_err());
        assert_eq!(
            state.issue(Kind::Worker, worker_wire.clone(), Some(page)),
            Err("STALE_REFERENCE")
        );
        objects.dispose(&worker_wire).unwrap();
        state.worker_disposed(&worker_wire);
        let replacement_wire = objects.create(&root, "worker", "Worker").unwrap();
        let replacement = state
            .issue(Kind::Worker, replacement_wire, Some(page))
            .unwrap();
        assert_ne!(replacement, worker);
        assert!(state.resource(worker, None).is_err());
        assert!(state.closed_workers.is_empty());
    }
    #[test]
    fn navigation_and_close_invalidate_descendants_without_retargeting() {
        let mut state = state();
        let mut objects = ObjectTable::default();
        let root = objects.lookup("", "Root").unwrap();
        let browser = state
            .issue(
                Kind::Browser,
                objects.create(&root, "browser", "Browser").unwrap(),
                None,
            )
            .unwrap();
        let context = state
            .issue(
                Kind::Context,
                objects.create(&root, "context", "BrowserContext").unwrap(),
                Some(browser),
            )
            .unwrap();
        let page = state
            .issue(
                Kind::Page,
                objects.create(&root, "page", "Page").unwrap(),
                Some(context),
            )
            .unwrap();
        let wire = objects.create(&root, "frame", "Frame").unwrap();
        let frame = state.issue(Kind::Frame, wire.clone(), Some(page)).unwrap();
        let handle = state
            .issue(
                Kind::Handle,
                objects.create(&root, "handle", "JSHandle").unwrap(),
                Some(frame),
            )
            .unwrap();
        state.selected_context = Some(context);
        state.selected_page = Some(page);
        state.navigate(&wire);
        assert!(state.resource(frame, None).is_err());
        assert!(state.resource(handle, None).is_err());
        assert!(state.resource(page, Some(Kind::Page)).is_ok());
        state.retire(context);
        assert_eq!(state.selected_page, None);
        assert_eq!(state.selected_context, None);
        let mut foreign = page;
        foreign.connection_id = Uuid::new_v4();
        assert_eq!(
            state.resource(foreign, None).err(),
            Some("FOREIGN_REFERENCE")
        );
    }
}
