//! Explicit context subscriptions and scoped diagnostic event projection.

use super::*;
use crate::browser::{
    diagnostics::{self, DiagnosticKind, Query},
    objects::ObjectRef,
};

impl Host {
    pub(super) async fn diagnostics_wait(
        &self,
        id: Uuid,
        context: Reference,
        after: u64,
        conditions: Vec<diagnostics::EventCondition>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.connection()?.event_barrier(deadline).await?;
        let mut cursor = after;
        let result = async {
            loop {
                let changed = self.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if Instant::now() >= deadline { return Err(Error::from("WAIT_EXPIRED")); }
                self.admissible(id, deadline)?;
                {
                    let state = self.state.lock().expect("Browser owner");
                    state.resource(context, Some(Kind::Context))?;
                    for condition in &conditions {
                        if let Some(page) = condition.page {
                            state.resource(page, Some(Kind::Page))?;
                            if state.ancestor(page, Kind::Context) != Some(context) { return Err("FOREIGN_PAGE".into()); }
                        }
                    }
                }
                let read = self.diagnostics.lock().expect("Diagnostics").read(context, &Query {
                    after: Some(cursor), limit:100, ..Default::default()
                })?;
                if read["active"] != true { return Err("DIAGNOSTICS_INACTIVE".into()); }
                for event in read["events"].as_array().expect("Diagnostic events") {
                    if let Some(index) = conditions.iter().position(|condition|condition.matches(event)) {
                        return Ok(json!({"context":context,"matched":true,"conditionIndex":index,"event":event,"after":after,"nextCursor":event["sequence"],"textIncluded":false}));
                    }
                }
                cursor = read["nextCursor"].as_u64().expect("Diagnostic cursor");
                if let Some(operation) = self.state.lock().expect("Browser owner").operations.get_mut(&id) {
                    operation.result = Some(json!({"context":context,"waiting":true,"after":after,"nextCursor":cursor}));
                }
                if read["hasMore"] == true { continue; }
                tokio::select! {
                    () = &mut changed => {},
                    () = tokio::time::sleep_until(deadline) => return Err(Error::from("WAIT_EXPIRED")),
                }
            }
        }.await;
        result.map_err(|mut error: Error| {
            error.partial = Some(json!({"context":context,"matched":false,"after":after,"nextCursor":cursor,"nextAction":"diagnostics_read"}));
            error
        })
    }

    pub(super) async fn diagnostics_subscribe(
        &self,
        context: Reference,
        enabled: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let wire = self.wire(context, Kind::Context)?;
        let connection = self.connection()?;
        if enabled {
            self.request_inputs
                .lock()
                .expect("Request input owner")
                .start(context.connection_id, context)?;
        }
        // Admit events before subscription replies; partial setup stays owned
        // and can be stopped or retried explicitly without touching other contexts.
        self.diagnostics
            .lock()
            .expect("Diagnostics")
            .begin_subscription(context, enabled);
        for event in diagnostics::SUBSCRIPTIONS {
            let subscribed = enabled
                || (*event != "console"
                    && self
                        .har_archives
                        .lock()
                        .expect("HAR owner")
                        .capturing(context));
            connection
                .retained(
                    &wire,
                    "updateSubscription",
                    json!({"event":event,"enabled":subscribed}),
                    deadline,
                    false,
                )
                .await?;
        }
        connection.event_barrier(deadline).await?;
        self.diagnostics
            .lock()
            .expect("Diagnostics")
            .enable(context, enabled);
        self.diagnostics_read(context, Query::default(), deadline)
            .await
    }
    pub(super) async fn diagnostics_read(
        &self,
        context: Reference,
        query: Query,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.connection()?.event_barrier(deadline).await?;
        {
            let state = self.state.lock().expect("Browser owner");
            state.resource(context, Some(Kind::Context))?;
            if let Some(page) = query.page {
                state.resource(page, Some(Kind::Page))?;
                if state.ancestor(page, Kind::Context) != Some(context) {
                    return Err("FOREIGN_PAGE".into());
                }
            }
            if let Some(worker) = query.worker {
                state.resource(worker, Some(Kind::Worker))?;
                if state.ancestor(worker, Kind::Context) != Some(context) {
                    return Err("FOREIGN_WORKER".into());
                }
            }
        }
        self.diagnostics
            .lock()
            .expect("Diagnostics")
            .page(context, &query)
            .map_err(Error::from)
    }

