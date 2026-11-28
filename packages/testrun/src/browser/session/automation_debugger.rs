//! Owned pinned-driver automation pauses and independent, identity-fenced replies.
use super::*;
use crate::browser::{automation_debugger::Action, objects::ObjectRef};
mod calls;

struct Debugger {
    wire: ObjectRef,
    observed: bool,
    paused: bool,
    pause: Option<Uuid>,
    requested: bool,
    requested_location: Option<crate::browser::automation_debugger::Location>,
    pending: Option<Uuid>,
    last: Value,
    details: Value,
    lost: bool,
    enabled: bool,
    enabling: bool,
    calls: calls::Calls,
}
struct Closing {
    wire: ObjectRef,
    started: bool,
}
#[derive(Default)]
pub(super) struct Debuggers {
    items: BTreeMap<Reference, Debugger>,
    closed: BTreeMap<Reference, Closing>,
}
impl Debuggers {
    pub(super) fn retain(&mut self, live: &BTreeSet<Reference>) {
        for (context, debugger) in &self.items {
            if !live.contains(context) && (debugger.paused || debugger.pending.is_some()) {
                self.closed.entry(*context).or_insert_with(|| Closing {
                    wire: debugger.wire.clone(),
                    started: false,
                });
            }
        }
        self.items.retain(|context, _| live.contains(context));
    }
    pub(super) fn released(&mut self) {
        self.items.clear();
        self.closed.clear();
    }
    pub(super) fn prepare_close(&mut self, context: Reference) {
        if let Some(debugger) = self.items.get(&context) {
            self.closed.entry(context).or_insert_with(|| Closing {
                wire: debugger.wire.clone(),
                started: false,
            });
        }
    }
    pub(super) fn cancel_close(&mut self, context: Reference) {
        if self
            .closed
            .get(&context)
            .is_some_and(|closing| !closing.started)
        {
            self.closed.remove(&context);
        }
    }
    pub(super) fn cleanup_summary(&self) -> Value {
        json!(self.closed.iter().map(|(context,closing)|json!({"context":context,"pauseReleasePending":true,"commandDispatchedOrUnknown":closing.started})).collect::<Vec<_>>())
    }
    pub(super) fn lose(&mut self) {
        for debugger in self.items.values_mut() {
            debugger.lost = true;
            debugger.pause = None;
            debugger.details = Value::Null;
            debugger.requested_location = None;
            debugger.calls = Default::default();
        }
    }
    fn adopt(&mut self, context: Reference, wire: ObjectRef) -> Result<(), Error> {
        if let Some(debugger) = self.items.get(&context) {
            return if debugger.wire == wire {
                Ok(())
            } else {
                Err("AUTOMATION_DEBUGGER_LOST".into())
            };
        }
        if self.items.len() + self.closed.len() >= 32 {
            return Err("RESOURCE_CAPACITY".into());
        }
        self.items.insert(
            context,
            Debugger {
                wire,
                observed: false,
                paused: false,
                pause: None,
                requested: false,
                requested_location: None,
                pending: None,
                last: Value::Null,
                details: Value::Null,
                lost: false,
                enabled: false,
                enabling: false,
                calls: Default::default(),
            },
        );
        Ok(())
    }
    pub(super) fn event(&mut self, wire: &ObjectRef, method: &str, params: &Value) -> bool {
        let Some(debugger) = self
            .items
            .values_mut()
            .find(|debugger| &debugger.wire == wire)
        else {
            return false;
        };
        if method == "apiCallsUpdated" {
            if !debugger.lost && (debugger.enabled || debugger.enabling) {
                debugger.calls.update(params);
            }
            return true;
        }
        if method != "pausedStateChanged" {
            return false;
        }
        if debugger.lost {
            return true;
        }
        debugger.observed = true;
        let paused = params["pausedDetails"].is_object();
        if paused {
            if !debugger.paused {
                debugger.pause = Some(Uuid::new_v4());
            }
            debugger.requested = false;
            debugger.requested_location = None;
            let details = &params["pausedDetails"];
            let title = details["title"].as_str().unwrap_or("");
            let file = details["location"]["file"].as_str().unwrap_or("");
            let short_title = title.chars().take(256).collect::<String>();
            let short_file = file.chars().take(256).collect::<String>();
            debugger.details = json!({"truncated":short_title.len()<title.len()||short_file.len()<file.len(),"title":short_title,"location":{"file":short_file,"line":details["location"]["line"],"column":details["location"]["column"]}});
        } else {
            debugger.pause = None;
            debugger.details = Value::Null;
        }
        debugger.paused = paused;
        true
    }
    fn status(&self, context: Reference, details: bool) -> Result<Value, Error> {
        let debugger = self
            .items
            .get(&context)
            .ok_or("AUTOMATION_DEBUGGER_NOT_OBSERVED")?;
        let mut status = json!({"context":context,"scope":"context-automation-calls","javascriptDebugger":false,"stateKnown":debugger.observed&&!debugger.lost,"paused":(debugger.observed&&!debugger.lost).then_some(debugger.paused),"pauseId":debugger.pause,"pauseRequested":debugger.requested,"pendingControlId":debugger.pending,"lastControl":debugger.last,"sourceLost":debugger.lost,"detailsIncluded":details});
        if details {
            status["details"] = debugger.details.clone();
        }
        status["callCollectionEnabled"] = json!(debugger.enabled && !debugger.lost);
        Ok(status)
    }
    fn begin(
        &mut self,
        context: Reference,
        action: &Action,
    ) -> Result<(Uuid, &'static str), Error> {
        let debugger = self
            .items
            .get_mut(&context)
            .ok_or("AUTOMATION_DEBUGGER_NOT_OBSERVED")?;
        if debugger.lost {
            return Err("AUTOMATION_DEBUGGER_LOST".into());
        }
        if debugger.pending.is_some() {
            return Err("AUTOMATION_DEBUGGER_BUSY".into());
        }
        let method = match action {
            Action::Enable => {
                debugger.enabling = true;
                "enable"
            }
            Action::Pause => {
                if debugger.paused {
                    return Err("AUTOMATION_ALREADY_PAUSED".into());
                }
                if debugger.requested {
                    return Err("AUTOMATION_PAUSE_ALREADY_REQUESTED".into());
                }
                debugger.requested = true;
                debugger.requested_location = None;
                "requestPause"
            }
            Action::Resume { pause_id }
            | Action::Next { pause_id }
            | Action::RunTo { pause_id, .. } => {
                if !debugger.paused || debugger.pause != Some(*pause_id) {
                    return Err("STALE_AUTOMATION_PAUSE".into());
                }
                debugger.requested = !matches!(action, Action::Resume { .. });
                debugger.requested_location = match action {
                    Action::RunTo { location, .. } => Some(location.clone()),
                    _ => None,
                };
                match action {
                    Action::RunTo { .. } => "runTo",
                    Action::Next { .. } => "next",
                    _ => "resume",
                }
            }
            Action::Status { .. } | Action::Calls { .. } => {
                return Err("CAPABILITY_UNAVAILABLE".into());
            }
        };
        let id = Uuid::new_v4();
        debugger.pending = Some(id);
        Ok((id, method))
    }
    fn finish(
        &mut self,
        context: Reference,
        id: Uuid,
        method: &str,
        result: &Result<(), Error>,
    ) -> Result<(), Error> {
        let debugger = self.items.get_mut(&context).ok_or("STALE_REFERENCE")?;
        if debugger.pending != Some(id) {
            return Err("AUTOMATION_DEBUGGER_LOST".into());
        }
        let unknown = result
            .as_ref()
            .err()
            .is_some_and(|error| error.code == "OUTCOME_UNKNOWN");
        if !unknown {
            debugger.pending = None;
        }
        if result.is_ok() && method == "enable" {
            debugger.enabled = true;
        }
        if !unknown && method == "enable" {
            debugger.enabling = false;
            if result.is_err() {
                debugger.calls = Default::default();
            }
        }
        if result.is_err() && !unknown && method == "requestPause" {
            debugger.requested = false;
        }
        debugger.last = json!({"id":id,"action":match method{"requestPause"=>"pause","runTo"=>"run_to",_=>method},"settled":!unknown,"acknowledged":result.is_ok(),"error":result.as_ref().err().map(|error|error.code)});
        Ok(())
    }
    pub(super) fn summary(&self) -> Value {
        json!(
            self.items
                .keys()
                .filter_map(|context| self.status(*context, false).ok())
                .collect::<Vec<_>>()
        )
    }

