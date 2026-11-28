//! Browser composition of the shared input authority and authenticated host channel.
use super::*;
use crate::control::{Credential, Owner};

struct PendingControl(Arc<Host>);
impl Drop for PendingControl {
    fn drop(&mut self) {
        let mut outcome = self
            .0
            .control_outcome
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if outcome["state"] == "running" {
            outcome["state"] = json!("failed");
            outcome["code"] = json!("CONTROL_OWNER_FAILED");
        }
        self.0.control_pending.store(false, Ordering::SeqCst);
        self.0.changed.notify_waiters();
    }
}

pub(super) fn responsive(request: &Request) -> bool {
    matches!(
        request,
        Request::CallbackResolve { .. }
            | Request::WebSocketRouteSend { .. }
            | Request::WebSocketRouteHandlers { .. }
            | Request::WebSocketRouteClose { .. }
            | Request::DiagnosticsArgumentsList { .. }
            | Request::DiagnosticsArgumentsRemove { .. }
            | Request::DiagnosticsArgumentsSet { enabled: false, .. }
            | Request::HttpContextList {}
            | Request::HttpRequestInputRemove { .. }
            | Request::HttpRequestInputsSet { enabled: false, .. }
            | Request::HarList {}
            | Request::HarCaptureStatus { .. }
            | Request::HarFinalize { .. }
            | Request::HarRemove { .. }
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
            | Request::BindingRemove { .. }
            | Request::ContextClose { .. }
            | Request::PageClose { .. }
            | Request::HandleDispose { .. }
            | Request::ChooserResolve { .. }
            | Request::DownloadCancel { .. }
            | Request::DownloadDelete { .. }
            | Request::PreviewStop { .. }
            | Request::RecordingStop { .. }
            | Request::TraceStop { .. }
            | Request::TraceChunkStop {}
            | Request::TraceGroupEnd {}
            | Request::ContextDebugger { .. }
            | Request::InputRelease {}
    )
}
pub(super) fn independent(request: &Request, responsive: bool) -> bool {
    (responsive && !matches!(request, Request::InputRelease {}))
        || crate::browser::socket_routes::independent_batch(request)
        || matches!(request, Request::Batch { requests }
            if !requests.is_empty() && requests.iter().all(|request| matches!(request,
                Request::CallbackResolve { .. } | Request::ContextClose { .. } | Request::PageClose { .. } | Request::NetworkRoute { .. })))
        // Shared debugger controls must reach the caller holding the lane.
        // They retain normal batch admission and ordered per-action fences.
        || matches!(request, Request::Batch { requests }
            if !requests.is_empty() && requests.iter().all(|request| matches!(request,
                Request::ContextDebugger { .. })))
        // Caller evaluations can release conditions awaited by an ordinary
        // action, including one paused by the SDK automation debugger. They
        // keep normal capacity and controller admission, not response slots.
        || caller_evaluation(request)
        || matches!(request, Request::Batch { requests }
            if !requests.is_empty() && requests.iter().all(|request| caller_evaluation(request)
                || matches!(request, Request::PageRead { read, .. }
                    if !matches!(read, crate::browser::page::Read::Geometry))))
        || matches!(
            request,
            Request::ContextList {}
                | Request::HttpFetch { .. }
                | Request::HttpFetchRequest { .. }
                // The paused request belongs to an operation already holding the lane.
                | Request::RouteFetch { .. }
                | Request::NetworkRoute {
                    action: crate::browser::routing::HandlerAction::List {},
                    ..
                }
                | Request::BindingRead { .. }
                | Request::BindingList { .. }
                | Request::BindingRemove { .. }
                | Request::PageList {}
                | Request::PageRead { .. }
                | Request::PageStyles { .. }
                | Request::PageWait { .. }
                | Request::FixtureList {}
                | Request::FixtureVerify { .. }
                | Request::DownloadList {}
                | Request::DownloadStatus { .. }
                | Request::DiagnosticsRead { .. }
                | Request::DiagnosticsWait { .. }
                | Request::EventsRead { .. }
                | Request::EventsWait { .. }
                | Request::PreviewStatus { .. }
                | Request::RecordingStatus { .. }
                | Request::TraceStatus {}
                | Request::DeveloperEvidenceList {}
                | Request::DeveloperEvidenceFinalize { .. }
                | Request::DeveloperEvidenceRemove { .. }
                | Request::WorkerList { .. }
                | Request::ContextWorkerList { .. }
        )
}

