//! Native diagnostic projection with bounded identities and explicit source loss.

use super::*;
use crate::browser::diagnostics::{self, DiagnosticKind, Diagnostics, MAX_REQUESTS, Query};
use std::collections::VecDeque;
mod network_log;

#[derive(Clone)]
struct NetworkRequest {
    context: Reference,
    source_url: Option<String>,
    data: Value,
    headers: Value,
    response_headers: Value,
    complete: bool,
    failed: bool,
    /// CDP `timing.requestTime` (monotonic seconds) for full-mode HAR receive time.
    request_time: Option<f64>,
    started: Option<f64>,
    wall_time: Option<f64>,
    served_from_cache: bool,
    response_details: Value,
    /// SDK raw headers: ExtraInfo values replace the Network event's headers.
    raw_request: Value,
    raw_response: Value,
    /// ExtraInfo `headersText` length, the SDK's reported response header size.
    response_headers_text: Option<usize>,
    encoded_length: Option<usize>,
    request_body_size: usize,
    status_text: Value,
    http_version: Value,
    frame: Option<Reference>,
}

fn exception_diagnostic(
    p: &Value,
    worker: Option<Reference>,
    setup_pending: bool,
    subscription_boundary: Option<&str>,
) -> Value {
    let exception = &p["exceptionDetails"];
    let message = exception["exception"]["description"]
        .as_str()
        .or_else(|| exception["exception"]["value"].as_str())
        .or_else(|| exception["text"].as_str())
        .unwrap_or("Non-Error exception");
    let (message, truncated) = diagnostics::text(message.lines().next().unwrap_or(""));
    json!({"text":message,"textTruncated":truncated,"location":location_summary(exception),"sourceTimestamp":p["timestamp"],"worker":worker,"stackAvailable":exception["stackTrace"].is_object(),"stack":exception.get("stackTrace").filter(|stack|stack.is_object()).map(diagnostics::stacks::cdp),"attachmentSetupPending":setup_pending,"subscriptionBoundary":subscription_boundary})
}

fn finish_response_timing(request: &mut NetworkRequest, event: &Value) {
    if request.response_details["responseAvailable"] != true {
        return;
    }
    let end = request
        .started
        .zip(event["timestamp"].as_f64())
        .and_then(|(start, end)| {
            let micros = ((end - start) * 1e6).trunc();
            if !micros.is_finite() {
                return None;
            }
            // Match the pinned adapter's secondsToRoundishMillis (JS ToInt32).
            let wrapped = micros.rem_euclid(4294967296.0);
            let signed = if wrapped >= 2147483648.0 {
                wrapped - 4294967296.0
            } else {
                wrapped
            };
            Some(
                (signed / 1000.0).max(
                    request.response_details["timing"]["responseStart"]
                        .as_f64()
                        .unwrap_or(-1.0),
                ),
            )
        });
    crate::browser::network::response::finish(&mut request.response_details, end);
}

#[derive(Default)]
pub(super) struct NativeDiagnostics {
    pub(super) history: Diagnostics,
    requests: BTreeMap<(String, String), NetworkRequest>,
    ids: BTreeMap<Uuid, (String, String)>,
    unpaired_inputs: BTreeMap<(String, String), Uuid>,
    order: VecDeque<(String, String)>,
    // Console remote objects are never exposed as public handles. Release only
    // this inspector session's console group, independently of evaluation.
    pub(super) discard: BTreeSet<String>,
    new_workers: BTreeMap<Reference, Reference>,
    log_sessions: BTreeSet<String>,
    /// Full-mode `Cookie` headers from ExtraInfo that preceded `requestWillBeSent`.
    pub(super) early_cookies: BTreeMap<(String, String), String>,
    /// Raw request headers from ExtraInfo that preceded `requestWillBeSent`.
    early_request_headers: BTreeMap<(String, String), Value>,
    /// Raw response headers and header text length that preceded the response.
    early_response_headers: BTreeMap<(String, String), (Value, Option<usize>)>,
}
const MAX_EARLY_COOKIES: usize = 256;

