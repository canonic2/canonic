//! Native Chrome trace stream ownership, source loss and bounded finalization.
use super::*;
use crate::browser::{artifacts::Format, tracing::Options};
use base64::{Engine, engine::general_purpose::STANDARD};
const MAX_BYTES: usize = 32 * 1024 * 1024;
mod analysis;
// chrome-devtools-mcp ada7963 -> DevTools daedf816, TraceEvents.ts:
// DefaultCategories + JsSampling + Screenshot. CDP lists split the upstream
// comma-separated navigation/rail entry into its two category tokens.
const PERFORMANCE_CATEGORIES: &[&str] = &[
    "-*",
    "blink.console",
    "blink.user_timing",
    "loading",
    "devtools.timeline",
    "disabled-by-default-devtools.target-rundown",
    "disabled-by-default-devtools.timeline.frame",
    "disabled-by-default-devtools.timeline.stack",
    "disabled-by-default-devtools.timeline",
    "disabled-by-default-devtools.v8-source-rundown-sources",
    "disabled-by-default-devtools.v8-source-rundown",
    "disabled-by-default-layout_shift.debug",
    "disabled-by-default-v8.inspector",
    "disabled-by-default-v8.cpu_profiler.hires",
    "disabled-by-default-lighthouse",
    "v8.execute",
    "v8",
    "cppgc",
    "navigation",
    "rail",
    "disabled-by-default-v8.cpu_profiler",
    "disabled-by-default-devtools.screenshot",
];
struct Tab {
    context: Reference,
    session: Option<String>,
    pending: bool,
}
#[derive(Default)]
pub(super) struct Tabs {
    items: BTreeMap<String, Tab>,
}
pub(super) struct Trace {
    id: Uuid,
    context: Reference,
    metadata_page: Option<Reference>,
    session: Option<String>,
    state: &'static str,
    options: Options,
    requested_file_path: Option<String>,
    end_sent: bool,
    completion: Option<Value>,
    stream: Option<String>,
    bytes: Vec<u8>,
    source_loss: bool,
    loss_reasons: BTreeSet<&'static str>,
    eof: bool,
    truncated: bool,
    started: Option<Instant>,
    expires: Option<Instant>,
    automatic_stop_attempted: bool,
    stop_reason: Option<&'static str>,
    restore_cache: Option<String>,
}

