//! Native resource authority and document generations, separate from Playwright.

use super::Event;
use crate::browser::contract::{MAX_RESOURCES, Reference};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

mod events;
mod frame_owners;
#[cfg(test)]
mod tests;

const MAX_RETIRED: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Context,
    Page,
    Frame,
    FrameOwner,
    Worker,
    World,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ResourceError {
    #[error("Native browser resource capacity is exhausted")]
    Capacity,
    #[error("Native reference belongs to another connection")]
    Foreign,
    #[error("Native reference is stale or its scope is closing")]
    Stale,
    #[error("Native resource type or ownership differs")]
    Scope,
    #[error("Native browser lifecycle transition is inconsistent")]
    Protocol,
    #[error("Native resource epoch is lost; reconnect before new work")]
    Lost,
}

struct Node {
    reference: Reference,
    kind: Kind,
    native: String,
    parent: Option<Reference>,
    session: Option<String>,
    loader: Option<String>,
    default_world: bool,
    execution_numeric_id: Option<i64>,
    initial_document: bool,
    initial_opener_world: Option<Reference>,
    origin: Option<String>,
    worker_kind: Option<WorkerKind>,
    native_parent: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkerKind {
    Dedicated,
    Shared,
    Service,
}

/// Private routing obtained from a checked reference. Never accept this as
/// caller authority or include it in ordinary public status.
#[derive(Debug)]
pub struct Route {
    pub session_id: String,
    pub frame: Option<Reference>,
    pub execution_unique_id: Option<String>,
    pub execution_numeric_id: Option<i64>,
}

pub struct Index {
    connection: Uuid,
    nodes: BTreeMap<Uuid, Node>,
    contexts: BTreeMap<String, Uuid>,
    targets: BTreeMap<String, Uuid>,
    sessions: BTreeMap<String, Uuid>,
    frames: BTreeMap<(Uuid, String), Uuid>,
    main_frames: BTreeMap<Uuid, Uuid>,
    worlds: BTreeMap<(String, i64), Uuid>,
    retired: BTreeSet<String>,
    closing: BTreeSet<Uuid>,
    suspended: BTreeSet<Uuid>,
    lost: bool,
    #[cfg(all(test, feature = "acceptance-browser-public"))]
    pub(crate) resource_observations: Vec<Value>,
}

impl Index {
    pub fn new(connection: Uuid) -> Self {
        Self {
            connection,
            nodes: BTreeMap::new(),
            contexts: BTreeMap::new(),
            targets: BTreeMap::new(),
            sessions: BTreeMap::new(),
            frames: BTreeMap::new(),
            main_frames: BTreeMap::new(),
            worlds: BTreeMap::new(),
            retired: BTreeSet::new(),
            closing: BTreeSet::new(),
            suspended: BTreeSet::new(),
            lost: false,
            #[cfg(all(test, feature = "acceptance-browser-public"))]
            resource_observations: Vec::new(),
        }
    }

    /// Account an owned createBrowserContext reply, including a late allocation.
    /// Target discovery must never use this method to adopt foreign contexts.
    pub fn register_context(&mut self, native: &str) -> Result<Reference, ResourceError> {
        if self.lost {
            return Err(ResourceError::Lost);
        }
        identity(native)?;
        if self.retired.contains(&format!("context:{native}")) {
            return Err(ResourceError::Stale);
        }
        if let Some(id) = self.contexts.get(native) {
            return Ok(self.nodes[id].reference);
        }
        let reference = self.issue(Kind::Context, native, None, None)?;
        self.contexts.insert(native.into(), reference.id);
        Ok(reference)
    }

