//! Retained native DevTools frontends, attached through the owned CDP connection.
use super::*;
pub(super) const PANELS: &[&str] = &[
    "elements",
    "console",
    "network",
    "sources",
    "resources",
    "timeline",
    "chrome-recorder",
    "heap-profiler",
    "lighthouse",
    "security",
];
#[derive(Clone)]
struct Developer {
    reference: Reference,
    page: Reference,
    target: String,
    session: Option<String>,
    created: bool,
    closed: bool,
    initializing: bool,
    closing: bool,
    detached: bool,
    panel: Option<String>,
    models: Value,
}
#[derive(Default)]
pub(super) struct Developers {
    // Unknown post-capture allocations cannot be cleared by replacing a trace receipt.
    pub(super) analysis_uncertain: BTreeSet<Reference>,
    items: BTreeMap<Reference, Developer>,
    closed: BTreeSet<Reference>,
    pub(super) evidence: crate::browser::artifacts::Store,
    pub(super) audit: Option<(Uuid, Reference)>,
    pub(super) css_busy: BTreeSet<Reference>,
}
impl Worker {
    pub(super) fn analysis_developer_for_page(
        &self,
        page: Reference,
    ) -> Result<Option<Reference>, Error> {
        let Some(item) = self
            .developers
            .items
            .values()
            .find(|item| item.page == page)
        else {
            if self.developers.analysis_uncertain.contains(&page) {
                return Err("TRACE_ANALYSIS_FRONTEND_RECOVERY_REQUIRED".into());
            }
            if self.developers.items.len() + self.developers.analysis_uncertain.len() >= 4 {
                return Err("DEVELOPER_SESSION_CAPACITY".into());
            }
            return Ok(None);
        };
        if !item.created {
            return Err("PERFORMANCE_OWNED_FRONTEND_REQUIRED".into());
        }
        if item.initializing || item.closing {
            return Err("DEVELOPER_BUSY".into());
        }
        self.developer_session(item.reference)?;
        Ok(Some(item.reference))
    }
    pub(super) async fn developer_open(
        &mut self,
        id: Uuid,
        page: Reference,
        panel: Option<String>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.developer_open_inner(id, page, panel, false, deadline)
            .await
    }

    pub(super) async fn developer_open_for_analysis(
        &mut self,
        id: Uuid,
        page: Reference,
        panel: Option<String>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.developer_open_inner(id, page, panel, true, deadline)
            .await
    }