pub(super) struct Completion {
    value: Value,
    error: Option<&'static str>,
    file_path: Option<String>,
    requested_file_path: Option<String>,
}
impl Completion {
    fn reply(&self) -> Result<Value, Error> {
        if let Some(code) = self.error {
            let mut error = Error::from(code);
            error.partial = Some(self.value.clone());
            Err(error)
        } else {
            Ok(self.value.clone())
        }
    }
}
impl Worker {
    pub(super) fn trace_tab_event(&mut self, event: &Event) {
        if event.method == "Target.attachedToTarget"
            && event.params["targetInfo"]["type"] == "tab"
            && let Some(target) = event.params["targetInfo"]["targetId"].as_str()
            && let Some(tab) = self.trace_tabs.items.get_mut(target)
            && let Some(session) = event.params["sessionId"].as_str()
        {
            tab.session = Some(session.into());
        }
        let removed = if event.method == "Target.targetDestroyed" {
            event.params["targetId"]
                .as_str()
                .and_then(|target| self.trace_tabs.items.remove(target))
        } else if event.method == "Target.detachedFromTarget" {
            let target = self
                .trace_tabs
                .items
                .iter()
                .find(|(_, tab)| tab.session.as_deref() == event.params["sessionId"].as_str())
                .map(|(target, _)| target.clone());
            target.and_then(|target| self.trace_tabs.items.remove(&target))
        } else {
            None
        };
        if let Some(tab) = removed
            && tab.session.is_some()
            && let Some(trace) = self.trace.as_mut()
            && trace.session == tab.session
        {
            trace.source_loss = true;
            trace.loss_reasons.insert("metadata-session-lost");
        }
    }
    async fn trace_metadata_session(
        &mut self,
        id: Uuid,
        page: Reference,
        context: Reference,
        deadline: Instant,
    ) -> Result<String, Error> {
        let Some(parent) = self
            .index
            .page_parent_target(page)
            .map_err(resource_error)?
        else {
            return self
                .index
                .route(page, Some(Kind::Page))
                .map(|route| route.session_id)
                .map_err(resource_error);
        };
        if let Some(tab) = self.trace_tabs.items.get(&parent) {
            if tab.context != context {
                return Err("INVALID_SCOPE".into());
            }
            if tab.pending {
                return Err("TRACE_ATTACHMENT_PENDING".into());
            }
            return tab.session.clone().ok_or("TRACE_ATTACHMENT_LOST".into());
        }
        if self.trace_tabs.items.len() >= 4 {
            return Err("TRACE_ATTACHMENT_CAPACITY".into());
        }
        let native_context = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        let info = self
            .call_read(
                id,
                None,
                "Target.getTargetInfo",
                json!({"targetId":parent}),
                deadline,
            )
            .await?;
        if info["targetInfo"]["targetId"] != parent
            || info["targetInfo"]["type"] != "tab"
            || info["targetInfo"]["browserContextId"]
                .as_str()
                .is_some_and(|owner| owner != native_context)
        {
            return Err("INVALID_SCOPE".into());
        }
        self.trace_tabs.items.insert(
            parent.clone(),
            Tab {
                context,
                session: None,
                pending: true,
            },
        );
        let reply = self
            .call(
                id,
                None,
                "Target.attachToTarget",
                json!({"targetId":parent,"flatten":true}),
                deadline,
            )
            .await;
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                if error.code != "OUTCOME_UNKNOWN" {
                    if self
                        .trace_tabs
                        .items
                        .get(&parent)
                        .is_some_and(|tab| tab.session.is_some())
                    {
                        self.trace_tabs
                            .items
                            .get_mut(&parent)
                            .expect("Retained tab")
                            .pending = false;
                    } else {
                        self.trace_tabs.items.remove(&parent);
                    }
                }
                return Err(error);
            }
        };
        let session = reply["sessionId"]
            .as_str()
            .ok_or("BACKEND_PROTOCOL")?
            .to_owned();
        let tab = self
            .trace_tabs
            .items
            .get_mut(&parent)
            .ok_or("TRACE_ATTACHMENT_LOST")?;
        tab.session = Some(session.clone());
        tab.pending = false;
        self.index
            .native(page, Kind::Page)
            .map_err(resource_error)?;
        Ok(session)
    }
    pub(super) fn trace_status(&self) -> Value {
        if let Some(t) = self.trace.as_ref() {
            return json!({
                "traceId":t.id,"context":t.context,"metadataPage":t.metadata_page,
                "active":true,"state":t.state,"options":t.options,
                "scope":"owned-browser-process","contextIsSelectionOnly":true,
                "format":"chrome-trace-event-json","bytesRead":t.bytes.len(),
                "sourceLoss":t.source_loss,"truncated":t.truncated,
                "sourceLossReasons":t.loss_reasons,
                "nativeCompletion":{"received":t.completion.is_some(),"dataLossOccurred":t.completion.as_ref().and_then(|event|event["dataLossOccurred"].as_bool())},
                "instrumentation":["CDP-Tracing"],
                "elapsedMs":t.started.map(|start|start.elapsed().as_millis()),
                "stopReason":t.stop_reason,"stopRequested":t.end_sent,
                "automaticFinalizationAttempted":t.automatic_stop_attempted,
                "cacheRestorePending":t.restore_cache.is_some(),
                "durationRemainingMs":t.expires.map(|expires|expires.saturating_duration_since(Instant::now()).as_millis()),
                "nextAction":(t.state=="finalization_pending").then_some("trace_stop")
            });
        }
        if let Some(last) = self.trace_last.as_ref() {
            let mut status = last.value.clone();
            status["error"] = json!(last.error);
            status["active"] = json!(false);
            status["traceId"] = status["report"]["traceId"].clone();
            status["context"] = status["report"]["context"].clone();
            return status;
        }
        json!({"state":"idle","active":false})
    }
    pub(super) fn trace_timer_pending(&self) -> bool {
        self.trace.as_ref().is_some_and(|trace| {
            trace.state == "recording" && trace.expires.is_some() && !trace.automatic_stop_attempted
        })
    }
    pub(super) fn trace_due(&self) -> bool {
        self.trace.as_ref().is_some_and(|trace| {
            trace.state == "recording"
                && !trace.automatic_stop_attempted
                && trace
                    .expires
                    .is_some_and(|expires| Instant::now() >= expires)
        })
    }
    pub(super) async fn poll_trace(&mut self) {
        if !self.trace_due() {
            return;
        }
        let trace = self.trace.as_mut().expect("Due native trace");
        trace.automatic_stop_attempted = true;
        trace.stop_reason = Some(if trace.options.automatic_stop() == Some(true) {
            "auto-stop"
        } else {
            "duration"
        });
        let _ = self
            .trace_stop(Instant::now() + Duration::from_secs(10))
            .await;
        self.publish();
    }
    #[expect(
        clippy::too_many_lines,
        reason = "One capture admission retains exact page authority through reset, start, return navigation and uncertain failure transitions"
    )]
    pub(super) async fn trace_start(
        &mut self,
        id: Uuid,
        context: Reference,
        mut options: Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.attached {
            return Err("TRACE_OWNED_PROCESS_REQUIRED".into());
        }
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        options.validate(true)?;
        let requested_file_path = options.file_path.clone();
        if let Some(path) = &options.file_path {
            options.file_path = Some(crate::browser::tracing::export::prepare(path)?);
        }
        if self.trace.is_some() {
            return Err("TRACE_CONFLICT".into());
        }
        let selected = self
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .selected_page;
        let pages = self
            .index
            .list(Kind::Page)
            .into_iter()
            .filter(|(_, parent)| *parent == Some(context))
            .map(|(page, _)| page)
            .collect::<Vec<_>>();
        let metadata_page = if let Some(page) = options.page {
            self.index
                .native(page, Kind::Page)
                .map_err(resource_error)?;
            if !pages.contains(&page) {
                return Err("INVALID_SCOPE".into());
            }
            Some(page)
        } else {
            selected
                .filter(|page| pages.contains(page))
                .or_else(|| pages.first().copied())
        };
        let performance_reload = options
            .performance
            .as_ref()
            .is_some_and(|options| options.reload);
        if (options.reload.is_some() || performance_reload) && metadata_page.is_none() {
            return Err("TRACE_PAGE_REQUIRED".into());
        }
        let return_navigation = if performance_reload {
            let page = metadata_page.expect("Performance reload page admitted");
            let route = self
                .index
                .route(page, Some(Kind::Page))
                .map_err(resource_error)?;
            let tree = self
                .call_read(
                    id,
                    Some(&route.session_id),
                    "Page.getFrameTree",
                    json!({}),
                    deadline,
                )
                .await?;
            self.index
                .native(page, Kind::Page)
                .map_err(resource_error)?;
            let frame = &tree["frameTree"]["frame"];
            let mut url = frame["url"].as_str().ok_or("BACKEND_PROTOCOL")?.to_owned();
            // CDP Frame.url explicitly excludes the fragment. Preserve its
            // separate encoded spelling, including an empty trailing '#'.
            if let Some(fragment) = frame["urlFragment"].as_str() {
                if !fragment.is_empty() && !fragment.starts_with('#') {
                    return Err("BACKEND_PROTOCOL".into());
                }
                url.push_str(fragment);
            }
            let navigation = crate::browser::page::Navigation::Goto {
                url,
                wait_until: crate::browser::page::Load::Load,
            };
            navigation.validate()?;
            Some(navigation)
        } else {
            None
        };
        let session = if let Some(page) = metadata_page {
            Some(
                self.trace_metadata_session(id, page, context, deadline)
                    .await?,
            )
        } else {
            None
        };
        let trace_id = Uuid::new_v4();
        self.developers.evidence.reserve(trace_id, 1, MAX_BYTES)?;
        let mut categories = if options.performance.is_some() {
            PERFORMANCE_CATEGORIES.to_vec()
        } else {
            vec![
                "-*",
                "devtools.timeline",
                "v8.execute",
                "blink.user_timing",
                "loading",
                "latencyInfo",
                "disabled-by-default-devtools.timeline",
                "disabled-by-default-devtools.timeline.frame",
                "disabled-by-default-devtools.timeline.stack",
            ]
        };
        if options.screenshots && options.performance.is_none() {
            categories.push("disabled-by-default-devtools.screenshot");
        }
        if options.cpu_sampling && options.performance.is_none() {
            categories.push("disabled-by-default-v8.cpu_profiler");
        }
        self.trace = Some(Trace {
            id: trace_id,
            context,
            metadata_page,
            session: session.clone(),
            state: if performance_reload {
                "preparing"
            } else {
                "starting"
            },
            options: options.clone(),
            requested_file_path,
            end_sent: false,
            completion: None,
            stream: None,
            bytes: Vec::new(),
            source_loss: false,
            loss_reasons: BTreeSet::new(),
            eof: false,
            truncated: false,
            started: None,
            expires: None,
            automatic_stop_attempted: false,
            stop_reason: None,
            restore_cache: None,
        });
        self.trace_last = None;
        // Retain the capture slot before the first mutating navigation. No
        // Tracing.start is dispatched until the blank document reaches load.
        let reset = if performance_reload {
            let page = metadata_page.expect("Performance reload page admitted");
            match self
                .navigate(
                    id,
                    page,
                    crate::browser::page::Navigation::Goto {
                        url: "about:blank".into(),
                        wait_until: crate::browser::page::Load::Load,
                    },
                    deadline,
                )
                .await
            {
                Ok(reset) => Some(reset),
                Err(mut error) => {
                    self.developers.evidence.release(trace_id);
                    self.trace = None;
                    error.partial = Some(
                        json!({"traceStarted":false,"resetAttempted":true,"resetPage":page,"resetFailure":error.partial,"replayed":false}),
                    );
                    return Err(error);
                }
            }
        } else {
            None
        };
        self.trace.as_mut().expect("Trace owner").state = "starting";
        let excluded_categories: Vec<_> = categories
            .iter()
            .filter_map(|category| category.strip_prefix('-'))
            .collect();
        let included_categories: Vec<_> = categories
            .iter()
            .copied()
            .filter(|category| !category.starts_with('-'))
            .collect();
        let result=self.call(id,session.as_deref(),"Tracing.start",json!({"transferMode":"ReturnAsStream","streamFormat":"json","streamCompression":"none","traceConfig":{"recordMode":"recordUntilFull","traceBufferSizeInKb":16384,"enableSampling":options.cpu_sampling || options.performance.is_some(),"includedCategories":included_categories,"excludedCategories":excluded_categories},"screenshotMaxSize":500,"screenshotMaxCount":60}),deadline).await;
        match result {
            Ok(_) => {
                let trace = self.trace.as_mut().expect("Trace owner");
                let started = Instant::now();
                trace.state = "recording";
                trace.started = Some(started);
                trace.expires = options
                    .max_duration_ms
                    .map(|ms| started + Duration::from_millis(ms));
                let reload = if let Some(navigation) = return_navigation {
                    let page = metadata_page.expect("Performance reload page admitted");
                    let bypass = options
                        .performance
                        .as_ref()
                        .is_some_and(|options| options.ignore_cache);
                    match self
                        .trace_return_navigation(id, page, navigation, bypass, deadline)
                        .await
                    {
                        Ok(result) => Some(result),
                        Err(mut error) => {
                            error.partial = Some(
                                json!({"traceStarted":true,"trace":self.trace_status(),"reset":reset,"reloadPage":page,"reloadFailure":error.partial,"replayed":false}),
                            );
                            return Err(error);
                        }
                    }
                } else if let Some(reload) = options.reload.as_ref() {
                    let page = metadata_page.expect("Reload page admitted");
                    match self
                        .navigate_with_reload_cache(
                            id,
                            page,
                            crate::browser::page::Navigation::Reload {
                                wait_until: crate::browser::page::Load::Load,
                            },
                            Some(reload.ignore_cache),
                            deadline,
                        )
                        .await
                    {
                        Ok(result) => Some(result),
                        Err(mut error) => {
                            error.partial = Some(
                                json!({"traceStarted":true,"trace":self.trace_status(),"reloadPage":page,"reloadFailure":error.partial,"replayed":false}),
                            );
                            return Err(error);
                        }
                    }
                } else {
                    None
                };
                // Unlike maxDurationMs, the performance-style observation
                // window begins only after an acknowledged reload settles.
                if options.automatic_stop() == Some(true)
                    && let Some(trace) = self.trace.as_mut()
                {
                    trace.expires = Some(Instant::now() + Duration::from_secs(5));
                }
                let mut result = self.trace_status();
                if let Some(reset) = reset {
                    result["reset"] = reset;
                }
                if let Some(reload) = reload {
                    result["reload"] = reload;
                }
                Ok(result)
            }
            Err(error) => {
                if error.code == "OUTCOME_UNKNOWN" {
                    self.trace.as_mut().expect("Trace owner").state = "uncertain";
                } else {
                    self.developers.evidence.release(trace_id);
                    self.trace = None;
                }
                Err(error)
            }
        }
    }
    async fn trace_restore_cache(&mut self, deadline: Instant) -> Result<(), Error> {
        let session = self
            .trace
            .as_ref()
            .and_then(|trace| trace.restore_cache.clone());
        if let Some(session) = session {
            self.cleanup_call(
                Some(&session),
                "Network.setCacheDisabled",
                json!({"cacheDisabled":false}),
                deadline,
            )
            .await?;
            self.trace
                .as_mut()
                .expect("Cache restoration owner")
                .restore_cache = None;
        }
        Ok(())
    }
    async fn trace_return_navigation(
        &mut self,
        id: Uuid,
        page: Reference,
        navigation: crate::browser::page::Navigation,
        bypass: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if !bypass {
            return self.navigate(id, page, navigation, deadline).await;
        }
        let route = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?;
        // Retain restoration before dispatch: even an unknown disable outcome
        // may have changed the page's network agent. Never replay navigation.
        self.trace.as_mut().expect("Trace owner").restore_cache = Some(route.session_id.clone());
        let result = match self
            .call(
                id,
                Some(&route.session_id),
                "Network.setCacheDisabled",
                json!({"cacheDisabled":true}),
                deadline,
            )
            .await
        {
            Ok(_) => self.navigate(id, page, navigation, deadline).await,
            Err(error) => Err(error),
        };
        let restored = self.trace_restore_cache(deadline).await;
        match (result, restored) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (result, Err(restore)) => {
                let mut error = Error::from("TRACE_CACHE_RESTORE_PENDING");
                error.partial = Some(
                    json!({"navigationResult":result.as_ref().ok(),"navigationFailure":result.err().map(|error|json!({"code":error.code,"partial":error.partial})),"cacheRestoreFailure":restore.code,"replayed":false}),
                );
                Err(error)
            }
        }
    }
    pub(super) fn trace_event(&mut self, event: &Event) {
        if event.method == "Tracing.tracingComplete"
            && let Some(t) = self.trace.as_mut()
            && event.session_id == t.session
        {
            t.source_loss |= event.params["dataLossOccurred"] == true;
            if event.params["dataLossOccurred"] == true {
                t.loss_reasons.insert("native-data-loss");
            }
            t.stream = event.params["stream"].as_str().map(str::to_owned);
            t.completion = Some(event.params.clone());
        }
    }
    pub(super) fn trace_gap(&mut self) {
        if let Some(t) = self.trace.as_mut() {
            t.source_loss = true;
            t.loss_reasons.insert("event-gap");
        }
    }
    pub(super) async fn trace_stop_to(
        &mut self,
        file_path: Option<String>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self
            .trace
            .as_ref()
            .is_some_and(|trace| matches!(trace.state, "preparing" | "starting" | "finalizing"))
        {
            return Err("TRACE_BUSY".into());
        }
        if let Some(path) = file_path {
            // Replay the original successful request before consulting mutable
            // filesystem aliases. A receipt owns the destination it published.
            if self.trace.is_none()
                && self.trace_last.as_ref().is_some_and(|last| {
                    last.value["export"].is_object()
                        && (last.requested_file_path.as_deref() == Some(path.as_str())
                            || last.value["export"]["path"] == path)
                })
            {
                return self.trace_last.as_ref().expect("Terminal receipt").reply();
            }
            let requested_file_path = path.clone();
            let path = crate::browser::tracing::export::resolve(&path)?;
            // A terminal receipt is replayable even though its published file exists.
            if self.trace.is_none()
                && self
                    .trace_last
                    .as_ref()
                    .is_some_and(|last| last.value["export"]["path"] == path)
            {
                return self.trace_last.as_ref().expect("Terminal receipt").reply();
            }
            crate::browser::tracing::export::prepare(&path)?;
            if let Some(trace) = self.trace.as_mut() {
                trace.options.file_path = Some(path);
                trace.requested_file_path = Some(requested_file_path);
            } else if let Some(last) = self.trace_last.as_mut() {
                last.file_path = Some(path);
                last.requested_file_path = Some(requested_file_path);
                last.value
                    .as_object_mut()
                    .expect("Trace receipt")
                    .remove("export");
            } else {
                return Err("TRACE_NOT_ACTIVE".into());
            }
        }
        self.trace_stop(deadline).await
    }
    fn trace_export_last(&mut self) -> Result<Value, Error> {
        let last = self.trace_last.as_mut().ok_or("TRACE_NOT_ACTIVE")?;
        if last.error.is_some() || last.file_path.is_none() || last.value["export"].is_object() {
            return last.reply();
        }
        let path = last.file_path.as_deref().expect("Export target");
        let result = (|| {
            let id = serde_json::from_value(last.value["report"]["evidenceId"].clone())
                .map_err(|_| Error::from("EVIDENCE_NOT_FOUND"))?;
            let artifact = self.developers.evidence.get(id)?;
            crate::browser::tracing::export::write(path, &artifact)
        })();
        match result {
            Ok(receipt) => {
                last.value
                    .as_object_mut()
                    .expect("Trace receipt")
                    .remove("exportPending");
                last.value["export"] = receipt;
                last.reply()
            }
            Err(mut error) => {
                last.value["exportPending"] = json!({"path":path,"code":error.code,"retry":"trace_stop with a new filePath, or retry the same path after resolving its cause"});
                error.partial = Some(last.value.clone());
                Err(error)
            }
        }
    }
    pub(super) async fn trace_stop(&mut self, deadline: Instant) -> Result<Value, Error> {
        let Some(trace) = self.trace.as_mut() else {
            return self.trace_export_last();
        };
        if matches!(trace.state, "preparing" | "starting" | "finalizing") {
            return Err("TRACE_BUSY".into());
        }
        trace.state = "finalizing";
        let file_path = trace.options.file_path.clone();
        let requested_file_path = trace.requested_file_path.clone();
        trace.stop_reason.get_or_insert("requested");
        self.publish();
        let result = self.finalize_trace(deadline).await;
        if self.trace.is_none() {
            self.trace_last = Some(match &result {
                Ok(value) => Completion {
                    value: value.clone(),
                    error: None,
                    file_path: file_path.clone(),
                    requested_file_path: requested_file_path.clone(),
                },
                Err(error) => Completion {
                    value: error.partial.clone().unwrap_or(Value::Null),
                    error: Some(error.code),
                    file_path,
                    requested_file_path,
                },
            });
        }
        if let Err(mut error) = result {
            if let Some(t) = self.trace.as_mut() {
                t.state = "finalization_pending";
            }
            if self.trace.is_some() {
                error.partial = Some(self.trace_status());
            }
            Err(error)
        } else {
            self.trace_export_last()
        }
    }
    async fn finalize_trace(&mut self, deadline: Instant) -> Result<Value, Error> {
        self.trace_restore_cache(deadline).await?;
        let session = self.trace.as_ref().expect("Trace owner").session.clone();
        if !self.trace.as_ref().expect("Trace owner").end_sent {
            self.trace.as_mut().expect("Trace owner").end_sent = true;
            if self
                .trace
                .as_ref()
                .expect("Trace owner")
                .completion
                .is_none()
            {
                self.cleanup_call(session.as_deref(), "Tracing.end", json!({}), deadline)
                    .await?;
            }
        }
        while self
            .trace
            .as_ref()
            .expect("Trace owner")
            .completion
            .is_none()
        {
            tokio::select! {
                ()=tokio::time::sleep(Duration::from_millis(25))=>{Box::pin(self.poll_har_routes()).await;},
                ()=tokio::time::sleep_until(deadline)=>return Err("TRACE_FINALIZATION_PENDING".into()),
                event=next_event(&mut self.events)=>match event{Some(event)=>self.apply(event)?,None=>{self.events=None;return Err("BACKEND_LOST".into());}},
                response=next_response(&mut self.responses)=>if let Some(response)=response{Box::pin(self.respond(response)).await;},
            }
        }
        if self.trace.as_ref().expect("Trace owner").stream.is_none()
            && !self.trace.as_ref().expect("Trace owner").eof
        {
            return Err("TRACE_STREAM_UNAVAILABLE".into());
        }
        while let Some(stream) = self.trace.as_ref().expect("Trace owner").stream.clone() {
            let reply = self
                .cleanup_call(
                    session.as_deref(),
                    "IO.read",
                    json!({"handle":stream,"size":65536}),
                    deadline,
                )
                .await;
            let reply = match reply {
                Ok(reply) => reply,
                Err(error) => {
                    self.trace.as_mut().expect("Trace owner").source_loss = true;
                    self.trace
                        .as_mut()
                        .expect("Trace owner")
                        .loss_reasons
                        .insert("stream-read-failed");
                    return Err(error);
                }
            };
            let encoded = reply["data"].as_str().ok_or("BACKEND_PROTOCOL")?;
            let data = if reply["base64Encoded"] == true {
                STANDARD
                    .decode(encoded)
                    .map_err(|_| Error::from("BACKEND_PROTOCOL"))?
            } else {
                encoded.as_bytes().to_vec()
            };
            let trace = self.trace.as_mut().expect("Trace owner");
            if trace.bytes.len() + data.len() > MAX_BYTES {
                trace.truncated = true;
            } else {
                trace.bytes.extend_from_slice(&data);
            }
            if reply["eof"] == true || trace.truncated {
                trace.eof = reply["eof"] == true;
                self.cleanup_call(
                    session.as_deref(),
                    "IO.close",
                    json!({"handle":stream}),
                    deadline,
                )
                .await?;
                self.trace.as_mut().expect("Trace owner").stream = None;
                break;
            }
        }
        let trace = self.trace.as_ref().expect("Trace owner");
        let complete = trace.eof && !trace.source_loss && !trace.truncated;
        let artifact=Format{prefix:"trace",extension:"json",mime:"application/json",name:"chrome-trace-event-json",max_bytes:MAX_BYTES}.write(self.journal.directory(),&trace.bytes,json!({"traceId":trace.id,"context":trace.context,"scope":"owned-browser-process","contextIsSelectionOnly":true,"private":true,"redacted":false,"complete":complete,"sourceLoss":trace.source_loss,"sourceLossReasons":trace.loss_reasons,"nativeCompletion":{"received":trace.completion.is_some(),"dataLossOccurred":trace.completion.as_ref().and_then(|event|event["dataLossOccurred"].as_bool())},"truncated":trace.truncated,"readers":["Chrome-DevTools-Performance","Perfetto"],"playwrightTraceViewer":false,"options":trace.options,"metadataPage":trace.metadata_page,"stopReason":trace.stop_reason,"elapsedMs":trace.started.map(|start|start.elapsed().as_millis())}))?;
        let report = self.developers.evidence.insert_for(trace.id, artifact)?;
        self.trace = None;
        if complete {
            Ok(json!({"state":"complete","report":report}))
        } else {
            let mut error = Error::from("TRACE_INCOMPLETE");
            error.partial = Some(json!({"state":"incomplete","report":report}));
            Err(error)
        }
    }
    pub(super) async fn close_trace(&mut self, deadline: Instant) {
        if self.trace.is_some() {
            self.trace
                .as_mut()
                .expect("Owned trace")
                .stop_reason
                .get_or_insert("owned-close");
            let _ = self.trace_stop(deadline).await;
        }
    }
    pub(super) fn seal_trace_after_exit(&mut self) -> Result<(), Error> {
        let Some(trace) = self.trace.as_mut() else {
            return Ok(());
        };
        trace.loss_reasons.insert("owned-browser-exited");
        let status = json!({"traceId":trace.id,"context":trace.context,"options":trace.options,"complete":false,"traceDataAvailable":false,"reason":"owned-browser-exited-before-trace-stream"});
        let status_bytes = serde_json::to_vec(&status).map_err(|_| Error::from("EVIDENCE_IO"))?;
        let (bytes, format) = if trace.bytes.is_empty() {
            (status_bytes.as_slice(), "chrome-trace-capture-status-json")
        } else {
            (
                trace.bytes.as_slice(),
                if trace.eof {
                    "chrome-trace-event-json"
                } else {
                    "chrome-trace-event-json-prefix"
                },
            )
        };
        let artifact=Format{prefix:"trace",extension:"json",mime:if trace.eof||trace.bytes.is_empty(){"application/json"}else{"text/plain"},name:format,max_bytes:MAX_BYTES}.write(self.journal.directory(),bytes,json!({"traceId":trace.id,"context":trace.context,"scope":"owned-browser-process","complete":false,"sourceLoss":true,"sourceLossReasons":trace.loss_reasons,"nativeCompletion":{"received":trace.completion.is_some(),"dataLossOccurred":trace.completion.as_ref().and_then(|event|event["dataLossOccurred"].as_bool())},"truncated":!trace.eof,"private":true,"redacted":false,"finalization":"owned-browser-exited"}))?;
        let report = self.developers.evidence.insert_for(trace.id, artifact)?;
        self.trace_last = Some(Completion {
            value: json!({"state":"incomplete","report":report}),
            error: Some("TRACE_INCOMPLETE"),
            file_path: trace.options.file_path.clone(),
            requested_file_path: trace.requested_file_path.clone(),
        });
        self.trace = None;
        Ok(())
    }
}