fn caller_evaluation(request: &Request) -> bool {
    matches!(
        request,
        Request::Evaluate { .. }
            | Request::SelectorEvaluate { .. }
            | Request::HandleRead { .. }
            | Request::HandleProperty { .. }
            | Request::HandleProperties { .. }
            | Request::HandleOwnerFrame { .. }
            | Request::HandleContentFrame { .. }
            | Request::FrameElement { .. }
            | Request::FrameRetain { .. }
            | Request::FrameOwnerRelease { .. }
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "One policy table keeps every browser operation's fencing classification visible together"
)]
pub(crate) fn fenced(request: &Request) -> bool {
    match request {
        Request::NetworkRoute {
            action: crate::browser::routing::HandlerAction::List {},
            ..
        } => false,
        Request::DiagnosticsArgumentsList { .. }
        | Request::DiagnosticsArgumentsRemove { .. }
        | Request::DiagnosticsArgumentsSet { enabled: false, .. } => false,
        Request::WebMcpDebug { action, .. } if action.responsive() => false,
        Request::WebMcp { action, .. } if action.responsive() => false,
        Request::BrowserExtension {
            action: crate::browser::extensions::Action::List { .. },
        } => false,
        Request::BrowserPwa {
            action: crate::browser::pwa::Action::List { .. },
        } => false,
        Request::ContextDebugger { action, .. } => action.fenced(),
        Request::PageDebugger {
            action:
                crate::browser::debugging::Action::Status { .. }
                | crate::browser::debugging::Action::Variables { .. }
                | crate::browser::debugging::Action::Resume { .. }
                | crate::browser::debugging::Action::Disable
                | crate::browser::debugging::Action::BreakpointRemove { .. },
            ..
        } => false,
        Request::Batch { requests } => requests.iter().any(fenced),
        Request::ContextList {}
        | Request::HttpContextList {}
        | Request::HttpRequestInputRemove { .. }
        | Request::HttpRequestInputsSet { enabled: false, .. }
        | Request::HarRegister { .. }
        | Request::HarList {}
        | Request::HarCaptureStart { .. }
        | Request::HarCaptureStatus { .. }
        | Request::HarCaptureStop { .. }
        | Request::HarFinalize { .. }
        | Request::HarRemove { .. }
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
        | Request::BindingList { .. }
        | Request::WebSocketRouteList { .. }
        | Request::BindingRead { .. }
        | Request::PageList {}
        | Request::PageRead { .. }
        | Request::PageStyles { .. }
        | Request::PageWait { .. }
        | Request::PageScreenshot { .. }
        | Request::CaptureCompare { .. }
        | Request::PageSnapshot { .. }
        | Request::FixtureList {}
        | Request::FixtureVerify { .. }
        | Request::DownloadList {}
        | Request::DownloadStatus { .. }
        | Request::DiagnosticsRead { .. }
        | Request::DeveloperStatus { .. }
        | Request::DeveloperClose { .. }
        | Request::ProfileStop { .. }
        | Request::ProfileTakeCoverage { .. }
        | Request::HeapStop { .. }
        | Request::ProfileStatus { .. }
        | Request::TraceStatus {}
        | Request::TraceStop { .. }
        | Request::TraceChunkStop {}
        | Request::TraceGroupEnd {}
        | Request::DeveloperEvidenceList { .. }
        | Request::DeveloperEvidenceFinalize { .. }
        | Request::DeveloperEvidenceRemove { .. }
        | Request::NetworkRead { .. }
        | Request::NetworkEvidenceList { .. }
        | Request::NetworkEvidenceRemove { .. }
        | Request::DiagnosticsWait { .. }
        | Request::EventsRead { .. }
        | Request::EventsWait { .. }
        | Request::PreviewStatus { .. }
        | Request::RecordingStatus { .. }
        | Request::PageInitScriptList { .. }
        | Request::ContextInitScriptList { .. }
        | Request::WorkerList { .. }
        | Request::ContextWorkerList { .. }
        | Request::PreviewStart { .. }
        | Request::RecordingStart { .. }
        | Request::DiagnosticsStart { .. }
        | Request::DiagnosticsStop { .. }
        | Request::FixtureRegister { .. }
        | Request::DirectoryRegister { .. }
        | Request::StorageRegister { .. }
        | Request::StorageExport { .. }
        | Request::StorageForget { .. }
        | Request::ContextClose { .. }
        | Request::PageClose { .. }
        | Request::HandleDispose { .. }
        | Request::DownloadCancel { .. }
        | Request::DownloadDelete { .. }
        | Request::PreviewStop { .. }
        | Request::RecordingStop { .. }
        | Request::InputRelease {} => false,
        _ => true,
    }
}