impl NativeDiagnostics {
    pub(super) fn source_gap(&mut self, dropped: u64, sessions: Vec<String>) {
        // Transport loss outside an active capture is not diagnostic evidence
        // loss. Preserve stopped history and its cursors as well.
        if self.history.capturing() {
            self.history.source_gap(dropped);
        }
        self.history.forget_requests(self.requests.len());
        self.requests.clear();
        self.ids.clear();
        self.unpaired_inputs.clear();
        self.order.clear();
        self.new_workers.clear();
        self.early_request_headers.clear();
        self.early_response_headers.clear();
        // A dropped console event may have allocated remote objects even
        // though none of its argument IDs reached this actor.
        self.discard.extend(sessions);
    }
    pub(super) fn stop(&mut self, context: Reference) {
        self.history.enable(context, false);
        self.requests
            .retain(|_, request| request.context != context);
        self.order.retain(|key| self.requests.contains_key(key));
        self.ids.retain(|_, key| self.requests.contains_key(key));
        self.unpaired_inputs
            .retain(|key, _| self.requests.contains_key(key));
        self.new_workers.retain(|_, owner| *owner != context);
    }
    pub(super) fn close(&mut self) {
        self.history.close();
        self.requests.clear();
        self.ids.clear();
        self.unpaired_inputs.clear();
        self.order.clear();
        self.discard.clear();
        self.new_workers.clear();
        self.log_sessions.clear();
        self.early_request_headers.clear();
        self.early_response_headers.clear();
    }
    fn insert(&mut self, key: (String, String), mut request: NetworkRequest) {
        // ExtraInfo can precede its Network event; apply what arrived first.
        if let Some(raw) = self.early_request_headers.remove(&key) {
            request.raw_request = raw;
        }
        if let Some((raw, text)) = self.early_response_headers.remove(&key) {
            request.raw_response = raw;
            request.response_headers_text = text;
        }
        if self.requests.contains_key(&key) {
            self.order.retain(|entry| entry != &key);
        } else if self.order.len() == MAX_REQUESTS {
            let oldest = self.order.pop_front().expect("Bounded request index");
            if let Some(previous) = self.requests.remove(&oldest)
                && let Ok(id) = serde_json::from_value::<Uuid>(previous.data["requestId"].clone())
            {
                self.ids.remove(&id);
            }
            self.history.forget_requests(1);
        }
        self.order.push_back(key.clone());
        self.ids.insert(
            serde_json::from_value(request.data["requestId"].clone())
                .expect("Generated request UUID"),
            key.clone(),
        );
        self.requests.insert(key, request);
        self.unpaired_inputs
            .retain(|_, id| self.ids.contains_key(id));
    }
    pub(super) fn take_request_input_id(
        &mut self,
        context: Reference,
        session: &str,
        native: &str,
    ) -> Option<Uuid> {
        let exact = (session.to_owned(), native.to_owned());
        if self
            .requests
            .get(&exact)
            .is_some_and(|request| request.context == context)
            && let Some(id) = self.unpaired_inputs.remove(&exact)
        {
            return Some(id);
        }
        // The pinned adapter can receive the matching events in different
        // inspector sessions. Require one exact network ID in this context;
        // neither URL similarity nor an ambiguous alias confers ownership.
        let mut matches = self.unpaired_inputs.iter().filter(|(key, _)| {
            key.1 == native
                && self
                    .requests
                    .get(*key)
                    .is_some_and(|request| request.context == context)
        });
        let key = matches.next()?.0.clone();
        if matches.next().is_some() {
            return None;
        }
        self.unpaired_inputs.remove(&key)
    }
}

impl Worker {
    pub(super) fn capture_post_data_budget(&self, session: &str) -> usize {
        if self
            .index
            .diagnostic_scope(session)
            .is_some_and(|(context, _, _)| {
                self.diagnostics.history.active(context) && self.request_inputs.enabled(context)
            })
        {
            // Chrome's zero means no post-data truncation, as in the pinned
            // adapter. Enforce our byte bound while decoding, rather than
            // turning an over-budget body into an apparently absent body.
            return 0;
        }
        if self
            .index
            .diagnostic_scope(session)
            .is_some_and(|(context, _, _)| self.har_archives.capture_bodies(context))
        {
            crate::browser::routing::MAX_INLINE_BODY
        } else {
            0
        }
    }
    pub(super) fn notice_diagnostic_worker(&mut self, native: Option<&str>) {
        self.diagnostics
            .new_workers
            .retain(|worker, _| self.index.worker_scope(*worker).is_ok());
        if let Some(worker) = native.and_then(|native| self.index.target_reference(native))
            && let Ok((context, _, _)) = self.index.worker_scope(worker)
            && self.diagnostics.history.active(context)
        {
            self.diagnostics.new_workers.insert(worker, context);
        }
    }
    pub(super) async fn configure_diagnostics(
        &mut self,
        id: Uuid,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        // Network.disable would break navigation's independent metadata owner.
        // Collection stop is therefore a local subscription barrier. Browser
        // buffers stay bounded and are released with their owned attachment.
        self.call(id, Some(session), "Network.enable",
            json!({"maxTotalBufferSize":1048576,"maxResourceBufferSize":262144,"maxPostDataSize":self.capture_post_data_budget(session)}), deadline).await?;
        self.diagnostics
            .log_sessions
            .retain(|session| self.index.diagnostic_scope(session).is_some());
        if self
            .index
            .diagnostic_scope(session)
            .is_some_and(|scope| self.diagnostics.history.active(scope.0))
            && !self.diagnostics.log_sessions.contains(session)
        {
            self.call(id, Some(session), "Log.enable", json!({}), deadline)
                .await?;
            self.diagnostics.log_sessions.insert(session.to_owned());
        }
        Ok(())
    }

    pub(super) async fn diagnostics_subscribe(
        &mut self,
        id: Uuid,
        context: Reference,
        enabled: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        if enabled {
            self.request_inputs.start(context.connection_id, context)?;
        }
        self.diagnostics
            .history
            .begin_subscription(context, enabled);
        if enabled {
            let sessions = self
                .index
                .targets()
                .into_iter()
                .filter_map(|(_, _, _, session)| session)
                .filter(|session| {
                    self.index
                        .diagnostic_scope(session)
                        .is_some_and(|scope| scope.0 == context)
                })
                .collect::<Vec<_>>();
            for session in sessions {
                self.configure_diagnostics(id, &session, deadline).await?;
            }
            self.diagnostics.history.enable(context, true);
        } else {
            self.drain()?;
            self.diagnostics.stop(context);
            self.websockets.close_context(context);
        }
        self.diagnostics_read(context, &Query::default())
    }

