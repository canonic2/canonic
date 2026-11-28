//! Scoped evaluation and handle reads; all dispatched evaluations may mutate.

use super::*;
use crate::browser::{
    evaluation::{self, EvaluationValue},
    lifecycle::Resource,
    objects::ObjectRef,
};

impl Host {
    pub(super) async fn frame_element(
        &self,
        frame: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let connection = self.connection()?;
        connection.event_barrier(deadline).await?;
        let (child, parent, page) = {
            let state = self.state.lock().expect("Browser owner");
            let child = state.document_frame(frame)?;
            let parent = state
                .resource(child.parent.ok_or("INVALID_SCOPE")?, Some(Kind::Frame))
                .map_err(|_| Error::from("INVALID_SCOPE"))?
                .clone();
            let page = state
                .ancestor(parent.reference, Kind::Page)
                .ok_or("INVALID_SCOPE")?;
            (child, parent, page)
        };
        let reply = connection
            .retained(&child.wire, "frameElement", json!({}), deadline, false)
            .await?
            .ok_or("MISSING_RESULT")?;
        let element = connection
            .resolve(
                reply["element"]["guid"].as_str().ok_or("MISSING_HANDLE")?,
                "ElementHandle",
            )
            .await?;
        let result = async {
            connection.event_barrier(deadline).await?;
            {
                let state = self.state.lock().expect("Browser owner");
                let current = state
                    .document_frame(frame)
                    .map_err(|_| Error::from("DOCUMENT_CHANGED"))?;
                if current.parent != Some(parent.reference) {
                    return Err("DOCUMENT_CHANGED".into());
                }
            }
            self.evaluation_document(&parent, deadline).await?;
            let mut state = self.state.lock().expect("Browser owner");
            let current = state
                .document_frame(frame)
                .map_err(|_| Error::from("DOCUMENT_CHANGED"))?;
            if current.parent != Some(parent.reference) {
                return Err("DOCUMENT_CHANGED".into());
            }
            state.resource(parent.reference, Some(Kind::Frame))?;
            state
                .issue(Kind::Handle, element.clone(), Some(parent.reference))
                .map_err(Error::from)
        }
        .await;
        if result.is_err() && self.dispose_element(&element).await.is_err() {
            self.set_failure("HANDLE_CLEANUP_PENDING");
        }
        let handle = result?;
        Ok(
            json!({"childFrame":frame,"frame":parent.reference,"page":page,"handle":handle,"potentiallyMutating":false,"relationshipCleanupPending":false}),
        )
    }

