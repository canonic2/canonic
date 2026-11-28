//! Context/page registration policy and retained per-attachment installations.
use super::*;

fn uncertain(code: &str) -> bool {
    matches!(
        code,
        "OUTCOME_UNKNOWN" | "BACKEND_LOST" | "BACKEND_PROTOCOL"
    )
}

impl Worker {
    fn binding_kind(&self, scope: Reference) -> Result<Kind, Error> {
        let kind = if self.index.retained(scope, Kind::Page) {
            Kind::Page
        } else {
            Kind::Context
        };
        self.index.native(scope, kind).map_err(resource_error)?;
        Ok(kind)
    }
    fn binding_pages(&self, scope: Reference, kind: Kind) -> Vec<Reference> {
        if kind == Kind::Page {
            return vec![scope];
        }
        self.index
            .list(Kind::Page)
            .into_iter()
            .map(|(page, _)| page)
            .filter(|page| {
                self.index
                    .route(*page, Some(Kind::Page))
                    .ok()
                    .and_then(|route| self.index.diagnostic_scope(&route.session_id))
                    .is_some_and(|(context, _, _)| context == scope)
            })
            .collect()
    }
    fn binding_worlds(&self, scope: Reference, kind: Kind, session: &str) -> Vec<Reference> {
        self.binding_pages(scope, kind)
            .into_iter()
            .flat_map(|page| self.index.page_worlds(page))
            .filter(|world| {
                self.index
                    .route(*world, Some(Kind::World))
                    .is_ok_and(|route| route.session_id == session)
            })
            .collect()
    }
    fn binding_sessions(&self, scope: Reference, kind: Kind) -> Vec<String> {
        self.index
            .targets()
            .into_iter()
            .filter(|(_, kind, _, _)| matches!(kind, Kind::Page | Kind::Frame))
            .filter_map(|(_, _, _, session)| session)
            .filter(|session| {
                self.index
                    .diagnostic_scope(session)
                    .is_some_and(|(context, page, _)| {
                        if kind == Kind::Context {
                            context == scope
                        } else {
                            page == Some(scope)
                        }
                    })
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub(super) async fn binding_registration(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.fence_bindings();
        let scope = match &request {
            Request::BindingAdd { scope, .. }
            | Request::BindingList { scope }
            | Request::BindingRemove { scope, .. } => *scope,
            _ => return Err("INVALID_SCOPE".into()),
        };
        let kind = self.binding_kind(scope)?;
        match request {
            Request::BindingList { .. } => {
                let mut result = self.evaluation.bindings.registrations.list(scope);
                result["installations"] = json!(self.evaluation.bindings.items.iter().filter(|(_,item)|item.scope == scope)
                    .map(|(id,item)|json!({"bindingId":id,"targets":item.targets.len(),"pendingTargets":item.targets.values().filter(|target|target.pending).count(),"initializing":item.initializing})).collect::<Vec<_>>());
                Ok(result)
            }
            Request::BindingAdd { name, .. } => {
                self.add_scoped_binding(id, scope, kind, name, deadline)
                    .await
            }
            Request::BindingRemove { binding_id, .. } => {
                self.remove_scoped_binding(id, scope, binding_id, deadline)
                    .await
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
    async fn add_scoped_binding(
        &mut self,
        id: Uuid,
        scope: Reference,
        kind: Kind,
        name: String,
        deadline: Instant,
    ) -> Result<Value, Error> {
        bindings::validate(&name)?;
        let context = if kind == Kind::Context {
            scope
        } else {
            let route = self
                .index
                .route(scope, Some(Kind::Page))
                .map_err(resource_error)?;
            self.index
                .diagnostic_scope(&route.session_id)
                .ok_or("INVALID_SCOPE")?
                .0
        };
        // A context claim overlaps every page claim in that context, including
        // registrations whose allocation or cleanup has not yet settled.
        if self.evaluation.bindings.items.values().any(|item| {
            item.context == context
                && item.name == name
                && (kind == Kind::Context || item.kind == Kind::Context || item.scope == scope)
        }) {
            return Err("BINDING_NAME_CONFLICT".into());
        }
        let binding = self
            .evaluation
            .bindings
            .registrations
            .begin(scope, name.clone())?;
        self.evaluation.bindings.items.insert(
            binding,
            Native {
                scope,
                context,
                kind,
                name,
                raw: format!("__testrun_binding_{}", binding.simple()),
                targets: BTreeMap::new(),
                initializing: true,
                closing: false,
            },
        );
        let result: Result<(), Error> = async {
            loop {
                self.index.native(scope, kind).map_err(resource_error)?;
                let next = self
                    .binding_sessions(scope, kind)
                    .into_iter()
                    .find(|session| {
                        self.evaluation
                            .bindings
                            .items
                            .get(&binding)
                            .is_some_and(|item| !item.targets.contains_key(session))
                    });
                let Some(session) = next else {
                    break;
                };
                self.install_binding(id, binding, &session, deadline)
                    .await?;
            }
            self.index.native(scope, kind).map_err(resource_error)?;
            self.evaluation
                .bindings
                .registrations
                .complete(binding, ())?;
            Ok(())
        }
        .await;
        if let Some(item) = self.evaluation.bindings.items.get_mut(&binding) {
            item.initializing = false;
        }
        match result {
            Ok(()) => Ok(json!({"scope":scope,"bindingId":binding,"registered":true})),
            Err(mut error) => {
                let initialization = error.partial.take();
                if self
                    .evaluation
                    .bindings
                    .items
                    .get(&binding)
                    .is_some_and(|item| item.targets.is_empty())
                {
                    self.evaluation.bindings.items.remove(&binding);
                    self.evaluation.bindings.registrations.forget(binding);
                }
                let retained = self.evaluation.bindings.items.contains_key(&binding);
                error.partial = Some(
                    json!({"scope":scope,"bindingId":binding,"registrationPending":retained,"cleanupPending":retained,"initialization":initialization}),
                );
                Err(error)
            }
        }
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Retain subscription and startup-script acknowledgements before document verification and cleanup"
    )]
    async fn install_binding(
        &mut self,
        id: Uuid,
        binding: Uuid,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        let item = self
            .evaluation
            .bindings
            .items
            .get(&binding)
            .ok_or("STALE_REFERENCE")?;
        if let Some(target) = item.targets.get(session) {
            return if !target.pending && target.script.is_some() {
                Ok(())
            } else {
                Err("BINDING_SETUP_PENDING".into())
            };
        }
        if item.targets.len() >= 512 {
            return Err("BINDING_TARGET_CAPACITY".into());
        }
        let (scope, kind, name, raw) = (item.scope, item.kind, item.name.clone(), item.raw.clone());
        let worlds = self.binding_worlds(scope, kind, session);
        for world in &worlds {
            let route = self
                .index
                .route(*world, Some(Kind::World))
                .map_err(resource_error)?;
            let existing = self.call(id, Some(session), "Runtime.callFunctionOn", json!({"functionDeclaration":"function(name){return Object.prototype.hasOwnProperty.call(globalThis,name);}","arguments":[{"value":name}],"uniqueContextId":route.execution_unique_id,"returnByValue":true}), deadline).await?;
            self.index
                .route(*world, Some(Kind::World))
                .map_err(resource_error)?;
            if existing["result"]["value"] != false {
                return Err("BINDING_NAME_CONFLICT".into());
            }
        }
        self.evaluation
            .bindings
            .items
            .get_mut(&binding)
            .ok_or("STALE_REFERENCE")?
            .targets
            .insert(
                session.into(),
                Installation {
                    pending: true,
                    ..Default::default()
                },
            );
        let result: Result<(), Error> = async {
            self.call(
                id,
                Some(session),
                "Runtime.addBinding",
                json!({"name":raw,"executionContextName":""}),
                deadline,
            )
            .await?;
            self.evaluation
                .bindings
                .items
                .get_mut(&binding)
                .and_then(|item| item.targets.get_mut(session))
                .ok_or("STALE_REFERENCE")?
                .registered = true;
            let source = format!(
                "({})({},{},{});",
                bindings::BRIDGE,
                json!(raw),
                json!(name),
                include_str!("../../../reading/evaluation.js")
            );
            let reply = self
                .call(
                    id,
                    Some(session),
                    "Page.addScriptToEvaluateOnNewDocument",
                    json!({"source":source,"runImmediately":true}),
                    deadline,
                )
                .await?;
            let script = reply["identifier"]
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 1024)
                .ok_or("BACKEND_PROTOCOL")?
                .to_owned();
            let target = self
                .evaluation
                .bindings
                .items
                .get_mut(&binding)
                .and_then(|item| item.targets.get_mut(session))
                .ok_or("STALE_REFERENCE")?;
            target.script = Some(script);
            target.pending = false;
            self.index.native(scope, kind).map_err(resource_error)?;
            self.order_init_scripts_last(id, session, deadline).await?;
            self.index.native(scope, kind).map_err(resource_error)?;
            for world in worlds {
                self.index
                    .route(world, Some(Kind::World))
                    .map_err(resource_error)?;
            }
            Ok(())
        }
        .await;
        if !self.index.retained_attachment(session) {
            if let Some(item) = self.evaluation.bindings.items.get_mut(&binding) {
                item.targets.remove(session);
            }
            self.index.native(scope, kind).map_err(resource_error)?;
            return Ok(());
        }
        if result
            .as_ref()
            .err()
            .is_some_and(|error| !uncertain(error.code))
            && let Some(target) = self
                .evaluation
                .bindings
                .items
                .get_mut(&binding)
                .and_then(|item| item.targets.get_mut(session))
        {
            target.pending = false;
        }
        result
    }
    pub(in super::super) async fn configure_bindings(
        &mut self,
        id: Uuid,
        kind: Kind,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        if !matches!(kind, Kind::Page | Kind::Frame) {
            return Ok(());
        }
        self.fence_bindings();
        let Some((context, page, _)) = self.index.diagnostic_scope(session) else {
            return Ok(());
        };
        let bindings = self
            .evaluation
            .bindings
            .items
            .iter()
            .filter(|(id, item)| {
                item.context == context
                    && (item.kind == Kind::Context || Some(item.scope) == page)
                    && self
                        .evaluation
                        .bindings
                        .registrations
                        .get(item.scope, **id)
                        .is_ok_and(|registration| {
                            registration.backend.is_some() && !registration.removing
                        })
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for binding in bindings {
            self.install_binding(id, binding, session, deadline).await?;
        }
        Ok(())
    }
    async fn binding_cleanup_call(
        &mut self,
        id: Uuid,
        binding: Uuid,
        session: &str,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<bool, Error> {
        self.evaluation
            .bindings
            .items
            .get_mut(&binding)
            .and_then(|item| item.targets.get_mut(session))
            .ok_or("STALE_REFERENCE")?
            .pending = true;
        let result = self.call(id, Some(session), method, params, deadline).await;
        if !self.index.retained_attachment(session) {
            if let Some(item) = self.evaluation.bindings.items.get_mut(&binding) {
                item.targets.remove(session);
            }
            return Ok(false);
        }
        if result
            .as_ref()
            .err()
            .is_none_or(|error| !uncertain(error.code))
            && let Some(target) = self
                .evaluation
                .bindings
                .items
                .get_mut(&binding)
                .and_then(|item| item.targets.get_mut(session))
        {
            target.pending = false;
        }
        result.map(|_| true)
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Retained removal stages fence script, live-world bridge and Runtime subscription cleanup under one owner"
    )]
    async fn remove_scoped_binding(
        &mut self,
        id: Uuid,
        scope: Reference,
        binding: Uuid,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.evaluation.bindings.registrations.get(scope, binding)?;
        if self
            .evaluation
            .bindings
            .calls
            .values()
            .any(|call| call.binding == binding)
        {
            return Err("BINDING_CALLS_PENDING".into());
        }
        let item = self
            .evaluation
            .bindings
            .items
            .get(&binding)
            .cloned()
            .ok_or("UNKNOWN_BINDING")?;
        if item.initializing {
            return Err("BINDING_PENDING".into());
        }
        if item.closing {
            return Err("BINDING_REMOVAL_PENDING".into());
        }
        self.evaluation
            .bindings
            .registrations
            .start_remove(scope, binding)?;
        self.evaluation
            .bindings
            .items
            .get_mut(&binding)
            .ok_or("UNKNOWN_BINDING")?
            .closing = true;
        let result: Result<(), Error> = async {
            for (session, target) in item.targets {
                if !self.index.retained_attachment(&session) {
                    continue;
                }
                if target.pending {
                    return Err("BINDING_CLEANUP_PENDING".into());
                }
                if let Some(script) = target.script {
                    if !self
                        .binding_cleanup_call(
                            id,
                            binding,
                            &session,
                            "Page.removeScriptToEvaluateOnNewDocument",
                            json!({"identifier":script}),
                            deadline,
                        )
                        .await?
                    {
                        continue;
                    }
                    if let Some(target) = self
                        .evaluation
                        .bindings
                        .items
                        .get_mut(&binding)
                        .and_then(|item| item.targets.get_mut(&session))
                    {
                        target.script = None;
                    }
                }
                self.evaluation
                    .bindings
                    .items
                    .get_mut(&binding)
                    .and_then(|item| item.targets.get_mut(&session))
                    .ok_or("STALE_REFERENCE")?
                    .pending = true;
                let closed: Result<(), Error> = async {
                    for world in self.binding_worlds(scope, item.kind, &session) {
                        let reply = self
                            .binding_bridge(
                                world,
                                bindings::CLOSE,
                                vec![json!({"value":item.raw})],
                                deadline,
                            )
                            .await;
                        if !self.index.retained(world, Kind::World) {
                            continue;
                        }
                        if reply? != true {
                            return Err("BINDING_CLEANUP_PENDING".into());
                        }
                    }
                    Ok(())
                }
                .await;
                if closed
                    .as_ref()
                    .err()
                    .is_none_or(|error| !uncertain(error.code))
                    && let Some(target) = self
                        .evaluation
                        .bindings
                        .items
                        .get_mut(&binding)
                        .and_then(|item| item.targets.get_mut(&session))
                {
                    target.pending = false;
                }
                closed?;
                if target.registered
                    && !self
                        .binding_cleanup_call(
                            id,
                            binding,
                            &session,
                            "Runtime.removeBinding",
                            json!({"name":item.raw}),
                            deadline,
                        )
                        .await?
                {
                    continue;
                }
                if let Some(item) = self.evaluation.bindings.items.get_mut(&binding) {
                    item.targets.remove(&session);
                }
            }
            Ok(())
        }
        .await;
        if let Some(item) = self.evaluation.bindings.items.get_mut(&binding) {
            item.closing = false;
        }
        if let Err(mut error) = result {
            self.evaluation.bindings.registrations.retry_remove(binding);
            error.partial = Some(
                json!({"scope":scope,"bindingId":binding,"cleanupPending":true,"nextAction":if item.kind == Kind::Context{"context_close"}else{"page_close"}}),
            );
            return Err(error);
        }
        self.evaluation
            .bindings
            .calls
            .retain(|_, call| call.binding != binding);
        self.evaluation.bindings.items.remove(&binding);
        self.evaluation.bindings.registrations.forget(binding);
        Ok(json!({"scope":scope,"bindingId":binding,"removed":true}))
    }
}