    pub(super) fn diagnostic_query_scope(
        &self,
        context: Reference,
        page: Option<Reference>,
    ) -> Result<(), Error> {
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        if let Some(page) = page {
            self.index
                .native(page, Kind::Page)
                .map_err(resource_error)?;
            if !self.index.list(Kind::Page).contains(&(page, Some(context))) {
                return Err("FOREIGN_PAGE".into());
            }
        }
        Ok(())
    }

    pub(super) fn diagnostics_read(
        &mut self,
        context: Reference,
        query: &Query,
    ) -> Result<Value, Error> {
        self.drain()?;
        self.diagnostic_query_scope(context, query.page)?;
        if let Some(worker) = query.worker {
            let (owner, _, _) = self.index.worker_scope(worker).map_err(resource_error)?;
            if owner != context {
                return Err("FOREIGN_WORKER".into());
            }
        }
        self.diagnostics
            .history
            .page(context, query)
            .map_err(Error::from)
    }

    pub(super) fn probe_diagnostics(
        &mut self,
        context: Reference,
        after: u64,
        conditions: &[diagnostics::EventCondition],
    ) -> Result<Option<Value>, Error> {
        self.drain()?;
        self.diagnostic_query_scope(context, None)?;
        for condition in conditions {
            self.diagnostic_query_scope(context, condition.page)?;
        }
        if !self.diagnostics.history.active(context) {
            return Err("DIAGNOSTICS_INACTIVE".into());
        }
        let mut cursor = after;
        loop {
            let read = self.diagnostics.history.read(
                context,
                &Query {
                    after: Some(cursor),
                    limit: 100,
                    ..Default::default()
                },
            )?;
            for event in read["events"].as_array().expect("Diagnostic events") {
                if let Some(index) = conditions
                    .iter()
                    .position(|condition| condition.matches(event))
                {
                    return Ok(Some(
                        json!({"context":context,"matched":true,"conditionIndex":index,"event":event,"after":after,"nextCursor":event["sequence"],"textIncluded":false}),
                    ));
                }
            }
            cursor = read["nextCursor"].as_u64().expect("Diagnostic cursor");
            if read["hasMore"] != true {
                return Ok(None);
            }
        }
    }

    pub(super) fn diagnostic_event(&mut self, event: &Event) {
        self.notice_argument_objects(event);
        let Some(session) = event.session_id.as_deref() else {
            return;
        };
        let Some((context, page, worker)) = self.index.diagnostic_scope(session) else {
            return;
        };
        let startup_paused = self.startup_paused(session);
        let setup_pending = !self.configured.contains(session) || startup_paused;
        let subscription_boundary = setup_pending.then_some(if startup_paused {
            "owned-target-startup-pending"
        } else {
            "new-worker-observed-during-subscription"
        });
        if event.method == "Log.entryAdded" {
            self.diagnostics.discard.insert(session.into());
        }
        if matches!(
            event.method.as_str(),
            "Runtime.consoleAPICalled" | "Runtime.exceptionThrown"
        ) {
            self.diagnostics.discard.insert(session.into());
            // Existing attachments replay old console entries at Runtime.enable.
            // Fresh paused targets can emit live init-script diagnostics before
            // resume is acknowledged. Manual worker attachments also retain
            // their separately identified startup-history boundary.
            if setup_pending
                && !startup_paused
                && !worker.is_some_and(|worker| {
                    self.diagnostics.new_workers.get(&worker) == Some(&context)
                })
            {
                return;
            }
        }
        let recorded = self.diagnostics.history.active(context)
            || event.method.starts_with("Network.") && self.har_archives.capturing(context);
        if !recorded {
            return;
        }
        let p = &event.params;
        let (kind, data) = match event.method.as_str() {
            "Log.entryAdded" => {
                let Some(data) =
                    self.network_log(session, &p["entry"], setup_pending, subscription_boundary)
                else {
                    return;
                };
                (DiagnosticKind::Console, data)
            }
            "Runtime.consoleAPICalled" => {
                let (message, truncated) = console_text(p);
                let location = &p["stackTrace"]["callFrames"][0];
                let arguments = self.capture_console_arguments(context, page, worker, session, p);
                (
                    DiagnosticKind::Console,
                    json!({"level":p["type"],"text":message,"textTruncated":truncated,"location":location_summary(location),"sourceTimestamp":p["timestamp"],"worker":worker,"argumentsRetained":arguments["retained"]==true,"arguments":arguments,"stackAvailable":p["stackTrace"].is_object(),"stack":p.get("stackTrace").filter(|stack|stack.is_object()).map(diagnostics::stacks::cdp),"attachmentSetupPending":setup_pending,"subscriptionBoundary":subscription_boundary}),
                )
            }
            "Runtime.exceptionThrown" => (
                DiagnosticKind::PageError,
                exception_diagnostic(p, worker, setup_pending, subscription_boundary),
            ),
            "Network.requestWillBeSent"
            | "Network.responseReceived"
            | "Network.loadingFinished"
            | "Network.loadingFailed" => {
                self.network_diagnostic(event, context, page, worker);
                return;
            }
            "Network.requestServedFromCache" => {
                if let Some(native) = p["requestId"].as_str()
                    && let Some(request) = self
                        .diagnostics
                        .requests
                        .get_mut(&(session.into(), native.into()))
                {
                    request.served_from_cache = true;
                }
                return;
            }
            "Network.requestWillBeSentExtraInfo" => {
                self.request_cookie_header(event, context);
                self.extra_info(event, false);
                return;
            }
            "Network.responseReceivedExtraInfo" => {
                self.extra_info(event, true);
                return;
            }
            _ => return,
        };
        self.diagnostics.history.push(
            context,
            page,
            kind,
            data,
            crate::browser::capture::now_ms().into(),
        );
    }

