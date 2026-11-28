//! Main-world execution with independent, document-bound native handle groups.
use super::*;
use crate::browser::evaluation::{self, EvaluationValue};

const CODEC: &str = include_str!("../../reading/evaluation.js");
const MAX_HANDLES: usize = 256;
mod adoption;
mod allocation;
mod frame_element;
mod relationships;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CallMode {
    Value,
    Handle,
    Selector,
    SelectorHandle,
}

#[derive(Clone)]
struct Handle {
    reference: Reference,
    world: Reference,
    scope: Reference,
    session: String,
    object: String,
    owned_group: Option<String>,
}
#[derive(Default)]
pub(super) struct Evaluation {
    pub(super) groups: super::execution_groups::Groups,
    pub(super) bindings: super::bindings::Bindings,
    pub(super) socket_routes: crate::browser::socket_routes::Shared,
    pub(super) scripts: super::init_scripts::Scripts,
    pub(super) clocks: super::clock::Clocks,
    handles: BTreeMap<Reference, Handle>,
    allocations: BTreeMap<Uuid, allocation::Allocation>,
    selectors: BTreeSet<Reference>,
    release: BTreeSet<(String, String)>,
}
fn group(world: Reference) -> String {
    format!("testrun:execution:{}:{}", world.id, world.generation)
}

impl Worker {
    pub(super) fn evaluation_handle_world(&self, handle: Reference) -> Result<Reference, Error> {
        self.require_handle(handle)?;
        Ok(self
            .evaluation
            .handles
            .get(&handle)
            .ok_or("STALE_REFERENCE")?
            .world)
    }
    pub(super) fn evaluation_handle_document(
        &self,
        handle: Reference,
    ) -> Result<(Reference, Reference), Error> {
        self.require_handle(handle)?;
        let (frame, _, _, _) = self.execution_scope(handle)?;
        self.index
            .native(frame, Kind::Frame)
            .map_err(resource_error)?;
        let page = self.index.owning_page(frame).map_err(resource_error)?;
        Ok((frame, page))
    }
    pub(super) fn evaluation_handle_live(&self, reference: Reference) -> bool {
        self.evaluation
            .handles
            .get(&reference)
            .is_some_and(|handle| self.index.route(handle.world, Some(Kind::World)).is_ok())
    }
    pub(super) fn execution_cleanup_capacity(&self) -> Result<(), Error> {
        if self.evaluation.release.len() >= 256 {
            return Err("EXECUTION_CLEANUP_PENDING".into());
        }
        Ok(())
    }
    pub(super) fn queue_execution_group(&mut self, session: String, group: String) {
        self.evaluation.release.insert((session, group));
    }
    pub(super) fn fence_evaluation(&mut self) {
        self.evaluation
            .selectors
            .retain(|world| self.index.route(*world, Some(Kind::World)).is_ok());
        let stale = self
            .evaluation
            .handles
            .iter()
            .filter(|(_, handle)| self.index.route(handle.world, Some(Kind::World)).is_err())
            .map(|(reference, _)| *reference)
            .collect::<Vec<_>>();
        for reference in stale {
            let handle = self
                .evaluation
                .handles
                .remove(&reference)
                .expect("Owned handle");
            self.evaluation.release.insert((
                handle.session,
                handle.owned_group.unwrap_or_else(|| group(handle.world)),
            ));
        }
    }
    pub(super) async fn release_evaluation(&mut self) {
        let pending = std::mem::take(&mut self.evaluation.release);
        let deadline = Instant::now() + Duration::from_secs(2);
        self.release_execution_groups(deadline).await;
        for (session, group) in pending {
            if self.index.diagnostic_scope(&session).is_none() {
                continue;
            }
            if self
                .cleanup_call(
                    Some(&session),
                    "Runtime.releaseObjectGroup",
                    json!({"objectGroup":group}),
                    deadline,
                )
                .await
                .is_err()
            {
                self.evaluation.release.insert((session, group));
            }
        }
    }
    pub(super) fn evaluation_resources(&self) -> Vec<Value> {
        self.evaluation.handles.values().map(|handle|json!({"reference":handle.reference,"kind":"handle","parent":handle.scope})).collect()
    }
    fn execution_scope(
        &self,
        scope: Reference,
    ) -> Result<(Reference, Reference, String, Option<String>), Error> {
        if let Some(handle) = self.evaluation.handles.get(&scope) {
            self.index
                .route(handle.world, Some(Kind::World))
                .map_err(resource_error)?;
            return Ok((
                handle.scope,
                handle.world,
                handle.session.clone(),
                Some(handle.object.clone()),
            ));
        }
        let (scope, world) = self.index.execution_world(scope).map_err(resource_error)?;
        let route = self
            .index
            .route(world, Some(Kind::World))
            .map_err(resource_error)?;
        Ok((scope, world, route.session_id, None))
    }
    fn normalize_evaluation_request(
        &self,
        request: Request,
        deadline: Instant,
    ) -> Result<Request, Error> {
        Ok(
            if let Request::SelectorEvaluate { scope, mut options } = request {
                let (frame, _, _, object) = self.execution_scope(scope)?;
                self.index
                    .native(frame, Kind::Frame)
                    .map_err(|_| Error::from("INVALID_SCOPE"))?;
                options.wait_timeout_ms = Some(
                    options.wait_timeout_ms.unwrap_or(30000).min(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .as_millis() as u64,
                    ),
                );
                let expression = options.native_expression(frame, object.is_some())?;
                Request::Evaluate {
                    scope,
                    expression,
                    is_function: Some(true),
                    argument: options.argument,
                    return_handle: options.return_handle,
                }
            } else {
                request
            },
        )
    }

