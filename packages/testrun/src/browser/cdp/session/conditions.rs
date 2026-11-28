//! Bounded backend-owned condition observation; no input retries.
use super::*;
use crate::browser::page::{Condition, Locator};

const QUERY_KEY: &str = "canonic.testrun.condition-query.v1";

#[derive(Default)]
pub(super) struct Probe {
    pub(super) dispatched: bool,
    scope: Option<page::Scope>,
    locator: crate::browser::reading::LocatorProgress,
}

impl Worker {
    async fn condition_scope(
        &mut self,
        id: Uuid,
        page: Reference,
        frame: Option<Reference>,
        follow_main_frame: bool,
        deadline: Instant,
    ) -> Result<Option<page::Scope>, Error> {
        self.drain()?;
        self.reconcile(id, deadline).await?;
        let current = self.index.page_frame(page, frame).map_err(resource_error)?;
        if let Some(scope) = self
            .condition_probes
            .get(&id)
            .and_then(|probe| probe.scope.as_ref())
            && scope.frame == current
            && self
                .index
                .route(scope.world, Some(Kind::World))
                .is_ok_and(|route| {
                    route.session_id == scope.session
                        && route.execution_unique_id.as_deref() == Some(scope.unique.as_str())
                })
        {
            return Ok(Some(scope.clone()));
        }
        let scope = self.page_scope(id, page, frame, deadline).await?;
        let initialize = format!(
            "(() => {{const key=Symbol.for({});if(!globalThis[key])Object.defineProperty(globalThis,key,{{value:({})}});return true;}})()",
            json!(QUERY_KEY),
            crate::browser::reading::query_function("chromium")
        );
        match self.page_expression(id, &scope, initialize, deadline).await {
            Err(error) if follow_main_frame && error.code == "STALE_REFERENCE" => return Ok(None),
            result => result?,
        };
        if let Some(probe) = self.condition_probes.get_mut(&id) {
            probe.scope = Some(scope.clone());
        }
        Ok(Some(scope))
    }

    pub(super) async fn poll_waits(&mut self) {
        let ids = self.waits.keys().copied().collect::<Vec<_>>();
        for id in ids {
            // A preceding DOM probe can service and settle metadata waits
            // while awaiting CDP, invalidating later IDs in this snapshot.
            let Some(command) = self.waits.remove(&id) else {
                continue;
            };
            let request = command.request.as_ref().expect("Wait request");
            let result = match self.observation_ready(id, command.deadline) {
                Err(error) => Err(error),
                Ok(()) => {
                    self.probe_condition(id, request, command.deadline, false)
                        .await
                }
            };
            self.settle_wait(command, result).await;
        }
    }
    /// Metadata observation can settle while a native input awaits a dialog.
    /// DOM conditions stay outside that call's pending renderer execution.
    pub(super) fn has_metadata_waits(&self) -> bool {
        self.waits
            .values()
            .any(|command| command.request.as_ref().is_some_and(execution::metadata))
    }