    pub(super) async fn selector_evaluate(
        &self,
        scope: Reference,
        options: evaluation::SelectorOptions,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        let (resource, frame) = self.evaluation_scope(scope, deadline).await?;
        if frame.kind != Kind::Frame {
            return Err("INVALID_SCOPE".into());
        }
        let connection = self.connection()?;
        if resource.kind == Kind::Handle {
            connection
                .resolve(resource.wire.guid(), "ElementHandle")
                .await
                .map_err(|error| match error {
                    connection::Error::Protocol(crate::browser::dispatcher::Error::Object(
                        crate::browser::objects::Error::WrongType,
                    )) => Error::from("INVALID_SCOPE"),
                    error => error.into(),
                })?;
        }
        let selector = options.locator.scoped_selector(frame.reference)?;
        let argument = {
            let state = self.state.lock().expect("Browser owner");
            evaluation::argument(&options.argument, |handle| {
                let handle = state.resource(handle, Some(Kind::Handle))?;
                if state.ancestor(handle.reference, Kind::Frame) != Some(frame.reference)
                    && state.ancestor(handle.reference, Kind::Page)
                        != state.ancestor(frame.reference, Kind::Page)
                {
                    return Err("FOREIGN_HANDLE");
                }
                Ok(json!({"guid":handle.wire.guid()}))
            })?
        };
        if options.return_handle || options.wait_for_attachment {
            let engine = self.state.lock().expect("Browser owner").engine;
            let expression = options.expression(
                frame.reference,
                resource.kind == Kind::Handle,
                &crate::browser::reading::selector(engine.name()),
                options.wait_timeout_ms.unwrap_or(30000).min(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis() as u64,
                ),
            )?;
            return self
                .evaluate_scoped(
                    scope,
                    expression,
                    Some(true),
                    options.argument,
                    options.return_handle,
                    deadline,
                )
                .await;
        }
        let mut params =
            json!({"selector":selector,"expression":options.expression,"arg":argument});
        if !options.all {
            params["strict"] = json!(options.strict());
        }
        if let Some(function) = options.is_function {
            params["isFunction"] = json!(function);
        }
        let reply = connection
            .retained(
                &resource.wire,
                if options.all {
                    "evalOnSelectorAll"
                } else {
                    "evalOnSelector"
                },
                params,
                deadline,
                false,
            )
            .await
            .map_err(|error| {
                if let connection::Error::Remote(remote) = &error
                    && remote.error["error"]["message"]
                        .as_str()
                        .is_some_and(|text| {
                            text.starts_with("Failed to find element matching selector")
                        })
                {
                    return completed(Error::from("ELEMENT_NOT_FOUND"));
                }
                completed(evaluation_failure(error))
            })?
            .ok_or_else(|| completed(Error::from("MISSING_RESULT")))?;
        self.evaluation_document(&frame, deadline).await?;
        let value =
            evaluation::result(&reply["value"]).map_err(|code| completed(Error::from(code)))?;
        Ok(
            json!({"scope":scope,"frame":frame.reference,"value":value,"dispatchCompleted":true,"potentiallyMutating":true}),
        )
    }
    pub(super) fn fence_init_scripts(&self) {
        let state = self.state.lock().expect("Browser owner");
        self.init_scripts
            .lock()
            .expect("Init scripts")
            .retain(|scope| {
                state.resources.get(&scope.id).is_some_and(|resource| {
                    resource.reference == scope
                        && matches!(resource.kind, Kind::Page | Kind::Context)
                })
            });
    }
    pub(super) async fn init_script(
        &self,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.connection()?.event_barrier(deadline).await?;
        self.fence_init_scripts();
        let (page, kind) = match &request {
            Request::PageInitScriptAdd { page, .. }
            | Request::PageInitScriptList { page }
            | Request::PageInitScriptRemove { page, .. } => (*page, Kind::Page),
            Request::ContextInitScriptAdd { context, .. }
            | Request::ContextInitScriptList { context }
            | Request::ContextInitScriptRemove { context, .. } => (*context, Kind::Context),
            _ => return Err("INVALID_SCOPE".into()),
        };
        let field = if kind == Kind::Context {
            "context"
        } else {
            "page"
        };
        let wire = self.wire(page, kind)?;
        let connection = self.connection()?;
        match request {
            Request::PageInitScriptAdd { source, .. }
            | Request::ContextInitScriptAdd { source, .. } => {
                self.add_init_script(page, kind, &wire, source, &connection, deadline)
                    .await
            }
            Request::PageInitScriptList { .. } | Request::ContextInitScriptList { .. } => {
                let mut value = self.init_scripts.lock().expect("Init scripts").list(page);
                if kind == Kind::Context {
                    value.as_object_mut().expect("Script list").remove("page");
                    value["context"] = json!(page);
                }
                Ok(value)
            }
            Request::PageInitScriptRemove { script_id, .. }
            | Request::ContextInitScriptRemove { script_id, .. } => {
                let disposable = self
                    .init_scripts
                    .lock()
                    .expect("Init scripts")
                    .native(page, script_id)?;
                // The pinned Disposable dispatcher emits disposal only after
                // its original cleanup finishes. This is a local completion
                // witness, including when that call's reply was lost.
                let closed = match connection.initializer(&disposable).await {
                    Err(connection::Error::Protocol(
                        crate::browser::dispatcher::Error::Object(
                            crate::browser::objects::Error::Stale
                            | crate::browser::objects::Error::Unknown,
                        ),
                    )) => true,
                    Ok(Some(_)) => false,
                    Ok(None) => return Err("BACKEND_PROTOCOL".into()),
                    Err(error) => return Err(error.into()),
                };
                if closed {
                    self.init_scripts
                        .lock()
                        .expect("Init scripts")
                        .forget(script_id);
                    return Ok(
                        json!({(field):page,"scriptId":script_id,"removed":true,"existingEffectsReverted":false,"cleanupWitness":"driver-disposable-closed","replayed":false}),
                    );
                }
                self.init_scripts.lock().expect("Init scripts").begin_remove(page, script_id)
                    .map_err(|code| {let mut error=Error::from(code);error.partial=Some(json!({(field):page,"scriptId":script_id,"cleanupPending":true,"nextAction":if kind==Kind::Context{"context_close"}else{"page_close"}}));error})?;
                let result = connection
                    .retained(&disposable, "dispose", json!({}), deadline, false)
                    .await
                    .map_err(Error::from);
                if let Err(mut error) = result {
                    if matches!(
                        error.code,
                        "NOT_DISPATCHED" | "BACKEND_BUSY" | "BROWSER_METHOD_FAILED"
                    ) {
                        self.init_scripts
                            .lock()
                            .expect("Init scripts")
                            .retry_remove(script_id);
                    }
                    error.partial = Some(
                        json!({(field):page,"scriptId":script_id,"cleanupPending":true,"nextAction":if kind==Kind::Context{"context_close"}else{"page_close"}}),
                    );
                    return Err(error);
                }
                self.init_scripts
                    .lock()
                    .expect("Init scripts")
                    .forget(script_id);
                Ok(
                    json!({(field):page,"scriptId":script_id,"removed":true,"existingEffectsReverted":false}),
                )
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
    async fn add_init_script(
        &self,
        page: Reference,
        kind: Kind,
        wire: &ObjectRef,
        source: String,
        connection: &Connection,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let field = if kind == Kind::Context {
            "context"
        } else {
            "page"
        };
        crate::browser::scripts::validate(&source)?;
        let script_id = self
            .init_scripts
            .lock()
            .expect("Init scripts")
            .begin(page)?;
        let result: Result<_, Error> = async {
            let reply = connection
                .retained(
                    wire,
                    "addInitScript",
                    json!({"source":source}),
                    deadline,
                    false,
                )
                .await?
                .ok_or("MISSING_RESULT")?;
            let disposable = connection
                .resolve(
                    reply["disposable"]["guid"]
                        .as_str()
                        .ok_or("BACKEND_PROTOCOL")?,
                    "Disposable",
                )
                .await?;
            Ok(disposable)
        }
        .await;
        match result {
            Ok(disposable) => {
                self.init_scripts
                    .lock()
                    .expect("Init scripts")
                    .complete(script_id, disposable)?;
                let scope = connection
                    .event_barrier(deadline)
                    .await
                    .map_err(Error::from)
                    .and_then(|()| self.wire(page, kind).map(|_| ()));
                if let Err(mut error) = scope {
                    if error.code == "STALE_REFERENCE" {
                        self.init_scripts
                            .lock()
                            .expect("Init scripts")
                            .forget(script_id);
                    }
                    error.partial = Some(
                        json!({(field):page,"scriptId":script_id,"dispatchCompleted":true,"scopeVerified":false}),
                    );
                    return Err(error);
                }
                Ok(
                    json!({(field):page,"scriptId":script_id,"registered":true,"currentDocumentExecuted":false}),
                )
            }
            Err(mut error) => {
                let uncertain = matches!(
                    error.code,
                    "OUTCOME_UNKNOWN" | "BACKEND_PROTOCOL" | "MISSING_RESULT"
                );
                if !uncertain {
                    self.init_scripts
                        .lock()
                        .expect("Init scripts")
                        .forget(script_id);
                }
                error.partial = Some(
                    json!({(field):page,"scriptId":script_id,"registrationUncertain":uncertain}),
                );
                Err(error)
            }
        }
    }
    pub(super) async fn context_worker_list(
        &self,
        context: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.connection()?.event_barrier(deadline).await?;
        let pages = {
            let state = self.state.lock().expect("Browser owner");
            state.resource(context, Some(Kind::Context))?;
            state
                .subtree(context)
                .into_iter()
                .filter(|r| r.kind == Kind::Page)
                .map(|r| r.reference)
                .collect::<Vec<_>>()
        };
        let mut workers = Vec::new();
        for page in pages {
            match self.worker_list(page, deadline).await {
                Ok(value) => workers.extend(
                    value["workers"]
                        .as_array()
                        .ok_or("BACKEND_PROTOCOL")?
                        .iter()
                        .cloned(),
                ),
                Err(error) if matches!(error.code, "STALE_REFERENCE" | "RESOURCE_CLOSING") => {}
                Err(error) => return Err(error),
            }
        }
        self.state
            .lock()
            .expect("Browser owner")
            .resource(context, Some(Kind::Context))?;
        Ok(
            json!({"context":context,"workers":workers,"availableKinds":["dedicated"],"unsupportedKinds":["shared","service"],"restriction":"pinned-Playwright-Firefox-WebKit-worker-channels","source":"playwright-owned-context-page-worker-graph"}),
        )
    }
    pub(super) async fn worker_list(
        &self,
        page: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.connection()?.event_barrier(deadline).await?;
        let page_wire = self.wire(page, Kind::Page)?;
        let connection = self.connection()?;
        let wires = connection.children(&page_wire, "Worker").await?;
        let mut workers = Vec::new();
        for wire in wires {
            let reference = {
                let mut state = self.state.lock().expect("Browser owner");
                state.resource(page, Some(Kind::Page))?;
                match state.issue(Kind::Worker, wire.clone(), Some(page)) {
                    Ok(reference) => reference,
                    Err("STALE_REFERENCE") => continue,
                    Err(code) => return Err(code.into()),
                }
            };
            // Validate the exact native generation after retaining it. A
            // children snapshot can outlive disposal while its owner is read.
            let initializer = match connection.initializer(&wire).await {
                Ok(Some(initializer)) => initializer,
                Ok(None) => return Err("MISSING_INITIALIZER".into()),
                Err(connection::Error::Protocol(crate::browser::dispatcher::Error::Object(
                    crate::browser::objects::Error::Stale | crate::browser::objects::Error::Unknown,
                ))) => {
                    self.state.lock().expect("Browser owner").retire(reference);
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            workers.push(json!({"worker":reference,"page":page,"url":crate::browser::diagnostics::url(initializer["url"].as_str().unwrap_or("")),"kind":"dedicated","executionIdentity":"worker-generation","navigationFence":"worker-channel-close-or-disposal"}));
        }
        connection.event_barrier(deadline).await?;
        let state = self.state.lock().expect("Browser owner");
        workers.retain(|worker| {
            serde_json::from_value::<Reference>(worker["worker"].clone())
                .is_ok_and(|reference| state.resource(reference, Some(Kind::Worker)).is_ok())
        });
        Ok(
            json!({"page":page,"workers":workers,"serviceWorkers":false,"sharedWorkers":false,"source":"playwright-owned-page-worker-graph"}),
        )
    }
    pub(super) async fn handle_property(
        &self,
        handle: Reference,
        name: String,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let (resource, frame) = self.evaluation_scope(handle, deadline).await?;
        if resource.kind != Kind::Handle {
            return Err("INVALID_SCOPE".into());
        }
        let reply = self
            .connection()?
            .retained(
                &resource.wire,
                "getProperty",
                json!({"name":name}),
                deadline,
                false,
            )
            .await?
            .ok_or("MISSING_RESULT")?;
        let wire = self.evaluation_handle(&reply).await.map_err(completed)?;
        let property = self
            .retain_evaluation_handle(wire, &frame, deadline)
            .await
            .map_err(completed)?;
        Ok(
            json!({"handle":property,"parent":handle,"frame":(frame.kind==Kind::Frame).then_some(frame.reference),"worker":(frame.kind==Kind::Worker).then_some(frame.reference),"name":name,"dispatchCompleted":true,"potentiallyMutating":true}),
        )
    }

    pub(super) async fn handle_properties(
        &self,
        handle: Reference,
        query: evaluation::PropertyQuery,
        deadline: Instant,
    ) -> Result<Value, Error> {
        query.validate()?;
        let (resource, frame) = self.evaluation_scope(handle, deadline).await?;
        if resource.kind != Kind::Handle {
            return Err("INVALID_SCOPE".into());
        }
        let result=self.evaluate(&resource.wire,r#"(object,query) => {
            const names=[]; let skipped=0,more=false;
            for (const name in Object(object)) {
                if (!Object.prototype.hasOwnProperty.call(object,name)) continue;
                if (skipped++<query.offset) continue;
                if (names.length>=query.limit) {more=true;break;}
                if (name.length>1024) throw new Error('PROPERTY_NAME_BOUNDS');
                if (JSON.stringify([...names,name]).length>2000) {
                    if (!names.length) throw new Error('PROPERTY_NAME_BOUNDS');
                    more=true;break;
                }
                names.push(name);
            }
            const next=query.offset+names.length,exhausted=more&&next>10000;
            return {names,hasMore:more,nextOffset:exhausted?null:next,offsetLimitReached:exhausted,snapshot:false,keys:'own-enumerable-string'};
        }"#,json!(query),deadline).await?;
        self.evaluation_document(&frame, deadline).await?;
        if result["names"].as_array().is_none_or(|names| {
            names
                .iter()
                .any(|name| name.as_str().is_none_or(|name| name.len() > 1024))
        }) {
            return Err(completed("PROPERTY_NAME_BOUNDS".into()));
        }
        if serde_json::to_vec(&result)
            .map_err(|_| completed(Error::from("PROPERTY_RESULT_BOUNDS")))?
            .len()
            > 7000
        {
            return Err(completed("PROPERTY_RESULT_BOUNDS".into()));
        }
        Ok(
            json!({"handle":handle,"frame":(frame.kind==Kind::Frame).then_some(frame.reference),"worker":(frame.kind==Kind::Worker).then_some(frame.reference),"properties":result,"dispatchCompleted":true,"potentiallyMutating":true}),
        )
    }

    async fn evaluation_scope(
        &self,
        scope: Reference,
        deadline: Instant,
    ) -> Result<(Resource, Resource), Error> {
        self.connection()?.event_barrier(deadline).await?;
        let resource = self
            .state
            .lock()
            .expect("Browser owner")
            .resource(scope, None)?
            .clone();
        if resource.kind == Kind::Page {
            let frame = self.page_scope(scope, None, deadline).await?;
            return Ok((frame.clone(), frame));
        }
        // A retained owner runs in its frame's current document.
        if resource.kind == Kind::FrameOwner {
            let frame = self
                .state
                .lock()
                .expect("Browser owner")
                .owner_frame(scope)?;
            return Ok((frame.clone(), frame));
        }
        if !matches!(resource.kind, Kind::Frame | Kind::Worker | Kind::Handle) {
            return Err("INVALID_SCOPE".into());
        }
        let state = self.state.lock().expect("Browser owner");
        let frame = state
            .ancestor(scope, Kind::Frame)
            .or_else(|| state.ancestor(scope, Kind::Worker))
            .ok_or("INVALID_SCOPE")?;
        Ok((resource, state.resource(frame, None)?.clone()))
    }

    pub(super) async fn evaluate_scoped(
        &self,
        scope: Reference,
        expression: String,
        is_function: Option<bool>,
        argument: EvaluationValue,
        return_handle: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let (resource, frame) = self.evaluation_scope(scope, deadline).await?;
        let connection = self.connection()?;
        let argument = {
            let state = self.state.lock().expect("Browser owner");
            state.resource(frame.reference, Some(frame.kind))?;
            evaluation::argument(&argument, |handle| {
                let resource = state.resource(handle, Some(Kind::Handle))?;
                if state.ancestor(handle, frame.kind) != Some(frame.reference)
                    && (frame.kind != Kind::Frame
                        || state.ancestor(handle, Kind::Page)
                            != state.ancestor(frame.reference, Kind::Page))
                {
                    return Err("FOREIGN_HANDLE");
                }
                Ok(json!({"guid":resource.wire.guid()}))
            })?
        };
        let mut params = json!({"expression":expression,"arg":argument});
        if let Some(is_function) = is_function {
            params["isFunction"] = json!(is_function);
        }
        let method = if return_handle {
            "evaluateExpressionHandle"
        } else {
            "evaluateExpression"
        };
        let reply = connection
            .retained(&resource.wire, method, params, deadline, false)
            .await
            .map_err(evaluation_failure)?
            .ok_or("MISSING_RESULT")?;
        if return_handle {
            let wire = self.evaluation_handle(&reply).await.map_err(completed)?;
            let reference = self
                .retain_evaluation_handle(wire, &frame, deadline)
                .await
                .map_err(completed)?;
            Ok(
                json!({"scope":scope,"frame":(frame.kind==Kind::Frame).then_some(frame.reference),"worker":(frame.kind==Kind::Worker).then_some(frame.reference),"handle":reference,"potentiallyMutating":true,"dispatchCompleted":true}),
            )
        } else {
            self.evaluation_document(&frame, deadline).await?;
            // A successful evaluation may have mutated state even when its
            // result cannot be represented. Preserve that fact for callers.
            let value = evaluation::result(&reply["value"]).map_err(|code| Error {
                partial: Some(
                    json!({"scope":scope,"dispatchCompleted":true,"potentiallyMutating":true}),
                ),
                ..Error::from(code)
            })?;
            Ok(
                json!({"scope":scope,"frame":(frame.kind==Kind::Frame).then_some(frame.reference),"worker":(frame.kind==Kind::Worker).then_some(frame.reference),"value":value,"potentiallyMutating":true,"dispatchCompleted":true}),
            )
        }
    }

    async fn evaluation_handle(&self, reply: &Value) -> Result<ObjectRef, Error> {
        let guid = reply["handle"]["guid"].as_str().ok_or("MISSING_HANDLE")?;
        let connection = self.connection()?;
        Ok(match connection.resolve(guid, "JSHandle").await {
            Ok(handle) => handle,
            Err(_) => connection.resolve(guid, "ElementHandle").await?,
        })
    }

    async fn evaluation_document(&self, frame: &Resource, deadline: Instant) -> Result<(), Error> {
        if frame.kind == Kind::Worker {
            self.connection()?
                .event_barrier(deadline)
                .await
                .map_err(|error| completed(error.into()))?;
            self.state
                .lock()
                .expect("Browser owner")
                .resource(frame.reference, Some(Kind::Worker))
                .map_err(|code| completed(code.into()))?;
            return Ok(());
        }
        let current = self
            .frame_current(&frame.wire, deadline)
            .await
            .map_err(|mut error| {
                error.partial = Some(json!({"dispatchCompleted":true,"potentiallyMutating":true}));
                error
            })?;
        if current != frame.reference {
            return Err(Error {
                partial: Some(json!({"dispatchCompleted":true,"potentiallyMutating":true})),
                ..Error::from("DOCUMENT_CHANGED")
            });
        }
        Ok(())
    }

    async fn retain_evaluation_handle(
        &self,
        wire: ObjectRef,
        frame: &Resource,
        deadline: Instant,
    ) -> Result<Reference, Error> {
        let result = async {
            self.evaluation_document(frame, deadline).await?;
            let mut state = self.state.lock().expect("Browser owner");
            state.resource(frame.reference, Some(frame.kind))?;
            state
                .issue(Kind::Handle, wire.clone(), Some(frame.reference))
                .map_err(Error::from)
        }
        .await;
        if result.is_err() && self.dispose_element(&wire).await.is_err() {
            self.set_failure("HANDLE_CLEANUP_PENDING");
        }
        result
    }

    pub(super) async fn handle_read(
        &self,
        handle: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let (resource, frame) = self.evaluation_scope(handle, deadline).await?;
        if resource.kind != Kind::Handle {
            return Err("INVALID_SCOPE".into());
        }
        let reply = self
            .connection()?
            .retained(&resource.wire, "jsonValue", json!({}), deadline, false)
            .await?
            .ok_or("MISSING_RESULT")?;
        self.evaluation_document(&frame, deadline).await?;
        Ok(
            json!({"handle":handle,"frame":(frame.kind==Kind::Frame).then_some(frame.reference),"worker":(frame.kind==Kind::Worker).then_some(frame.reference),"value":evaluation::result(&reply["value"])?}),
        )
    }

    pub(super) async fn handle_frame(
        &self,
        handle: Reference,
        owner: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let (resource, source) = self.evaluation_scope(handle, deadline).await?;
        if resource.kind != Kind::Handle || source.kind != Kind::Frame {
            return Err("INVALID_SCOPE".into());
        }
        let connection = self.connection()?;
        let element = connection
            .resolve(resource.wire.guid(), "ElementHandle")
            .await
            .map_err(|error| match error {
                connection::Error::Protocol(crate::browser::dispatcher::Error::Object(
                    crate::browser::objects::Error::WrongType,
                )) => Error::from("INVALID_SCOPE"),
                error => error.into(),
            })?;
        let (context, source_page) = {
            let state = self.state.lock().expect("Browser owner");
            (
                state
                    .ancestor(source.reference, Kind::Context)
                    .ok_or("INVALID_SCOPE")?,
                state
                    .ancestor(source.reference, Kind::Page)
                    .ok_or("INVALID_SCOPE")?,
            )
        };
        let reply = connection
            .retained(
                &element,
                if owner { "ownerFrame" } else { "contentFrame" },
                json!({}),
                deadline,
                false,
            )
            .await?
            .ok_or("MISSING_RESULT")?;
        let related = match reply.get("frame") {
            Some(value) if !value.is_null() => Some(
                connection
                    .resolve(value["guid"].as_str().ok_or("BACKEND_PROTOCOL")?, "Frame")
                    .await?,
            ),
            _ => None,
        };
        connection.event_barrier(deadline).await?;
        let state = self.state.lock().expect("Browser owner");
        state
            .resource(source.reference, Some(Kind::Frame))
            .map_err(|_| Error::from("DOCUMENT_CHANGED"))?;
        let frame = related
            .as_ref()
            .map(|wire| {
                let frame = state.by_wire(wire).ok_or("FRAME_UNAVAILABLE")?.reference;
                state.resource(frame, Some(Kind::Frame))?;
                if state.ancestor(frame, Kind::Context) != Some(context)
                    || (!owner && state.ancestor(frame, Kind::Page) != Some(source_page))
                {
                    return Err("FOREIGN_FRAME");
                }
                Ok(frame)
            })
            .transpose()?;
        let page = frame.and_then(|frame| state.ancestor(frame, Kind::Page));
        Ok(
            json!({"handle":handle,"frame":frame,"page":page,"sourcePage":source_page,"relationship":if owner {"owner-frame"} else {"content-frame"},"relationshipCleanupPending":false,"potentiallyMutating":owner}),
        )
    }
}

fn completed(mut error: Error) -> Error {
    if error.partial.is_none() {
        error.partial = Some(json!({"dispatchCompleted":true,"potentiallyMutating":true}));
    }
    error
}

fn evaluation_failure(error: connection::Error) -> Error {
    if let connection::Error::Remote(remote) = &error {
        let message = remote.error["error"]["message"].as_str().unwrap_or("");
        if message == "JSHandles can be evaluated only in the context they were created!" {
            return completed(Error::from("FOREIGN_HANDLE"));
        }
        for code in [
            "ELEMENT_NOT_FOUND",
            "AMBIGUOUS_LOCATOR",
            "INVALID_SCOPE",
            "SELECTOR_WAIT_EXPIRED",
            "SELECTOR_ROOT_DETACHED",
        ] {
            if message.starts_with(&format!("Error: canonic:{code}"))
                || message.starts_with(&format!("canonic:{code}"))
            {
                return completed(Error::from(code));
            }
        }
    }
    error.into()
}