    pub(super) async fn diagnostic_event(
        &self,
        context: Reference,
        method: &str,
        params: &Value,
    ) -> Result<(), Error> {
        let recorded = self
            .diagnostics
            .lock()
            .expect("Diagnostics")
            .active(context)
            || matches!(
                method,
                "request" | "response" | "requestFinished" | "requestFailed"
            ) && self
                .har_archives
                .lock()
                .expect("HAR owner")
                .capturing(context);
        if !recorded {
            if method == "console" {
                self.dispose_console_arguments(params).await?;
            }
            return Ok(());
        }
        let connection = self.connection()?;
        let page = if let Some(guid) = params["page"]["guid"].as_str() {
            let wire = connection.resolve(guid, "Page").await?;
            let state = self.state.lock().expect("Browser owner");
            state
                .by_wire(&wire)
                .filter(|resource| {
                    state.ancestor(resource.reference, Kind::Context) == Some(context)
                })
                .map(|resource| resource.reference)
        } else {
            None
        };
        let worker = if let Some(guid) = params["worker"]["guid"].as_str() {
            let wire = connection.resolve(guid, "Worker").await?;
            let mut state = self.state.lock().expect("Browser owner");
            if let Some(resource) = state.by_wire(&wire) {
                (state.ancestor(resource.reference, Kind::Context) == Some(context))
                    .then_some(resource.reference)
            } else if let Some(page) = page {
                match state.issue(Kind::Worker, wire, Some(page)) {
                    Ok(reference) => Some(reference),
                    Err("STALE_REFERENCE") => None,
                    Err(code) => return Err(code.into()),
                }
            } else {
                None
            }
        } else {
            None
        };
        let (kind, data) = match method {
            "console" => {
                let (text, truncated) = diagnostics::text(params["text"].as_str().unwrap_or(""));
                let arguments = self
                    .capture_console_arguments(context, page, worker, params)
                    .await;
                self.dispose_console_arguments(params).await?;
                (
                    DiagnosticKind::Console,
                    json!({"level":diagnostics::text(params["type"].as_str().unwrap_or("unknown")).0,"text":text,"textTruncated":truncated,"location":{"url":diagnostics::url(params["location"]["url"].as_str().unwrap_or("")),"line":params["location"]["lineNumber"],"column":params["location"]["columnNumber"]},"sourceTimestamp":params["timestamp"],"worker":worker,"workerIdentityUnavailable":params.get("worker").is_some()&&worker.is_none(),"argumentsRetained":arguments["retained"]==true,"arguments":arguments}),
                )
            }
            "pageError" => {
                let stack = params["error"]["error"]["stack"]
                    .as_str()
                    .filter(|stack| !stack.is_empty());
                let (text, truncated) = diagnostics::text(
                    params["error"]["error"]["message"]
                        .as_str()
                        .unwrap_or("Non-Error exception"),
                );
                (
                    DiagnosticKind::PageError,
                    json!({"name":diagnostics::text(params["error"]["error"]["name"].as_str().unwrap_or("Error")).0,"text":text,"textTruncated":truncated,"location":{"url":diagnostics::url(params["location"]["url"].as_str().unwrap_or("")),"line":params["location"]["line"],"column":params["location"]["column"]},"stackAvailable":stack.is_some(),"stack":stack.map(diagnostics::stacks::engine)}),
                )
            }
            _ => {
                self.network_diagnostic(context, page, method, params)
                    .await?
            }
        };
        self.diagnostics.lock().expect("Diagnostics").push(
            context,
            page,
            kind,
            data,
            super::page::now_ms(),
        );
        Ok(())
    }

