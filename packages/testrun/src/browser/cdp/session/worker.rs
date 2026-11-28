//! Native lifecycle supervision, journal publication and retained cleanup.

use super::*;

fn operation_cancelled(state: &State, id: Uuid) -> bool {
    state.phase == Phase::Closing
        || state
            .operations
            .get(&id)
            .is_some_and(|operation| operation.cancel_requested)
}

fn check_startup(state: &State, id: Uuid, deadline: Instant) -> Result<(), &'static str> {
    if operation_cancelled(state, id) {
        return Err("CANCELLED");
    }
    if Instant::now() >= deadline {
        return Err("BROWSER_START_TIMEOUT");
    }
    Ok(())
}

async fn wait_startup_cancel(shared: &Shared, id: Uuid) {
    loop {
        let changed = shared.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if operation_cancelled(&shared.state.lock().expect("Chrome lifecycle"), id) {
            return;
        }
        changed.await;
    }
}

impl Worker {
    pub(super) async fn wait_retained_reply(
        &mut self,
        id: Option<Uuid>,
        response: &mut tokio::task::JoinHandle<NativeReply>,
        deadline: Instant,
    ) -> Option<Result<NativeReply, tokio::task::JoinError>> {
        loop {
            let shared = self.shared.clone();
            let changed = shared.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if id.is_some_and(|id| self.observation_ready(id, deadline).is_err()) {
                return None;
            }
            tokio::select! {
                biased;
                reply = &mut *response => return Some(reply),
                () = tokio::time::sleep_until(deadline) => return None,
                event = next_event(&mut self.events) => match event {
                    Some(event) => {
                        if self.apply(event).is_err() { return None; }
                        self.service_startup(deadline).await;
                    },
                    None => { self.events = None; }
                },
                response = next_response(&mut self.responses) => if let Some(response) = response { Box::pin(self.respond(response)).await; },
                () = &mut changed => {},
            }
        }
    }

    pub(super) fn publish(&self) {
        let mut details = self.interactions.snapshot();
        details["callbacks"]
            .as_array_mut()
            .expect("Callbacks")
            .extend(self.binding_snapshot());
        self.shared.control.reconcile(
            &self
                .index
                .list(Kind::Page)
                .iter()
                .map(|(page, _)| *page)
                .collect::<Vec<_>>(),
        );
        details["control"] = self.shared.control.snapshot();
        details["previews"] = self.media.snapshot();
        details["recordings"] = json!(
            self.media
                .recordings
                .values()
                .map(|recording| recording.snapshot())
                .collect::<Vec<_>>()
        );
        details["input"] = self.inputs.snapshot();
        details["diagnostics"] = self.diagnostics.history.summary();
        details["targetStartup"] = self.startup_summary();
        details["handleCapacity"] = self.handle_allocation_snapshot();
        #[cfg(all(test, feature = "acceptance-browser-public"))]
        {
            details["privateResourceEvents"] = json!(self.index.resource_observations);
        }
        details["developers"] = self.developer_snapshots();
        details["profiles"] = self.profile_snapshots();
        details["trace"] = self.trace_status();
        details["heap"] = self.heap_status();
        details["performance"] = self.performance_status();
        details["debuggers"] = self.debugger_snapshots();
        details["transfers"] = json!({"fixtures":self.transfers.fixtures.values().map(|fixture|fixture.summary.clone()).chain(self.transfers.directories.values().map(|directory|directory.summary.clone())).collect::<Vec<_>>(),"choosers":self.transfers.choosers.iter().map(|(reference,chooser)|json!({"reference":reference,"page":chooser.page,"multiple":chooser.multiple})).collect::<Vec<_>>(),"downloads":self.transfers.downloads.values().map(|download|download.summary.clone()).collect::<Vec<_>>(),"refused":[]});
        details["transfers"]["refused"] = json!(self.transfers.refused);
        details["storage"] = self.snapshots.summary();
        *self.shared.details.lock().expect("Chrome details") = details;
        let mut resources = self.index.snapshot()["resources"]
            .as_array()
            .expect("Native resources")
            .clone();
        resources.extend(self.evaluation_resources());
        *self.shared.resources.lock().expect("Chrome resources") = json!(resources);
        let mut state = self.shared.state.lock().expect("Chrome lifecycle");
        let contexts = self.index.list(Kind::Context);
        let pages = self.index.list(Kind::Page);
        if state
            .selected_context
            .is_some_and(|r| !contexts.iter().any(|(id, _)| *id == r))
        {
            state.selected_context = None;
        }
        if state
            .selected_page
            .is_some_and(|r| !pages.iter().any(|(id, _)| *id == r))
        {
            state.selected_page = None;
        }
        self.shared.changed.notify_waiters();
    }
    fn apply_gap(&mut self, dropped: u64) {
        self.http_state.gap(&self.index);
        self.fetch.authentication_lost = true;
        self.argument_source_gap();
        // Gaps cover only evicted diagnostics. Target/frame/world events and
        // debugger-start attachments are critical: their overflow closes the
        // transport instead. Keep setup/resume available for retained targets.
        self.har_archives.capture_gap(dropped);
        self.heap_gap();
        self.trace_gap();
        self.profile_gap();
        self.webmcp_debug.gap();
        self.diagnostics
            .history
            .forget_sockets(self.websockets.clear());
        self.navigation.gaps = self.navigation.gaps.saturating_add(dropped);
        self.diagnostics.source_gap(
            dropped,
            self.index
                .targets()
                .into_iter()
                .filter_map(|(_, _, _, session)| session)
                .collect(),
        );
        self.publish();
    }
    pub(super) fn apply(&mut self, item: EventItem) -> Result<(), Error> {
        if let EventItem::Gap { dropped } = &item {
            self.apply_gap(*dropped);
        }
        if let EventItem::Event(event) = item {
            if self.http_state.private_event(&event)? {
                return Ok(());
            }
            let storage_observation = http_state::Observation::event(&event);
            let new_worker = if matches!(
                event.method.as_str(),
                "Target.targetCreated" | "Target.targetInfoChanged" | "Target.attachedToTarget"
            ) && matches!(
                event.params["targetInfo"]["type"].as_str(),
                Some("worker" | "shared_worker" | "service_worker")
            ) {
                event.params["targetInfo"]["targetId"]
                    .as_str()
                    .filter(|native| self.index.target_reference(native).is_none())
                    .map(str::to_owned)
            } else {
                None
            };
            let old_pages = self
                .index
                .list(Kind::Page)
                .into_iter()
                .map(|(reference, _)| reference)
                .collect::<BTreeSet<_>>();
            let old_frames = self.child_frames();
            let old_workers = self.scoped_workers();
            self.navigation.observe(&self.index, &event);
            self.observe_before_index_update(&event)?;
            if event.method == "Target.targetDestroyed"
                && let Some(native) = event.params["targetId"].as_str()
                && let Some(reference) = self.index.target_reference(native)
                && self.index.retained(reference, Kind::Page)
            {
                self.record_activity(
                    reference,
                    crate::browser::activity::Event::PageClosed,
                    reference,
                );
                self.interactions
                    .dialogs
                    .retain(|_, dialog| dialog.page != reference);
                self.remember_close(reference, Kind::Page)?;
            }
            let startup_event = matches!(
                event.method.as_str(),
                "Target.attachedToTarget" | "Target.detachedFromTarget"
            )
            .then(|| Event {
                method: event.method.clone(),
                session_id: event.session_id.clone(),
                params: event.params.clone(),
            });
            let allocation_end = matches!(
                event.method.as_str(),
                "Runtime.executionContextDestroyed" | "Target.detachedFromTarget"
            )
            .then(|| Event {
                method: event.method.clone(),
                session_id: event.session_id.clone(),
                params: event.params.clone(),
            });
            self.index.apply(event).map_err(resource_error)?;
            if let Some(event) = allocation_end {
                self.handle_allocation_end(&event);
            }
            if let Some(observation) = storage_observation {
                self.http_state.observe(&self.index, observation);
            }
            if let Some(event) = startup_event {
                self.startup_event(&event);
            }
            self.fence_init_scripts();
            self.fence_clocks();
            self.fence_credentials();
            self.page_headers
                .retain(|page, _| self.index.native(*page, Kind::Page).is_ok());
            self.emulation.prune(&self.index);
            self.prune_har_routes();
            self.fence_bindings();
            self.notice_diagnostic_worker(new_worker.as_deref());
            self.fence_debuggers();
            self.fence_evaluation();
            self.fence_diagnostic_arguments();
            self.fence_webmcp_debug();
            for (page, _) in self.index.list(Kind::Page) {
                if !old_pages.contains(&page) {
                    self.record_activity(page, crate::browser::activity::Event::PageCreated, page);
                }
            }
            self.record_frame_and_worker_activity(&old_frames, &old_workers);
            self.publish();
        }
        Ok(())
    }
    /// Observe the old graph before a target event changes document ownership.
    fn observe_before_index_update(&mut self, event: &Event) -> Result<(), Error> {
        self.developer_event(event);
        self.evaluation.groups.observe_private_event(event);
        self.heap_reset(event);
        self.profile_event(event);
        self.trace_event(event);
        self.trace_tab_event(event);
        if let Some(probe) = self.app_tab_probe.as_mut() {
            probe.observe(event);
        }
        self.debugger_event(event);
        self.webmcp_debug_event(event);
        self.diagnostic_event(event);
        self.websocket_event(event);
        self.fence_native_media(event);
        self.credential_event(event);
        self.interaction_event(event)?;
        self.route_event(event)?;
        self.binding_event(event)?;
        self.download_event(event)?;
        self.chooser_event(event)?;
        Ok(())
    }

