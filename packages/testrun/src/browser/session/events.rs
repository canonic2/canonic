//! Applies lifetime events independently of page mutations and expires callbacks.

use super::*;
use crate::browser::activity::Event as Activity;
use crate::browser::{dispatcher::Event, events::Item, objects::ObjectRef};

impl Host {
    pub(super) async fn start_events(self: &Arc<Self>, mut events: Events) {
        self.start_input_expiry();
        let host = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            while let Some((ordinal, item)) = events.next_ordered().await {
                let Some(host) = host.upgrade() else {
                    return;
                };
                match item {
                    Item::Barrier(reply) => {
                        let _ = reply.send(());
                    }
                    Item::Gap { dropped } => {
                        host.automation_debuggers.lock().unwrap().lose();
                        host.har_archives
                            .lock()
                            .expect("HAR owner")
                            .capture_gap(dropped);
                        host.set_failure("LIFECYCLE_EVENT_LOSS");
                        host.stopping.store(true, Ordering::SeqCst);
                    }
                    Item::Event(event) => {
                        if let Err(error) = host.event(event, ordinal).await {
                            host.remember_failure(&error);
                            // Disposal can precede a queued event; it must not revive
                            // an object or make a closed parent selectable again.
                            if !matches!(
                                error.code,
                                "STALE_REFERENCE"
                                    | "PAGE_CLOSED"
                                    | "SCOPE_CLOSING"
                                    | "BACKEND_LOST"
                                    | "BACKEND_PROTOCOL"
                            ) {
                                host.set_failure(error.code);
                            }
                        }
                    }
                    Item::Settled { .. } => {}
                }
                let live: BTreeSet<_> = host
                    .state
                    .lock()
                    .expect("Browser owner")
                    .resources
                    .values()
                    .map(|r| r.reference)
                    .collect();
                host.revoke_closed_viewer(&live);
                host.automation_debuggers.lock().unwrap().retain(&live);
                host.prune_disposed_diagnostic_arguments().await;
                host.routing
                    .rules
                    .lock()
                    .expect("Route owner")
                    .retain(|page| live.contains(&page));
                host.callbacks
                    .lock()
                    .expect("Callback owner")
                    .retain(|reference, _| live.contains(reference));
                host.responding
                    .lock()
                    .expect("Callback response owner")
                    .retain(|reference| live.contains(reference));
                host.binding_calls
                    .lock()
                    .expect("Binding calls")
                    .retain(|reference, _| live.contains(reference));
                host.bindings
                    .lock()
                    .expect("Bindings")
                    .retain(|scope| live.contains(&scope));
                host.transfers
                    .lock()
                    .expect("Transfers")
                    .choosers
                    .retain(|reference, _| live.contains(reference));
                host.changed.notify_waiters();
            }
            if let Some(host) = host.upgrade() {
                host.automation_debuggers.lock().unwrap().lose();
            }
            if let Some(host) = host.upgrade()
                && !host.stopping.load(Ordering::SeqCst)
            {
                host.set_failure("BACKEND_LOST");
            }
        });
        *self.events.lock().await = Some(task);
    }
    #[expect(
        clippy::too_many_lines,
        reason = "One ordered lifecycle event transition indexes scopes, applies document fences and retains cleanup before the next event"
    )]
    async fn event(self: &Arc<Self>, event: Event, ordinal: u64) -> Result<(), Error> {
        match event {
            Event::Disposed { object, .. } => {
                self.diagnostics
                    .lock()
                    .expect("Diagnostics")
                    .forget_object(&object);
                let mut state = self.state.lock().expect("Browser owner");
                state.worker_disposed(&object);
                if let Some(resource) = state.by_wire(&object) {
                    if resource.kind == Kind::Context {
                        self.request_inputs
                            .lock()
                            .expect("Request input owner")
                            .close_context(resource.reference);
                        self.http_contexts
                            .lock()
                            .expect("HTTP owner")
                            .context_defaults_closed(resource.reference);
                        self.diagnostics
                            .lock()
                            .expect("Diagnostics")
                            .enable(resource.reference, false);
                    }
                    state.retire(resource.reference);
                }
            }
            Event::Message {
                object,
                method,
                params,
            } => {
                let params = params.unwrap_or(json!({}));
                if self
                    .automation_debuggers
                    .lock()
                    .unwrap()
                    .event(&object, &method, &params)
                {
                    return Ok(());
                }
                let socket = match method.as_str() {
                    "open" => Some(crate::browser::har::SocketEvent::Open { status: None }),
                    "frameSent" | "frameReceived" => {
                        Some(crate::browser::har::SocketEvent::Frame {
                            sent: method == "frameSent",
                            opcode: params["opcode"].as_u64().unwrap_or(1),
                            data: params["data"].as_str().unwrap_or(""),
                        })
                    }
                    "socketError" => Some(crate::browser::har::SocketEvent::Error),
                    "close" => Some(crate::browser::har::SocketEvent::Closed),
                    _ => None,
                };
                if let Some(socket) = socket {
                    self.har_archives
                        .lock()
                        .expect("HAR owner")
                        .capture_socket_event(object.guid(), &socket);
                }
                if self.diagnostics.lock().expect("Diagnostics").socket_event(
                    &object,
                    &method,
                    &params,
                    super::page::now_ms(),
                ) {
                    return Ok(());
                }
                let resource = self.state.lock().expect("Browser owner").by_wire(&object);
                let Some(resource) = resource else {
                    return Ok(());
                };
                if resource.kind == Kind::Context
                    && matches!(
                        method.as_str(),
                        "console"
                            | "pageError"
                            | "request"
                            | "response"
                            | "requestFinished"
                            | "requestFailed"
                    )
                {
                    self.diagnostic_event(resource.reference, &method, &params)
                        .await?;
                    return Ok(());
                }
                match (resource.kind, method.as_str()) {
                    (Kind::Page, "webSocket") => {
                        let connection = self.connection()?;
                        let wire = connection
                            .resolve(
                                params["webSocket"]["guid"]
                                    .as_str()
                                    .ok_or("MISSING_WEBSOCKET")?,
                                "WebSocket",
                            )
                            .await?;
                        let init = connection
                            .initializer(&wire)
                            .await?
                            .ok_or("MISSING_INITIALIZER")?;
                        let context = self
                            .state
                            .lock()
                            .expect("Browser owner")
                            .ancestor(resource.reference, Kind::Context)
                            .ok_or("INVALID_SCOPE")?;
                        self.har_archives
                            .lock()
                            .expect("HAR owner")
                            .capture_socket_start(
                                context,
                                wire.guid().to_owned(),
                                Some(resource.reference),
                                init["url"].as_str().unwrap_or(""),
                            );
                        self.diagnostics.lock().expect("Diagnostics").socket_start(
                            wire,
                            context,
                            resource.reference,
                            init["url"].as_str().unwrap_or(""),
                            super::page::now_ms(),
                        );
                    }
                    (Kind::Page, "worker") => {
                        let wire = self
                            .connection()?
                            .resolve(
                                params["worker"]["guid"].as_str().ok_or("MISSING_WORKER")?,
                                "Worker",
                            )
                            .await?;
                        let worker = self.state.lock().expect("Browser owner").issue(
                            Kind::Worker,
                            wire,
                            Some(resource.reference),
                        )?;
                        self.record_activity(resource.reference, Activity::WorkerCreated, worker)?;
                    }
                    (Kind::Worker, "close") => self
                        .state
                        .lock()
                        .expect("Browser owner")
                        .close_worker(resource.reference)?,
                    (Kind::Page, "fileChooser") => {
                        self.chooser_event(resource.reference, &params).await?
                    }
                    (Kind::Page, "download") => {
                        self.download_event(resource.reference, &params).await?
                    }
                    (Kind::Context, "page") => {
                        let connection = self.connection()?;
                        let page = connection
                            .resolve(
                                params["page"]["guid"].as_str().ok_or("MISSING_PAGE")?,
                                "Page",
                            )
                            .await?;
                        self.index_page(&connection, page, resource.reference)
                            .await?;
                    }
                    (Kind::Page, "frameAttached") => {
                        self.inputs
                            .lock()
                            .expect("Browser input")
                            .document_changed(resource.reference);
                        self.index_frame(&params, resource.reference).await?;
                        let connection = self.connection()?;
                        if let Some(guid) = params["frame"]["guid"].as_str()
                            && let Ok(wire) = connection.resolve(guid, "Frame").await
                        {
                            let frame = self.state.lock().expect("Browser owner").by_wire(&wire);
                            if let Some(frame) = frame {
                                self.record_activity(
                                    resource.reference,
                                    Activity::FrameAttached,
                                    frame.reference,
                                )?;
                            }
                        }
                    }
                    (Kind::Page, "frameDetached") => {
                        self.inputs
                            .lock()
                            .expect("Browser input")
                            .document_changed(resource.reference);
                        let connection = self.connection()?;
                        if let Some(guid) = params["frame"]["guid"].as_str()
                            && let Ok(frame) = connection.resolve(guid, "Frame").await
                        {
                            let frame = self.state.lock().expect("Browser owner").by_wire(&frame);
                            if let Some(frame) = frame {
                                self.record_activity(
                                    resource.reference,
                                    Activity::FrameDetached,
                                    frame.reference,
                                )?;
                                self.state
                                    .lock()
                                    .expect("Browser owner")
                                    .retire(frame.reference);
                            }
                        }
                    }
                    // SDK `load`/`domcontentloaded` describe the main frame only.
                    (Kind::Frame, "loadstate") => {
                        let event = match params["add"].as_str() {
                            Some("load") => Activity::Load,
                            Some("domcontentloaded") => Activity::DomContentLoaded,
                            _ => return Ok(()),
                        };
                        let page = {
                            let state = self.state.lock().expect("Browser owner");
                            resource
                                .parent
                                .and_then(|parent| state.resource(parent, Some(Kind::Page)).ok())
                                .map(|page| page.reference)
                        };
                        if let Some(page) = page {
                            self.record_activity(page, event, page)?;
                        }
                    }
                    (Kind::Frame, "navigated") => {
                        let page = self
                            .state
                            .lock()
                            .expect("Browser owner")
                            .ancestor(resource.reference, Kind::Page);
                        if let Some(page) = page {
                            self.record_activity(
                                page,
                                Activity::FrameNavigated,
                                resource.reference,
                            )?;
                        }
                        if params.get("newDocument").is_none() {
                            return Ok(());
                        }
                        if let Some(page) = page {
                            self.inputs
                                .lock()
                                .expect("Browser input")
                                .document_changed(page);
                            self.previews
                                .lock()
                                .expect("Browser previews")
                                .document_changed(page, ordinal);
                        }
                        self.retire_document_handles(&object, resource.reference);
                    }
                    (Kind::Page, "viewportSizeChanged") => self
                        .inputs
                        .lock()
                        .expect("Browser input")
                        .invalidate(resource.reference),
                    (Kind::Context | Kind::Page | Kind::Browser, "close")
                    | (Kind::Page, "crash") => {
                        if resource.kind == Kind::Page {
                            self.record_activity(
                                resource.reference,
                                if method == "crash" {
                                    Activity::PageCrashed
                                } else {
                                    Activity::PageClosed
                                },
                                resource.reference,
                            )?;
                        }
                        let pages = self
                            .state
                            .lock()
                            .expect("Browser owner")
                            .subtree(resource.reference)
                            .into_iter()
                            .filter(|resource| resource.kind == Kind::Page)
                            .map(|resource| resource.reference)
                            .collect::<Vec<_>>();
                        for page in pages {
                            self.end_page_previews(page, method == "crash").await?;
                        }
                        if resource.kind == Kind::Context {
                            self.request_inputs
                                .lock()
                                .expect("Request input owner")
                                .close_context(resource.reference);
                            self.http_contexts
                                .lock()
                                .expect("HTTP owner")
                                .context_defaults_closed(resource.reference);
                            self.diagnostics
                                .lock()
                                .expect("Diagnostics")
                                .enable(resource.reference, false);
                        }
                        self.state
                            .lock()
                            .expect("Browser owner")
                            .retire(resource.reference);
                        if resource.kind == Kind::Browser && !self.stopping.load(Ordering::SeqCst) {
                            self.set_failure("BROWSER_LOST");
                        }
                    }
                    (Kind::Context | Kind::Page, "dialog" | "route" | "bindingCall") => {
                        self.index_callback(resource.reference, &method, &params)
                            .await?
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn retire_document_handles(self: &Arc<Self>, object: &ObjectRef, frame: Reference) {
        let handles = {
            let mut state = self.state.lock().expect("Browser owner");
            let handles = state
                .subtree(frame)
                .into_iter()
                .filter(|resource| resource.kind == Kind::Handle)
                .collect::<Vec<_>>();
            // Worker channels own their handles and close/dispose witnesses.
            // A frame document change cannot identify a worker's creator and
            // must not rotate or dispose an independently live worker scope.
            state.navigate(object);
            handles
        };
        let host = self.clone();
        tokio::spawn(async move {
            for handle in handles {
                if let Ok(connection) = host.connection() {
                    let result = connection
                        .retained(
                            &handle.wire,
                            "dispose",
                            json!({}),
                            Instant::now() + Duration::from_secs(30),
                            true,
                        )
                        .await;
                    let already_disposed = matches!(
                        &result,
                        Err(connection::Error::Protocol(
                            crate::browser::dispatcher::Error::Object(
                                crate::browser::objects::Error::Stale
                                    | crate::browser::objects::Error::Unknown
                            )
                        ))
                    );
                    if result.is_err() && !already_disposed && !host.stopping.load(Ordering::SeqCst)
                    {
                        host.set_failure("HANDLE_CLEANUP_PENDING");
                    }
                }
            }
        });
    }
    async fn index_frame(&self, params: &Value, page: Reference) -> Result<(), Error> {
        let connection = self.connection()?;
        let wire = connection
            .resolve(
                params["frame"]["guid"].as_str().ok_or("MISSING_FRAME")?,
                "Frame",
            )
            .await?;
        let init = connection
            .initializer(&wire)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        let parent = if let Some(guid) = init["parentFrame"]["guid"].as_str() {
            let parent = connection.resolve(guid, "Frame").await?;
            self.state
                .lock()
                .expect("Browser owner")
                .by_wire(&parent)
                .map(|r| r.reference)
                .ok_or("STALE_REFERENCE")?
        } else {
            page
        };
        self.state
            .lock()
            .expect("Browser owner")
            .issue(Kind::Frame, wire, Some(parent))?;
        Ok(())
    }
    async fn index_callback(
        self: &Arc<Self>,
        parent: Reference,
        method: &str,
        params: &Value,
    ) -> Result<(), Error> {
        let (field, wire_kind, kind) = match method {
            "dialog" => ("dialog", "Dialog", Kind::Dialog),
            "route" => ("route", "Route", Kind::Route),
            _ => ("binding", "BindingCall", Kind::Binding),
        };
        let connection = self.connection()?;
        let wire = connection
            .resolve(
                params[field]["guid"].as_str().ok_or("MISSING_CALLBACK")?,
                wire_kind,
            )
            .await?;
        let init = connection
            .initializer(&wire)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        let policy_scope = parent;
        let parent = self
            .callback_parent(&connection, parent, kind, &init)
            .await?;
        let reference =
            self.state
                .lock()
                .expect("Browser owner")
                .issue(kind, wire, Some(parent))?;
        if kind == Kind::Route
            && (self
                .callbacks
                .lock()
                .expect("Callback owner")
                .contains_key(&reference)
                || self
                    .responding
                    .lock()
                    .expect("Callback response owner")
                    .contains(&reference))
        {
            return Ok(());
        }
        let har_matches = if kind == Kind::Route {
            self.admit_route_callback(&connection, reference, policy_scope, &init)
                .await?
        } else {
            true
        };
        if kind == Kind::Binding {
            let state = self.state.lock().expect("Browser owner");
            let page = state.ancestor(parent, Kind::Page);
            let context = state.ancestor(parent, Kind::Context);
            let registrations = self.bindings.lock().expect("Bindings");
            if let Some(id) = init["name"].as_str().and_then(|name| {
                page.and_then(|scope| registrations.find(scope, name))
                    .or_else(|| context.and_then(|scope| registrations.find(scope, name)))
            }) {
                self.binding_calls
                    .lock()
                    .expect("Binding calls")
                    .insert(reference, id);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        if self
            .callbacks
            .lock()
            .expect("Callback owner")
            .insert(reference, deadline)
            .is_some()
        {
            return Ok(());
        }
        if kind == Kind::Dialog {
            let page = self
                .state
                .lock()
                .expect("Browser owner")
                .ancestor(parent, Kind::Page);
            if let Some(page) = page {
                self.record_activity(page, Activity::Dialog, reference)?;
            }
        }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep_until(deadline).await;
            let Some(host) = Weak::upgrade(&weak) else {
                return;
            };
            host.expire_callback(reference).await;
        });
        if kind == Kind::Route {
            self.automatic_route_callback(reference, policy_scope, har_matches)
                .await?;
        }
        Ok(())
    }
    async fn automatic_route_callback(
        self: &Arc<Self>,
        reference: Reference,
        policy_scope: Reference,
        har_matches: bool,
    ) -> Result<(), Error> {
        let unmatched = {
            let rules = self.routing.rules.lock().expect("Route owner");
            !har_matches
                || (rules.handlers.len(policy_scope) > 0
                    && rules.handlers.current(reference).is_none())
        };
        if unmatched {
            self.start_automatic_route_reply(reference, None).await?;
            return Ok(());
        }
        let owner = self
            .routing
            .owners
            .lock()
            .expect("Route owners")
            .get(&reference)
            .copied();
        let rule = owner
            .and_then(|scope| {
                self.routing
                    .rules
                    .lock()
                    .expect("Route owner")
                    .automatic(scope)
            })
            .filter(|rule| rule.phase == crate::browser::routing::HarPhase::Active);
        let handler = self
            .routing
            .rules
            .lock()
            .expect("Route owner")
            .handlers
            .current_har(reference);
        if let Some(rule) = rule {
            self.start_har_reply(reference, rule).await?;
        } else if let Some(handler) = handler {
            self.start_handler_har_reply(reference, handler).await?;
        }
        Ok(())
    }
    async fn admit_route_callback(
        &self,
        connection: &Connection,
        reference: Reference,
        policy_scope: Reference,
        init: &Value,
    ) -> Result<bool, Error> {
        let live_wires = self
            .state
            .lock()
            .expect("Browser owner")
            .resources
            .values()
            .filter(|resource| resource.kind == Kind::Route)
            .map(|resource| resource.wire.clone())
            .collect::<std::collections::HashSet<_>>();
        self.routing
            .overrides
            .lock()
            .expect("Route overrides")
            .retain(|wire, _| live_wires.contains(wire));
        let live = self
            .state
            .lock()
            .expect("Browser owner")
            .resources
            .values()
            .filter(|resource| resource.kind == Kind::Route)
            .map(|resource| resource.reference)
            .collect::<std::collections::BTreeSet<_>>();
        {
            let mut owners = self.routing.owners.lock().expect("Route owners");
            owners.retain(|reference, _| live.contains(reference));
            owners.insert(reference, policy_scope);
        }
        let request = connection
            .resolve(
                init["request"]["guid"].as_str().ok_or("MISSING_REQUEST")?,
                "Request",
            )
            .await?;
        let request = connection
            .initializer(&request)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        let mut rules = self.routing.rules.lock().expect("Route owner");
        if rules
            .automatic(policy_scope)
            .is_some_and(|rule| rule.url_filter)
            && request["url"].as_str().is_none()
        {
            return Err("ROUTE_REQUEST_UNAVAILABLE".into());
        }
        rules
            .handlers
            .retain_callbacks(|callback| live.contains(&callback));
        rules.handlers.admit(
            reference,
            policy_scope,
            request["url"].as_str().unwrap_or(""),
        );
        Ok(rules.automatic(policy_scope).is_none()
            || rules.har_matches(policy_scope, request["url"].as_str().unwrap_or("")))
    }
    pub(super) async fn expire_callback(self: &Arc<Self>, reference: Reference) {
        let reference = self
            .routing
            .handoffs
            .lock()
            .expect("Route handoffs")
            .get(&reference)
            .map_or(reference, |handoff| handoff.target);
        if self
            .callbacks
            .lock()
            .expect("Callback owner")
            .get(&reference)
            .is_none_or(|deadline| *deadline > Instant::now())
        {
            return;
        }
        let kind = self
            .state
            .lock()
            .expect("Browser owner")
            .resource(reference, None)
            .map(|r| r.kind);
        let Ok(kind) = kind else {
            return;
        };
        let response = match kind {
            Kind::Dialog => CallbackResponse::Dialog {
                accept: false,
                prompt_text: None,
            },
            // Callback expiry is Testrun's own policy; it keeps the aborted code.
            Kind::Route => CallbackResponse::RouteAbort {
                error_code: Some(crate::browser::routing::AbortCode::Aborted),
            },
            _ => CallbackResponse::BindingReject {},
        };
        let _ = self
            .callback_reply(
                reference,
                response,
                Instant::now() + Duration::from_secs(30),
                true,
            )
            .await;
    }
    async fn callback_parent(
        &self,
        connection: &Connection,
        parent: Reference,
        kind: Kind,
        init: &Value,
    ) -> Result<Reference, Error> {
        let mut scope = None::<ObjectRef>;
        match kind {
            Kind::Dialog => {
                if let Some(guid) = init["page"]["guid"].as_str() {
                    scope = Some(connection.resolve(guid, "Page").await?);
                }
            }
            Kind::Binding => {
                if let Some(guid) = init["frame"]["guid"].as_str() {
                    scope = Some(connection.resolve(guid, "Frame").await?);
                }
            }
            Kind::Route => {
                let request = connection
                    .resolve(
                        init["request"]["guid"].as_str().ok_or("MISSING_REQUEST")?,
                        "Request",
                    )
                    .await?;
                if let Some(request) = connection.initializer(&request).await?
                    && let Some(guid) = request["frame"]["guid"].as_str()
                {
                    scope = Some(connection.resolve(guid, "Frame").await?);
                }
            }
            _ => {}
        }
        let admitted = self
            .state
            .lock()
            .expect("Browser owner")
            .resource(parent, None)?
            .clone();
        if let Some(wire) = scope {
            {
                let state = self.state.lock().expect("Browser owner");
                if let Some(scope) = state.by_wire(&wire) {
                    let context = state
                        .ancestor(scope.reference, Kind::Context)
                        .ok_or("INVALID_SCOPE")?;
                    if state.ancestor(admitted.reference, Kind::Context) != Some(context) {
                        return Err("FOREIGN_CALLBACK".into());
                    }
                    return Ok(scope.reference);
                }
            }
            if kind == Kind::Route && admitted.kind == Kind::Context {
                // A context route can pause a popup's first navigation before
                // its Page event. Its typed frame channel already belongs to
                // the context, which can own the request without a guessed page.
                if !self
                    .unpublished_route_frame(connection, &admitted.wire, &wire)
                    .await?
                {
                    return Err("FOREIGN_CALLBACK".into());
                }
                self.state
                    .lock()
                    .expect("Browser owner")
                    .resource(parent, Some(Kind::Context))?;
                return Ok(parent);
            }
            return Err("STALE_REFERENCE".into());
        }
        Ok(parent)
    }
    async fn unpublished_route_frame(
        &self,
        connection: &Connection,
        context: &ObjectRef,
        frame: &ObjectRef,
    ) -> Result<bool, Error> {
        let mut pending = vec![context.clone()];
        let mut visited = 0;
        while let Some(parent) = pending.pop() {
            visited += 1;
            if visited > contract::MAX_RESOURCES {
                return Err("RESOURCE_CAPACITY".into());
            }
            let frames = connection.children(&parent, "Frame").await?;
            if frames.contains(frame) {
                return Ok(true);
            }
            pending.extend(frames);
            if parent == *context {
                pending.extend(connection.children(context, "Page").await?);
            }
            if pending.len() > contract::MAX_RESOURCES {
                return Err("RESOURCE_CAPACITY".into());
            }
        }
        Ok(false)
    }
}