    async fn dispose_console_arguments(&self, params: &Value) -> Result<(), Error> {
        let connection = self.connection()?;
        if let Some(arguments) = params["args"].as_array() {
            for argument in arguments {
                let guid = argument["guid"].as_str().ok_or("MISSING_HANDLE")?;
                let wire = match connection.resolve(guid, "JSHandle").await {
                    Ok(wire) => wire,
                    Err(_) => connection.resolve(guid, "ElementHandle").await?,
                };
                // Never dispose a handle already owned by a public operation.
                if !self.diagnostic_argument_owned(&wire)
                    && self
                        .state
                        .lock()
                        .expect("Browser owner")
                        .by_wire(&wire)
                        .is_none()
                {
                    self.dispose_element(&wire).await?;
                }
            }
        }
        Ok(())
    }

    /// Captures a new request's input while its context collects them,
    /// unless it was sent during trusted sign-in entry.
    fn capture_request_input(
        &self,
        context: Reference,
        id: Uuid,
        wire: &crate::browser::objects::ObjectRef,
        init: &Value,
        data: &mut Value,
    ) {
        let (withheld, active) = {
            let diagnostics = self.diagnostics.lock().expect("Diagnostics");
            (
                diagnostics.request_body_withheld(wire),
                diagnostics.active(context),
            )
        };
        if withheld {
            data["requestBodyWithheld"] = json!(true);
            return;
        }
        let mut inputs = self.request_inputs.lock().expect("Request input owner");
        if !active || !inputs.enabled(context) {
            return;
        }
        let captured = crate::browser::http::request_inputs::Snapshot::pinned(init)
            .and_then(|snapshot| inputs.insert(context.connection_id, context, id, snapshot));
        match captured {
            Ok(_) => data["requestInputAvailable"] = json!(true),
            Err(error) => {
                data["requestInputAvailable"] = json!(false);
                data["requestInputUnavailable"] = json!(error.code);
            }
        }
    }
    async fn network_diagnostic(
        &self,
        context: Reference,
        page: Option<Reference>,
        method: &str,
        params: &Value,
    ) -> Result<(DiagnosticKind, Value), Error> {
        let connection = self.connection()?;
        let (wire, response) = Self::diagnostic_request(&connection, method, params).await?;
        let init = connection
            .initializer(&wire)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        let redirect = self.redirected_request(&init).await?;
        let (id, missing_initial) = self
            .diagnostics
            .lock()
            .expect("Diagnostics")
            .request(wire.clone(), method == "request");
        if method == "request"
            && let Some(predecessor) = redirect
        {
            self.diagnostics
                .lock()
                .expect("Diagnostics")
                .redirected(predecessor, id);
        }
        self.diagnostics.lock().expect("Diagnostics").scope_request(
            &wire,
            context,
            method == "response",
            matches!(method, "requestFinished" | "requestFailed"),
        );
        if matches!(method, "requestFinished" | "requestFailed") {
            self.diagnostics
                .lock()
                .expect("Diagnostics")
                .finish_timing(&wire, &params["responseEndTiming"]);
        }
        self.capture_network_diagnostic(
            context,
            (id, page),
            method,
            &init,
            response.as_ref(),
            params,
        );
        let mut data = json!({"requestId":id,"url":diagnostics::url(init["url"].as_str().unwrap_or("")),"method":diagnostics::text(init["method"].as_str().unwrap_or("")).0,"resourceType":diagnostics::text(init["resourceType"].as_str().unwrap_or("")).0,"navigation":init["isNavigationRequest"],"redirectedFrom":redirect,"redirectPredecessorUnavailable":init.get("redirectedFrom").is_some() && redirect.is_none(),"initialEventMissing":missing_initial,"serviceWorker":init.get("serviceWorker").is_some()});
        let kind = match method {
            "request" => {
                self.capture_request_input(context, id, &wire, &init, &mut data);
                DiagnosticKind::Request
            }
            "response" => {
                let response = response.expect("Response initialized above");
                data["status"] = response["status"].clone();
                data["fromServiceWorker"] = response["fromServiceWorker"].clone();
                DiagnosticKind::Response
            }
            "requestFinished" => DiagnosticKind::RequestFinished,
            "requestFailed" => {
                let (text, truncated) = diagnostics::text(
                    params["failureText"]
                        .as_str()
                        .unwrap_or("Unavailable failure detail"),
                );
                data["text"] = json!(text);
                data["textTruncated"] = json!(truncated);
                DiagnosticKind::RequestFailed
            }
            _ => return Err("UNKNOWN_DIAGNOSTIC_EVENT".into()),
        };
        Ok((kind, data))
    }