fn authority_error(error: crate::error::Error) -> Error {
    let code = match error.code.as_str() {
        "STALE_CONTROL_LEASE" => "STALE_CONTROL_LEASE",
        "CONTROL_HANDOFF_PENDING" => "CONTROL_HANDOFF_PENDING",
        "CONTROL_NOT_OWNED" => "CONTROL_NOT_OWNED",
        "INVALID_ARGUMENT" => "INVALID_ARGUMENT",
        _ => "CONTROL_HANDOFF_FAILED",
    };
    Error::caused(code, error)
}

impl Session {
    pub(crate) fn control_status(&self) -> Value {
        let host = &self.0.host;
        let mut status =
            crate::managed::clean_result(host.authority.status(std::time::Instant::now()));
        status["page"] = json!(*host.viewer_page.lock().expect("Viewer page"));
        if let Some(channel) = host.viewer.lock().expect("Browser viewer").as_ref()
            && status["owner"] == "viewer"
            && channel.metadata()["controlVersion"] == status["controlVersion"]
        {
            status["channel"] = channel.metadata();
        }
        status["coordinateSpace"] = json!("css-viewport");
        status["handoff"] = host
            .control_outcome
            .lock()
            .expect("Browser handoff outcome")
            .clone();
        status
    }

    pub(crate) async fn change_control(&self, name: &str, args: Value) -> Result<Value, Error> {
        if self.0.host.state.lock().expect("Browser owner").phase != Phase::Ready {
            return Err("SESSION_NOT_READY".into());
        }
        let desired = if name == "browser_control_release" {
            if args.get("page").is_some() {
                return Err("INVALID_SCOPE".into());
            }
            None
        } else {
            Some(crate::control::owner(&args).map_err(authority_error)?)
        };
        let page: Option<Reference> = args
            .get("page")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| Error::from("INVALID_SCOPE"))?;
        if matches!(desired, Some(Owner::Viewer(_))) != page.is_some() {
            return Err("INVALID_SCOPE".into());
        }
        if let Some(page) = page {
            self.0
                .host
                .state
                .lock()
                .expect("Browser owner")
                .resource(page, Some(Kind::Page))?;
        }
        let credential = self.0.host.authority.credential(std::time::Instant::now());
        if args["expectedVersion"].as_u64() != Some(credential.version) {
            return Err("STALE_CONTROL_LEASE".into());
        }
        let host = &self.0.host;
        if host.control_pending.swap(true, Ordering::SeqCst) {
            return Err("CONTROL_HANDOFF_PENDING".into());
        }
        let guard = PendingControl(host.clone());
        let owner = match &desired {
            Some(Owner::Agent) => json!("agent"),
            Some(Owner::Viewer(_)) => json!("viewer"),
            None => Value::Null,
        };
        if host.authority.status(std::time::Instant::now())["state"] == "handoff_pending" {
            let previous = host
                .control_outcome
                .lock()
                .expect("Browser handoff outcome");
            if previous["requestedOwner"] != owner
                || previous["page"] != json!(page)
                || previous["viewerId"] != args["viewerId"]
            {
                return Err("CONTROL_HANDOFF_PENDING".into());
            }
        }
        host.authority
            .plan(&credential, desired.clone(), std::time::Instant::now())
            .map_err(authority_error)?;
        *host
            .control_outcome
            .lock()
            .expect("Browser handoff outcome") = json!({"state":"running","expectedVersion":credential.version,"requestedOwner":owner,"viewerId":args["viewerId"],"page":page});
        // Once fencing starts, an interrupted caller cannot abandon the handoff.
        let (send, receive) = tokio::sync::oneshot::channel();
        let session = self.clone();
        let task = tokio::spawn(async move {
            let mut result = session.apply_control(credential, desired, page).await;
            session.control_completed(&mut result);
            drop(guard);
            let _ = send.send(result);
        });
        {
            let mut tasks = self
                .0
                .host
                .control_tasks
                .lock()
                .expect("Browser control tasks");
            tasks.retain(|task| !task.is_finished());
            tasks.push(task);
        }
        receive
            .await
            .map_err(|_| Error::from("CONTROL_HANDOFF_FAILED"))?
    }

    fn control_completed(&self, result: &mut Result<Value, Error>) {
        let mut outcome = self
            .0
            .host
            .control_outcome
            .lock()
            .expect("Browser handoff outcome");
        match &result {
            Ok(status) => {
                outcome["state"] = json!("succeeded");
                outcome["controlVersion"] = status["controlVersion"].clone();
            }
            Err(error) => {
                outcome["state"] = json!("failed");
                outcome["code"] = json!(error.code);
                outcome["partial"] = json!(error.partial);
            }
        }
        if let Ok(status) = result.as_mut() {
            status["handoff"] = outcome.clone();
        }
    }

    async fn apply_control(
        &self,
        expected: Credential,
        desired: Option<Owner>,
        page: Option<Reference>,
    ) -> Result<Value, Error> {
        let host = &self.0.host;
        let _control = host.control_lane.lock().await;
        let deadline = Instant::now() + Duration::from_millis(contract::MAX_TIMEOUT_MS);
        let _lane = host.control_cleanup_lane(deadline).await?;
        if host.stopping.load(Ordering::SeqCst) {
            return Err("SESSION_CLOSING".into());
        }
        if host.authority.credential(std::time::Instant::now()) != expected {
            return Err("STALE_CONTROL_LEASE".into());
        }
        if host.state.lock().expect("Browser owner").phase != Phase::Ready {
            return Err("SESSION_NOT_READY".into());
        }
        if let Some(page) = page {
            host.state
                .lock()
                .expect("Browser owner")
                .resource(page, Some(Kind::Page))?;
        }
        let cleanup = host
            .input_release(Instant::now() + Duration::from_secs(2))
            .await?;
        if cleanup["pages"]
            .as_array()
            .is_some_and(|pages| pages.iter().any(|page| page["cleanup"]["complete"] != true))
        {
            return Err(Error {
                partial: Some(json!({"cleanup":cleanup,"nextAction":"close_owned_page"})),
                ..Error::from("SESSION_UNCERTAIN")
            });
        }
        host.input_epoch.fetch_add(1, Ordering::SeqCst);
        let credential = host
            .authority
            .apply(&expected, desired.clone(), std::time::Instant::now())
            .map_err(authority_error)?;
        *host.viewer_page.lock().expect("Viewer page") = page;
        {
            let old = host.viewer.lock().expect("Browser viewer").take();
            if let Some(old) = old {
                old.close().await;
            }
            if matches!(desired, Some(Owner::Viewer(_))) {
                let weak = Arc::downgrade(&self.0);
                let input_credential = credential.clone();
                let handler: crate::control::channel::Handler = Arc::new(move |input| {
                    let weak = weak.clone();
                    let credential = input_credential.clone();
                    Box::pin(async move {
                        let client = weak.upgrade().ok_or_else(|| {
                            crate::error::Error::new("SESSION_CLOSING", "Browser session ended")
                        })?;
                        Session(client).viewer_input(credential, input).await
                    })
                });
                let channel = match crate::control::channel::Channel::start_browser(
                    host.authority.clone(),
                    credential.clone(),
                    handler,
                )
                .await
                {
                    Ok(channel) => channel,
                    Err(error) => {
                        host.authority.disconnect(&credential);
                        return Err(authority_error(error));
                    }
                };
                *host.viewer.lock().expect("Browser viewer") = Some(channel);
                host.monitor_control(credential);
            }
        }
        Ok(self.control_status())
    }

    pub(super) async fn viewer_input(
        &self,
        credential: Credential,
        input: Value,
    ) -> crate::error::Result<Value> {
        if input["viewerLeaseId"] != credential.id || input["viewerVersion"] != credential.version {
            return Err(crate::error::Error::new(
                "STALE_CONTROL_LEASE",
                "Viewer input lease changed",
            ));
        }
        let page: Reference = serde_json::from_value(input["page"].clone())
            .map_err(|_| crate::error::Error::invalid("Use the complete page reference"))?;
        if *self.0.host.viewer_page.lock().expect("Viewer page") != Some(page) {
            return Err(crate::error::Error::new(
                "INVALID_SCOPE",
                "Viewer input belongs to another page",
            ));
        }
        let id = input["commandId"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| crate::error::Error::invalid("Use a UUID commandId"))?;
        let mut request = json!({"action":"page_input","page":page,"actions":input["actions"]});
        if !input["observation"].is_null() {
            request["observation"] = input["observation"].clone();
        }
        let (_, envelope) = crate::browser::api::translate(
            "act",
            json!({"targetId":format!("browser:{}", page.connection_id),"actions":[request]}),
        )?;
        let request: Request =
            serde_json::from_value(envelope["request"]["requests"][0].clone())
                .map_err(|_| crate::error::Error::invalid("Invalid browser input"))?;
        self.submit_as(id, request, 30000, Some(credential))
            .map_err(|e| {
                crate::error::Error::new(
                    e.code,
                    "Browser viewer input refused; inspect browser status",
                )
            })?;
        let operation = self
            .wait(id, Instant::now() + Duration::from_secs(31))
            .await
            .map_err(|e| crate::error::Error::new(e.code, "Browser viewer status unavailable"))?;
        Ok(json!({"operation":operation}))
    }
}