    pub(super) fn caller_owns_actionability(
        &self,
        context: Reference,
        source: Option<&crate::browser::automation_debugger::Location>,
    ) -> bool {
        let Some(debugger) = self.items.get(&context) else {
            return false;
        };
        if debugger.lost || debugger.pending.is_some() || debugger.last["acknowledged"] != true {
            return false;
        }
        if debugger.enabled {
            return true;
        }
        if !debugger.requested {
            return false;
        }
        match &debugger.requested_location {
            Some(target) => source.is_some_and(|source| {
                source.file.contains(&target.file)
                    && target.line.is_none_or(|line| source.line == Some(line))
                    && target
                        .column
                        .is_none_or(|column| source.column == Some(column))
            }),
            None => true,
        }
    }
}
impl Host {
    pub(super) async fn finish_closed_debugger(
        &self,
        context: Reference,
        deadline: Instant,
    ) -> Result<(), Error> {
        if self.state.lock().unwrap().retired.get(&context) != Some(&Kind::Context) {
            return Err("AUTOMATION_DEBUGGER_CLEANUP_PENDING".into());
        }
        let wire = {
            let mut debuggers = self.automation_debuggers.lock().unwrap();
            let Some(closing) = debuggers.closed.get_mut(&context) else {
                return Ok(());
            };
            if closing.started {
                return Err("AUTOMATION_DEBUGGER_CLEANUP_PENDING".into());
            }
            closing.started = true;
            closing.wire.clone()
        };
        // The exact context-close witness precedes this release. A late SDK
        // continuation can no longer dispatch input into any page in that scope.
        let connection = self.connection()?;
        let result = connection
            .retained(&wire, "resume", json!({}), deadline, true)
            .await;
        let released = result.is_ok()
            || matches!(&result,
            Err(connection::Error::Remote(error)) if error.error["error"]["message"]=="Debugger is not paused")
            || matches!(
                &result,
                Err(connection::Error::Protocol(
                    crate::browser::dispatcher::Error::Object(
                        crate::browser::objects::Error::Stale
                            | crate::browser::objects::Error::Unknown
                    )
                ))
            );
        if released {
            let mut debuggers = self.automation_debuggers.lock().unwrap();
            debuggers.closed.remove(&context);
            debuggers.items.remove(&context);
            return Ok(());
        }
        let mut error = Error::from(result.unwrap_err());
        if matches!(error.code, "NOT_DISPATCHED" | "BACKEND_BUSY")
            && let Some(closing) = self
                .automation_debuggers
                .lock()
                .unwrap()
                .closed
                .get_mut(&context)
        {
            closing.started = false;
        }
        error.partial = Some(
            json!({"context":context,"closed":true,"automationCleanupPending":true,"nextAction":"retry-context-close-or-browser-disconnect"}),
        );
        Err(error)
    }
    fn read_debugger_calls(
        &self,
        context: Reference,
        offset: u16,
        limit: u16,
        include_details: bool,
    ) -> Result<Value, Error> {
        let debuggers = self.automation_debuggers.lock().unwrap();
        let debugger = debuggers
            .items
            .get(&context)
            .ok_or("AUTOMATION_DEBUGGER_NOT_OBSERVED")?;
        if debugger.lost {
            return Err("AUTOMATION_DEBUGGER_LOST".into());
        }
        if !debugger.enabled {
            return Err("AUTOMATION_CALL_COLLECTION_DISABLED".into());
        }
        let mut result = debugger.calls.read(offset, limit, include_details);
        result["context"] = json!(context);
        result["source"] = json!("pinned-Playwright-context-automation-debugger");
        Ok(result)
    }
    async fn debugger_wire(
        &self,
        connection: &Connection,
        wire: &ObjectRef,
    ) -> Result<ObjectRef, Error> {
        let initializer = connection
            .initializer(wire)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        let debugger = connection
            .resolve(
                initializer["debugger"]["guid"]
                    .as_str()
                    .ok_or("AUTOMATION_DEBUGGER_UNAVAILABLE")?,
                "Debugger",
            )
            .await?;
        Ok(debugger)
    }
    pub(super) async fn context_debugger(
        &self,
        context: Reference,
        action: Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        action.validate()?;
        let wire = self.wire(context, Kind::Context)?;
        let connection = self.connection()?;
        let debugger = self.debugger_wire(&connection, &wire).await?;
        self.automation_debuggers
            .lock()
            .unwrap()
            .adopt(context, debugger.clone())?;
        if let Action::Status { include_details } = action {
            connection.event_barrier(deadline).await?;
            self.wire(context, Kind::Context)?;
            return self
                .automation_debuggers
                .lock()
                .unwrap()
                .status(context, include_details);
        }
        if let Action::Calls {
            offset,
            limit,
            include_details,
        } = action
        {
            connection.event_barrier(deadline).await?;
            self.wire(context, Kind::Context)?;
            return self.read_debugger_calls(context, offset, limit, include_details);
        }
        if matches!(action, Action::Enable)
            && self
                .automation_debuggers
                .lock()
                .unwrap()
                .items
                .get(&context)
                .is_some_and(|debugger| {
                    debugger.enabled && !debugger.lost && debugger.pending.is_none()
                })
        {
            return self
                .automation_debuggers
                .lock()
                .unwrap()
                .status(context, false);
        }
        let (id, method) = self
            .automation_debuggers
            .lock()
            .unwrap()
            .begin(context, &action)?;
        let params = match action {
            Action::RunTo { location, .. } => json!({"location":location}),
            _ => json!({}),
        };
        let reply = connection
            .retained(&debugger, method, params, deadline, true)
            .await
            .map(|_| ())
            .map_err(Error::from);
        let acknowledged = reply.is_ok();
        let result = match reply {
            Ok(()) => connection
                .event_barrier(Instant::now() + Duration::from_secs(5))
                .await
                .map_err(Error::from),
            Err(error) => Err(error),
        };
        if let Err(error) = self
            .automation_debuggers
            .lock()
            .unwrap()
            .finish(context, id, method, &result)
        {
            let mut error = result.err().unwrap_or(error);
            error.partial = Some(
                json!({"context":context,"controlId":id,"dispatchAcknowledged":acknowledged,"contextRetired":true}),
            );
            return Err(error);
        }
        if let Err(mut error) = result {
            error.partial = Some(
                json!({"context":context,"controlId":id,"dispatchAcknowledged":acknowledged,"debugger":self.automation_debuggers.lock().unwrap().status(context,false).ok()}),
            );
            return Err(error);
        }
        self.wire(context, Kind::Context)?;
        let mut status = self
            .automation_debuggers
            .lock()
            .unwrap()
            .status(context, false)?;
        status["controlId"] = json!(id);
        status["dispatchCompleted"] = json!(true);
        Ok(status)
    }
}
