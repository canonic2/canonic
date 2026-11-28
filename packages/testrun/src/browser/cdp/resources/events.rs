//! Scoped CDP target, frame and execution-context transitions for the native index.

use super::*;

impl Index {
    /// Seed existing document metadata from a validated snapshot; an existing
    /// frame tree is not a new navigation event.
    pub(crate) fn seed_frame_tree(
        &mut self,
        session: &str,
        result: &Value,
    ) -> Result<(), ResourceError> {
        super::super::schema::result("Page.getFrameTree", result)
            .map_err(|_| ResourceError::Protocol)?;
        let source = self
            .sessions
            .get(session)
            .copied()
            .ok_or(ResourceError::Scope)?;
        let mut frames = vec![&result["frameTree"]];
        while let Some(tree) = frames.pop() {
            self.index_document(&tree["frame"], source)?;
            if let Some(children) = tree["childFrames"].as_array() {
                frames.extend(children);
            }
        }
        Ok(())
    }

    fn index_document(&mut self, value: &Value, source: Uuid) -> Result<(), ResourceError> {
        let frame = self.frame(source, text(value, "id")?, value["parentId"].as_str())?;
        if value.get("parentId").is_none() && self.nodes[&source].kind == Kind::Page {
            self.main_frames.insert(source, frame);
        }
        let loader = text(value, "loaderId")?.to_owned();
        if self.nodes[&frame].loader.as_ref() != Some(&loader) {
            let initial = self.nodes[&frame].loader.is_none() && value["securityOrigin"] == "://";
            let opaque_popup = value.get("parentId").is_none()
                && value["url"] == "about:blank"
                && value["securityOrigin"] == "://"
                && self.nodes[&source].kind == Kind::Page
                && self.nodes[&source]
                    .initial_opener_world
                    .is_some_and(|world| {
                        self.resolve(world, Some(Kind::World))
                            .is_ok_and(|node| node.origin.as_deref() == Some("://"))
                    });
            let origin = value["securityOrigin"]
                .as_str()
                .filter(|origin| origin.starts_with("http://") || origin.starts_with("https://"))
                .or(opaque_popup.then_some("://"));
            if self.nodes[&frame].initial_document && origin.is_some() {
                // Chrome can retain a popup's initial realm across its first
                // same-origin commit. Opaque about:blank also requires the exact
                // still-live same-context opener realm captured at allocation.
                // Keep only witnessed matching realms, but
                // rotate their authority along with the document generation.
                let stale = self
                    .nodes
                    .values()
                    .filter(|node| {
                        node.parent.is_some_and(|parent| parent.id == frame)
                            && node.kind == Kind::World
                            && node.origin.as_deref() != origin
                    })
                    .map(|node| node.reference)
                    .collect::<Vec<_>>();
                for world in stale {
                    self.retire(world)?;
                }
                self.content_replaced(self.nodes[&frame].reference)?;
                let node = self.nodes.get_mut(&frame).ok_or(ResourceError::Stale)?;
                node.loader = Some(loader);
                node.initial_document = false;
            } else {
                self.document_changed(frame, Some(loader))?;
                self.nodes
                    .get_mut(&frame)
                    .ok_or(ResourceError::Stale)?
                    .initial_document = initial;
            }
        }
        self.suspended.remove(&frame);
        Ok(())
    }

    pub(super) fn apply_inner(&mut self, event: Event) -> Result<(), ResourceError> {
        let source = event
            .session_id
            .as_deref()
            .and_then(|session| self.sessions.get(session))
            .copied();
        if event.session_id.is_some() && source.is_none() {
            return Ok(());
        }
        if event.method.starts_with("Target.") {
            return self.target_event(&event, source);
        }
        if let Some(source) = source {
            if event.method.starts_with("Page.") {
                return self.page_event(&event, source);
            }
            if event.method.starts_with("Runtime.") {
                return self.runtime_event(&event, source);
            }
            if matches!(
                event.method.as_str(),
                "Inspector.detached" | "Inspector.targetCrashed"
            ) {
                self.retire(self.nodes[&source].reference)?;
            }
        }
        Ok(())
    }