impl Host {
    pub(super) fn admit_controller(
        &self,
        viewer: Option<&Credential>,
    ) -> Result<Credential, Error> {
        let status = self.authority.status(std::time::Instant::now());
        if viewer.is_none() && status["owner"] != "agent" {
            return Err("CONTROL_NOT_OWNED".into());
        }
        let credential = viewer
            .cloned()
            .unwrap_or_else(|| self.authority.credential(std::time::Instant::now()));
        self.authority
            .admit(&credential.id, std::time::Instant::now())
            .map_err(authority_error)
    }

    pub(super) fn controller_current(&self, id: Uuid, queued: bool) -> Result<(), Error> {
        let credential = self
            .input_credentials
            .lock()
            .expect("Browser input credentials")
            .get(&id)
            .cloned();
        if let Some(credential) = credential {
            if queued {
                self.authority
                    .admit(&credential.id, std::time::Instant::now())
                    .map_err(authority_error)?;
            } else if self.authority.credential(std::time::Instant::now()) != credential {
                return Err("STALE_CONTROL_LEASE".into());
            }
        }
        Ok(())
    }

    pub(super) async fn control_cleanup_lane(
        &self,
        deadline: Instant,
    ) -> Result<tokio::sync::MutexGuard<'_, ()>, Error> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.stopping.load(Ordering::SeqCst) {
                return Err("SESSION_CLOSING".into());
            }
            tokio::select! {
                lane = self.lane.lock() => return Ok(lane),
                () = &mut changed => {},
                () = tokio::time::sleep_until(deadline) => return Err("CONTROL_HANDOFF_PENDING".into()),
            }
        }
    }

    fn monitor_control(self: &Arc<Self>, credential: Credential) {
        let authority = self.authority.clone();
        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            authority.lost(&credential).await;
            let Some(host) = weak.upgrade() else {
                return;
            };
            if let Ok(_lane) = host
                .control_cleanup_lane(
                    Instant::now() + Duration::from_millis(contract::MAX_TIMEOUT_MS),
                )
                .await
            {
                let status = host.authority.status(std::time::Instant::now());
                if status["owner"].is_null()
                    && status["controlVersion"].as_u64() == Some(credential.version + 1)
                {
                    host.input_epoch.fetch_add(1, Ordering::SeqCst);
                    if let Err(error) = host
                        .input_release(Instant::now() + Duration::from_secs(2))
                        .await
                    {
                        host.set_failure(error.code);
                    }
                }
            }
        });
        let mut tasks = self.control_tasks.lock().expect("Browser control tasks");
        tasks.retain(|task| !task.is_finished());
        tasks.push(task);
    }

    pub(super) fn revoke_closed_viewer(&self, live: &BTreeSet<Reference>) {
        if self
            .viewer_page
            .lock()
            .expect("Viewer page")
            .is_some_and(|page| !live.contains(&page))
        {
            let credential = self.authority.credential(std::time::Instant::now());
            self.authority.disconnect(&credential);
        }
    }

    pub(super) async fn close_control(&self, deadline: Instant) -> Result<(), Error> {
        self.authority.close();
        self.changed.notify_waiters();
        {
            let viewer = recovered(&self.viewer).take();
            if let Some(viewer) = viewer {
                viewer.close().await;
            }
        }
        let tasks = std::mem::take(&mut *recovered(&self.control_tasks));
        let mut pending = tasks.into_iter();
        let mut failed = false;
        while let Some(mut task) = pending.next() {
            match tokio::time::timeout_at(deadline, &mut task).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => failed = true,
                Err(_) => {
                    let mut tasks = recovered(&self.control_tasks);
                    tasks.push(task);
                    tasks.extend(pending);
                    return Err("CLEANUP_PENDING".into());
                }
            }
        }
        if failed {
            return Err("CONTROL_OWNER_FAILED".into());
        }
        Ok(())
    }
}