    async fn diagnostic_request(
        connection: &Connection,
        method: &str,
        params: &Value,
    ) -> Result<(crate::browser::objects::ObjectRef, Option<Value>), Error> {
        Ok(if method == "response" {
            let response = connection
                .resolve(
                    params["response"]["guid"]
                        .as_str()
                        .ok_or("MISSING_RESPONSE")?,
                    "Response",
                )
                .await?;
            let init = connection
                .initializer(&response)
                .await?
                .ok_or("MISSING_INITIALIZER")?;
            (
                connection
                    .resolve(
                        init["request"]["guid"].as_str().ok_or("MISSING_REQUEST")?,
                        "Request",
                    )
                    .await?,
                Some(init),
            )
        } else {
            (
                connection
                    .resolve(
                        params["request"]["guid"]
                            .as_str()
                            .ok_or("MISSING_REQUEST")?,
                        "Request",
                    )
                    .await?,
                None,
            )
        })
    }

    fn capture_network_diagnostic(
        &self,
        context: Reference,
        (id, page): (Uuid, Option<Reference>),
        method: &str,
        init: &Value,
        response: Option<&Value>,
        params: &Value,
    ) {
        if self
            .har_archives
            .lock()
            .expect("HAR owner")
            .capturing(context)
        {
            match method {
                "request" => {
                    use base64::{Engine, engine::general_purpose::STANDARD};
                    let mut archives = self.har_archives.lock().expect("HAR owner");
                    if !archives.capture_accepts(context, page, init["url"].as_str().unwrap_or(""))
                    {
                        return;
                    }
                    let body = init["postData"]
                        .as_str()
                        .filter(|_| archives.capture_bodies(context))
                        .filter(|body| body.len() <= crate::browser::har::MAX_BODY.div_ceil(3) * 4)
                        .and_then(|body| STANDARD.decode(body).ok())
                        .filter(|body| body.len() <= crate::browser::har::MAX_BODY);
                    let owner = page
                        .map(crate::browser::har::RequestOwner::Page)
                        .unwrap_or(crate::browser::har::RequestOwner::Unattributed);
                    archives.capture_request(context,(id,owner),json!({"url":init["url"],"method":init["method"],"headers":init["headers"]}),init.get("postData").is_some(),body.as_deref());
                    if let Some(encoded) = init["postData"].as_str() {
                        // Decoded length of the driver's Base64 post data, without retaining it.
                        let padding = encoded
                            .bytes()
                            .rev()
                            .take_while(|byte| *byte == b'=')
                            .count();
                        let size = (encoded.len() / 4 * 3).saturating_sub(padding);
                        archives.capture_details(context, id, &json!({"requestBodySize":size}));
                    }
                }
                "response" => {
                    if let Some(response) = response {
                        self.har_archives.lock().expect("HAR owner").capture_response(context,id,json!({"status":response["status"],"statusText":response["statusText"],"headers":response["headers"],"_testrunTiming":response["timing"]}));
                    }
                }
                "requestFinished" | "requestFailed" => {
                    let mut archives = self.har_archives.lock().expect("HAR owner");
                    archives.capture_finish(context, id, method == "requestFailed");
                    if let Some(end) = params["responseEndTiming"].as_f64() {
                        archives.capture_details(context, id, &json!({"responseEnd":end}));
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) async fn redirected_request(&self, init: &Value) -> Result<Option<Uuid>, Error> {
        let Some(guid) = init["redirectedFrom"]["guid"].as_str() else {
            return Ok(None);
        };
        let wire: ObjectRef = self.connection()?.resolve(guid, "Request").await?;
        Ok(self
            .diagnostics
            .lock()
            .expect("Diagnostics")
            .known_request(&wire))
    }
}
