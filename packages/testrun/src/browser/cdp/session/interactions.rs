//! Dialog ownership and typed page-event barriers on the responsive control lane.
use super::*;
use crate::browser::activity::{Event as Activity, History};

pub(super) struct Dialog {
    pub page: Reference,
    session: String,
    expires: Instant,
    summary: Value,
}
#[derive(Default)]
pub(super) struct Interactions {
    pub routes: BTreeMap<Reference, super::routing::Route>,
    pub dialogs: BTreeMap<Reference, Dialog>,
    pub history: History,
    pub drags: BTreeMap<String, Value>,
}
impl Worker {
    pub(super) fn interaction_event(&mut self, event: &Event) -> Result<(), Error> {
        let Some(session) = event.session_id.as_deref() else {
            return Ok(());
        };
        let Some((target, kind, _, _)) = self
            .index
            .targets()
            .into_iter()
            .find(|(_, _, _, route)| route.as_deref() == Some(session))
        else {
            return Ok(());
        };
        let page = if kind == Kind::Page {
            target
        } else {
            let Ok(page) = self.index.owning_page(target) else {
                return Ok(());
            };
            page
        };
        match event.method.as_str() {
            "Input.dragIntercepted" => {
                if serde_json::to_vec(&event.params["data"])
                    .map_err(|_| Error::from("BACKEND_PROTOCOL"))?
                    .len()
                    > 65536
                {
                    return Err("DRAG_DATA_BOUNDS".into());
                }
                self.interactions
                    .drags
                    .insert(session.into(), event.params["data"].clone());
            }
            "Page.javascriptDialogOpening" => {
                if self.interactions.dialogs.len()
                    + self.interactions.routes.len()
                    + self.binding_count()
                    >= 64
                {
                    return Err("CALLBACK_CAPACITY".into());
                }
                let reference = Reference {
                    connection_id: page.connection_id,
                    id: Uuid::new_v4(),
                    generation: Uuid::new_v4(),
                };
                self.interactions.dialogs.insert(
                    reference,
                    Dialog {
                        page,
                        session: session.into(),
                        expires: Instant::now() + Duration::from_secs(30),
                        summary:json!({"type":event.params["type"],"message":event.params["message"].as_str().unwrap_or_default().chars().take(1024).collect::<String>(),"defaultPrompt":event.params["defaultPrompt"].as_str().unwrap_or_default().chars().take(1024).collect::<String>()}),
                    },
                );
                self.record_activity(page, Activity::Dialog, reference);
            }
            "Page.javascriptDialogClosed" => self
                .interactions
                .dialogs
                .retain(|_, dialog| dialog.page != page),
            "Page.frameNavigated" => self.record_activity(page, Activity::FrameNavigated, page),
            // The SDK reports these for the page's main frame only, so an
            // out-of-process child frame's own session never produces them.
            "Page.loadEventFired" if kind == Kind::Page => {
                self.record_activity(page, Activity::Load, page);
            }
            "Page.domContentEventFired" if kind == Kind::Page => {
                self.record_activity(page, Activity::DomContentLoaded, page);
            }
            "Inspector.targetCrashed" => self.record_activity(page, Activity::PageCrashed, page),
            _ => {}
        }
        Ok(())
    }
    pub(super) fn record_activity(
        &mut self,
        page: Reference,
        event: Activity,
        resource: Reference,
    ) {
        if let Some(context) = self.index.retained_page_context(page) {
            self.interactions
                .history
                .push(context, page, event, resource);
        }
    }
    #[expect(
        clippy::too_many_lines,
        reason = "The responsive lane dispatches each admitted callback or observation and settles its original command once"
    )]
    pub(super) async fn respond(&mut self, command: Command) {
        let result = match command.request {
            Some(request @ Request::HandleDispose { handle }) => match self.observation_ready(command.id, command.deadline) {
                Ok(()) if self.frameworks.require(handle).is_ok() => Err("FRAMEWORK_RELEASE_REQUIRED".into()),
                Ok(()) => Box::pin(self.execute_evaluation(command.id, request, command.deadline)).await,
                Err(error) => Err(error),
            },
            Some(Request::NetworkRoute {
                scope,
                action: crate::browser::routing::HandlerAction::List {},
            }) => match self.observation_ready(command.id, command.deadline) {
                Ok(()) => {
                    Box::pin(self.network_route(
                        command.id,
                        scope,
                        crate::browser::routing::HandlerAction::List {},
                        command.deadline,
                    ))
                    .await
                }
                Err(error) => Err(error),
            },
            Some(request @ (Request::WebSocketRouteSend { .. }|Request::WebSocketRouteHandlers { .. }|Request::WebSocketRouteClose { .. })) => match self
                .observation_ready(command.id, command.deadline)
            {
                Ok(()) => Box::pin(self.socket_send(command.id, request, command.deadline)).await,
                Err(error) => Err(error),
            },
            Some(Request::HarCaptureStatus { context }) => self
                .observation_ready(command.id, command.deadline)
                .and_then(|()| self.har_archives.capture_status(context)),
            Some(Request::HarList {}) => self
                .observation_ready(command.id, command.deadline)
                .map(|()| self.har_list()),
            Some(Request::HarFinalize { archive_id }) => self
                .observation_ready(command.id, command.deadline)
                .and_then(|()| {
                    self.har_archives
                        .finalize(self.journal.directory(), archive_id)
                }),
            Some(Request::HarRemove { archive_id }) => self
                .observation_ready(command.id, command.deadline)
                .and_then(|()| self.har_remove(archive_id)),
            Some(
                request @ (Request::HttpContextList {}
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
                // The paused request belongs to an operation already holding the command lane.
                | Request::RouteFetch { .. }),
            ) => match self.http_running(command.id, command.deadline).await {
                Ok(()) => {
                    Box::pin(self.http_operation(command.id, request, command.deadline)).await
                }
                Err(error) => Err(error),
            },
            Some(
                request @ (Request::BindingRead { .. }
                | Request::BindingList { .. }
                | Request::BindingRemove { .. }),
            ) => match self.observation_ready(command.id, command.deadline) {
                Ok(()) => {
                    Box::pin(self.binding_operation(command.id, request, command.deadline)).await
                }
                Err(error) => Err(error),
            },
            Some(
                request @ (Request::DiagnosticsArgumentsList { .. }
                | Request::DiagnosticsArgumentsRemove { .. }
                | Request::DiagnosticsArgumentsSet { enabled: false, .. }),
            ) => {
                Box::pin(self.diagnostic_arguments_operation(command.id, request, command.deadline))
                    .await
            }
            Some(Request::WebMcpDebug { scope, action }) if action.responsive() => {
                Box::pin(self.webmcp_debug_operation(command.id, scope, action, command.deadline))
                    .await
            }
            Some(Request::WebMcp { scope, action }) if action.responsive() => {
                Box::pin(self.webmcp_page_operation(command.id, scope, action, command.deadline))
                    .await
            }
            Some(Request::PageDebugger { page, action }) => {
                Box::pin(self.page_debugger(command.id, page, action, command.deadline)).await
            }
            Some(Request::TraceStatus {}) => Ok(self.trace_status()),
            Some(Request::TraceStop { file_path, analysis_developer: None, analysis_page: None }) => self.trace_stop_to(file_path, command.deadline).await,
            Some(Request::ProfileStatus { page }) => self.profile_status(page),
            Some(Request::ProfileStop { page }) => self.profile_stop(page, command.deadline).await,
            Some(Request::ProfileTakeCoverage { page }) => {
                Box::pin(self.profile_take_coverage(command.id, page, command.deadline)).await
            }
            Some(Request::HeapStop { page }) => self.heap_stop(page, command.deadline).await,
            Some(Request::DeveloperStatus { developer }) => self.developer_status(developer),
            Some(Request::DeveloperClose { developer }) => {
                self.developer_close(developer, command.deadline).await
            }
            Some(Request::InputRelease {}) => self.release_inputs().await,
            Some(Request::ChooserResolve { chooser, fixtures }) => {
                self.resolve_chooser(command.id, chooser, fixtures, command.deadline)
                    .await
            }
            Some(Request::DownloadCancel { download }) => {
                self.cancel_download(download, command.deadline).await
            }
            Some(Request::CallbackResolve { callback, response }) => {
                self.resolve_callback(command.id, callback, response, command.deadline)
                    .await
            }
            _ => Err("CAPABILITY_UNAVAILABLE".into()),
        };
        if result
            .as_ref()
            .is_err_and(|error| error.code == "OUTCOME_UNKNOWN")
        {
            self.lose("OUTCOME_UNKNOWN");
        }
        self.publish();
        self.finish(command.id, result).await;
    }
    pub(super) async fn resolve_callback(
        &mut self,
        id: Uuid,
        reference: Reference,
        response: contract::CallbackResponse,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if matches!(
            response,
            contract::CallbackResponse::BindingResolve { .. }
                | contract::CallbackResponse::BindingReject {}
                | contract::CallbackResponse::BindingResolveValue { .. }
                | contract::CallbackResponse::BindingRejectValue { .. }
        ) {
            self.observation_ready(id, deadline)?;
            self.journal.record(json!({"operationId":id,"stage":"binding_response_admitted","callback":reference})).await?;
            return self.resolve_binding(reference, response, deadline).await;
        }
        if matches!(
            response,
            contract::CallbackResponse::RouteAbort { .. }
                | contract::CallbackResponse::RouteContinue {}
                | contract::CallbackResponse::RouteFallback {}
                | contract::CallbackResponse::RouteContinueWith { .. }
                | contract::CallbackResponse::RouteFallbackWith { .. }
                | contract::CallbackResponse::RouteFulfill { .. }
                | contract::CallbackResponse::RouteFulfillBinary { .. }
                | contract::CallbackResponse::RouteFulfillEvidence { .. }
                | contract::CallbackResponse::RouteFulfillHttpEvidence { .. }
                | contract::CallbackResponse::RouteHar { .. }
                | contract::CallbackResponse::AuthCancel {}
                | contract::CallbackResponse::AuthDefault {}
                | contract::CallbackResponse::AuthCredentials { .. }
        ) {
            self.observation_ready(id, deadline)?;
            self.journal.record(json!({"operationId":id,"stage":"route_response_admitted","callback":reference})).await?;
            return self.resolve_route(reference, response, deadline).await;
        }
        if reference.connection_id != self.shared.state.lock().expect("Chrome state").connection {
            return Err("FOREIGN_REFERENCE".into());
        }
        self.observation_ready(id, deadline)?;
        let contract::CallbackResponse::Dialog {
            accept,
            prompt_text,
        } = response
        else {
            return Err("INVALID_CALLBACK_RESPONSE".into());
        };
        let dialog = self
            .interactions
            .dialogs
            .remove(&reference)
            .ok_or("STALE_REFERENCE")?;
        if Instant::now() >= dialog.expires {
            return Err("CALLBACK_EXPIRED".into());
        }
        self.index
            .route(dialog.page, Some(Kind::Page))
            .map_err(resource_error)?;
        self.journal.record(json!({"operationId":id,"stage":"dispatch_admitted","method":"Page.handleJavaScriptDialog"})).await?;
        let mut params = json!({"accept":accept});
        if let Some(text) = prompt_text {
            params["promptText"] = json!(text);
        }
        self.connection
            .as_ref()
            .ok_or("BACKEND_LOST")?
            .call(
                Some(&dialog.session),
                "Page.handleJavaScriptDialog",
                params,
                Lane::Control,
                deadline,
            )
            .await
            .map_err(protocol_error)?;
        Ok(json!({"callback":reference,"resolved":true,"replayed":false}))
    }
    pub(super) async fn expire_callbacks(&mut self) {
        Box::pin(self.expire_bindings()).await;
        self.expire_routes().await;
        self.transfers.choosers.retain(|_, chooser| {
            chooser.expires > Instant::now()
                && self.index.route(chooser.frame, Some(Kind::Frame)).is_ok()
        });
        let expired = self
            .interactions
            .dialogs
            .iter()
            .filter(|(_, dialog)| dialog.expires <= Instant::now())
            .map(|(reference, _)| *reference)
            .collect::<Vec<_>>();
        for reference in expired {
            let dialog = self
                .interactions
                .dialogs
                .remove(&reference)
                .expect("Expired dialog");
            if let Some(connection) = &self.connection {
                let _ = connection
                    .call(
                        Some(&dialog.session),
                        "Page.handleJavaScriptDialog",
                        json!({"accept":false}),
                        Lane::Control,
                        Instant::now() + Duration::from_secs(2),
                    )
                    .await;
            }
        }
    }
    pub(super) fn events_read(
        &self,
        context: Reference,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Value, Error> {
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        self.interactions
            .history
            .page(context, cursor, limit)
            .map_err(Error::from)
    }
    pub(super) fn probe_events(
        &self,
        id: Uuid,
        context: Reference,
        after: u64,
        conditions: &[crate::browser::activity::Condition],
    ) -> Result<Option<Value>, Error> {
        let cursor = self
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .operations
            .get(&id)
            .and_then(|operation| operation.result.as_ref())
            .filter(|result| result["waiting"] == true)
            .and_then(|result| result["nextCursor"].as_u64())
            .unwrap_or(after);
        let result = (|| {
            self.index
                .native(context, Kind::Context)
                .map_err(resource_error)?;
            for condition in conditions {
                if let Some(page) = condition.page {
                    self.transfer_scope(page)?;
                    if let Some((_, parent)) = self
                        .index
                        .list(Kind::Page)
                        .into_iter()
                        .find(|(reference, _)| *reference == page)
                    {
                        if parent != Some(context) {
                            return Err("FOREIGN_PAGE".into());
                        }
                    } else if !self.interactions.history.knows_page(context, page) {
                        return Err("STALE_REFERENCE".into());
                    }
                }
            }
            let read = self
                .interactions
                .history
                .wait(context, after, cursor, conditions)?;
            if read["matched"] == true {
                return Ok(Some(read));
            }
            if let Some(operation) = self
                .shared
                .state
                .lock()
                .expect("Chrome lifecycle")
                .operations
                .get_mut(&id)
            {
                operation.result = Some(read);
            }
            Ok(None)
        })();
        result
            .map_err(|error| crate::browser::activity::wait_failure(context, after, cursor, error))
    }
    pub(super) fn event_wait_failure(
        &self,
        id: Uuid,
        request: &Request,
        mut error: Error,
    ) -> Error {
        let Request::EventsWait { context, after, .. } = request else {
            return error;
        };
        if error.code == "CONDITION_TIMEOUT" {
            error.code = "WAIT_EXPIRED";
        }
        let cursor = self
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .operations
            .get(&id)
            .and_then(|operation| operation.result.as_ref())
            .and_then(|result| result["nextCursor"].as_u64())
            .unwrap_or(*after);
        crate::browser::activity::wait_failure(*context, *after, cursor, error)
    }
}

impl Interactions {
    pub(super) fn snapshot(&self) -> Value {
        let mut result = json!({"callbacks":self.dialogs.iter().map(|(reference,dialog)|json!({"reference":reference,"page":dialog.page,"kind":"dialog","details":dialog.summary,"expiresInMs":dialog.expires.saturating_duration_since(Instant::now()).as_millis()})).chain(self.routes.iter().filter(|(_,route)|!route.auto_pass).map(|(reference,route)|route.snapshot(*reference))).collect::<Vec<_>>()});
        result["routeContinuationsPending"] =
            json!(self.routes.values().filter(|route| route.auto_pass).count());
        result
    }
}