    /// Child frames by stable identity, with their owning page.
    fn child_frames(&self) -> BTreeMap<Uuid, (Reference, Reference)> {
        self.index
            .list(Kind::Frame)
            .into_iter()
            .filter(|(frame, _)| self.index.parent_frame(*frame).is_ok())
            .filter_map(|(frame, _)| {
                self.index
                    .owning_page(frame)
                    .ok()
                    .map(|page| (frame.id, (frame, page)))
            })
            .collect()
    }
    /// Dedicated workers that have their owning page, and context service
    /// workers. Browser-level discovery indexes a dedicated worker before the
    /// page attachment that names its page, so the event follows the page.
    fn scoped_workers(&self) -> BTreeMap<Uuid, (Reference, Reference, Option<Reference>)> {
        use crate::browser::cdp::resources::WorkerKind;
        self.index
            .list(Kind::Worker)
            .into_iter()
            .filter_map(|(worker, _)| match self.index.worker_scope(worker) {
                Ok((context, Some(page), WorkerKind::Dedicated)) => {
                    Some((worker.id, (worker, context, Some(page))))
                }
                Ok((context, None, WorkerKind::Service)) => {
                    Some((worker.id, (worker, context, None)))
                }
                _ => None,
            })
            .collect()
    }
    /// SDK `frameattached`/`framedetached`, `worker` and `serviceworker` events.
    /// Document changes keep a frame's identity; a detach while its page is
    /// closing is page closure rather than a frame event.
    fn record_frame_and_worker_activity(
        &mut self,
        old_frames: &BTreeMap<Uuid, (Reference, Reference)>,
        old_workers: &BTreeMap<Uuid, (Reference, Reference, Option<Reference>)>,
    ) {
        use crate::browser::activity::Event as Activity;
        let frames = self.child_frames();
        for (id, (frame, page)) in &frames {
            if !old_frames.contains_key(id) {
                self.record_activity(*page, Activity::FrameAttached, *frame);
            }
        }
        for (id, (frame, page)) in old_frames {
            if !frames.contains_key(id) && self.index.retained(*page, Kind::Page) {
                self.record_activity(*page, Activity::FrameDetached, *frame);
            }
        }
        for (id, (worker, context, page)) in self.scoped_workers() {
            if old_workers.contains_key(&id) {
                continue;
            }
            match page {
                Some(page) => self.record_activity(page, Activity::WorkerCreated, worker),
                None => self.interactions.history.push_context(
                    context,
                    Activity::ServiceWorkerCreated,
                    worker,
                ),
            }
        }
    }
    pub(super) fn remember_close(&mut self, reference: Reference, kind: Kind) -> Result<(), Error> {
        if self.closed.len() >= 4096 && !self.closed.contains_key(&reference) {
            return Err("RESOURCE_CAPACITY".into());
        }
        self.closed.insert(reference, kind);
        if kind == Kind::Context {
            self.request_inputs.close_context(reference);
        }
        self.http_state.close(reference, kind);
        Ok(())
    }
    pub(super) fn drain(&mut self) -> Result<(), Error> {
        while let Some(event) = self.events.as_mut().and_then(Events::try_next) {
            self.apply(event)?;
        }
        Ok(())
    }
    fn stop_diagnostics(&mut self) {
        self.diagnostics.close();
        self.shared.details.lock().expect("Chrome details")["diagnostics"] =
            self.diagnostics.history.summary();
    }
    pub(super) fn lose(&mut self, code: &str) {
        self.http_state.gap(&self.index);
        self.stop_diagnostics();
        self.websockets.clear();
        self.index.lose();
        self.page_headers.clear();
        let mut state = self.shared.state.lock().expect("Chrome lifecycle");
        if !matches!(
            state.phase,
            Phase::Closing | Phase::CleanupPending | Phase::Closed
        ) {
            state.phase = Phase::Lost;
        }
        state.failure = Some(code.into());
        self.shared.changed.notify_waiters();
    }
    pub(super) async fn finish(&mut self, id: Uuid, result: Result<Value, Error>) {
        self.condition_probes.remove(&id);
        self.shared.control.finish(id);
        let outcome = match &result {
            Ok(_) => Outcome::Succeeded,
            Err(error) if error.code == "CANCELLED" => Outcome::Cancelled,
            Err(error)
                if matches!(
                    error.code,
                    "OUTCOME_UNKNOWN" | "HTTP_REQUEST_OUTCOME_UNKNOWN" | "CLEANUP_PENDING"
                ) || error
                    .partial
                    .as_ref()
                    .is_some_and(|partial| partial["callerEffectsUnknown"] == true) =>
            {
                Outcome::Unknown
            }
            _ => Outcome::Failed,
        };
        let code = result.as_ref().err().map(|error| error.code.to_owned());
        let mut journal_failure = self
            .journal
            .record(json!({"operationId":id,"outcome":outcome,"settled":true,"code":code}))
            .await
            .err();
        let mut state = self.shared.state.lock().expect("Chrome lifecycle");
        if let Some(operation) = state.operations.get_mut(&id) {
            let value = match result {
                Ok(value) => Some(value),
                Err(error) => error.partial,
            };
            journal_failure = operation.settle(outcome, value, code, journal_failure);
        }
        if let Some(code) = journal_failure {
            state.failure = Some(code.into());
            if matches!(state.phase, Phase::Ready | Phase::Starting) {
                state.phase = Phase::Lost;
                drop(state);
                // The transport may still be live and needed to release owned resources.
                self.stop_diagnostics();
            }
        }
        self.shared.changed.notify_waiters();
    }
    pub(super) async fn call(
        &mut self,
        id: Uuid,
        session: Option<&str>,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.call_kind(
            id, session, method, params, deadline, false, None, None, false,
        )
        .await
    }
    pub(super) async fn call_with_settlement(
        &mut self,
        id: Uuid,
        session: Option<&str>,
        method: &str,
        params: Value,
        deadline: Instant,
        group: Option<&execution_groups::Key>,
    ) -> (Result<Value, Error>, bool) {
        let mut settled = false;
        let result = self
            .call_kind(
                id,
                session,
                method,
                params,
                deadline,
                false,
                Some(&mut settled),
                group,
                false,
            )
            .await;
        (result, settled)
    }
    pub(super) async fn call_read(
        &mut self,
        id: Uuid,
        session: Option<&str>,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.call_kind(
            id, session, method, params, deadline, true, None, None, false,
        )
        .await
    }
    pub(super) async fn call_fixture_payload(
        &mut self,
        id: Uuid,
        session: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.call_kind(
            id,
            Some(session),
            "Runtime.evaluate",
            params,
            deadline,
            false,
            None,
            None,
            true,
        )
        .await
    }
    pub(super) async fn cleanup_call(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.responsive_call(session, method, params, Lane::Control, deadline)
            .await
    }
    /// Pinned Chromium `catchDisallowedErrors`: a route reply's protocol error
    /// is ignored unless it reports an invalid status/phrase or unsafe header,
    /// such as a request whose worker or frame already went away.
    pub(super) async fn route_reply_call(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match self
            .responsive_call_raw(session, method, params, Lane::Control, deadline, false)
            .await?
        {
            Ok(value) => Ok(value),
            Err(super::super::Error::Remote(super::super::RemoteError::Command {
                message,
                ..
            })) if !message.contains("Invalid http status code or phrase")
                && !message.contains("Unsafe header") =>
            {
                Ok(json!({"protocolErrorIgnored":true}))
            }
            Err(error) => Err(protocol_error(error)),
        }
    }
    pub(super) async fn responsive_call(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
        lane: Lane,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.responsive_call_raw(session, method, params, lane, deadline, false)
            .await?
            .map_err(protocol_error)
    }
    /// The call's raw CDP outcome; the outer error covers local session failures.
    pub(super) async fn responsive_call_raw(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
        lane: Lane,
        deadline: Instant,
        fixture_payload: bool,
    ) -> Result<Result<Value, super::super::Error>, Error> {
        let connection = self.connection.clone().ok_or("BACKEND_LOST")?;
        let call = connection.call_sized(session, method, params, lane, deadline, fixture_payload);
        tokio::pin!(call);
        loop {
            self.http_poll().await;
            self.poll_metadata_waits().await;
            tokio::select! {
                ()=tokio::time::sleep(Duration::from_millis(25)), if self.has_metadata_waits() || !self.http_requests.is_empty() || self.binding_count()>0 || !self.interactions.routes.is_empty() || !self.interactions.dialogs.is_empty() || !self.transfers.choosers.is_empty() || !self.inputs.held.is_empty() => {
                    Box::pin(self.poll_har_routes()).await;
                    self.expire_callbacks().await;
                    self.expire_inputs().await;
                },
                result=&mut call=>{self.drain()?;return Ok(result);},
                event=next_event(&mut self.events)=>match event{Some(event)=>{self.apply(event)?;self.service_startup(deadline).await;Box::pin(self.poll_har_routes()).await;},None=>{self.events=None;}},
                response=next_response(&mut self.responses)=>if let Some(response)=response{Box::pin(self.respond(response)).await;},
                execution=next_response(&mut self.executions)=>match execution {
                    Some(command) if command.request.as_ref().is_some_and(execution::metadata) => self.perform_independent(command).await,
                    // Keep the ordinary permit and dispatch this in arrival order after control settles.
                    Some(command) => self.deferred_executions.push_back(command),
                    None => self.executions = None,
                },
            }
        }
    }
    async fn record_call_dispatch(&mut self, id: Uuid, method: &str) -> Result<(), Error> {
        // Repeated read probes belong to one pending DOM condition. Retain
        // its first dispatch witness without consuming the lifecycle journal
        // on every sample; all other operations keep per-call witnesses.
        if self
            .condition_probes
            .get(&id)
            .is_none_or(|probe| !probe.dispatched)
        {
            self.journal
                .record(json!({"operationId":id,"stage":"dispatch_admitted","method":method}))
                .await?;
            if let Some(probe) = self.condition_probes.get_mut(&id) {
                probe.dispatched = true;
            }
        }
        Ok(())
    }

