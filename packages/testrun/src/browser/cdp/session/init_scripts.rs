//! Scoped future-document sources and per-attachment installation ownership.
use super::*;
use std::borrow::Cow;

#[derive(Clone)]
enum Installation {
    Pending,
    Installed(String),
    Removing { _identifier: String },
}
struct Policy {
    scope: Reference,
    source: Cow<'static, str>,
    targets: BTreeMap<String, Installation>,
    removing: bool,
}
#[derive(Default)]
pub(super) struct Scripts {
    catalog: crate::browser::scripts::Scripts<()>,
    policies: BTreeMap<Uuid, Policy>,
}
impl Scripts {
    fn forget(&mut self, id: Uuid) {
        self.catalog.forget(id);
        self.policies.remove(&id);
    }
    pub(super) fn list(&self, scope: Reference) -> Value {
        let mut value = self.catalog.list(scope);
        value["installations"]=json!(self.policies.iter().filter(|(_,p)|p.scope==scope).map(|(id,p)|json!({"scriptId":id,"targets":p.targets.len(),"pendingTargets":p.targets.values().filter(|state|matches!(state,Installation::Pending|Installation::Removing{..})).count(),"removing":p.removing})).collect::<Vec<_>>());
        value
    }
}
fn uncertain(code: &str) -> bool {
    matches!(
        code,
        "OUTCOME_UNKNOWN" | "BACKEND_LOST" | "BACKEND_PROTOCOL"
    )
}
impl Worker {
    pub(super) fn fence_init_scripts(&mut self) {
        let index = &self.index;
        let scripts = &mut self.evaluation.scripts;
        scripts.catalog.retain(|scope| {
            index.retained(scope, Kind::Page) || index.retained(scope, Kind::Context)
        });
        scripts.policies.retain(|_, policy| {
            policy
                .targets
                .retain(|session, _| index.retained_attachment(session));
            index.retained(policy.scope, Kind::Page) || index.retained(policy.scope, Kind::Context)
        });
    }
    fn script_scope(&self, scope: Reference) -> Result<Kind, Error> {
        let kind = if self.index.retained(scope, Kind::Page) {
            Kind::Page
        } else {
            Kind::Context
        };
        self.index.native(scope, kind).map_err(resource_error)?;
        Ok(kind)
    }
    pub(super) fn script_targets(&self, scope: Reference) -> Vec<String> {
        self.index
            .targets()
            .into_iter()
            .filter(|(_, kind, _, _)| matches!(kind, Kind::Page | Kind::Frame))
            .filter_map(|(_, _, _, session)| session)
            .filter(|session| {
                self.index
                    .diagnostic_scope(session)
                    .is_some_and(|(context, page, _)| context == scope || page == Some(scope))
            })
            .collect()
    }
    pub(super) async fn init_script(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.drain()?;
        self.fence_init_scripts();
        let (scope, context) = match &request {
            Request::PageInitScriptAdd { page, .. }
            | Request::PageInitScriptList { page }
            | Request::PageInitScriptRemove { page, .. } => (*page, false),
            Request::ContextInitScriptAdd { context, .. }
            | Request::ContextInitScriptList { context }
            | Request::ContextInitScriptRemove { context, .. } => (*context, true),
            _ => return Err("INVALID_SCOPE".into()),
        };
        self.index
            .native(scope, if context { Kind::Context } else { Kind::Page })
            .map_err(resource_error)?;
        match request {
            Request::PageInitScriptAdd { source, .. }
            | Request::ContextInitScriptAdd { source, .. } => {
                crate::browser::scripts::validate(&source)?;
                self.register_init_script(id, scope, Cow::Owned(source), deadline)
                    .await
            }
            Request::PageInitScriptList { .. } | Request::ContextInitScriptList { .. } => {
                let mut value = self.evaluation.scripts.list(scope);
                if context {
                    value.as_object_mut().expect("Script list").remove("page");
                    value["context"] = json!(scope);
                }
                Ok(value)
            }
            Request::PageInitScriptRemove { script_id, .. }
            | Request::ContextInitScriptRemove { script_id, .. } => {
                self.remove_init_script(id, scope, script_id, deadline)
                    .await
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
    pub(super) async fn register_init_script(
        &mut self,
        id: Uuid,
        scope: Reference,
        source: Cow<'static, str>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.drain()?;
        self.fence_init_scripts();
        let kind = self.script_scope(scope)?;
        let field = if kind == Kind::Context {
            "context"
        } else {
            "page"
        };
        let script_id = self.evaluation.scripts.catalog.begin(scope)?;
        self.evaluation.scripts.policies.insert(
            script_id,
            Policy {
                scope,
                source,
                targets: BTreeMap::new(),
                removing: false,
            },
        );
        let result: Result<(), Error> = async {
            loop {
                self.script_scope(scope)?;
                let next = self.script_targets(scope).into_iter().find(|session| {
                    self.evaluation
                        .scripts
                        .policies
                        .get(&script_id)
                        .is_some_and(|policy| !policy.targets.contains_key(session))
                });
                let Some(session) = next else {
                    break;
                };
                self.install_init_script(id, script_id, &session, false, deadline)
                    .await?;
            }
            self.script_scope(scope)?;
            self.evaluation.scripts.catalog.complete(script_id, ())?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => Ok(
                json!({(field):scope,"scriptId":script_id,"registered":true,"currentDocumentExecuted":false,"crossProcessFrames":true}),
            ),
            Err(mut error) => {
                let closed = !self.index.retained(scope, kind);
                if closed
                    || self
                        .evaluation
                        .scripts
                        .policies
                        .get(&script_id)
                        .is_some_and(|policy| policy.targets.is_empty())
                {
                    self.evaluation.scripts.forget(script_id);
                }
                error.partial = Some(
                    json!({(field):scope,"scriptId":script_id,"scopeClosed":closed,"registrationUncertain":uncertain(error.code),"cleanupPending":self.evaluation.scripts.policies.contains_key(&script_id)}),
                );
                Err(error)
            }
        }
    }
    async fn install_init_script(
        &mut self,
        id: Uuid,
        script_id: Uuid,
        session: &str,
        immediate: bool,
        deadline: Instant,
    ) -> Result<(), Error> {
        let policy = self
            .evaluation
            .scripts
            .policies
            .get_mut(&script_id)
            .ok_or("UNKNOWN_INIT_SCRIPT")?;
        if let Some(state) = policy.targets.get(session) {
            return if matches!(state, Installation::Installed(_)) {
                Ok(())
            } else {
                Err("INIT_SCRIPT_PENDING".into())
            };
        }
        if policy.targets.len() >= 512 {
            return Err("INIT_SCRIPT_TARGET_CAPACITY".into());
        }
        let scope = policy.scope;
        let source = policy.source.clone();
        policy.targets.insert(session.into(), Installation::Pending);
        let result = self
            .call(
                id,
                Some(session),
                "Page.addScriptToEvaluateOnNewDocument",
                json!({"source":source,"runImmediately":immediate}),
                deadline,
            )
            .await;
        if !self.index.retained(scope, Kind::Page) && !self.index.retained(scope, Kind::Context) {
            self.script_scope(scope)?;
            return Err("STALE_REFERENCE".into());
        }
        if !self.index.retained_attachment(session) {
            if let Some(policy) = self.evaluation.scripts.policies.get_mut(&script_id) {
                policy.targets.remove(session);
            }
            return Ok(());
        }
        match result {
            Ok(reply) => {
                let identifier = reply["identifier"]
                    .as_str()
                    .filter(|id| !id.is_empty() && id.len() <= 1024)
                    .ok_or("BACKEND_PROTOCOL")?
                    .to_owned();
                self.evaluation
                    .scripts
                    .policies
                    .get_mut(&script_id)
                    .ok_or("UNKNOWN_INIT_SCRIPT")?
                    .targets
                    .insert(session.into(), Installation::Installed(identifier));
                self.script_scope(scope)?;
                Ok(())
            }
            Err(error) => {
                if !uncertain(error.code)
                    && let Some(policy) = self.evaluation.scripts.policies.get_mut(&script_id)
                {
                    policy.targets.remove(session);
                }
                Err(error)
            }
        }
    }
    pub(super) async fn configure_init_scripts(
        &mut self,
        id: Uuid,
        kind: Kind,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        if !matches!(kind, Kind::Page | Kind::Frame) {
            return Ok(());
        }
        self.fence_init_scripts();
        let Some((context, page, _)) = self.index.diagnostic_scope(session) else {
            return Ok(());
        };
        let scripts = self
            .evaluation
            .scripts
            .policies
            .iter()
            .filter(|(id, p)| {
                !p.removing
                    && (p.scope == context || Some(p.scope) == page)
                    && self
                        .evaluation
                        .scripts
                        .catalog
                        .native(p.scope, **id)
                        .is_ok()
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for script in scripts {
            self.install_init_script(id, script, session, true, deadline)
                .await?;
        }
        Ok(())
    }
    /// Keep internal runtime sources before user startup code, even when user
    /// code was registered first. Reinstall only after confirmed removal.
    pub(super) async fn order_init_scripts_last(
        &mut self,
        id: Uuid,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.fence_init_scripts();
        let scripts = self
            .evaluation
            .scripts
            .policies
            .iter()
            .filter(|(_, policy)| !policy.removing && policy.targets.contains_key(session))
            .map(|(script, policy)| {
                self.evaluation
                    .scripts
                    .catalog
                    .native(policy.scope, *script)
                    .map_err(Error::from)?;
                let Installation::Installed(identifier) = &policy.targets[session] else {
                    return Err("INIT_SCRIPT_PENDING".into());
                };
                Ok((*script, policy.scope, identifier.clone()))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        for (script, scope, identifier) in scripts {
            self.script_scope(scope)?;
            self.evaluation
                .scripts
                .policies
                .get_mut(&script)
                .ok_or("UNKNOWN_INIT_SCRIPT")?
                .targets
                .insert(
                    session.into(),
                    Installation::Removing {
                        _identifier: identifier.clone(),
                    },
                );
            let removed = self
                .call(
                    id,
                    Some(session),
                    "Page.removeScriptToEvaluateOnNewDocument",
                    json!({"identifier":identifier}),
                    deadline,
                )
                .await;
            if !self.index.retained_attachment(session) {
                self.fence_init_scripts();
                return Ok(());
            }
            if let Err(mut error) = removed {
                if !uncertain(error.code)
                    && let Some(policy) = self.evaluation.scripts.policies.get_mut(&script)
                {
                    policy
                        .targets
                        .insert(session.into(), Installation::Installed(identifier));
                }
                error.partial =
                    Some(json!({"scope":scope,"scriptId":script,"cleanupPending":true}));
                return Err(error);
            }
            let Some(policy) = self.evaluation.scripts.policies.get_mut(&script) else {
                continue;
            };
            policy.targets.remove(session);
            if !policy.removing {
                let installed = async {
                    self.script_scope(scope)?;
                    self.install_init_script(id, script, session, false, deadline)
                        .await
                }
                .await;
                if let Err(mut error) = installed {
                    error.partial =
                        Some(json!({"scope":scope,"scriptId":script,"cleanupPending":true}));
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    async fn remove_init_script(
        &mut self,
        id: Uuid,
        scope: Reference,
        script_id: Uuid,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let kind = self.script_scope(scope)?;
        let field = if kind == Kind::Context {
            "context"
        } else {
            "page"
        };
        let policy = self
            .evaluation
            .scripts
            .policies
            .get_mut(&script_id)
            .filter(|p| p.scope == scope)
            .ok_or("UNKNOWN_INIT_SCRIPT")?;
        policy.removing = true;
        let targets = policy.targets.clone();
        for (session, state) in targets {
            if !self.index.retained_attachment(&session) {
                if let Some(policy) = self.evaluation.scripts.policies.get_mut(&script_id) {
                    policy.targets.remove(&session);
                }
                continue;
            }
            let identifier = match state {
                Installation::Installed(id) => id,
                Installation::Pending | Installation::Removing { .. } => {
                    let mut error = Error::from("INIT_SCRIPT_CLEANUP_PENDING");
                    error.partial = Some(
                        json!({(field):scope,"scriptId":script_id,"nextAction":if kind==Kind::Context{"context_close"}else{"page_close"}}),
                    );
                    return Err(error);
                }
            };
            self.evaluation
                .scripts
                .policies
                .get_mut(&script_id)
                .ok_or("UNKNOWN_INIT_SCRIPT")?
                .targets
                .insert(
                    session.clone(),
                    Installation::Removing {
                        _identifier: identifier.clone(),
                    },
                );
            let removed = self
                .call(
                    id,
                    Some(&session),
                    "Page.removeScriptToEvaluateOnNewDocument",
                    json!({"identifier":identifier}),
                    deadline,
                )
                .await;
            if let Err(mut error) = removed
                && self.index.retained_attachment(&session)
            {
                if !uncertain(error.code)
                    && let Some(policy) = self.evaluation.scripts.policies.get_mut(&script_id)
                {
                    policy
                        .targets
                        .insert(session, Installation::Installed(identifier));
                }
                error.partial = Some(
                    json!({(field):scope,"scriptId":script_id,"cleanupPending":true,"removalUncertain":uncertain(error.code)}),
                );
                return Err(error);
            }
            if let Some(policy) = self.evaluation.scripts.policies.get_mut(&script_id) {
                policy.targets.remove(&session);
            }
        }
        self.evaluation.scripts.forget(script_id);
        Ok(
            json!({(field):scope,"scriptId":script_id,"removed":true,"existingEffectsReverted":false}),
        )
    }
}