    fn issue(
        &mut self,
        kind: Kind,
        native: &str,
        parent: Option<Reference>,
        session: Option<String>,
    ) -> Result<Reference, ResourceError> {
        identity(native)?;
        if self.nodes.len() >= MAX_RESOURCES {
            return Err(ResourceError::Capacity);
        }
        if let Some(parent) = parent {
            // Account late allocations while their parent closes; routing still
            // refuses every closing ancestor.
            if parent.connection_id != self.connection {
                return Err(ResourceError::Foreign);
            }
            if self
                .nodes
                .get(&parent.id)
                .is_none_or(|node| node.reference != parent)
            {
                return Err(ResourceError::Stale);
            }
        }
        let reference = Reference {
            connection_id: self.connection,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        self.nodes.insert(
            reference.id,
            Node {
                reference,
                kind,
                native: native.into(),
                parent,
                session,
                loader: None,
                default_world: false,
                execution_numeric_id: None,
                initial_document: false,
                initial_opener_world: None,
                origin: None,
                worker_kind: None,
                native_parent: None,
            },
        );
        Ok(reference)
    }

    /// Cleanup retains closing/suspended ownership; dispatch must still use route.
    pub(crate) fn retained_attachment(&self, session: &str) -> bool {
        self.sessions
            .get(session)
            .and_then(|id| self.nodes.get(id))
            .is_some_and(|node| node.session.as_deref() == Some(session))
    }
    /// Cleanup retains closing/suspended ownership; dispatch must still use route.
    pub(crate) fn retained(&self, reference: Reference, kind: Kind) -> bool {
        reference.connection_id == self.connection
            && self
                .nodes
                .get(&reference.id)
                .is_some_and(|node| node.reference == reference && node.kind == kind)
    }
    /// Observation ownership survives the closing fence; this never authorizes dispatch.
    pub(crate) fn retained_page_context(&self, page: Reference) -> Option<Reference> {
        self.retained(page, Kind::Page)
            .then(|| self.ancestor(page.id, Kind::Context))
            .flatten()
    }
    fn resolve(&self, reference: Reference, kind: Option<Kind>) -> Result<&Node, ResourceError> {
        if self.lost {
            return Err(ResourceError::Lost);
        }
        if reference.connection_id != self.connection {
            return Err(ResourceError::Foreign);
        }
        let node = self
            .nodes
            .get(&reference.id)
            .filter(|node| node.reference == reference)
            .ok_or(ResourceError::Stale)?;
        if kind.is_some_and(|kind| kind != node.kind) {
            return Err(ResourceError::Scope);
        }
        let mut current = Some(reference);
        while let Some(reference) = current {
            let parent = self
                .nodes
                .get(&reference.id)
                .filter(|node| node.reference == reference)
                .ok_or(ResourceError::Stale)?;
            if self.closing.contains(&reference.id) || self.suspended.contains(&reference.id) {
                return Err(ResourceError::Stale);
            }
            current = parent.parent;
        }
        Ok(node)
    }

    fn ancestor(&self, id: Uuid, kind: Kind) -> Option<Reference> {
        let mut current = self.nodes.get(&id);
        while let Some(node) = current {
            if node.kind == kind {
                return Some(node.reference);
            }
            current = node.parent.and_then(|parent| self.nodes.get(&parent.id));
        }
        None
    }

    pub fn route(&self, reference: Reference, kind: Option<Kind>) -> Result<Route, ResourceError> {
        let selected = self.resolve(reference, kind)?;
        let mut current = Some(selected);
        while let Some(node) = current {
            if let Some(session) = node
                .session
                .as_ref()
                .filter(|session| self.sessions.contains_key(*session))
            {
                return Ok(Route {
                    session_id: session.clone(),
                    frame: self.ancestor(reference.id, Kind::Frame),
                    execution_unique_id: (selected.kind == Kind::World)
                        .then(|| selected.native.clone()),
                    execution_numeric_id: selected.execution_numeric_id,
                });
            }
            current = node.parent.and_then(|parent| self.nodes.get(&parent.id));
        }
        Err(ResourceError::Scope)
    }

    pub(crate) fn native(&self, reference: Reference, kind: Kind) -> Result<String, ResourceError> {
        Ok(self.resolve(reference, Some(kind))?.native.clone())
    }
    pub(crate) fn page_parent_target(
        &self,
        page: Reference,
    ) -> Result<Option<String>, ResourceError> {
        Ok(self.resolve(page, Some(Kind::Page))?.native_parent.clone())
    }
    pub(crate) fn execution_world(
        &self,
        reference: Reference,
    ) -> Result<(Reference, Reference), ResourceError> {
        let node = self.resolve(reference, None)?;
        let scope = match node.kind {
            Kind::Page => self.page_frame(reference, None)?,
            Kind::FrameOwner => self.owner_frame(reference)?,
            Kind::Frame | Kind::Worker => reference,
            _ => return Err(ResourceError::Scope),
        };
        let world = self
            .nodes
            .values()
            .find(|node| {
                node.kind == Kind::World && node.parent == Some(scope) && node.default_world
            })
            .ok_or(ResourceError::Scope)?;
        self.resolve(world.reference, Some(Kind::World))?;
        Ok((scope, world.reference))
    }
    /// Diagnostic events derive authority from a live owned attachment only.
    pub(crate) fn diagnostic_scope(
        &self,
        session: &str,
    ) -> Option<(Reference, Option<Reference>, Option<Reference>)> {
        let id = *self.sessions.get(session)?;
        let node = self.nodes.get(&id)?;
        self.resolve(node.reference, None).ok()?;
        Some((
            self.ancestor(id, Kind::Context)?,
            self.ancestor(id, Kind::Page),
            self.ancestor(id, Kind::Worker),
        ))
    }
    /// Closing descendants still belong to a live context's disposal witness.
    pub(crate) fn context_sessions(
        &self,
        context: Reference,
    ) -> Result<Vec<String>, ResourceError> {
        self.resolve(context, Some(Kind::Context))?;
        Ok(self
            .nodes
            .values()
            .filter(|node| self.ancestor(node.reference.id, Kind::Context) == Some(context))
            .filter_map(|node| node.session.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect())
    }
    pub(crate) fn owning_page(&self, frame: Reference) -> Result<Reference, ResourceError> {
        self.resolve(frame, Some(Kind::Frame))?;
        self.ancestor(frame.id, Kind::Page)
            .ok_or(ResourceError::Scope)
    }

    pub(crate) fn parent_frame(&self, frame: Reference) -> Result<Reference, ResourceError> {
        let node = self.resolve(frame, Some(Kind::Frame))?;
        let parent = node.parent.ok_or(ResourceError::Scope)?;
        self.resolve(parent, Some(Kind::Frame))?;
        Ok(parent)
    }

    /// Page disposal owns only its page/frame/dedicated-worker attachments.
    pub(crate) fn page_sessions(&self, page: Reference) -> Result<Vec<String>, ResourceError> {
        self.resolve(page, Some(Kind::Page))?;
        Ok(self
            .nodes
            .values()
            .filter(|node| self.ancestor(node.reference.id, Kind::Page) == Some(page))
            .filter_map(|node| node.session.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    /// Resolve the requested document only inside its explicitly owned page.
    pub(crate) fn page_frame(
        &self,
        page: Reference,
        frame: Option<Reference>,
    ) -> Result<Reference, ResourceError> {
        self.resolve(page, Some(Kind::Page))?;
        let frame = match frame {
            Some(frame) => frame,
            None => self
                .main_frames
                .get(&page.id)
                .and_then(|id| self.nodes.get(id))
                .map(|node| node.reference)
                .ok_or(ResourceError::Scope)?,
        };
        self.resolve(frame, Some(Kind::Frame))?;
        if self.ancestor(frame.id, Kind::Page) != Some(page) {
            return Err(ResourceError::Scope);
        }
        Ok(frame)
    }

    /// Report the current document of a frame whose reference was already
    /// checked before an action. This lookup is never used to admit input.
    pub(crate) fn page_frame_after_action(
        &self,
        page: Reference,
        previous: Reference,
    ) -> Result<Reference, ResourceError> {
        if previous.connection_id != self.connection {
            return Err(ResourceError::Foreign);
        }
        let current = self.nodes.get(&previous.id).ok_or(ResourceError::Stale)?;
        self.page_frame(page, Some(current.reference))
    }

    pub(crate) fn world(
        &self,
        frame: Reference,
        session: &str,
        numeric: i64,
    ) -> Result<Reference, ResourceError> {
        self.resolve(frame, Some(Kind::Frame))?;
        let node = self
            .worlds
            .get(&(session.into(), numeric))
            .and_then(|id| self.nodes.get(id))
            .ok_or(ResourceError::Scope)?;
        self.resolve(node.reference, Some(Kind::World))?;
        if node.parent != Some(frame) {
            return Err(ResourceError::Scope);
        }
        Ok(node.reference)
    }
    /// Console arguments retain their actual execution world, including isolated
    /// worlds; no URL or main-frame guess can supply that identity.
    pub(crate) fn diagnostic_world(
        &self,
        session: &str,
        numeric: i64,
    ) -> Result<(Reference, Reference), ResourceError> {
        let node = self
            .worlds
            .get(&(session.into(), numeric))
            .and_then(|id| self.nodes.get(id))
            .ok_or(ResourceError::Scope)?;
        self.resolve(node.reference, Some(Kind::World))?;
        let scope = node.parent.ok_or(ResourceError::Scope)?;
        self.resolve(scope, None)?;
        let route = self.route(node.reference, Some(Kind::World))?;
        if route.session_id != session {
            return Err(ResourceError::Scope);
        }
        Ok((scope, node.reference))
    }
    /// Cleanup authority survives a closing parent until actual detachment.
    pub(crate) fn session_attached(&self, session: &str) -> bool {
        self.sessions
            .get(session)
            .and_then(|id| self.nodes.get(id))
            .is_some_and(|node| node.session.as_deref() == Some(session))
    }
    pub(crate) fn binding_world(
        &self,
        page: Reference,
        session: &str,
        numeric: i64,
    ) -> Result<Reference, ResourceError> {
        self.resolve(page, Some(Kind::Page))?;
        let node = self
            .worlds
            .get(&(session.into(), numeric))
            .and_then(|id| self.nodes.get(id))
            .ok_or(ResourceError::Scope)?;
        self.resolve(node.reference, Some(Kind::World))?;
        if !node.default_world || self.ancestor(node.reference.id, Kind::Page) != Some(page) {
            return Err(ResourceError::Scope);
        }
        Ok(node.reference)
    }
    pub(crate) fn page_worlds(&self, page: Reference) -> Vec<Reference> {
        self.nodes
            .values()
            .filter(|n| {
                n.kind == Kind::World
                    && n.default_world
                    && n.parent
                        .and_then(|p| self.nodes.get(&p.id))
                        .is_some_and(|p| p.kind == Kind::Frame)
                    && self.ancestor(n.reference.id, Kind::Page) == Some(page)
            })
            .map(|n| n.reference)
            .collect()
    }

    /// setDocumentContent replaces nodes without necessarily replacing its JS
    /// contexts. Fence document references while retaining those native worlds.
    pub(crate) fn content_replaced(
        &mut self,
        frame: Reference,
    ) -> Result<Reference, ResourceError> {
        self.resolve(frame, Some(Kind::Frame))?;
        let children: Vec<_> = self
            .nodes
            .values()
            .filter(|node| node.parent == Some(frame) && node.kind != Kind::World)
            .map(|node| node.reference)
            .collect();
        for child in children {
            self.retire(child)?;
        }
        let next = self.nodes.get_mut(&frame.id).ok_or(ResourceError::Stale)?;
        next.reference.generation = Uuid::new_v4();
        let next = next.reference;
        for node in self
            .nodes
            .values_mut()
            .filter(|node| node.parent == Some(frame))
        {
            node.reference.generation = Uuid::new_v4();
            node.parent = Some(next);
        }
        Ok(next)
    }

    pub(crate) fn event_frame(&self, session: &str, native: &str) -> Option<Reference> {
        let source = *self.sessions.get(session)?;
        let page = self.ancestor(source, Kind::Page)?;
        let node = self
            .nodes
            .get(self.frames.get(&(page.id, native.into()))?)?;
        self.resolve(node.reference, Some(Kind::Frame)).ok()?;
        Some(node.reference)
    }

    /// ownerFrame follows DOM adoption across pages, within the original context.
    pub(crate) fn context_frame(&self, context: Reference, native: &str) -> Option<Reference> {
        self.resolve(context, Some(Kind::Context)).ok()?;
        let mut matches = self.nodes.values().filter(|node| {
            node.kind == Kind::Frame
                && node.native == native
                && self.ancestor(node.reference.id, Kind::Context) == Some(context)
                && self.resolve(node.reference, Some(Kind::Frame)).is_ok()
        });
        let first = matches.next()?.reference;
        matches.next().is_none().then_some(first)
    }

    pub(crate) fn loader(&self, frame: Reference) -> Result<Option<String>, ResourceError> {
        Ok(self.resolve(frame, Some(Kind::Frame))?.loader.clone())
    }

    pub(crate) fn target_reference(&self, native: &str) -> Option<Reference> {
        self.targets
            .get(native)
            .and_then(|id| self.nodes.get(id))
            .map(|node| node.reference)
    }

    /// Setup can observe a navigation while enabling a frame. Refresh only this
    /// retained native attachment's document generation, never a caller's stale
    /// reference or a detached/replaced session.
    pub(crate) fn attachment_current(
        &self,
        reference: Reference,
        kind: Kind,
        session: &str,
    ) -> Result<Reference, ResourceError> {
        if self.lost {
            return Err(ResourceError::Lost);
        }
        if reference.connection_id != self.connection {
            return Err(ResourceError::Foreign);
        }
        let node = self.nodes.get(&reference.id).ok_or(ResourceError::Stale)?;
        if node.kind != kind
            || node.session.as_deref() != Some(session)
            || self.sessions.get(session) != Some(&reference.id)
            || !self.targets.values().any(|id| *id == reference.id)
        {
            return Err(ResourceError::Scope);
        }
        if kind != Kind::Frame && node.reference != reference {
            return Err(ResourceError::Stale);
        }
        self.resolve(node.reference, Some(kind))?;
        Ok(node.reference)
    }

    /// An explicit attach reply and its event may arrive in either order. Both
    /// witnesses bind the same owned target; neither can adopt a foreign target.
    pub(crate) fn register_attachment(
        &mut self,
        reference: Reference,
        session: &str,
    ) -> Result<(), ResourceError> {
        identity(session)?;
        let node = self.resolve(reference, None)?;
        if !matches!(node.kind, Kind::Page | Kind::Frame | Kind::Worker)
            || !self.targets.values().any(|id| *id == reference.id)
        {
            return Err(ResourceError::Scope);
        }
        if self.retired.contains(&format!("session:{session}")) {
            return Err(ResourceError::Stale);
        }
        if self
            .sessions
            .get(session)
            .is_some_and(|id| *id != reference.id)
            || node
                .session
                .as_deref()
                .is_some_and(|previous| previous != session)
        {
            return Err(ResourceError::Protocol);
        }
        self.nodes
            .get_mut(&reference.id)
            .ok_or(ResourceError::Stale)?
            .session = Some(session.into());
        self.sessions.insert(session.into(), reference.id);
        Ok(())
    }

    pub(crate) fn list(&self, kind: Kind) -> Vec<(Reference, Option<Reference>)> {
        self.nodes
            .values()
            .filter(|node| node.kind == kind && self.resolve(node.reference, Some(kind)).is_ok())
            .map(|node| (node.reference, node.parent))
            .collect()
    }

    pub(crate) fn targets(&self) -> Vec<(Reference, Kind, String, Option<String>)> {
        self.targets
            .iter()
            .filter_map(|(native, id)| {
                let node = self.nodes.get(id)?;
                self.resolve(node.reference, None).ok()?;
                Some((
                    node.reference,
                    node.kind,
                    native.clone(),
                    node.session.clone(),
                ))
            })
            .collect()
    }
    pub(crate) fn worker_scope(
        &self,
        reference: Reference,
    ) -> Result<(Reference, Option<Reference>, WorkerKind), ResourceError> {
        let node = self.resolve(reference, Some(Kind::Worker))?;
        Ok((
            self.ancestor(reference.id, Kind::Context)
                .ok_or(ResourceError::Scope)?,
            self.ancestor(reference.id, Kind::Page),
            node.worker_kind.ok_or(ResourceError::Protocol)?,
        ))
    }
    pub(crate) fn context_worker(&self, reference: Reference) -> bool {
        self.worker_scope(reference)
            .is_ok_and(|(_, page, kind)| page.is_none() && kind != WorkerKind::Dedicated)
    }

    pub fn mark_closing(&mut self, reference: Reference) -> Result<(), ResourceError> {
        self.resolve(reference, None)?;
        self.closing.insert(reference.id);
        Ok(())
    }

    /// Retire after a native close witness. Preserve the remaining private table
    /// on epoch loss so the lifecycle owner can still reconcile owned cleanup.
    pub fn retire(&mut self, reference: Reference) -> Result<(), ResourceError> {
        if reference.connection_id != self.connection {
            return Err(ResourceError::Foreign);
        }
        if self
            .nodes
            .get(&reference.id)
            .is_none_or(|node| node.reference != reference)
        {
            return Err(ResourceError::Stale);
        }
        let mut pending = vec![reference.id];
        while let Some(id) = pending.pop() {
            // Actual frame retirement ends every owner retained for it.
            if let Some(node) = self.nodes.get(&id)
                && node.kind == Kind::Frame
            {
                let page = self.ancestor(id, Kind::Page);
                pending.extend(
                    self.nodes
                        .values()
                        .filter(|owner| {
                            owner.kind == Kind::FrameOwner
                                && owner.parent == page
                                && owner.native == node.native
                        })
                        .map(|owner| owner.reference.id),
                );
            }
            pending.extend(
                self.nodes
                    .values()
                    .filter(|node| node.parent.is_some_and(|parent| parent.id == id))
                    .map(|node| node.reference.id),
            );
            if let Some(node) = self.nodes.remove(&id) {
                if node.kind == Kind::Context {
                    self.retired.insert(format!("context:{}", node.native));
                }
                if node.kind == Kind::World
                    && let Some(session) = &node.session
                {
                    self.retired
                        .insert(format!("world:{session}:{}", node.native));
                }
                for ((page, native), frame) in &self.frames {
                    if *frame == id {
                        self.retired.insert(format!("frame:{page}:{native}"));
                    }
                }
                for (native, target) in &self.targets {
                    if *target == id {
                        self.retired.insert(format!("target:{native}"));
                    }
                }
                for (session, target) in &self.sessions {
                    if *target == id {
                        self.retired.insert(format!("session:{session}"));
                    }
                }
            }
            self.contexts.retain(|_, value| *value != id);
            self.targets.retain(|_, value| *value != id);
            self.sessions.retain(|_, value| *value != id);
            self.frames.retain(|_, value| *value != id);
            self.main_frames
                .retain(|page, frame| *page != id && *frame != id);
            self.worlds.retain(|_, value| *value != id);
            self.closing.remove(&id);
            self.suspended.remove(&id);
        }
        self.retired_limit()
    }

    fn retired_limit(&mut self) -> Result<(), ResourceError> {
        if self.retired.len() > MAX_RETIRED {
            self.lost = true;
            Err(ResourceError::Capacity)
        } else {
            Ok(())
        }
    }

    fn document_changed(
        &mut self,
        frame: Uuid,
        loader: Option<String>,
    ) -> Result<(), ResourceError> {
        let children: Vec<_> = self
            .nodes
            .values()
            .filter(|node| node.parent.is_some_and(|parent| parent.id == frame))
            .map(|node| node.reference)
            .collect();
        for child in children {
            self.retire(child)?;
        }
        let node = self.nodes.get_mut(&frame).ok_or(ResourceError::Stale)?;
        node.reference.generation = Uuid::new_v4();
        node.loader = loader;
        node.initial_document = false;
        Ok(())
    }

    /// Apply events before exposing dependent references. Inconsistent admitted
    /// transitions fence the epoch; foreign metadata cannot create authority.
    pub fn apply(&mut self, event: Event) -> Result<(), ResourceError> {
        #[cfg(all(test, feature = "acceptance-browser-public"))]
        if self.resource_observations.len() < 256
            && matches!(
                event.method.as_str(),
                "Target.attachedToTarget"
                    | "Page.frameNavigated"
                    | "Runtime.executionContextCreated"
                    | "Runtime.executionContextDestroyed"
                    | "Runtime.executionContextsCleared"
            )
        {
            self.resource_observations.push(
                json!({"method":event.method,"sessionId":event.session_id,"params":event.params}),
            );
        }
        if self.lost {
            return Err(ResourceError::Lost);
        }
        let result = super::schema::event(&event.method, &event.params)
            .map_err(|_| ResourceError::Protocol)
            .and_then(|()| self.apply_inner(event));
        if result.is_err() {
            self.lost = true;
        }
        result
    }

    pub fn lose(&mut self) {
        self.lost = true;
    }

    /// Public identities only: no endpoints, native IDs, URLs or page data.
    pub fn snapshot(&self) -> Value {
        json!({"connectionId":self.connection,"lost":self.lost,"resources":self.nodes.values().map(|node| json!({"reference":node.reference,"kind":node.kind,"parent":node.parent,"closing":self.closing.contains(&node.reference.id)})).collect::<Vec<_>>()})
    }
}

fn identity(value: &str) -> Result<(), ResourceError> {
    if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        Err(ResourceError::Protocol)
    } else {
        Ok(())
    }
}

fn text<'a>(value: &'a Value, name: &str) -> Result<&'a str, ResourceError> {
    let text = value[name].as_str().ok_or(ResourceError::Protocol)?;
    identity(text)?;
    Ok(text)
}