    fn awaited_caller_scope(
        &self,
        id: Uuid,
        session: Option<&str>,
        method: &str,
        params: &Value,
        group: Option<&execution_groups::Key>,
    ) -> Option<(Reference, Option<Reference>, Option<Reference>)> {
        let group_scope = group.and_then(|key| {
            (session == Some(key.0.as_str()))
                .then(|| {
                    self.evaluation
                        .groups
                        .call_scope(self.execution_operation(id), key)
                })
                .flatten()
        });
        ((method == "Runtime.callFunctionOn" && params["awaitPromise"] == true)
            || group_scope.is_some())
        .then(|| session.and_then(|session| self.index.diagnostic_scope(session)))
        .flatten()
        .filter(|(context, _, _)| group_scope.is_none_or(|owner| owner == *context))
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The private RPC path carries settlement and an exact optional temporary-group scope"
    )]
    async fn call_kind(
        &mut self,
        id: Uuid,
        session: Option<&str>,
        method: &str,
        params: Value,
        deadline: Instant,
        read_only: bool,
        mut settlement: Option<&mut bool>,
        group: Option<&execution_groups::Key>,
        fixture_payload: bool,
    ) -> Result<Value, Error> {
        if let Some(settlement) = settlement.as_deref_mut() {
            *settlement = true;
        }
        self.controller_current(id, true)?;
        self.record_call_dispatch(id, method).await?;
        // Creating the isolated probe world cannot change application state.
        let read_only = read_only
            || (method == "Page.createIsolatedWorld" && self.condition_probes.contains_key(&id));
        let connection = self.connection.clone().ok_or("BACKEND_LOST")?;
        let edge_page_setup = self.startup.busy
            && method == "Page.enable"
            && connection.is_edge()
            && session.is_some_and(|session| self.startup_paused(session));
        let caller_scope = self.awaited_caller_scope(id, session, method, &params, group);
        if let Some(settlement) = settlement.as_deref_mut() {
            *settlement = false;
        }
        let call = connection.call_sized(
            session,
            method,
            params,
            Lane::Ordinary,
            deadline,
            fixture_payload,
        );
        tokio::pin!(call);
        let result = loop {
            self.http_poll().await;
            self.poll_metadata_waits().await;
            if self.trace_due() {
                Box::pin(self.poll_trace()).await;
            }
            let shared = self.shared.clone();
            let changed = shared.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if operation_cancelled(&shared.state.lock().expect("Chrome lifecycle"), id) {
                break Err(if read_only {
                    "CANCELLED"
                } else {
                    "OUTCOME_UNKNOWN"
                }
                .into());
            }
            if let Some((context, page, _worker)) = caller_scope
                && (self.closed.get(&context) == Some(&Kind::Context)
                    || page.is_some_and(|page| self.closed.get(&page) == Some(&Kind::Page)))
                && connection.status().loss.is_none()
            {
                // A witnessed page/context disposal ends this caller's scope, not
                // the connection. Its effects remain unknown, and dropping
                // this wait preserves the transport's original late-reply owner.
                let mut error = Error::from("STALE_REFERENCE");
                error.partial = Some(
                    json!({"context":context,"scopeClosed":true,"dispatchCompleted":false,"potentiallyMutating":true,"callerEffectsUnknown":true,"replayed":false}),
                );
                if let Some(page) = page {
                    error.partial.as_mut().expect("Closed scope evidence")["page"] = json!(page);
                }
                break Err(error);
            }
            tokio::select! {
                ()=tokio::time::sleep(Duration::from_millis(25)), if edge_page_setup || self.has_metadata_waits() || !self.http_requests.is_empty() || self.trace_timer_pending() || self.binding_count()>0 || !self.interactions.routes.is_empty() || !self.interactions.dialogs.is_empty() || !self.transfers.choosers.is_empty() || !self.inputs.held.is_empty() || self.shared.control.needs_poll() => {
                    if edge_page_setup && Box::pin(self.retire_edge_browser_ui(id, session.expect("Paused Edge page"), deadline)).await? {
                        break Err("STALE_REFERENCE".into());
                    }
                    if self.trace_due() { Box::pin(self.poll_trace()).await; }
                    self.poll_har_routes().await;
                    self.expire_callbacks().await;
                    self.expire_inputs().await;
                    self.expire_control().await?;
                },
                response=next_response(&mut self.responses)=>if let Some(response)=response {self.respond(response).await;},
                execution=next_execution(&mut self.deferred_executions, &mut self.executions)=>if let Some(execution)=execution {self.perform_independent(execution).await;},
                result = &mut call => {
                    if let Some(settlement)=settlement.as_deref_mut() {
                        use super::super::{Error as NativeError,Dispatch};
                        *settlement=matches!(&result,Ok(_)|Err(NativeError::Remote(_)|NativeError::Busy|NativeError::ScopeClosing|NativeError::RetirementCapacity|NativeError::ScopeClosed {dispatch:Dispatch::NotDispatched}|NativeError::Timeout {dispatch:Dispatch::NotDispatched}|NativeError::Lost {dispatch:Dispatch::NotDispatched,..}));
                    }
                    break result.map_err(|error| {
                    if read_only && matches!(error, super::super::Error::Timeout { .. }) {
                        Error::from("CONDITION_TIMEOUT")
                    } else { protocol_error(error) }
                    });
                },
                event = next_event(&mut self.events) => match event {
                    Some(event) => {self.apply(event)?;self.service_startup(deadline).await;self.poll_har_routes().await;self.poll_transfers().await;self.discard_diagnostic_objects().await;},
                    // The command future owns dispatch knowledge. Stream closure
                    // cannot turn an acknowledged write into a safe-to-retry loss.
                    None => { self.events = None; },
                },
                () = &mut changed => {},
            }
        };
        self.drain()?;
        result
    }
    async fn staged_fixtures(
        &mut self,
        id: Uuid,
        deadline: Instant,
    ) -> Result<media_fixtures::Staged, &'static str> {
        let Some(pending) = self.fixture_stage.as_mut() else {
            return Ok(media_fixtures::Staged::default());
        };
        let result = tokio::select! {
            biased;
            () = wait_startup_cancel(&self.shared, id) => {
                pending.cancel();
                return Err("CANCELLED");
            }
            () = tokio::time::sleep_until(deadline) => {
                pending.cancel();
                return Err("MEDIA_FIXTURE_STAGE_TIMEOUT");
            }
            result = &mut pending.task => result,
        };
        self.fixture_stage = None;
        result.map_err(|_| "MEDIA_FIXTURE_IO")?
    }

    pub(super) async fn launch_after_staging<T>(
        &mut self,
        profile: &Path,
        id: Uuid,
        deadline: Instant,
        launch: impl FnOnce(media_fixtures::Staged) -> Result<T, &'static str>,
    ) -> Result<T, Error> {
        let result = self
            .staged_fixtures(id, deadline)
            .await
            .and_then(|fixtures| {
                // Linearize spawn against cancellation and disconnect admission.
                let state = self.shared.state.lock().expect("Chrome lifecycle");
                check_startup(&state, id, deadline)?;
                launch(fixtures)
            });
        if result.is_err() {
            // The staging task can open files but never recreates the profile.
            // Unlink private copies now and retain any outstanding writer for close.
            Process::remove_profile(profile, deadline).await?;
        }
        result.map_err(Error::from)
    }

    async fn prepare_launch_profile(
        &mut self,
        id: Uuid,
        deadline: Instant,
        fixtures: contract::MediaFixtures,
    ) -> Result<std::path::PathBuf, Error> {
        self.journal
            .record(json!({"operationId":id,"stage":"launch_admitted"}))
            .await?;
        check_startup(
            &self.shared.state.lock().expect("Chrome lifecycle"),
            id,
            deadline,
        )?;
        let profile = Process::profile_path(self.journal.directory());
        crate::artifacts::directory(&profile).map_err(|_| "PROFILE_IO")?;
        self.fixture_stage = media_fixtures::Pending::start(&profile, fixtures);
        Ok(profile)
    }

    pub(super) async fn start(
        &mut self,
        bundle: ChromeBundle,
        mut options: LaunchOptions,
        id: Uuid,
        deadline: Instant,
        disposable_profile: bool,
    ) -> Result<Value, Error> {
        let installed = bundle.installed.clone();
        let managed_apps = cfg!(unix)
            && installed.is_none()
            && self
                .profile
                .as_ref()
                .is_none_or(|profile| profile.is_retained());
        self.managed_apps = managed_apps;
        let regular_context = disposable_profile || self.profile.is_some();
        self.extension_profile = managed_apps && regular_context;
        let scope = crate::browser::processes::LaunchScope::prepare(deadline)
            .await
            .map_err(|_| Error::from("BROWSER_LAUNCH_FAILED"))?;
        self.process = Some(if let Some(profile) = &self.profile {
            let profile = profile.path().to_path_buf();
            let state = self.shared.state.lock().expect("Chrome lifecycle");
            check_startup(&state, id, deadline)?;
            Process::spawn(
                bundle,
                profile,
                options.mode,
                options.proxy.as_ref(),
                Default::default(),
                true,
                scope,
            )?
        } else {
            let profile = self
                .prepare_launch_profile(id, deadline, std::mem::take(&mut options.media_fixtures))
                .await?;
            self.launch_after_staging(&profile, id, deadline, |fixtures| {
                Process::spawn(
                    bundle,
                    profile.clone(),
                    options.mode,
                    options.proxy.as_ref(),
                    fixtures,
                    false,
                    scope,
                )
            })
            .await?
        });
        #[cfg(all(test, feature = "acceptance-browser-lifecycle"))]
        self.shared.native_pid.store(
            self.process
                .as_ref()
                .expect("Owned Chrome")
                .pid()
                .expect("Unreaped Chrome"),
            std::sync::atomic::Ordering::SeqCst,
        );
        let (connection, events) = self
            .process
            .as_mut()
            .expect("Retained Chrome")
            .connect(deadline)
            .await?;
        self.connection = Some(connection.clone());
        self.events = Some(events);
        let (bootstrap, build) = if let Some(installed) = installed {
            let bootstrap = crate::browser::cdp::Bootstrap::initialize_channel(
                &connection,
                &installed.version,
                Some(installed.channel),
                deadline,
            )
            .await
            .map_err(protocol_error)?;
            (
                bootstrap,
                json!({"product":installed.channel.product(),"channel":installed.channel,"version":installed.version}),
            )
        } else {
            let bootstrap = crate::browser::cdp::Bootstrap::initialize(
                &connection,
                crate::browser::preparation::CHROME_VERSION,
                deadline,
            )
            .await
            .map_err(protocol_error)?;
            (bootstrap, crate::browser::preparation::chrome_identity())
        };
        self.drain()?;
        self.shared.state.lock().expect("Chrome lifecycle").identity = Some(json!({
            "browser":bootstrap.identity,"protocolSchemaCommit":bootstrap.schema_commit,
            "build":build,"mode":options.mode,
            "launch":if regular_context {"owned-profile"} else {"owned-isolated"},
            "profile":if disposable_profile { json!({"kind":"disposable"}) } else {
                json!(self.profile.as_ref().map(crate::browser::profiles::Profile::summary))
            }
        }));
        self.start_context(options, id, deadline, true, regular_context)
            .await
    }

    pub(super) async fn attach(
        &mut self,
        attachment: crate::browser::attachment::Attachment,
        options: LaunchOptions,
        id: Uuid,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.attached = true;
        self.journal
            .record(json!({"operationId":id,"stage":"attachment_admitted"}))
            .await?;
        let socket = tokio::select! {
            biased;
            () = wait_startup_cancel(&self.shared, id) => return Err("CANCELLED".into()),
            result = attachment.connect(None, super::super::MAX_MESSAGE_BYTES, deadline) => result?,
        };
        let (connection, events) = Connection::attached(socket);
        self.connection = Some(connection.clone());
        self.events = Some(events);
        let bootstrap = crate::browser::cdp::Bootstrap::initialize_attached(&connection, deadline)
            .await
            .map_err(protocol_error)?;
        self.drain()?;
        self.shared.state.lock().expect("Chrome lifecycle").identity = Some(json!({
            "browser":{"product":bootstrap.identity.product,"protocolVersion":bootstrap.identity.protocol_version},
            "protocolSchemaCommit":bootstrap.schema_commit,"launch":"attached"
        }));
        if let Some(native) = &attachment.context_id {
            let (context, page) = self.adopt_isolated_context(id, native, deadline).await?;
            self.ready(id, context, page)
        } else {
            self.start_context(options, id, deadline, false, false)
                .await
        }
    }

    async fn start_context(
        &mut self,
        options: LaunchOptions,
        id: Uuid,
        deadline: Instant,
        owned_process: bool,
        regular_context: bool,
    ) -> Result<Value, Error> {
        let explicit_proxy = options.context.proxy.is_some();
        let initial_state = self
            .native_load_initial_storage(
                id,
                options.context.storage_state.as_deref().cloned(),
                deadline,
            )
            .await?;
        let context_options = options
            .context
            .effective_for(contract::Engine::Chrome, options.proxy.as_ref())?;
        let (context, context_options, initial_page) = if regular_context {
            let (context, page) = self.open_profile_context(id, deadline).await?;
            self.persistent_context = Some(context);
            (context, context_options, Some(page))
        } else {
            let (context, options) = self
                .context_create_configured(id, context_options, explicit_proxy, deadline)
                .await?;
            (context, options, None)
        };
        self.context_layout.insert(context, context_options.clone());
        self.http_contexts
            .context_credentials_initial(context, context_options.http_credentials.as_ref());
        if options.context.has_touch {
            self.touch_contexts.insert(context);
        }
        self.configure_downloads(id, context, options.context.accept_downloads, deadline)
            .await?;
        self.initialize_context_emulation(id, context, &context_options, deadline)
            .await?;
        if let Some(record) = &options.context.record_har {
            self.har_capture_start(
                id,
                context,
                None,
                record.options()?,
                None,
                record.filter(&options.context),
                deadline,
            )
            .await?;
        }
        // Applied before the launch creates its first page.
        if let Some(state) = initial_state {
            self.native_initial_storage(id, context, state, deadline)
                .await?;
        }
        if owned_process {
            self.enable_startup(id, deadline).await?;
        }
        let page = if let Some(page) = initial_page {
            self.startup_attachment(id, page, deadline).await?;
            page
        } else {
            self.page_create(id, context, deadline).await?
        };
        self.reconcile(id, deadline).await?;
        self.ready(id, context, Some(page))
    }

    fn ready(&self, id: Uuid, context: Reference, page: Option<Reference>) -> Result<Value, Error> {
        {
            let mut state = self.shared.state.lock().expect("Chrome lifecycle");
            if operation_cancelled(&state, id) {
                return Err("CANCELLED".into());
            }
            state.selected_context = Some(context);
            state.selected_page = page;
            state.phase = Phase::Ready;
        }
        self.publish();
        Ok(json!({"context":context,"page":page}))
    }

    pub(super) async fn adopt_isolated_context(
        &mut self,
        id: Uuid,
        native: &str,
        deadline: Instant,
    ) -> Result<(Reference, Option<Reference>), Error> {
        let known = self
            .call(id, None, "Target.getBrowserContexts", json!({}), deadline)
            .await?;
        if !known["browserContextIds"]
            .as_array()
            .ok_or("BACKEND_PROTOCOL")?
            .iter()
            .any(|context| context.as_str() == Some(native))
        {
            return Err("ATTACHMENT_CONTEXT_UNAVAILABLE".into());
        }
        let lease = crate::lease::ControlLease::acquire_browser(&format!(
            "browser-adopted-context:{native}"
        ))
        .map_err(|error| {
            if error.code == "TARGET_BUSY" {
                "ATTACHMENT_CONTEXT_BUSY"
            } else {
                "ATTACHMENT_CONTEXT_LEASE_FAILED"
            }
        })?;
        let context = self
            .index
            .register_context(native)
            .map_err(resource_error)?;
        self.adoption = Some((context, lease));
        let targets = self
            .call(id, None, "Target.getTargets", json!({}), deadline)
            .await?;
        for target in targets["targetInfos"]
            .as_array()
            .ok_or("BACKEND_PROTOCOL")?
        {
            if target["browserContextId"].as_str() == Some(native) {
                self.index
                    .apply(Event {
                        method: "Target.targetInfoChanged".into(),
                        session_id: None,
                        params: json!({"targetInfo":target}),
                    })
                    .map_err(resource_error)?;
            }
        }
        self.reconcile(id, deadline).await?;
        let pages = self.index.list(Kind::Page);
        Ok((context, (pages.len() == 1).then(|| pages[0].0)))
    }
    async fn open_profile_context(
        &mut self,
        id: Uuid,
        deadline: Instant,
    ) -> Result<(Reference, Reference), Error> {
        // Without browserContextId CDP creates a page in the owned process's
        // default context. Use its identity instead of racing restored tabs.
        let created = self
            .call(
                id,
                None,
                "Target.createTarget",
                json!({"url":"about:blank"}),
                deadline,
            )
            .await?;
        let target = created["targetId"].as_str().ok_or("BACKEND_PROTOCOL")?;
        let info = self
            .call(
                id,
                None,
                "Target.getTargetInfo",
                json!({"targetId":target}),
                deadline,
            )
            .await?;
        let target_info = &info["targetInfo"];
        if target_info["targetId"] != target || target_info["type"] != "page" {
            return Err("PROFILE_CONTEXT_UNAVAILABLE".into());
        }
        let native = target_info["browserContextId"]
            .as_str()
            .ok_or("PROFILE_CONTEXT_UNAVAILABLE")?;
        let context = self
            .index
            .register_context(native)
            .map_err(resource_error)?;
        self.call(
            id,
            None,
            "Browser.setDownloadBehavior",
            json!({"behavior":"deny","browserContextId":native}),
            deadline,
        )
        .await?;
        self.index
            .apply(Event {
                method: "Target.targetInfoChanged".into(),
                session_id: None,
                params: info,
            })
            .map_err(resource_error)?;
        let page = self.index.target_reference(target).ok_or("PAGE_CLOSED")?;
        Ok((context, page))
    }
    async fn admit_wait(&mut self, mut command: Command) {
        // Waits bypass `execute_one`, so resolve their URL conditions here.
        if let Some(request @ (Request::DiagnosticsWait { .. } | Request::PageWait { .. })) =
            &mut command.request
            && request.uses_base()
        {
            request.resolve_base(&mut |scope| {
                let context = if self.context_layout.contains_key(&scope) {
                    scope
                } else {
                    self.index
                        .list(Kind::Page)
                        .into_iter()
                        .find_map(|(page, context)| (page == scope).then_some(context))??
                };
                self.context_layout.get(&context)?.base()
            });
            if let Err(error) = super::supported(request) {
                self.finish(command.id, Err(error)).await;
                return;
            }
        }
        if self.waits.len() >= 32 {
            self.finish(command.id, Err("OPERATION_CAPACITY".into()))
                .await;
            return;
        }
        if let Err(error) = self
            .journal
            .record(json!({"operationId":command.id,"outcome":"running","settled":false}))
            .await
        {
            self.finish(command.id, Err(error.into())).await;
            return;
        }
        if let Some(operation) = self
            .shared
            .state
            .lock()
            .expect("Chrome state")
            .operations
            .get_mut(&command.id)
        {
            operation.outcome = Outcome::Running;
        }
        let metadata = !matches!(command.request, Some(Request::PageWait { .. }));
        if !metadata {
            self.condition_probes
                .insert(command.id, conditions::Probe::default());
        }
        self.waits.insert(command.id, command);
        if metadata {
            self.poll_metadata_waits().await;
        } else {
            self.poll_waits().await;
        }
    }

    pub(super) async fn perform(&mut self, mut command: Command) {
        if matches!(
            command.request.as_ref(),
            Some(Request::HttpFetch { .. } | Request::HttpFetchRequest { .. })
        ) {
            self.http_begin(command).await;
            return;
        }
        // A normal command releases its queued slot when dequeued. Independent
        // execution retains that same slot until settlement, bounding recursive
        // caller execution while preserving the reserved control/callback lane.
        let _execution_slot = if command.request.as_ref().is_some_and(execution::independent) {
            command.queue_slot.take()
        } else {
            drop(command.queue_slot.take());
            None
        };
        if matches!(
            command.request,
            Some(
                Request::PageWait { .. }
                    | Request::EventsWait { .. }
                    | Request::DiagnosticsWait { .. }
            )
        ) {
            self.admit_wait(command).await;
            return;
        }
        let id = command.id;
        let metadata_read = matches!(
            command.request,
            Some(Request::EventsRead { .. } | Request::DiagnosticsRead { .. })
        );
        let result = match command.request {
            Some(request) => self.perform_request(id, request, command.deadline).await,
            None => self.close_with_receipt(command.deadline).await,
        };
        if !metadata_read {
            self.discard_diagnostic_objects().await;
            self.release_evaluation().await;
            self.release_webmcp_objects().await;
        }
        if result.as_ref().err().is_some_and(|error| {
            (error.code == "OUTCOME_UNKNOWN"
                && !self.owns_uncertain_execution(id)
                && !self.pwas.owns_pending(id)
                && !self.webmcp_debug.owns_pending(id))
                || matches!(
                    error.code,
                    "BACKEND_PROTOCOL"
                        | "BACKEND_LOST"
                        | "CAPTURE_CLEANUP_PENDING"
                        | "AUDIT_CLEANUP_PENDING"
                )
        }) {
            self.lose("BACKEND_UNCERTAIN");
        }
        self.publish();
        self.finish(id, result).await;
    }
    async fn perform_request(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let local_result = matches!(
            request,
            Request::EventsRead { .. }
                | Request::DiagnosticsRead { .. }
                | Request::HeapCapture { .. }
                | Request::HeapImport { .. }
                | Request::PerformanceLoad { .. }
                | Request::PerformanceQuery { .. }
                | Request::PerformanceRelease { .. }
                | Request::HeapStop { .. }
                | Request::HeapLoad { .. }
                | Request::HeapQuery { .. }
                | Request::HeapCompare { .. }
                | Request::HeapRelease { .. }
                | Request::ProfileQuery { .. }
                | Request::DeveloperEvidenceList {}
                | Request::DeveloperEvidenceFinalize { .. }
                | Request::DeveloperEvidenceRemove { .. }
                | Request::HttpContextList {}
                | Request::HttpRequestInputRemove { .. }
                | Request::HttpRequestInputsSet { enabled: false, .. }
                | Request::HttpContextRemove { .. }
                | Request::HttpContextDispose { .. }
                | Request::HttpContextRead { .. }
                | Request::HttpResponseBodyRead { .. }
                | Request::HttpResponseDispose { .. }
                | Request::HttpContextCookiesClear { .. }
                | Request::HttpContextCookiesAdd { .. }
                | Request::HttpContextCookiesRead { .. }
                | Request::HttpContextStorageState { .. }
                | Request::HttpEvidenceFinalize { .. }
                | Request::HttpResponseFinalize { .. }
                | Request::HttpResponseDiscard { .. }
                | Request::HttpResponseRead { .. }
                | Request::HttpResponseRemove { .. }
                | Request::HttpEvidenceList {}
                | Request::HttpEvidenceRead { .. }
                | Request::HttpEvidenceRemove { .. }
                | Request::NetworkEvidenceList {}
                | Request::NetworkEvidenceRemove { .. }
                | Request::HarRegister { .. }
                | Request::HarList {}
                | Request::HarFinalize { .. }
                | Request::HarRemove { .. }
        );
        self.controller_current(id, true)?;
        if Instant::now() >= deadline {
            Err("NOT_DISPATCHED".into())
        } else if self
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .operations
            .get(&id)
            .is_some_and(|o| o.cancel_requested)
        {
            Err("CANCELLED".into())
        } else if self.shared.state.lock().expect("Chrome lifecycle").phase != Phase::Ready {
            Err("SESSION_NOT_READY".into())
        } else {
            match self
                .journal
                .record(json!({"operationId":id,"outcome":"running","settled":false}))
                .await
            {
                Ok(()) => {
                    if let Some(operation) = self
                        .shared
                        .state
                        .lock()
                        .expect("Chrome lifecycle")
                        .operations
                        .get_mut(&id)
                    {
                        operation.outcome = Outcome::Running;
                    }
                    self.shared.changed.notify_waiters();
                    let result = self.execute(id, request, deadline).await;
                    match result {
                        Ok(value) if local_result => Ok(value),
                        Ok(value) => self.reconcile(id, deadline).await.map(|()| value),
                        Err(error) => Err(error),
                    }
                }
                Err(code) => Err(code.into()),
            }
        }
    }

    async fn close_with_receipt(&mut self, deadline: Instant) -> Result<Value, Error> {
        let extensions = self
            .profile
            .as_ref()
            .filter(|profile| profile.is_retained())
            .map(|_| self.extensions.possible_registrations())
            .unwrap_or_default();
        self.close(deadline).await?;
        let mut receipt = json!({"closed":true});
        if !self.pwas.cleanup_unverified.is_empty() {
            receipt["cleanup"]["pwaCleanupUnverified"] = json!(self.pwas.cleanup_unverified);
        }
        if let Some(code) = self.pwas.cleanup_journal_failure {
            receipt["cleanup"]["journalFailure"] = json!(code);
        }
        if !extensions.is_empty() {
            receipt["cleanup"]["extensionCleanupUnverified"] = json!(extensions);
        }
        Ok(receipt)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "One connection teardown releases and resets every owned native state in order"
    )]
    pub(super) async fn close(&mut self, deadline: Instant) -> Result<(), Error> {
        if self
            .connection
            .as_ref()
            .is_some_and(|connection| connection.status().open)
        {
            // A private profile can still create OS app registrations. Give
            // cleanup a bounded share of the deadline, then reap the browser.
            let app_deadline = Instant::now()
                + deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(5))
                    / 2;
            let _ = self.cleanup_pwas(app_deadline).await;
        }
        if let Some(pending) = self.fixture_stage.as_mut() {
            pending.cancel();
            if tokio::time::timeout_at(deadline, &mut pending.task)
                .await
                .is_err()
            {
                self.shared.state.lock().expect("Chrome lifecycle").phase = Phase::CleanupPending;
                return Err("CLEANUP_PENDING".into());
            }
            self.fixture_stage = None;
        }
        let certificate_cleanup = self.certificate_tunnels.close(None, deadline).await;
        self.http_contexts.shutdown_work();
        self.http_cancel_owned().await;
        self.close_native_control().await;
        self.close_debuggers(deadline).await;
        self.close_profiles(deadline).await;
        self.close_trace(deadline).await;
        let _developer_cleanup = self.close_developers(deadline).await;
        self.websockets.clear();
        self.diagnostics.close();
        self.request_inputs.clear();
        let media_cleanup = self.close_media(deadline).await;
        // Stop the private browser as a whole: every pending/late allocation
        // belongs to this exclusive process. Never replay creation for cleanup.
        let result = if let Some(process) = self.process.as_mut() {
            process.shutdown(self.connection.as_ref(), deadline).await
        } else {
            if let Some(connection) = &self.connection {
                connection.close(deadline).await.map_err(protocol_error)?;
            }
            Process::remove_profile(&Process::profile_path(self.journal.directory()), deadline)
                .await
        };
        if result.is_ok() {
            // An empty Windows tree no longer needs its process/job handles.
            // Release those kernel references before acknowledging disconnect
            // so a preserved profile can be moved while this manager stays live.
            #[cfg(windows)]
            {
                self.process = None;
            }
            self.profile = None;
        }
        if result.is_ok() {
            self.pwas.record_unverified_cleanup(&self.journal).await;
        }
        let result = if result.is_ok() {
            let mut cleanup = Ok(());
            for path in self.transfers.download_contexts.values() {
                match tokio::fs::remove_dir_all(path).await {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => cleanup = Err("CLEANUP_PENDING"),
                }
            }
            cleanup
        } else {
            result
        };
        let result = if result.is_ok() {
            self.finalize_profiles_after_exit()
                .and_then(|()| self.seal_trace_after_exit())
                .and_then(|()| {
                    self.developers
                        .evidence
                        .finalize_all(self.journal.directory())
                })
                .and_then(|()| self.close_http())
                .and_then(|()| self.har_archives.finalize_all(self.journal.directory()))
                .and_then(|()| self.seal_heap().map(|_| ()))
                .map_err(|_| "CLEANUP_PENDING")
        } else {
            result
        };
        if result.is_ok() {
            self.clear_developers();
            self.http_state = Default::default();
            self.clear_profiles();
            self.trace = None;
            self.trace_tabs = Default::default();
            self.heap = Default::default();
            self.performance = Default::default();
            self.clear_debuggers();
        }
        let result = if result.is_ok() && (media_cleanup.is_err() || certificate_cleanup.is_err()) {
            Err("CLEANUP_PENDING")
        } else {
            result
        };
        if result.is_ok() {
            self.connection = None;
            self.context_layout.clear();
            self.managed_apps = false;
            self.extension_profile = false;
            self.extensions = Default::default();
            self.persistent_context = None;
            self.adoption = None;
            self.page_headers.clear();
            self.emulation.clear();
            self.evaluation = Default::default();
            self.inputs = Default::default();
            self.pointer_positions.clear();
            self.interactions.dialogs.clear();
            self.interactions.routes.clear();
            self.events = None;
            self.configured.clear();
            self.startup = Default::default();
            self.evaluation.scripts = Default::default();
            self.closed.clear();
        }
        let mut state = self.shared.state.lock().expect("Chrome lifecycle");
        state.phase = if result.is_ok() {
            Phase::Closed
        } else {
            Phase::CleanupPending
        };
        if result.is_ok() {
            self.lease = None;
            state.selected_context = None;
            state.selected_page = None;
            state.launch_proxy = None;
        }
        if result.is_ok() {
            self.index = Index::new(state.connection);
        }
        result.map_err(Error::from)
    }
}