    /// Raw request or response headers from Chromium ExtraInfo, which the
    /// pinned SDK's `rawRequestHeaders`/`rawResponseHeaders` and sizes use.
    fn extra_info(&mut self, event: &Event, response: bool) {
        let (Some(session), Some(native)) =
            (event.session_id.clone(), event.params["requestId"].as_str())
        else {
            return;
        };
        let key = (session, native.to_owned());
        let raw = crate::browser::network::raw_headers(&event.params["headers"]);
        let text = event.params["headersText"].as_str().map(str::len);
        if let Some(request) = self.diagnostics.requests.get_mut(&key) {
            if response {
                request.raw_response = raw;
                request.response_headers_text = text;
            } else {
                request.raw_request = raw;
            }
        } else if response {
            if self.diagnostics.early_response_headers.len() < MAX_EARLY_COOKIES {
                self.diagnostics
                    .early_response_headers
                    .insert(key, (raw, text));
            }
        } else if self.diagnostics.early_request_headers.len() < MAX_EARLY_COOKIES {
            self.diagnostics.early_request_headers.insert(key, raw);
        }
    }
    /// Attach Chromium's sent `Cookie` header to a full-mode capture entry.
    fn request_cookie_header(&mut self, event: &Event, context: Reference) {
        if !self.har_archives.capture_full(context) {
            return;
        }
        let (Some(session), Some(native)) =
            (event.session_id.clone(), event.params["requestId"].as_str())
        else {
            return;
        };
        let Some(cookie) = event.params["headers"]
            .as_object()
            .and_then(|headers| {
                headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("cookie"))
            })
            .and_then(|(_, value)| value.as_str())
            .filter(|value| value.len() <= 8192)
        else {
            return;
        };
        let key = (session, native.to_owned());
        if let Some(request) = self.diagnostics.requests.get(&key) {
            if let Ok(id) = serde_json::from_value::<Uuid>(request.data["requestId"].clone()) {
                self.har_archives.capture_details(
                    context,
                    id,
                    &json!({"requestCookieHeader":cookie}),
                );
            }
        } else if self.diagnostics.early_cookies.len() < MAX_EARLY_COOKIES {
            self.diagnostics
                .early_cookies
                .insert(key, cookie.to_owned());
        }
    }
    #[expect(
        clippy::too_many_lines,
        reason = "one match over every network event a diagnostic records"
    )]
    fn network_diagnostic(
        &mut self,
        event: &Event,
        context: Reference,
        page: Option<Reference>,
        worker: Option<Reference>,
    ) {
        let p = &event.params;
        let Some(native) = p["requestId"].as_str() else {
            return;
        };
        let key = (
            event.session_id.clone().expect("Scoped diagnostic"),
            native.into(),
        );
        let initial = event.method == "Network.requestWillBeSent";
        let predecessor = if initial {
            self.diagnostic_redirect(&key, p, context, page)
        } else {
            None
        };
        if !self.diagnostics.requests.contains_key(&key) {
            let request = if initial {
                &p["request"]
            } else {
                &p["response"]
            };
            let mut data = json!({"requestId":Uuid::new_v4(),"url":diagnostics::url(request["url"].as_str().unwrap_or("")),"method":diagnostics::text(request["method"].as_str().unwrap_or("")).0,"resourceType":p["type"],"navigation":p["type"]=="Document","worker":worker,"redirectedFrom":predecessor,"redirectPredecessorUnavailable":p.get("redirectResponse").is_some() && predecessor.is_none(),"initialEventMissing":!initial});
            if initial {
                self.diagnostics.unpaired_inputs.insert(
                    key.clone(),
                    serde_json::from_value(data["requestId"].clone())
                        .expect("Generated request UUID"),
                );
            }
            // Bodies sent during trusted sign-in entry can carry its values.
            let withheld = self.diagnostics.history.withholds_bodies(context);
            if withheld {
                data["requestBodyWithheld"] = json!(true);
            }
            if initial
                && !withheld
                && self.diagnostics.history.active(context)
                && self.request_inputs.enabled(context)
            {
                let id = serde_json::from_value(data["requestId"].clone())
                    .expect("Generated request UUID");
                let captured = crate::browser::http::request_inputs::Snapshot::native(request)
                    .and_then(|snapshot| {
                        self.request_inputs
                            .insert(context.connection_id, context, id, snapshot)
                    });
                match captured {
                    Ok(_) => data["requestInputAvailable"] = json!(true),
                    Err(error) => {
                        data["requestInputAvailable"] = json!(false);
                        data["requestInputUnavailable"] = json!(error.code);
                    }
                }
            }
            let capture_owner = match worker {
                Some(worker)
                    if self
                        .index
                        .worker_scope(worker)
                        .is_ok_and(|(owner, page, kind)| {
                            owner == context
                                && page.is_none()
                                && kind == crate::browser::cdp::resources::WorkerKind::Service
                        }) =>
                {
                    crate::browser::har::RequestOwner::ServiceWorker(worker)
                }
                None => page
                    .map(crate::browser::har::RequestOwner::Page)
                    .unwrap_or(crate::browser::har::RequestOwner::Unattributed),
                Some(_) => crate::browser::har::RequestOwner::Unattributed,
            };
            if initial
                && self.har_archives.capture_accepts(
                    context,
                    capture_owner.page(),
                    request["url"].as_str().unwrap_or(""),
                )
                && let Ok(id) = serde_json::from_value::<Uuid>(data["requestId"].clone())
            {
                let body = if self.har_archives.capture_bodies(context) {
                    super::routing::request_body(request).ok().flatten()
                } else {
                    None
                };
                self.har_archives.capture_request(context,(id,capture_owner),json!({"url":request["url"],"method":request["method"],"headers":crate::browser::har::capture_headers(&request["headers"]),"_testrunWallTime":p["wallTime"]}),request["hasPostData"]==true||request.get("postData").is_some()||request.get("postDataEntries").is_some(),body.as_deref());
                if let Some(cookie) = self.diagnostics.early_cookies.get(&key) {
                    self.har_archives.capture_details(
                        context,
                        id,
                        &json!({"requestCookieHeader":cookie}),
                    );
                }
            }
            if initial {
                self.diagnostics.early_cookies.remove(&key);
            }
            self.diagnostics.insert(
                key.clone(),
                NetworkRequest {
                    context,
                    source_url: request["url"]
                        .as_str()
                        .filter(|url| url.len() <= 8192)
                        .map(str::to_owned),
                    data,
                    headers: crate::browser::network::headers(&request["headers"]),
                    response_headers: Value::Null,
                    complete: false,
                    failed: false,
                    request_time: None,
                    started: initial.then(|| p["timestamp"].as_f64()).flatten(),
                    wall_time: initial.then(|| p["wallTime"].as_f64()).flatten(),
                    served_from_cache: false,
                    response_details: crate::browser::network::response::details(
                        &Value::Null,
                        None,
                        &Value::Null,
                        &Value::Null,
                    ),
                    raw_request: crate::browser::network::raw_headers(&request["headers"]),
                    raw_response: crate::browser::network::raw_headers(&Value::Null),
                    response_headers_text: None,
                    encoded_length: None,
                    request_body_size: if initial {
                        super::routing::request_body(request)
                            .ok()
                            .flatten()
                            .map_or(0, |body| body.len())
                    } else {
                        0
                    },
                    status_text: Value::Null,
                    http_version: Value::Null,
                    frame: p["frameId"].as_str().and_then(|frame| {
                        self.index
                            .event_frame(&event.session_id.clone().unwrap_or_default(), frame)
                    }),
                },
            );
            if let Some(predecessor) = predecessor
                .as_deref()
                .and_then(|id| id.parse::<Uuid>().ok())
                && let Ok(successor) = serde_json::from_value::<Uuid>(
                    self.diagnostics.requests[&key].data["requestId"].clone(),
                )
            {
                self.diagnostics.history.redirected(predecessor, successor);
            }
        }
        let request = self
            .diagnostics
            .requests
            .get_mut(&key)
            .expect("Registered request");
        request.data["sourceTimestamp"] = p["timestamp"].clone();
        let capture_id = serde_json::from_value::<Uuid>(request.data["requestId"].clone()).ok();
        let kind = match event.method.as_str() {
            "Network.requestWillBeSent" => DiagnosticKind::Request,
            "Network.responseReceived" => {
                let response = &p["response"];
                let timing = crate::browser::network::response::cdp_timing(
                    response,
                    request.started,
                    request.wall_time,
                    request.served_from_cache || response["fromDiskCache"] == true,
                );
                request.response_details = crate::browser::network::response::details(
                    &timing,
                    None,
                    &json!({"ipAddress":response["remoteIPAddress"],"port":response["remotePort"]}),
                    &response["securityDetails"],
                );
                request.response_details["responseAvailable"] = json!(true);
                if self.har_archives.capturing(context)
                    && let Some(id) = capture_id
                {
                    self.har_archives.capture_response(context,id,json!({"status":p["response"]["status"],"statusText":p["response"]["statusText"],"headers":crate::browser::har::capture_headers(&p["response"]["headers"]),"httpVersion":p["response"]["protocol"],"_testrunMimeType":p["response"]["mimeType"]}));
                    if self.har_archives.capture_full(context) {
                        let response = &p["response"];
                        let timing = &response["timing"];
                        request.request_time = timing["requestTime"].as_f64();
                        // Pinned Chromium adapter mapping from CDP ResourceTiming.
                        let mut details = json!({"timing":{"domainLookupStart":timing["dnsStart"],"domainLookupEnd":timing["dnsEnd"],"connectStart":timing["connectStart"],"secureConnectionStart":timing["sslStart"],"connectEnd":timing["connectEnd"],"requestStart":timing["sendStart"],"responseStart":timing["receiveHeadersEnd"]},"serverIPAddress":response["remoteIPAddress"],"serverPort":response["remotePort"],"securityDetails":response["securityDetails"]});
                        if let Some(protocol) = response["protocol"].as_str() {
                            details["httpVersion"] =
                                json!(crate::browser::har::http_version(protocol));
                        }
                        self.har_archives.capture_details(context, id, &details);
                    }
                }
                request.data["status"] = p["response"]["status"].clone();
                request.data["fromServiceWorker"] = p["response"]["fromServiceWorker"].clone();
                request.response_headers =
                    crate::browser::network::headers(&p["response"]["headers"]);
                request.status_text = p["response"]["statusText"].clone();
                request.http_version = p["response"]["protocol"]
                    .as_str()
                    .map_or(Value::Null, |protocol| {
                        json!(crate::browser::har::http_version(protocol))
                    });
                // ExtraInfo raw headers, when they arrived first, take precedence.
                if request.raw_response["entries"]
                    .as_array()
                    .is_none_or(Vec::is_empty)
                {
                    request.raw_response =
                        crate::browser::network::raw_headers(&p["response"]["headers"]);
                }
                DiagnosticKind::Response
            }
            "Network.loadingFinished" => {
                finish_response_timing(request, p);
                if let Some(id) = capture_id {
                    let end = request
                        .request_time
                        .zip(p["timestamp"].as_f64())
                        .map(|(start, end)| (end - start) * 1000.0);
                    self.har_archives.capture_details(
                        context,
                        id,
                        &json!({"responseEnd":end,"transferSize":p["encodedDataLength"]}),
                    );
                    self.har_archives.capture_finish(context, id, false);
                }
                request.complete = true;
                request.data["encodedDataLength"] = p["encodedDataLength"].clone();
                request.encoded_length = p["encodedDataLength"]
                    .as_f64()
                    .filter(|length| *length >= 0.0)
                    .map(|length| length as usize);
                DiagnosticKind::RequestFinished
            }
            "Network.loadingFailed" => {
                finish_response_timing(request, p);
                if let Some(id) = capture_id {
                    self.har_archives.capture_finish(context, id, true);
                }
                request.complete = true;
                request.failed = true;
                let (text, truncated) = diagnostics::text(
                    p["errorText"]
                        .as_str()
                        .unwrap_or("Unavailable failure detail"),
                );
                request.data["text"] = json!(text);
                request.data["textTruncated"] = json!(truncated);
                request.data["cancelled"] = p["canceled"].clone();
                request.data["blockedReason"] = p["blockedReason"].clone();
                DiagnosticKind::RequestFailed
            }
            _ => unreachable!(),
        };
        let data = request.data.clone();
        self.push_network(context, page, kind, data);
    }

    fn diagnostic_redirect(
        &mut self,
        key: &(String, String),
        p: &Value,
        context: Reference,
        page: Option<Reference>,
    ) -> Option<String> {
        let mut predecessor = None;
        if let Some(previous) = self.diagnostics.requests.remove(key) {
            if let Ok(id) = serde_json::from_value::<Uuid>(previous.data["requestId"].clone()) {
                self.diagnostics.ids.remove(&id);
            }
            self.diagnostics.order.retain(|entry| entry != key);
            if let Some(response) = p.get("redirectResponse") {
                if self.har_archives.capturing(context)
                    && let Ok(id) =
                        serde_json::from_value::<Uuid>(previous.data["requestId"].clone())
                {
                    self.har_archives.capture_response(context,id,json!({"status":response["status"],"statusText":response["statusText"],"headers":crate::browser::har::capture_headers(&response["headers"]),"httpVersion":response["protocol"]}));
                    self.har_archives
                        .capture_body(context, id, None, "redirect-body-unavailable");
                    self.har_archives.capture_finish(context, id, false);
                }
                predecessor = previous.data["requestId"].as_str().map(str::to_owned);
                let mut data = previous.data.clone();
                data["status"] = response["status"].clone();
                data["fromServiceWorker"] = response["fromServiceWorker"].clone();
                data["redirect"] = json!(true);
                self.push_network(context, page, DiagnosticKind::Response, data.clone());
                self.push_network(context, page, DiagnosticKind::RequestFinished, data.clone());
                // Chromium reuses the request ID for the redirect target. Keep
                // the finished predecessor readable under its own key, as the
                // SDK keeps the predecessor Request and its redirect response.
                let mut finished = previous;
                finished.data = data;
                finished.complete = true;
                finished.response_headers = crate::browser::network::headers(&response["headers"]);
                if finished.raw_response["entries"]
                    .as_array()
                    .is_none_or(Vec::is_empty)
                {
                    finished.raw_response =
                        crate::browser::network::raw_headers(&response["headers"]);
                }
                finished.status_text = response["statusText"].clone();
                // The SDK's redirect response has no transfer or encoded size.
                finished.response_details["responseAvailable"] = json!(true);
                finished.encoded_length = None;
                finished.http_version = response["protocol"]
                    .as_str()
                    .map_or(Value::Null, |protocol| {
                        json!(crate::browser::har::http_version(protocol))
                    });
                let predecessor_key = (
                    key.0.clone(),
                    format!("{}#redirect-{}", key.1, Uuid::new_v4()),
                );
                self.diagnostics.insert(predecessor_key, finished);
            }
        }
        predecessor
    }
    fn push_network(
        &mut self,
        context: Reference,
        page: Option<Reference>,
        kind: DiagnosticKind,
        data: Value,
    ) {
        self.diagnostics.history.push(
            context,
            page,
            kind,
            data,
            crate::browser::capture::now_ms().into(),
        );
    }

    fn network_read_metadata(
        &self,
        context: Reference,
        request_id: Uuid,
        request: &NetworkRequest,
        options: &crate::browser::network::Read,
    ) -> Value {
        let mut result = json!({"context":context,"requestId":request_id,"request":request.data.clone(),"complete":request.complete,"failed":request.failed,"headersIncluded":options.headers,"bodyIncluded":options.body.is_some(),"source":"chrome-cdp-network","headerCompleteness":"Network-request-response-events"});
        // The SDK Request/Response facts every engine's read reports; the
        // pinned Chromium adapter lowercases the CDP resource type.
        result["method"] = request.data["method"].clone();
        result["resourceType"] = request.data["resourceType"]
            .as_str()
            .map_or(Value::Null, |kind| json!(kind.to_lowercase()));
        result["status"] = request.data["status"].clone();
        // SDK Request relations: navigation, owning frame, service worker
        // and the redirect chain in both directions.
        result["navigation"] = request.data["navigation"].clone();
        result["frame"] = json!(request.frame);
        let service_worker = serde_json::from_value::<Reference>(request.data["worker"].clone())
            .ok()
            .and_then(|worker| self.index.worker_scope(worker).ok())
            .is_some_and(|(_, _, kind)| {
                kind == crate::browser::cdp::resources::WorkerKind::Service
            });
        result["serviceWorker"] = json!(service_worker);
        result["redirectedFrom"] = request.data["redirectedFrom"].clone();
        result["redirectedTo"] = json!(self.diagnostics.history.redirected_to(request_id));
        result["statusText"] = request.status_text.clone();
        result["fromServiceWorker"] = request.data["fromServiceWorker"].clone();
        let response_available = request.response_details["responseAvailable"] == true;
        if options.response_details {
            result["responseDetails"] = request.response_details.clone();
            result["responseDetails"]["responseAvailable"] = json!(response_available);
            result["responseDetails"]["source"] =
                json!("native-cdp-resource-timing-and-connection-facts");
            if response_available {
                let url = request.source_url.as_deref().unwrap_or("");
                let method = request.data["method"].as_str().unwrap_or("");
                result["responseDetails"]["httpVersion"] = request.http_version.clone();
                result["responseDetails"]["statusText"] = request.status_text.clone();
                // Pinned SDK Chromium sizes: reported header text and the
                // encoded transfer length once loading finishes.
                result["responseDetails"]["sizes"] = if request.complete {
                    crate::browser::network::sizes(
                        request.request_body_size,
                        crate::browser::network::request_headers_size(
                            &request.raw_request,
                            url,
                            method,
                        ),
                        &request.raw_response,
                        request.status_text.as_str().unwrap_or(""),
                        request.response_headers_text,
                        request.encoded_length,
                    )
                } else {
                    Value::Null
                };
                result["responseDetails"]["sizesPending"] = json!(!request.complete);
            }
        }
        if options.headers {
            result["requestHeaders"] = request.headers.clone();
            result["responseHeaders"] = request.response_headers.clone();
        }
        if options.raw_headers {
            result["rawRequestHeaders"] = request.raw_request.clone();
            if response_available {
                result["rawResponseHeaders"] = request.raw_response.clone();
            }
        }
        result["rawHeadersIncluded"] = json!(options.raw_headers);
        result
    }

    pub(super) fn network_read(
        &mut self,
        id: Uuid,
        context: Reference,
        request_id: Uuid,
        options: crate::browser::network::Read,
        deadline: Instant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + '_>>
    {
        Box::pin(async move {
            use crate::browser::network::{self, BodySide};
            self.drain()?;
            self.index
                .native(context, Kind::Context)
                .map_err(resource_error)?;
            let key = self
                .diagnostics
                .ids
                .get(&request_id)
                .cloned()
                .ok_or("NETWORK_REQUEST_EXPIRED")?;
            let request = self
                .diagnostics
                .requests
                .get(&key)
                .cloned()
                .ok_or("NETWORK_REQUEST_EXPIRED")?;
            if request.context != context {
                return Err("NETWORK_REQUEST_EXPIRED".into());
            }
            let mut result = self.network_read_metadata(context, request_id, &request, &options);
            if let Some(side) = options.body {
                if matches!(side, BodySide::Request) && request.data["requestBodyWithheld"] == true
                {
                    return Err(network::body_withheld());
                }
                self.network_evidence.capacity()?;
                if !request.complete {
                    return Err("BODY_NOT_FINISHED".into());
                }
                if request.failed {
                    return Err("BODY_REQUEST_FAILED".into());
                }
                let captured = if matches!(side, BodySide::Request)
                    && request.data["requestInputAvailable"] == true
                {
                    Some(
                        self.request_inputs
                            .get(context.connection_id, request_id)
                            .map_err(|_| Error::from("BODY_UNAVAILABLE"))?,
                    )
                } else {
                    None
                };
                let bytes = if let Some(snapshot) = captured {
                    result["requestBodyFidelity"] = json!(snapshot.body_fidelity());
                    snapshot.body().ok_or("BODY_UNAVAILABLE")?.to_vec()
                } else {
                    let method = match side {
                        BodySide::Request => "Network.getRequestPostData",
                        BodySide::Response => "Network.getResponseBody",
                    };
                    let reply = self
                        .call_read(
                            id,
                            Some(&key.0),
                            method,
                            json!({"requestId":key.1}),
                            deadline,
                        )
                        .await
                        .map_err(|error| {
                            if error.code == "BROWSER_METHOD_FAILED" {
                                Error::from("BODY_UNAVAILABLE")
                            } else {
                                error
                            }
                        })?;
                    match side {
                        BodySide::Request => {
                            result["requestBodyFidelity"] = json!("cdp-text-utf8-not-lossless");
                            reply["postData"]
                                .as_str()
                                .ok_or("BODY_UNAVAILABLE")?
                                .as_bytes()
                                .to_vec()
                        }
                        BodySide::Response => {
                            let body = reply["body"].as_str().ok_or("BODY_UNAVAILABLE")?;
                            network::decode_payload(
                                body,
                                reply["base64Encoded"] == true,
                                options.max_bytes,
                            )?
                        }
                    }
                };
                let artifact = network::write_body(
                    self.journal.directory(),
                    context,
                    request_id,
                    side,
                    &bytes,
                    options.max_bytes,
                )?;
                result["body"] = self.network_evidence.insert(artifact)?;
            }
            Ok(result)
        })
    }

    pub(super) async fn har_capture_payload(
        &mut self,
        id: Uuid,
        context: Reference,
        request_id: Uuid,
        deadline: Instant,
    ) -> Result<Vec<u8>, Error> {
        let key = self
            .diagnostics
            .ids
            .get(&request_id)
            .cloned()
            .ok_or("NETWORK_REQUEST_EXPIRED")?;
        let request = self
            .diagnostics
            .requests
            .get(&key)
            .ok_or("NETWORK_REQUEST_EXPIRED")?;
        if request.context != context || !request.complete || request.failed {
            return Err("BODY_UNAVAILABLE".into());
        }
        let reply = self
            .call_read(
                id,
                Some(&key.0),
                "Network.getResponseBody",
                json!({"requestId":key.1}),
                deadline,
            )
            .await?;
        if self.diagnostics.ids.get(&request_id) != Some(&key) {
            return Err("NETWORK_REQUEST_EXPIRED".into());
        }
        crate::browser::network::decode_payload(
            reply["body"].as_str().ok_or("BODY_UNAVAILABLE")?,
            reply["base64Encoded"] == true,
            crate::browser::har::MAX_BODY,
        )
    }

    pub(super) async fn discard_diagnostic_objects(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        self.release_unretained_arguments(deadline).await;
        let sessions = std::mem::take(&mut self.diagnostics.discard);
        for session in sessions {
            if !self.index.session_attached(&session) {
                self.argument_console_group_released(&session);
                continue;
            }
            if self.argument_objects_retained(&session) {
                continue;
            }
            if !self.argument_groups.active_groups.insert(session.clone()) {
                self.diagnostics.discard.insert(session);
                continue;
            }
            self.argument_groups
                .releasing_groups
                .insert(session.clone());
            // This cleanup cannot depend on a page expression or ordinary lane;
            // Runtime.releaseObjectGroup runs while JavaScript is paused.
            // Discarding console entries would also reset application counters.
            let released = self
                .cleanup_call(
                    Some(&session),
                    "Runtime.releaseObjectGroup",
                    json!({"objectGroup":"console"}),
                    deadline,
                )
                .await;
            self.argument_groups.active_groups.remove(&session);
            if released.is_err() {
                self.diagnostics.discard.insert(session);
            } else {
                self.argument_console_group_released(&session);
            }
        }
    }
}

fn location_summary(location: &Value) -> Value {
    json!({"url":diagnostics::url(location["url"].as_str().unwrap_or("")),"line":location["lineNumber"],"column":location["columnNumber"]})
}

fn console_text(p: &Value) -> (String, bool) {
    let mut message = String::new();
    let mut truncated = false;
    if let Some(args) = p["args"].as_array() {
        for argument in args {
            let value = argument["value"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| argument.get("value").map(Value::to_string))
                .unwrap_or_else(|| {
                    argument["description"]
                        .as_str()
                        .unwrap_or("[object]")
                        .into()
                });
            if !message.is_empty() {
                message.push(' ');
            }
            let bounded = diagnostics::text(&value);
            truncated |= bounded.1;
            message.push_str(&bounded.0);
            let bounded = diagnostics::text(&message);
            truncated |= bounded.1;
            message = bounded.0;
            if truncated {
                break;
            }
        }
    }
    (message, truncated)
}