    fn target_event(&mut self, event: &Event, source: Option<Uuid>) -> Result<(), ResourceError> {
        let params = &event.params;
        match event.method.as_str() {
            "Target.targetCreated" | "Target.targetInfoChanged" => {
                if event.method == "Target.targetInfoChanged"
                    && self.retired.contains(&format!(
                        "target:{}",
                        text(&params["targetInfo"], "targetId")?
                    ))
                {
                    // Chrome can report detached metadata after an acknowledged
                    // close but before targetDestroyed. It grants no allocation
                    // authority and cannot resurrect the retired reference.
                    return Ok(());
                }
                self.target(&params["targetInfo"], source)?;
            }
            "Target.attachedToTarget" => {
                let Some(target) = self.target(&params["targetInfo"], source)? else {
                    return Ok(());
                };
                let session = text(params, "sessionId")?;
                if self.retired.contains(&format!("session:{session}")) {
                    return Err(ResourceError::Stale);
                }
                if self.sessions.get(session).is_some_and(|id| *id != target) {
                    return Err(ResourceError::Scope);
                }
                if self.nodes[&target].kind == Kind::Frame && self.nodes[&target].session.is_none()
                {
                    // Scoped auto-attach may precede the parent's swap event.
                    // Fence the old in-process document before admitting the
                    // new renderer, so that late parent metadata cannot tear
                    // down its attachment or newly reported worlds.
                    self.document_changed(target, None)?;
                }
                self.suspended.remove(&target);
                let node = self.nodes.get_mut(&target).ok_or(ResourceError::Protocol)?;
                if node
                    .session
                    .as_deref()
                    .is_some_and(|previous| previous != session)
                {
                    return Err(ResourceError::Protocol);
                }
                node.session = Some(session.into());
                self.sessions.insert(session.into(), target);
            }
            "Target.targetDestroyed" | "Target.targetCrashed" => {
                if let Some(target) = self.targets.get(text(params, "targetId")?).copied() {
                    if self.nodes[&target].kind == Kind::Frame {
                        self.suspend_frame(target)?;
                    } else {
                        self.retire(self.nodes[&target].reference)?;
                    }
                }
            }
            "Target.detachedFromTarget" => {
                if let Some(target) = self.sessions.get(text(params, "sessionId")?).copied() {
                    if self.nodes[&target].kind == Kind::Frame {
                        self.suspend_frame(target)?;
                    } else {
                        self.retire(self.nodes[&target].reference)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn suspend_frame(&mut self, frame: Uuid) -> Result<(), ResourceError> {
        if self.suspended.contains(&frame) {
            return Ok(());
        }
        self.document_changed(frame, None)?;
        if let Some(session) = self
            .nodes
            .get_mut(&frame)
            .and_then(|node| node.session.take())
        {
            self.sessions.remove(&session);
            self.retired.insert(format!("session:{session}"));
            self.retired_limit()?;
        }
        // Parent Page.frameNavigated or a new scoped auto-attachment is the
        // witness that makes this logical frame routable again. Removal still
        // retires it through Page.frameDetached(reason: remove).
        self.suspended.insert(frame);
        Ok(())
    }

    fn target(
        &mut self,
        info: &Value,
        source: Option<Uuid>,
    ) -> Result<Option<Uuid>, ResourceError> {
        let native = text(info, "targetId")?;
        let Some(context) = self.target_context(info, source, native)? else {
            return Ok(None);
        };
        if self.retired.contains(&format!("target:{native}")) {
            return Err(ResourceError::Stale);
        }
        let (kind, worker_kind) = match info["type"].as_str() {
            Some("page") => (Kind::Page, None),
            Some("iframe") => (Kind::Frame, None),
            Some("worker") => (Kind::Worker, Some(WorkerKind::Dedicated)),
            Some("shared_worker") => (Kind::Worker, Some(WorkerKind::Shared)),
            Some("service_worker") => (Kind::Worker, Some(WorkerKind::Service)),
            _ => return Ok(None),
        };
        if let Some(target) = self.targets.get(native).copied() {
            if self
                .ancestor(target, Kind::Context)
                .is_none_or(|owner| owner.id != context)
                || self.nodes[&target].kind != kind
                || self.nodes[&target].worker_kind != worker_kind
            {
                return Err(ResourceError::Scope);
            }
            if kind == Kind::Page
                && let Some(parent) = info["parentId"].as_str()
            {
                let node = self.nodes.get_mut(&target).ok_or(ResourceError::Protocol)?;
                if node
                    .native_parent
                    .as_deref()
                    .is_some_and(|previous| previous != parent)
                {
                    return Err(ResourceError::Scope);
                }
                node.native_parent = Some(parent.into());
            }
            if worker_kind == Some(WorkerKind::Dedicated)
                && let Some(source) = source
            {
                if let Some(previous) = self.ancestor(target, Kind::Page) {
                    if self.ancestor(source, Kind::Page) != Some(previous) {
                        return Err(ResourceError::Scope);
                    }
                } else {
                    let parent = self.nodes[&source].reference;
                    self.nodes
                        .get_mut(&target)
                        .ok_or(ResourceError::Protocol)?
                        .parent = Some(parent);
                }
            }
            return Ok(Some(target));
        }
        let reference = if kind == Kind::Frame {
            let Some(source) = source else {
                return Ok(None);
            };
            let frame = self.frame(source, native, None)?;
            self.nodes[&frame].reference
        } else {
            let parent = if worker_kind == Some(WorkerKind::Dedicated) {
                source.map(|id| self.nodes[&id].reference)
            } else {
                None
            }
            .unwrap_or(self.nodes[&context].reference);
            self.issue(kind, native, Some(parent), None)?
        };
        self.targets.insert(native.into(), reference.id);
        if kind == Kind::Page {
            let opener_world = self.opaque_popup_opener(info, context);
            self.nodes
                .get_mut(&reference.id)
                .ok_or(ResourceError::Protocol)?
                .native_parent = info["parentId"].as_str().map(str::to_owned);
            self.nodes
                .get_mut(&reference.id)
                .ok_or(ResourceError::Protocol)?
                .initial_opener_world = opener_world;
        }
        self.nodes
            .get_mut(&reference.id)
            .ok_or(ResourceError::Protocol)?
            .worker_kind = worker_kind;
        Ok(Some(reference.id))
    }

    fn target_context(
        &self,
        info: &Value,
        source: Option<Uuid>,
        native: &str,
    ) -> Result<Option<Uuid>, ResourceError> {
        let explicit = info["browserContextId"].as_str();
        let context = explicit
            .and_then(|context| self.contexts.get(context))
            .copied();
        let inherited = source
            .and_then(|source| self.ancestor(source, Kind::Context))
            .map(|reference| reference.id);
        if explicit.is_some() && context.is_none() {
            return if self.targets.contains_key(native) {
                Err(ResourceError::Scope)
            } else {
                Ok(None)
            };
        }
        if context.is_some() && inherited.is_some() && context != inherited {
            return Err(ResourceError::Scope);
        }
        Ok(context.or(inherited))
    }

    fn opaque_popup_opener(&self, info: &Value, context: Uuid) -> Option<Reference> {
        if info["canAccessOpener"] != true
            || !matches!(info["url"].as_str(), Some("" | "about:blank"))
        {
            return None;
        }
        let page = *self.targets.get(info["openerId"].as_str()?)?;
        if self.nodes.get(&page)?.kind != Kind::Page
            || self.ancestor(page, Kind::Context)?.id != context
        {
            return None;
        }
        let frame = *self
            .frames
            .get(&(page, info["openerFrameId"].as_str()?.to_owned()))?;
        let (_, world) = self
            .execution_world(self.nodes.get(&frame)?.reference)
            .ok()?;
        (self
            .resolve(world, Some(Kind::World))
            .ok()?
            .origin
            .as_deref()
            == Some("://"))
        .then_some(world)
    }

    fn page_event(&mut self, event: &Event, source: Uuid) -> Result<(), ResourceError> {
        let params = &event.params;
        match event.method.as_str() {
            "Page.frameAttached" => {
                self.frame(
                    source,
                    text(params, "frameId")?,
                    Some(text(params, "parentFrameId")?),
                )?;
            }
            "Page.frameNavigated" => {
                self.index_document(&params["frame"], source)?;
                // A snapshot can seed the initial document before its first
                // navigation event. Only that event consumes the opener proof.
                if params["frame"].get("parentId").is_none()
                    && self.nodes[&source].kind == Kind::Page
                {
                    self.nodes
                        .get_mut(&source)
                        .ok_or(ResourceError::Protocol)?
                        .initial_opener_world = None;
                }
            }
            "Page.frameDetached" => {
                let page = self
                    .ancestor(source, Kind::Page)
                    .ok_or(ResourceError::Scope)?;
                if let Some(frame) = self
                    .frames
                    .get(&(page.id, text(params, "frameId")?.into()))
                    .copied()
                {
                    if params["reason"] == "swap" {
                        if self.nodes[&frame]
                            .session
                            .as_deref()
                            .is_some_and(|session| Some(session) != event.session_id.as_deref())
                        {
                            return Ok(());
                        }
                        self.document_changed(frame, None)?;
                        if let Some(session) = self
                            .nodes
                            .get_mut(&frame)
                            .and_then(|node| node.session.take())
                        {
                            self.sessions.remove(&session);
                            self.retired.insert(format!("session:{session}"));
                            self.retired_limit()?;
                        }
                    } else {
                        self.retire(self.nodes[&frame].reference)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn frame(
        &mut self,
        source: Uuid,
        native: &str,
        parent: Option<&str>,
    ) -> Result<Uuid, ResourceError> {
        let page = self
            .ancestor(source, Kind::Page)
            .ok_or(ResourceError::Scope)?;
        let key = (page.id, native.to_owned());
        if self
            .retired
            .contains(&format!("frame:{}:{native}", page.id))
        {
            return Err(ResourceError::Stale);
        }
        if self
            .frames
            .keys()
            .any(|(owner, frame)| *owner != page.id && frame == native)
        {
            return Err(ResourceError::Scope);
        }
        let parent = match parent {
            Some(native) => self
                .frames
                .get(&(page.id, native.into()))
                .map(|id| self.nodes[id].reference)
                .ok_or(ResourceError::Protocol)?,
            None => page,
        };
        if let Some(id) = self.frames.get(&key).copied() {
            // Worlds/OOPIF attachment can precede frame-parent metadata.
            if parent != page {
                let mut ancestor = Some(parent);
                while let Some(node) = ancestor {
                    if node.id == id {
                        return Err(ResourceError::Protocol);
                    }
                    ancestor = self.nodes.get(&node.id).and_then(|node| node.parent);
                }
                self.nodes
                    .get_mut(&id)
                    .ok_or(ResourceError::Protocol)?
                    .parent = Some(parent);
            }
            return Ok(id);
        }
        let reference = self.issue(Kind::Frame, native, Some(parent), None)?;
        self.frames.insert(key, reference.id);
        Ok(reference.id)
    }

    fn runtime_event(&mut self, event: &Event, source: Uuid) -> Result<(), ResourceError> {
        let session = event.session_id.as_deref().ok_or(ResourceError::Scope)?;
        match event.method.as_str() {
            "Runtime.executionContextCreated" => {
                let context = &event.params["context"];
                let numeric = context["id"].as_i64().ok_or(ResourceError::Protocol)?;
                let unique = text(context, "uniqueId")?;
                if self.retired.contains(&format!("world:{session}:{unique}")) {
                    return Err(ResourceError::Stale);
                }
                if self.worlds.contains_key(&(session.into(), numeric)) {
                    return Err(ResourceError::Protocol);
                }
                let parent = if self.nodes[&source].kind == Kind::Worker {
                    self.nodes[&source].reference
                } else if let Some(frame) = context["auxData"]["frameId"].as_str() {
                    let frame = self.frame(source, frame, None)?;
                    self.nodes[&frame].reference
                } else {
                    self.nodes[&source].reference
                };
                let reference =
                    self.issue(Kind::World, unique, Some(parent), Some(session.into()))?;
                self.worlds.insert((session.into(), numeric), reference.id);
                self.nodes
                    .get_mut(&reference.id)
                    .ok_or(ResourceError::Protocol)?
                    .execution_numeric_id = Some(numeric);
                self.nodes
                    .get_mut(&reference.id)
                    .ok_or(ResourceError::Protocol)?
                    .origin = context["origin"]
                    .as_str()
                    .filter(|origin| origin.len() <= 2048)
                    .map(str::to_owned);
                let default_world = context["auxData"]["isDefault"]
                    .as_bool()
                    .unwrap_or(self.nodes[&source].kind == Kind::Worker);
                self.nodes
                    .get_mut(&reference.id)
                    .ok_or(ResourceError::Protocol)?
                    .default_world = default_world;
            }
            "Runtime.executionContextsCleared" => {
                let worlds: Vec<_> = self
                    .worlds
                    .iter()
                    .filter(|((owner, _), _)| owner == session)
                    .map(|(_, id)| self.nodes[id].reference)
                    .collect();
                for world in worlds {
                    self.retire(world)?;
                }
            }
            "Runtime.executionContextDestroyed" => {
                let numeric = event.params["executionContextId"]
                    .as_i64()
                    .ok_or(ResourceError::Protocol)?;
                if let Some(world) = self.worlds.get(&(session.into(), numeric)).copied() {
                    if event.params["executionContextUniqueId"]
                        .as_str()
                        .is_some_and(|unique| unique != self.nodes[&world].native)
                    {
                        return Err(ResourceError::Protocol);
                    }
                    self.retire(self.nodes[&world].reference)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