    pub(super) async fn execute_evaluation(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.drain()?;
        self.reconcile(id, deadline).await?;
        self.fence_evaluation();
        let selector = matches!(&request, Request::SelectorEvaluate { .. });
        let request = self.normalize_evaluation_request(request, deadline)?;
        match request {
            Request::FrameRetain { frame } => {
                let page = self.index.owning_page(frame).map_err(resource_error)?;
                let owner = self.index.retain_frame(frame).map_err(resource_error)?;
                Ok(
                    json!({"frame":owner,"documentFrame":frame,"page":page,"retained":true,"lifetime":"frame"}),
                )
            }
            Request::FrameOwnerRelease { frame } => {
                self.index.release_frame(frame).map_err(resource_error)?;
                Ok(json!({"frame":frame,"released":true,"nativeScopeClosed":false}))
            }
            Request::FrameElement { frame } => {
                self.evaluation_frame_element(id, frame, deadline).await
            }
            Request::HandleOwnerFrame { handle } => {
                self.evaluation_handle_frame(id, handle, true, deadline)
                    .await
            }
            Request::HandleContentFrame { handle } => {
                self.evaluation_handle_frame(id, handle, false, deadline)
                    .await
            }
            Request::WorkerList { page } => {
                self.index
                    .native(page, Kind::Page)
                    .map_err(resource_error)?;
                self.worker_inventory(id, Some(page), None, deadline).await
            }
            Request::ContextWorkerList { context } => {
                self.index
                    .native(context, Kind::Context)
                    .map_err(resource_error)?;
                self.worker_inventory(id, None, Some(context), deadline)
                    .await
            }
            Request::HandleDispose { handle } => {
                self.dispose_evaluation_handle(handle, deadline).await
            }
            Request::Evaluate {
                scope,
                expression,
                is_function,
                argument,
                return_handle,
            } => {
                let body = evaluation_body(
                    &expression,
                    is_function,
                    self.evaluation.handles.contains_key(&scope),
                    return_handle,
                );
                let mode = match (selector, return_handle) {
                    (true, true) => CallMode::SelectorHandle,
                    (true, false) => CallMode::Selector,
                    (false, true) => CallMode::Handle,
                    (false, false) => CallMode::Value,
                };
                self.evaluation_call(id, scope, argument, body, mode, deadline)
                    .await
            }
            Request::HandleRead { handle } => {
                self.require_handle(handle)?;
                self.evaluation_call(
                    id,
                    handle,
                    EvaluationValue::default(),
                    "return codec.encode(this.value);".into(),
                    CallMode::Value,
                    deadline,
                )
                .await
            }
            Request::HandleProperty { handle, name } => {
                self.require_handle(handle)?;
                self.evaluation_call(
                    id,
                    handle,
                    EvaluationValue::default(),
                    format!("return {{value:this.value[{}]}};", json!(name)),
                    CallMode::Handle,
                    deadline,
                )
                .await
            }
            Request::HandleProperties { handle, query } => {
                self.evaluation_handle_properties(id, handle, query, deadline)
                    .await
            }
            _ => Err("CAPABILITY_UNAVAILABLE".into()),
        }
    }
    async fn evaluation_handle_properties(
        &mut self,
        id: Uuid,
        handle: Reference,
        query: evaluation::PropertyQuery,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.require_handle(handle)?;
        let body = format!(
            "const names=[];let skipped=0,more=false;for(const name in Object(this.value)){{if(!Object.prototype.hasOwnProperty.call(this.value,name))continue;if(skipped++<{})continue;if(names.length>={}){{more=true;break;}}if(name.length>1024)throw Error('canonic:PROPERTY_NAME_BOUNDS');if(JSON.stringify([...names,name]).length>2000){{if(!names.length)throw Error('canonic:PROPERTY_NAME_BOUNDS');more=true;break;}}names.push(name);}}const next={}+names.length,exhausted=more&&next>10000;return codec.encode({{names,hasMore:more,nextOffset:exhausted?null:next,offsetLimitReached:exhausted,snapshot:false,keys:'own-enumerable-string'}});",
            query.offset, query.limit, query.offset
        );
        let mut result = self
            .evaluation_call(
                id,
                handle,
                EvaluationValue::default(),
                body,
                CallMode::Value,
                deadline,
            )
            .await?;
        // Keep the shared structured result and expose the conventional
        // JSON property page for existing clients as well.
        result["properties"] = crate::browser::values::result(
            &evaluation::argument(
                &serde_json::from_value(result["value"].clone())
                    .map_err(|_| Error::from("BACKEND_PROTOCOL"))?,
                |_| Err("FOREIGN_HANDLE"),
            )?["value"],
        )?;
        Ok(result)
    }
    async fn dispose_evaluation_handle(
        &mut self,
        handle: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.require_handle(handle)?;
        let entry = self
            .evaluation
            .handles
            .get(&handle)
            .cloned()
            .ok_or("STALE_REFERENCE")?;
        self.cleanup_call(
            Some(&entry.session),
            if entry.owned_group.is_some() {
                "Runtime.releaseObjectGroup"
            } else {
                "Runtime.releaseObject"
            },
            if let Some(group) = &entry.owned_group {
                json!({"objectGroup":group})
            } else {
                json!({"objectId":entry.object})
            },
            deadline,
        )
        .await?;
        self.evaluation.handles.remove(&handle);
        Ok(json!({"handle":handle,"disposed":true}))
    }
    fn require_handle(&self, reference: Reference) -> Result<(), Error> {
        if reference.connection_id != self.shared.state.lock().expect("Native owner").connection {
            return Err("FOREIGN_REFERENCE".into());
        }
        if self.evaluation.handles.contains_key(&reference) {
            Ok(())
        } else if self.index.route(reference, None).is_ok() {
            Err("INVALID_SCOPE".into())
        } else {
            Err("STALE_REFERENCE".into())
        }
    }
    async fn worker_inventory(
        &mut self,
        id: Uuid,
        page: Option<Reference>,
        context: Option<Reference>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let mut workers = Vec::new();
        for (worker, kind, target, _) in self.index.targets() {
            if kind != Kind::Worker {
                continue;
            }
            let (owner, owning_page, worker_kind) =
                self.index.worker_scope(worker).map_err(resource_error)?;
            if page.is_some_and(|p| Some(p) != owning_page) || context.is_some_and(|c| c != owner) {
                continue;
            }
            let info = self
                .call_read(
                    id,
                    None,
                    "Target.getTargetInfo",
                    json!({"targetId":target}),
                    deadline,
                )
                .await?;
            self.index.worker_scope(worker).map_err(resource_error)?;
            workers.push(json!({"worker":worker,"page":owning_page,"context":owner,"kind":worker_kind,"url":crate::browser::diagnostics::url(info["targetInfo"]["url"].as_str().unwrap_or("")),"executionIdentity":"native-world-generation","executionAvailable":self.index.execution_world(worker).is_ok()}));
        }
        Ok(
            json!({"page":page,"context":context,"workers":workers,"availableKinds":if context.is_some(){json!(["dedicated","shared","service"])}else{json!(["dedicated"])},"source":"chrome-cdp-owned-worker-graph"}),
        )
    }
    /// Only an acknowledged initialization in this exact live world is reused.
    /// Argument authority is checked by the caller before any initialization I/O.
    async fn ensure_selector(
        &mut self,
        id: Uuid,
        world: Reference,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        if self.evaluation.selectors.contains(&world) {
            return Ok(());
        }
        let unique = self
            .index
            .route(world, Some(Kind::World))
            .map_err(resource_error)?
            .execution_unique_id
            .ok_or("INVALID_SCOPE")?;
        let reply=self.call(id,Some(session),"Runtime.evaluate",json!({"expression":format!("void({});true",crate::browser::reading::selector("chromium")),"uniqueContextId":unique,"returnByValue":true}),deadline).await?;
        self.index
            .route(world, Some(Kind::World))
            .map_err(resource_error)?;
        if reply.get("exceptionDetails").is_some() {
            return Err(evaluation_error(&reply));
        }
        if reply["result"]["value"] != true {
            return Err("BACKEND_PROTOCOL".into());
        }
        self.evaluation.selectors.insert(world);
        Ok(())
    }
    fn evaluation_parameters(
        &self,
        prepared: &adoption::Arguments,
        world: Reference,
        object: Option<&str>,
        body: &str,
        retain: bool,
    ) -> Result<Value, Error> {
        let mut arguments = vec![json!({"value":prepared.encoded["value"]})];
        arguments.extend(
            prepared.encoded["handles"]
                .as_array()
                .expect("Validated handles")
                .iter()
                .cloned(),
        );
        let mut params = json!({"functionDeclaration":format!("function(serialized,...handles){{const codec={CODEC};{body}}}"),"arguments":arguments,"returnByValue":!retain,"awaitPromise":true,"objectGroup":group(world),"userGesture":true});
        if let Some(object) = object {
            params["objectId"] = json!(object);
        } else {
            params["uniqueContextId"] = json!(
                self.index
                    .route(world, Some(Kind::World))
                    .map_err(resource_error)?
                    .execution_unique_id
                    .ok_or("INVALID_SCOPE")?
            );
        }
        Ok(params)
    }