    pub(super) async fn poll_metadata_waits(&mut self) {
        let ids = self
            .waits
            .iter()
            .filter_map(|(id, command)| {
                (!matches!(command.request, Some(Request::PageWait { .. }))).then_some(*id)
            })
            .collect::<Vec<_>>();
        for id in ids {
            let command = self.waits.remove(&id).expect("Pending condition");
            let request = command.request.as_ref().expect("Wait request");
            let result = match self.observation_ready(id, command.deadline) {
                Err(error) => Err(error),
                Ok(()) => self.probe_metadata(id, request),
            };
            self.settle_wait(command, result).await;
        }
    }
    async fn settle_wait(&mut self, command: Command, result: Result<Option<Value>, Error>) {
        let id = command.id;
        let result = result.map_err(|error| {
            self.event_wait_failure(
                id,
                command.request.as_ref().expect("Wait request"),
                annotate_timeout(self.condition_probes.get(&id), error, command.deadline),
            )
        });
        match result {
            Ok(Some(value)) => self.finish(id, Ok(value)).await,
            Ok(None) => {
                self.waits.insert(id, command);
            }
            Err(error) => self.finish(id, Err(error)).await,
        }
    }
    fn probe_metadata(&mut self, id: Uuid, request: &Request) -> Result<Option<Value>, Error> {
        match request {
            Request::DiagnosticsWait {
                context,
                after,
                conditions,
            } => self.probe_diagnostics(*context, *after, conditions),
            Request::EventsWait {
                context,
                after,
                conditions,
            } => self.probe_events(id, *context, *after, conditions),
            _ => unreachable!("Metadata waits cannot dispatch renderer calls"),
        }
    }
    async fn probe_condition(
        &mut self,
        id: Uuid,
        request: &Request,
        deadline: Instant,
        follow_locator_navigation: bool,
    ) -> Result<Option<Value>, Error> {
        match request {
            Request::DiagnosticsWait { .. } | Request::EventsWait { .. } => {
                self.probe_metadata(id, request)
            }
            Request::PageWait {
                page,
                frame,
                locator,
                condition,
            } => {
                let follow_main_frame =
                    frame.is_none() && (locator.is_none() || follow_locator_navigation);
                let Some(scope) = self
                    .condition_scope(id, *page, *frame, follow_main_frame, deadline)
                    .await?
                else {
                    return Ok(None);
                };
                let frame_visible =
                    if matches!(condition, Condition::Visible {} | Condition::Hidden {}) {
                        self.frame_visible(id, *page, scope.frame, deadline).await?
                    } else {
                        true
                    };
                let selector = locator
                    .as_ref()
                    .map(|locator| locator.scoped_selector(scope.frame))
                    .transpose()?
                    .unwrap_or_else(|| "css=body".into());
                let idle = self.network_idle(id, condition, &scope, deadline).await?;
                let expression = format!(
                    "globalThis[Symbol.for({})]({})",
                    json!(QUERY_KEY),
                    json!({"selector":selector,"condition":condition,"frameVisible":frame_visible})
                );
                match self
                    .page_expression(id, &scope, expression, deadline)
                    .await
                    .and_then(page::query_result)
                    .map(|mut value| {
                        if locator.is_some()
                            && let Some(probe) = self.condition_probes.get_mut(&id)
                        {
                            probe.locator.observe(&value);
                        }
                        value["networkIdle"] = json!(idle);
                        condition.settle(value)
                    }) {
                    Ok(value) if value["satisfied"] == true => Ok(Some(
                        json!({"page":page,"frame":scope.frame,"satisfied":true,"source":"chrome-cdp-condition"}),
                    )),
                    Ok(_) => Ok(None),
                    Err(error) if follow_main_frame && error.code == "STALE_REFERENCE" => Ok(None),
                    Err(error) => Err(error),
                }
            }
            _ => Err("INVALID_WAIT".into()),
        }
    }
    pub(super) async fn events_wait(
        &mut self,
        id: Uuid,
        context: Reference,
        after: u64,
        conditions: &[crate::browser::activity::Condition],
        deadline: Instant,
    ) -> Result<Value, Error> {
        let request = Request::EventsWait {
            context,
            after,
            conditions: conditions.to_vec(),
        };
        // Shared act steps reuse one operation ID. Each inline wait owns its
        // own cursor, even when an earlier step waited over the same history.
        if let Some(operation) = self
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .operations
            .get_mut(&id)
        {
            operation.result = Some(crate::browser::activity::wait_progress(
                context, after, after,
            ));
        }
        let result = self.wait_condition(id, &request, deadline).await;
        result.map_err(|error| self.event_wait_failure(id, &request, timed_out(error, deadline)))
    }
    pub(super) async fn page_wait(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        // A batch shares an operation ID between waits and input. Limit
        // dispatch coalescing to this step so each later step keeps its witness.
        self.condition_probes.insert(id, Probe::default());
        let result = self.wait_condition(id, &request, deadline).await;
        let probe = self.condition_probes.remove(&id);
        result.map_err(|error| annotate_timeout(probe.as_ref(), error, deadline))
    }
    async fn wait_condition(
        &mut self,
        id: Uuid,
        request: &Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        loop {
            self.observation_ready(id, deadline)?;
            if let Some(result) = self.probe_condition(id, request, deadline, true).await? {
                return Ok(result);
            }
            self.observation_tick(deadline).await?;
        }
    }