    #[expect(
        clippy::too_many_lines,
        reason = "One frontend attachment transaction retains its identity through uncertain attachment, target readiness and pinned module probing"
    )]
    async fn developer_open_inner(
        &mut self,
        id: Uuid,
        page: Reference,
        panel: Option<String>,
        owned_only: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.developers.items.len() >= 4 {
            return Err("DEVELOPER_SESSION_CAPACITY".into());
        }
        if self.developers.items.values().any(|item| item.page == page) {
            return Err("DEVELOPER_CONFLICT".into());
        }
        let target = self
            .index
            .native(page, Kind::Page)
            .map_err(resource_error)?;
        let inspected_target = target.clone();
        let previous = self
            .call_read(
                id,
                None,
                "Target.getDevToolsTarget",
                json!({"targetId":target}),
                deadline,
            )
            .await?;
        let created = previous["targetId"].is_null();
        if owned_only && !created {
            return Err("PERFORMANCE_OWNED_FRONTEND_REQUIRED".into());
        }
        let mut params = json!({"targetId":target});
        if let Some(panel) = &panel {
            params["panelId"] = json!(panel);
        }
        let reply = self
            .call(id, None, "Target.openDevTools", params, deadline)
            .await
            .inspect_err(|error| {
                if owned_only && error.code == "OUTCOME_UNKNOWN" {
                    self.developers.analysis_uncertain.insert(page);
                }
            })?;
        let target = reply["targetId"]
            .as_str()
            .ok_or("BACKEND_PROTOCOL")?
            .to_owned();
        let reference = Reference {
            connection_id: page.connection_id,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        self.developers.items.insert(
            reference,
            Developer {
                reference,
                page,
                target: target.clone(),
                session: None,
                created,
                closed: false,
                initializing: true,
                closing: false,
                detached: false,
                panel,
                models: Value::Null,
            },
        );
        let attachment = self
            .call(
                id,
                None,
                "Target.attachToTarget",
                json!({"targetId":target,"flatten":true}),
                deadline,
            )
            .await;
        let attachment = match attachment {
            Ok(value) => value,
            Err(error) => {
                if error.code != "OUTCOME_UNKNOWN"
                    && let Some(item) = self.developers.items.get_mut(&reference)
                {
                    item.initializing = false;
                }
                return Err(error);
            }
        };
        let session = attachment["sessionId"]
            .as_str()
            .ok_or("BACKEND_PROTOCOL")?
            .to_owned();
        let item = self
            .developers
            .items
            .get_mut(&reference)
            .ok_or("DEVELOPER_SESSION_CLOSED")?;
        item.initializing = false;
        if item.detached || item.closed {
            return Err("DEVELOPER_SESSION_LOST".into());
        }
        item.session = Some(session.clone());
        self.call(id, Some(&session), "Runtime.enable", json!({}), deadline)
            .await?;
        let ready_expression = format!(
            "(async()=>{{if(location.protocol!=='devtools:')throw Error('Unexpected developer frontend');const SDK=await import('devtools://devtools/bundled/core/sdk/sdk.js');const until=Date.now()+15000;while(Date.now()<until){{const target=SDK.TargetManager.TargetManager.instance().primaryPageTarget();if(target?.id()==={})return true;await new Promise(resolve=>setTimeout(resolve,25));}}throw Error('Developer page target not ready');}})()",
            json!(inspected_target)
        );
        let ready = self
            .call_read(
                id,
                Some(&session),
                "Runtime.evaluate",
                json!({"expression":ready_expression,"awaitPromise":true,"returnByValue":true}),
                deadline,
            )
            .await?;
        if ready["result"]["value"] != true {
            let mut error = Error::from("DEVTOOLS_INITIALIZATION_FAILED");
            error.partial = Some(
                json!({"developer":reference,"page":page,"exception":ready["exceptionDetails"]["exception"]["description"]}),
            );
            return Err(error);
        }
        // Probe only pinned browser modules. Application text cannot select a
        // privileged frontend expression, endpoint or host path.
        let expression = r#"(async()=>{if(location.protocol!=='devtools:')throw Error('Unexpected developer frontend');const paths=['panels/lighthouse/lighthouse.js','models/trace/trace.js','models/heap_snapshot/heap_snapshot.js'];const modules={};for(const path of paths){try{const module=await import('devtools://devtools/bundled/'+path);modules[path]=Object.keys(module);}catch{modules[path]=null;}}return {nativeFrontend:true,modules};})()"#;
        let result = self
            .call_read(
                id,
                Some(&session),
                "Runtime.evaluate",
                json!({"expression":expression,"awaitPromise":true,"returnByValue":true}),
                deadline,
            )
            .await?;
        if result.get("exceptionDetails").is_some() {
            return Err("DEVTOOLS_INITIALIZATION_FAILED".into());
        }
        self.developers
            .items
            .get_mut(&reference)
            .ok_or("DEVELOPER_SESSION_CLOSED")?
            .models = result["result"]["value"].clone();
        self.developer_status(reference)
    }
    pub(super) fn developer_status(&self, reference: Reference) -> Result<Value, Error> {
        let item = self
            .developers
            .items
            .get(&reference)
            .ok_or("STALE_REFERENCE")?;
        Ok(
            json!({"developer":item.reference,"page":item.page,"native":true,"frontendOwned":item.created,"attached":item.session.is_some(),"closed":item.closed,"initializing":item.initializing,"closing":item.closing,"panel":item.panel,"models":item.models,"audit":self.developers.audit.filter(|(_,page)|*page==item.page).map(|(id,_)|id),"backend":"chrome-cdp-native-devtools"}),
        )
    }
    pub(super) async fn developer_close(
        &mut self,
        reference: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.heap.busy == Some(reference)
            || self.performance.busy == Some(reference)
            || self.developers.css_busy.contains(&reference)
        {
            return Err("DEVELOPER_BUSY".into());
        }
        if self.developers.closed.contains(&reference) {
            return Ok(json!({"developer":reference,"closed":true}));
        }
        if self.developers.closed.len() >= 4096 {
            return Err("RESOURCE_CAPACITY".into());
        }
        let item = self
            .developers
            .items
            .get(&reference)
            .cloned()
            .ok_or("STALE_REFERENCE")?;
        if item.closing || (item.initializing && !item.closed) {
            return Err("DEVELOPER_BUSY".into());
        }
        self.developers
            .items
            .get_mut(&reference)
            .ok_or("STALE_REFERENCE")?
            .closing = true;
        let result = self
            .close_developer_attachment(reference, &item, deadline)
            .await;
        if result.is_ok() {
            self.developers.items.remove(&reference);
            self.developers.closed.insert(reference);
        } else if let Some(item) = self.developers.items.get_mut(&reference) {
            item.closing = false;
        }
        result.map(|()|json!({"developer":reference,"closed":true,"borrowedFrontendPreserved":!item.created}))
    }
    async fn close_developer_attachment(
        &mut self,
        reference: Reference,
        item: &Developer,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.close_heap_models(reference, deadline).await?;
        self.close_performance_model(reference, deadline).await?;
        if let Some(session) = &item.session {
            let detached = self
                .cleanup_call(
                    None,
                    "Target.detachFromTarget",
                    json!({"sessionId":session}),
                    deadline,
                )
                .await;
            let current = self
                .developers
                .items
                .get_mut(&reference)
                .ok_or("DEVELOPER_SESSION_CLOSED")?;
            if let Err(error) = detached
                && current.session.is_some()
            {
                return Err(error);
            }
            current.session = None;
            self.evaluation.groups.end_session(session);
        }
        if item.created
            && !self
                .developers
                .items
                .get(&reference)
                .is_some_and(|item| item.closed)
        {
            let result = self
                .cleanup_call(
                    None,
                    "Target.closeTarget",
                    json!({"targetId":item.target}),
                    deadline,
                )
                .await;
            if !self
                .developers
                .items
                .get(&reference)
                .is_some_and(|item| item.closed)
                && result?["success"] != true
            {
                return Err("DEVELOPER_CLEANUP_PENDING".into());
            }
        }
        Ok(())
    }
    pub(super) async fn close_developers(&mut self, deadline: Instant) -> Result<(), Error> {
        for reference in self.developers.items.keys().copied().collect::<Vec<_>>() {
            self.developer_close(reference, deadline).await?;
        }
        Ok(())
    }
    pub(super) fn clear_developers(&mut self) {
        self.developers.analysis_uncertain.clear();
        self.developers.items.clear();
        self.developers.closed.clear();
        self.developers.audit = None;
    }
    pub(super) fn developer_session(&self, reference: Reference) -> Result<String, Error> {
        self.developers
            .items
            .get(&reference)
            .filter(|item| !item.closed)
            .and_then(|item| item.session.clone())
            .ok_or("DEVELOPER_SESSION_LOST".into())
    }
    pub(super) fn developer_target(&self, reference: Reference) -> Result<String, Error> {
        self.developers
            .items
            .get(&reference)
            .filter(|item| !item.closed)
            .map(|item| item.target.clone())
            .ok_or("DEVELOPER_SESSION_LOST".into())
    }
    pub(super) fn developer_snapshots(&self) -> Value {
        json!(
            self.developers
                .items
                .keys()
                .filter_map(|reference| self.developer_status(*reference).ok())
                .collect::<Vec<_>>()
        )
    }
    pub(super) fn developer_event(&mut self, event: &Event) {
        for item in self.developers.items.values_mut() {
            if event.method == "Target.attachedToTarget"
                && event.params["targetInfo"]["targetId"] == item.target
                && let Some(session) = event.params["sessionId"].as_str()
            {
                item.session = Some(session.to_owned());
            }
            if event.method == "Runtime.exceptionThrown"
                && event.session_id.as_deref() == item.session.as_deref()
            {
                let text = event.params["exceptionDetails"]["exception"]["description"]
                    .as_str()
                    .or_else(|| event.params["exceptionDetails"]["text"].as_str())
                    .unwrap_or("Developer exception");
                item.models["lastException"] = json!(crate::browser::diagnostics::text(text).0);
            }
            if event.method == "Target.targetDestroyed" && event.params["targetId"] == item.target {
                item.closed = true;
                item.session = None;
            }
            if event.method == "Target.detachedFromTarget"
                && event.params["sessionId"].as_str() == item.session.as_deref()
            {
                item.session = None;
                item.detached = true;
            }
            if event.method == "Runtime.executionContextsCleared"
                && event.session_id.as_deref() == item.session.as_deref()
            {
                item.models = Value::Null;
            }
        }
    }
}
