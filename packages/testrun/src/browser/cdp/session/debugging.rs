//! Debugger ownership and independent stack/resume commands during paused calls.
use super::*;
use crate::browser::debugging::{Action, Step};
struct Pause {
    id: Uuid,
    reason: String,
    frames: Vec<Value>,
    total_frames: usize,
}
struct Debugger {
    id: Uuid,
    session: String,
    enabled: bool,
    pause: Option<Pause>,
    breakpoints: BTreeMap<Uuid, Breakpoint>,
}
struct Breakpoint {
    native: String,
    marker: Option<String>,
}
fn variable_text(value: &str) -> String {
    value.chars().take(128).collect()
}
#[derive(Default)]
pub(super) struct Debuggers {
    items: BTreeMap<Reference, Debugger>,
}
impl Worker {
    pub(super) fn debugger_owned(&self, page: Reference) -> bool {
        self.debuggers.items.contains_key(&page)
    }
    pub(super) fn fence_debuggers(&mut self) {
        self.debuggers
            .items
            .retain(|page, _| self.index.route(*page, Some(Kind::Page)).is_ok());
    }
    pub(super) fn debugger_status(
        &self,
        page: Reference,
        include_frames: bool,
    ) -> Result<Value, Error> {
        self.index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?;
        let Some(d) = self.debuggers.items.get(&page) else {
            return Ok(json!({"page":page,"enabled":false,"paused":false}));
        };
        let mut result = json!({"page":page,"enabled":d.enabled,"paused":d.pause.is_some(),"pauseId":d.pause.as_ref().map(|p|p.id),"reason":d.pause.as_ref().map(|p|&p.reason),"frameCount":d.pause.as_ref().map(|p|p.total_frames),"breakpoints":d.breakpoints.keys().collect::<Vec<_>>(),"scope":"selected-page-target","automationDebugger":false});
        result["sessionId"] = json!(d.id);
        result["protocol"] = json!("cdp");
        result["temporaryLogpoints"] = json!(
            d.breakpoints
                .iter()
                .filter_map(|(id, b)| b
                    .marker
                    .as_ref()
                    .map(|m| json!({"breakpointId":id,"marker":m,"pending":b.native.is_empty()})))
                .collect::<Vec<_>>()
        );
        if include_frames {
            let mut frames=d.pause.as_ref().map(|p|p.frames.iter().enumerate().map(|(index,f)|json!({"frame":index,"function":crate::browser::diagnostics::text(f["functionName"].as_str().unwrap_or("")).0,"url":crate::browser::diagnostics::url(f["url"].as_str().unwrap_or("")),"line":f["location"]["lineNumber"],"column":f["location"]["columnNumber"],"scopes":f["scopeChain"].as_array().map(|s|s.iter().take(32).enumerate().map(|(i,s)|json!({"scope":i,"type":crate::browser::diagnostics::text(s["type"].as_str().unwrap_or("unknown")).0})).collect::<Vec<_>>())})).collect::<Vec<_>>()).unwrap_or_default();
            while serde_json::to_vec(&frames)
                .map_err(|_| Error::from("BACKEND_PROTOCOL"))?
                .len()
                > 6144
            {
                frames.pop();
            }
            result["framesTruncated"] = json!(
                d.pause
                    .as_ref()
                    .is_some_and(|p| p.total_frames > frames.len())
            );
            result["frames"] = json!(frames);
        }
        Ok(result)
    }
    pub(super) fn debugger_snapshots(&self) -> Value {
        json!(
            self.debuggers
                .items
                .keys()
                .filter_map(|p| self.debugger_status(*p, false).ok())
                .collect::<Vec<_>>()
        )
    }
    pub(super) fn debugger_event(&mut self, event: &Event) {
        for d in self
            .debuggers
            .items
            .values_mut()
            .filter(|d| Some(d.session.as_str()) == event.session_id.as_deref())
        {
            match event.method.as_str() {
                "Debugger.paused" => {
                    let frames = event.params["callFrames"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    let total_frames = frames.len();
                    d.pause = Some(Pause {
                        id: Uuid::new_v4(),
                        reason: crate::browser::diagnostics::text(
                            event.params["reason"].as_str().unwrap_or("unknown"),
                        )
                        .0,
                        frames: frames.into_iter().take(100).collect(),
                        total_frames,
                    });
                }
                "Debugger.resumed"
                | "Runtime.executionContextsCleared"
                | "Inspector.targetCrashed" => d.pause = None,
                _ => {}
            }
        }
    }
    pub(super) async fn page_debugger(
        &mut self,
        id: Uuid,
        page: Reference,
        action: Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        action.validate()?;
        if matches!(
            action,
            Action::Enable { .. }
                | Action::Pause
                | Action::Step { .. }
                | Action::BreakpointSet { .. }
                | Action::LogpointSet { .. }
                | Action::Evaluate { .. }
        ) {
            self.controller_current(id, true)?;
        }
        let session = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?
            .session_id;
        if let Action::Enable { exceptions } = action {
            if self.profile_owns_debugger(page) {
                return Err("COVERAGE_DEBUGGER_CONFLICT".into());
            }
            if self.profile_active(page) {
                return Err("DEBUGGER_PROFILE_CONFLICT".into());
            }
            return self
                .enable_page_debugger(id, page, session, exceptions, deadline)
                .await;
        }
        if let Action::Status { include_frames } = action {
            return self.debugger_status(page, include_frames);
        }
        if !self
            .debuggers
            .items
            .get(&page)
            .is_some_and(|d| d.enabled || matches!(action, Action::Disable))
        {
            return Err("DEBUGGER_NOT_ENABLED".into());
        }
        match action {
            Action::Pause => {
                self.cleanup_call(Some(&session), "Debugger.pause", json!({}), deadline)
                    .await?;
            }
            Action::Resume { pause_id } | Action::Step { pause_id, .. } => {
                self.require_pause(page, pause_id)?;
                let method = match action {
                    Action::Step {
                        step: Step::Into, ..
                    } => "Debugger.stepInto",
                    Action::Step {
                        step: Step::Over, ..
                    } => "Debugger.stepOver",
                    Action::Step {
                        step: Step::Out, ..
                    } => "Debugger.stepOut",
                    _ => "Debugger.resume",
                };
                self.cleanup_call(Some(&session), method, json!({}), deadline)
                    .await?;
            }
            Action::Disable => {
                self.cleanup_call(Some(&session), "Debugger.disable", json!({}), deadline)
                    .await?;
                self.debuggers.items.remove(&page);
            }
            action @ (Action::BreakpointSet { .. } | Action::LogpointSet { .. }) => {
                return self
                    .set_page_breakpoint(page, &session, action, deadline)
                    .await;
            }
            Action::BreakpointRemove { breakpoint_id } => {
                self.remove_page_breakpoint(page, &session, breakpoint_id, deadline)
                    .await?;
            }
            action @ Action::Evaluate { .. } => {
                return self
                    .evaluate_debugger_pause(page, &session, action, deadline)
                    .await;
            }
            action @ Action::Variables { .. } => {
                return self
                    .debugger_variables(page, &session, action, deadline)
                    .await;
            }
            Action::Enable { .. } | Action::Status { .. } => unreachable!(),
        }
        self.debugger_status(page, false)
    }
    async fn remove_page_breakpoint(
        &mut self,
        page: Reference,
        session: &str,
        breakpoint_id: Uuid,
        deadline: Instant,
    ) -> Result<(), Error> {
        let native = self.debuggers.items[&page]
            .breakpoints
            .get(&breakpoint_id)
            .ok_or("UNKNOWN_BREAKPOINT")?
            .native
            .clone();
        if native.is_empty() {
            return Err("BREAKPOINT_PENDING".into());
        }
        self.cleanup_call(
            Some(session),
            "Debugger.removeBreakpoint",
            json!({"breakpointId":native}),
            deadline,
        )
        .await?;
        if let Some(d) = self.debuggers.items.get_mut(&page) {
            d.breakpoints.remove(&breakpoint_id);
        }
        Ok(())
    }

    async fn debugger_variables(
        &mut self,
        page: Reference,
        session: &str,
        action: Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let Action::Variables {
            pause_id,
            frame,
            scope,
            offset,
            limit,
        } = action
        else {
            unreachable!()
        };
        let pause = self.require_pause(page, pause_id)?;
        let object = pause
            .frames
            .get(frame)
            .and_then(|f| f["scopeChain"].get(scope))
            .and_then(|s| s["object"]["objectId"].as_str())
            .ok_or("INVALID_DEBUGGER_SCOPE")?
            .to_owned();
        let reply = self
            .cleanup_call(
                Some(session),
                "Runtime.getProperties",
                json!({"objectId":object,"ownProperties":true,"generatePreview":false}),
                deadline,
            )
            .await?;
        self.require_pause(page, pause_id)?;
        if reply.get("exceptionDetails").is_some() {
            return Err("VARIABLE_READ_FAILED".into());
        }
        let descriptors = reply["result"].as_array().ok_or("BACKEND_PROTOCOL")?;
        let mut variables = descriptors.iter().take(10000).skip(offset).take(limit).map(|p| {
            let value = &p["value"];
            let primitive = match &value["value"] {
                Value::String(s) => json!(variable_text(s)),
                Value::Bool(_) | Value::Number(_) | Value::Null => value["value"].clone(),
                _ => Value::Null,
            };
            json!({"name":variable_text(p["name"].as_str().unwrap_or("")),
                "type":variable_text(value["type"].as_str().unwrap_or("accessor")),
                "value":primitive,"unserializableValue":variable_text(value["unserializableValue"].as_str().unwrap_or("")),
                "accessorSkipped":p.get("get").is_some() || p.get("set").is_some(),
                "expandable":value.get("objectId").is_some()})
        }).collect::<Vec<_>>();
        while serde_json::to_vec(&variables)
            .map_err(|_| Error::from("BACKEND_PROTOCOL"))?
            .len()
            > 6144
        {
            variables.pop();
        }
        let next = offset.saturating_add(variables.len());
        Ok(
            json!({"page":page,"pauseId":pause_id,"frame":frame,"scope":scope,"variables":variables,
            "offset":offset,"nextOffset":if next<descriptors.len().min(10000){Some(next)}else{None},
            "total":descriptors.len(),"private":true,"potentiallyMutating":false,
            "readMode":"own_property_descriptors_no_getter_invocation","nestedObjects":"not expanded","stringCharacterLimit":128,"enumerationTruncated":descriptors.len()>10000}),
        )
    }
    async fn evaluate_debugger_pause(
        &mut self,
        page: Reference,
        session: &str,
        action: Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let Action::Evaluate {
            pause_id,
            frame,
            expression,
        } = action
        else {
            unreachable!()
        };
        let pause = self.require_pause(page, pause_id)?;
        let frame = pause.frames.get(frame).ok_or("INVALID_DEBUGGER_FRAME")?;
        let codec = include_str!("../../reading/evaluation.js");
        let reply=self.cleanup_call(Some(session),"Debugger.evaluateOnCallFrame",json!({"callFrameId":frame["callFrameId"],"expression":format!("(()=>{{const codec={codec};return codec.encode(eval({}));}})()",json!(expression)),"returnByValue":true,"silent":true}),deadline).await?;
        self.require_pause(page, pause_id).map_err(|mut error|{error.partial=Some(json!({"page":page,"pauseId":pause_id,"dispatchCompleted":true,"potentiallyMutating":true,"valueDiscardedAfterPauseChanged":true}));error})?;
        if reply.get("exceptionDetails").is_some() {
            let mut error = Error::from("JAVASCRIPT_EXCEPTION");
            error.partial = Some(
                json!({"page":page,"pauseId":pause_id,"dispatchCompleted":true,"potentiallyMutating":true,"exception":reply["exceptionDetails"]["text"]}),
            );
            return Err(error);
        }
        Ok(
            json!({"page":page,"pauseId":pause_id,"value":crate::browser::evaluation::result(&reply["result"]["value"])? ,"potentiallyMutating":true,"dispatchCompleted":true}),
        )
    }
    async fn enable_page_debugger(
        &mut self,
        id: Uuid,
        page: Reference,
        session: String,
        exceptions: crate::browser::debugging::Exceptions,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.debuggers.items.len() >= 4 && !self.debuggers.items.contains_key(&page) {
            return Err("DEBUGGER_CAPACITY".into());
        }
        self.debuggers
            .items
            .entry(page)
            .or_insert_with(|| Debugger {
                id: Uuid::new_v4(),
                session: session.clone(),
                enabled: false,
                pause: None,
                breakpoints: BTreeMap::new(),
            });
        if !self.debuggers.items[&page].enabled {
            self.call(
                id,
                Some(&session),
                "Debugger.enable",
                json!({"maxScriptsCacheSize":1048576}),
                deadline,
            )
            .await?;
            self.debuggers
                .items
                .get_mut(&page)
                .expect("Debugger owner")
                .enabled = true;
        }
        self.call(
            id,
            Some(&session),
            "Debugger.setPauseOnExceptions",
            json!({"state":exceptions}),
            deadline,
        )
        .await?;
        self.debugger_status(page, false)
    }
    async fn set_page_breakpoint(
        &mut self,
        page: Reference,
        session: &str,
        action: Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let (url, line, column, condition, marker) = match action {
            Action::BreakpointSet {
                url,
                line,
                column,
                condition,
            } => (url, line, column, condition, None),
            Action::LogpointSet {
                url,
                line,
                column,
                marker,
            } => {
                let condition = format!(
                    "(console.debug({}),false)",
                    json!(json!({"testrunMarker":marker}).to_string())
                );
                (url, line, column, Some(condition), Some(marker))
            }
            _ => unreachable!(),
        };
        if self.debuggers.items[&page].breakpoints.len() >= 128 {
            return Err("BREAKPOINT_CAPACITY".into());
        }
        let mut params = json!({"url":url,"lineNumber":line,"columnNumber":column});
        if let Some(condition) = condition {
            params["condition"] = json!(condition);
        }
        let breakpoint_id = Uuid::new_v4();
        self.debuggers
            .items
            .get_mut(&page)
            .ok_or("DEBUGGER_NOT_ENABLED")?
            .breakpoints
            .insert(
                breakpoint_id,
                Breakpoint {
                    native: String::new(),
                    marker: marker.clone(),
                },
            );
        let reply = self
            .cleanup_call(
                Some(session),
                "Debugger.setBreakpointByUrl",
                params,
                deadline,
            )
            .await;
        let reply = match reply {
            Ok(value) => value,
            Err(error) => {
                if error.code != "OUTCOME_UNKNOWN"
                    && let Some(d) = self.debuggers.items.get_mut(&page)
                {
                    d.breakpoints.remove(&breakpoint_id);
                }
                return Err(error);
            }
        };
        let native = reply["breakpointId"]
            .as_str()
            .ok_or("BACKEND_PROTOCOL")?
            .to_owned();
        self.debuggers
            .items
            .get_mut(&page)
            .ok_or("DEBUGGER_NOT_ENABLED")?
            .breakpoints
            .insert(
                breakpoint_id,
                Breakpoint {
                    native,
                    marker: marker.clone(),
                },
            );
        Ok(
            json!({"page":page,"breakpointId":breakpoint_id,"resolvedLocations":reply["locations"].as_array().map(Vec::len),"line":line,"column":column,"url":crate::browser::diagnostics::url(&url),"marker":marker,"temporary":marker.is_some(),"potentiallyMutating":true}),
        )
    }
    fn require_pause(&self, page: Reference, id: Uuid) -> Result<&Pause, Error> {
        self.debuggers
            .items
            .get(&page)
            .and_then(|d| d.pause.as_ref())
            .filter(|p| p.id == id)
            .ok_or("STALE_DEBUGGER_PAUSE".into())
    }
    pub(super) async fn close_debuggers(&mut self, deadline: Instant) {
        for (page, session) in self
            .debuggers
            .items
            .iter()
            .map(|(p, d)| (*p, d.session.clone()))
            .collect::<Vec<_>>()
        {
            if self
                .cleanup_call(Some(&session), "Debugger.disable", json!({}), deadline)
                .await
                .is_ok()
            {
                self.debuggers.items.remove(&page);
            }
        }
    }
    pub(super) fn clear_debuggers(&mut self) {
        self.debuggers.items.clear();
    }
}