    fn retain_evaluation_handle(
        &mut self,
        scope: Reference,
        document: Reference,
        world: Reference,
        session: &str,
        reply: &Value,
    ) -> Result<Reference, Error> {
        let object = reply["result"]["objectId"]
            .as_str()
            .ok_or("MISSING_HANDLE")?
            .to_owned();
        let reference = Reference {
            connection_id: scope.connection_id,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        self.evaluation.handles.insert(
            reference,
            Handle {
                reference,
                world,
                scope: document,
                session: session.to_owned(),
                object,
                owned_group: None,
            },
        );
        Ok(reference)
    }

    fn evaluation_call_error(
        &self,
        mut error: Error,
        scope: Reference,
        prepared: &adoption::Arguments,
        adopted: bool,
    ) -> Error {
        if adopted {
            let cause = error.partial.take();
            let closed = cause.as_ref().is_some_and(|partial| {
                partial["scopeClosed"] == true && partial["callerEffectsUnknown"] == true
            });
            error.partial = Some(
                json!({"scope":scope,"potentiallyMutating":true,"argumentCleanupPending":prepared.cleanup.iter().any(|key|self.evaluation.groups.contains(key)),"cause":cause}),
            );
            if closed {
                let partial = error.partial.as_mut().unwrap();
                partial["scopeClosed"] = json!(true);
                partial["callerEffectsUnknown"] = json!(true);
            }
        }
        error
    }

    async fn evaluation_call(
        &mut self,
        id: Uuid,
        scope: Reference,
        argument: EvaluationValue,
        body: String,
        mode: CallMode,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let retain = matches!(mode, CallMode::Handle | CallMode::SelectorHandle);
        if retain {
            self.execution_cleanup_capacity()?;
            self.evaluation.handle_capacity()?;
        }
        let (document, world, session, object) = self.execution_scope(scope)?;
        let mut prepared = self.evaluation_arguments(&argument, document, world)?;
        let allocation = if retain {
            Some(self.reserve_handle_allocation(id, world, &session)?)
        } else {
            None
        };
        let result: Result<Value, Error> = async {
        if matches!(mode, CallMode::Selector | CallMode::SelectorHandle) {
            self.ensure_selector(id,world,&session,deadline).await.map_err(|mut error| {
                error.partial=Some(json!({"scope":scope,"callerDispatched":false,"stage":"selector-preparation"}));
                error
            })?;
        }
        let adoption = if prepared.needs_adoption() {
            let key = self.begin_argument_adoption(world, &session)?;
            prepared.cleanup.push(key.clone());
            if let Err(mut error) = self
                .adopt_evaluation_arguments(id, world, &key, &mut prepared, deadline)
                .await
            {
                self.release_execution_group(
                    &key,
                    deadline.min(Instant::now() + Duration::from_secs(2)),
                )
                .await;
                let cause = error.partial.take();
                error.partial = Some(
                    json!({"scope":scope,"callerDispatched":false,"stage":"argument-adoption","argumentCleanupPending":prepared.cleanup.iter().any(|key|self.evaluation.groups.contains(key)),"cause":cause}),
                );
                return Err(error);
            }
            Some(key)
        } else {
            None
        };
        let params = self.evaluation_parameters(&prepared, world, object.as_deref(), &body, retain)?;
        let (reply, settled) = if let Some(key) = &adoption {
            self.execution_group_call_settled(id, key, "Runtime.callFunctionOn", params, deadline).await
        } else if retain {
            self.call_with_settlement(id, Some(&session), "Runtime.callFunctionOn", params, deadline, None).await
        } else {
            (self.call(
                id,
                Some(&session),
                "Runtime.callFunctionOn",
                params,
                deadline,
            )
            .await, true)
        };
        if let Some(allocation) = allocation.and_then(|key| self.evaluation.allocations.get_mut(&key)) { allocation.settled = settled; }
        if let Some(key) = &adoption {
            self.release_execution_group(
                key,
                deadline.min(Instant::now() + Duration::from_secs(2)),
            )
            .await;
        }
        let reply = reply.map_err(|error| self.evaluation_call_error(error, scope, &prepared, adoption.is_some()))?;
        let result = (|| {
            self.index
                .route(world, Some(Kind::World))
                .map_err(resource_error)?;
            if reply.get("exceptionDetails").is_some() {
                return Err(evaluation_error(&reply));
            }
            let mut result = json!({"scope":scope,"frame":self.index.native(document,Kind::Frame).is_ok().then_some(document),"worker":self.index.native(document,Kind::Worker).is_ok().then_some(document),"dispatchCompleted":true,"potentiallyMutating":true});
            if adoption.is_some() {
                result["argumentCleanupPending"] = json!(self.argument_cleanup_pending(&prepared));
            }
            if retain {
                let reference = self.retain_evaluation_handle(scope, document, world, &session, &reply)?;
                result["handle"] = json!(reference);
            } else {
                result["value"] = json!(evaluation::result(&reply["result"]["value"])?);
            }
            Ok(result)
        })();
        result.map_err(|mut error: Error| {
            if retain && self.index.route(world, Some(Kind::World)).is_err() {
                self.evaluation.release.insert((session, group(world)));
            }
            if error.partial.is_none() {
                error.partial = Some(
                    json!({"scope":scope,"dispatchCompleted":true,"potentiallyMutating":true}),
                );
            }
            error
        })
        }.await;
        self.finish_handle_allocation(allocation, scope, result)
    }
    fn argument_cleanup_pending(&self, prepared: &adoption::Arguments) -> bool {
        prepared
            .cleanup
            .iter()
            .any(|key| self.evaluation.groups.contains(key))
    }
}
fn evaluation_body(
    expression: &str,
    is_function: Option<bool>,
    handle: bool,
    return_handle: bool,
) -> String {
    format!(
        "const fn=(0,eval)(codec.normalize({},{})); const argument=codec.decode(serialized,handles); const value={} ; return Promise.resolve(value).then(value=>{});",
        json!(expression),
        json!(is_function),
        match is_function {
            Some(false) => "fn",
            Some(true) if handle => "fn(this.value,argument)",
            Some(true) => "fn(argument)",
            None if handle => "typeof fn==='function'?fn(this.value,argument):fn",
            None => "typeof fn==='function'?fn(argument):fn",
        },
        if return_handle {
            "({value})"
        } else {
            "codec.encodeResult(value)"
        }
    )
}

fn evaluation_error(reply: &Value) -> Error {
    let text = reply["exceptionDetails"]["exception"]["description"]
        .as_str()
        .or_else(|| reply["exceptionDetails"]["text"].as_str())
        .unwrap_or("JavaScript exception");
    let code = [
        "VALUE_BOUNDS",
        "UNSAFE_INTEGER",
        "VALUE_CONVERSION_REQUIRED",
        "PROPERTY_NAME_BOUNDS",
        "ELEMENT_NOT_FOUND",
        "AMBIGUOUS_LOCATOR",
        "INVALID_SCOPE",
        "SELECTOR_WAIT_EXPIRED",
        "SELECTOR_ROOT_DETACHED",
    ]
    .into_iter()
    .find(|code| text.starts_with(&format!("Error: canonic:{code}")))
    .unwrap_or("JAVASCRIPT_EXCEPTION");
    let mut error = Error::from(code);
    error.partial = Some(
        json!({"dispatchCompleted":true,"potentiallyMutating":true,"exception":crate::browser::diagnostics::text(text).0}),
    );
    error
}