    /// Chrome's `networkIdle` lifecycle witness for a wait that needs it. The
    /// first need turns lifecycle events on, and Chrome then reports the
    /// milestones the document already reached; until then the frame is not idle.
    async fn network_idle(
        &mut self,
        id: Uuid,
        condition: &Condition,
        scope: &page::Scope,
        deadline: Instant,
    ) -> Result<bool, Error> {
        if !condition.needs_network_idle() {
            return Ok(false);
        }
        let native = self
            .index
            .native(scope.frame, Kind::Frame)
            .map_err(resource_error)?;
        if let Some(idle) = self.navigation.network_idle(&scope.session, &native) {
            return Ok(idle);
        }
        self.call(
            id,
            Some(&scope.session),
            "Page.setLifecycleEventsEnabled",
            json!({"enabled":true}),
            deadline,
        )
        .await?;
        Ok(false)
    }
    pub(super) fn observation_ready(&self, id: Uuid, deadline: Instant) -> Result<(), Error> {
        self.shared.control.input_current(id)?;
        self.controller_current(id, true)?;
        let state = self.shared.state.lock().expect("Chrome lifecycle");
        if state.phase != Phase::Ready {
            return Err("SESSION_NOT_READY".into());
        }
        if state
            .operations
            .get(&id)
            .is_some_and(|o| o.cancel_requested)
        {
            return Err("CANCELLED".into());
        }
        if Instant::now() >= deadline {
            return Err("CONDITION_TIMEOUT".into());
        }
        Ok(())
    }

    pub(super) async fn observation_tick(&mut self, deadline: Instant) -> Result<(), Error> {
        self.http_poll().await;
        if self.trace_due() {
            Box::pin(self.poll_trace()).await;
        }
        self.service_startup(deadline).await;
        self.poll_har_routes().await;
        self.expire_callbacks().await;
        let shared = self.shared.clone();
        tokio::select! {
            execution=next_execution(&mut self.deferred_executions, &mut self.executions) => {
                if let Some(execution)=execution { self.perform_independent(execution).await; }
                Ok(())
            },
            response=next_response(&mut self.responses) => {
                if let Some(response)=response { Box::pin(self.respond(response)).await; }
                Ok(())
            },
            event=next_event(&mut self.events) => self.apply(event.ok_or("BACKEND_LOST")?),
            ()=shared.changed.notified() => Ok(()),
            ()=tokio::time::sleep_until(deadline.min(Instant::now()+Duration::from_millis(25))) => Ok(()),
        }
    }
}

/// A wait's own scope or probe call that reaches the wait's deadline before
/// dispatch is the wait timing out, as on the driver backend; it changed nothing.
fn timed_out(mut error: Error, deadline: Instant) -> Error {
    if error.code == "NOT_DISPATCHED" && Instant::now() >= deadline {
        error.code = "CONDITION_TIMEOUT";
    }
    error
}

fn annotate_timeout(probe: Option<&Probe>, error: Error, deadline: Instant) -> Error {
    let error = timed_out(error, deadline);
    match probe {
        Some(probe) => probe.locator.timeout(error),
        None => error,
    }
}

pub(super) fn validate(locator: &Option<Locator>, condition: &Condition) -> Result<(), Error> {
    if let Some(locator) = locator {
        locator.selector()?;
    }
    condition.validate()?;
    Ok(())
}
