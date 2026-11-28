//! Independent context network subscriptions and bounded private capture finalization.
use super::*;
const EVENTS: &[&str] = &["request", "response", "requestFinished", "requestFailed"];
impl Host {
    pub(super) async fn har_capture_start(
        &self,
        context: Reference,
        page: Option<Reference>,
        options: (
            crate::browser::contract::HarCaptureContent,
            crate::browser::contract::HarCaptureMode,
            Option<crate::browser::har::Export>,
        ),
        replaces: Option<Uuid>,
        filter: Option<crate::browser::routing::Matcher>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let wire = self.wire(context, Kind::Context)?;
        if let Some(page) = page {
            let state = self.state.lock().expect("Browser owner");
            state.resource(page, Some(Kind::Page))?;
            if state.ancestor(page, Kind::Context) != Some(context) {
                return Err("FOREIGN_PAGE".into());
            }
        }
        self.prune_har_rules();
        if replaces.is_some_and(|id| self.routing.rules.lock().expect("Route owner").used(id)) {
            return Err("HAR_ARCHIVE_IN_USE".into());
        }
        let summary = self
            .har_archives
            .lock()
            .expect("HAR owner")
            .capture_start(context, page, options, replaces, filter)?;
        let result = async {
            let connection = self.connection()?;
            for event in EVENTS {
                connection
                    .retained(
                        &wire,
                        "updateSubscription",
                        json!({"event":event,"enabled":true}),
                        deadline,
                        false,
                    )
                    .await?;
            }
            connection.event_barrier(deadline).await?;
            self.wire(context, Kind::Context)?;
            Ok::<_, Error>(())
        }
        .await;
        if let Err(mut error) = result {
            self.har_archives
                .lock()
                .expect("HAR owner")
                .capture_context_gap(context, 1);
            error.partial = Some(
                json!({"capture":summary,"setupPending":true,"nextAction":"har_capture_stop"}),
            );
            return Err(error);
        }
        Ok(summary)
    }
    pub(super) async fn har_capture_stop(
        &self,
        context: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.har_capture_stop_inner(context,deadline).await.map_err(|mut error|{
            if error.partial.is_none() && let Ok(capture)=self.har_archives.lock().expect("HAR owner").capture_status(context){error.partial=Some(json!({"capture":capture,"cleanupPending":true,"nextAction":"retry-stop-or-disconnect"}));}error
        })
    }
    async fn har_capture_stop_inner(
        &self,
        context: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if let Ok(connection) = self.connection() {
            connection.event_barrier(deadline).await?;
        }
        let pending = self
            .har_archives
            .lock()
            .expect("HAR owner")
            .capture_stop(context)?;
        for request in pending {
            let result = self.har_capture_payload(context, request, deadline).await;
            let mut archives = self.har_archives.lock().expect("HAR owner");
            match result {
                Ok(bytes) => archives.capture_body(context, request, Some(&bytes), ""),
                Err(error) => archives.capture_body(context, request, None, error.code),
            }
        }
        let pending = self
            .har_archives
            .lock()
            .expect("HAR owner")
            .capture_pending_details(context);
        for request in pending {
            // Details are optional evidence: an unavailable reply leaves the pinned unknown value.
            if let Ok(details) = self.har_capture_details(context, request, deadline).await {
                self.har_archives
                    .lock()
                    .expect("HAR owner")
                    .capture_details(context, request, &details);
            }
        }
        let pages = self
            .har_archives
            .lock()
            .expect("HAR owner")
            .capture_pages(context);
        for page in pages {
            // A closed or unreadable page keeps the pinned initial page values.
            let Ok(frame) = self.page_scope(page, None, deadline).await else {
                continue;
            };
            if let Ok(details) = self
                .evaluate(
                    &frame.wire,
                    crate::browser::har::PAGE_DETAILS,
                    Value::Null,
                    deadline,
                )
                .await
            {
                self.har_archives
                    .lock()
                    .expect("HAR owner")
                    .capture_page_details(context, page, &details);
            }
        }
        if let Ok(wire) = self.wire(context, Kind::Context) {
            let enabled = self
                .diagnostics
                .lock()
                .expect("Diagnostics")
                .active(context);
            let connection = self.connection()?;
            for event in EVENTS {
                connection
                    .retained(
                        &wire,
                        "updateSubscription",
                        json!({"event":event,"enabled":enabled}),
                        deadline,
                        false,
                    )
                    .await?;
            }
            connection.event_barrier(deadline).await?;
        }
        self.har_archives
            .lock()
            .expect("HAR owner")
            .capture_publish(self.journal.directory(), context)
    }
    async fn har_capture_response(
        &self,
        context: Reference,
        request_id: Uuid,
        deadline: Instant,
    ) -> Result<crate::browser::objects::ObjectRef, Error> {
        let (request, response, complete) = self
            .diagnostics
            .lock()
            .expect("Diagnostics")
            .resolve_request(context, request_id)?;
        if !response || !complete {
            return Err("BODY_NOT_FINISHED".into());
        }
        let connection = self.connection()?;
        let value = connection
            .retained(&request, "response", json!({}), deadline, false)
            .await?
            .ok_or("BODY_UNAVAILABLE")?;
        Ok(connection
            .resolve(
                value["response"]["guid"]
                    .as_str()
                    .ok_or("BODY_UNAVAILABLE")?,
                "Response",
            )
            .await?)
    }
    /// Pinned full-mode Response facts; each missing reply leaves its field unknown.
    async fn har_capture_details(
        &self,
        context: Reference,
        request_id: Uuid,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let response = self
            .har_capture_response(context, request_id, deadline)
            .await?;
        let connection = self.connection()?;
        let mut details = json!({});
        let call = async |method: &str| {
            connection
                .retained(&response, method, json!({}), deadline, false)
                .await
                .ok()
                .flatten()
                .unwrap_or(Value::Null)
        };
        // Pinned tracer cookies come from the sent headers, not provisional ones.
        let (request, _, _) = self
            .diagnostics
            .lock()
            .expect("Diagnostics")
            .resolve_request(context, request_id)?;
        if let Some(cookie) = connection
            .retained(&request, "rawRequestHeaders", json!({}), deadline, false)
            .await
            .ok()
            .flatten()
            .and_then(|raw| {
                raw["headers"].as_array().and_then(|headers| {
                    headers
                        .iter()
                        .find(|header| {
                            header["name"]
                                .as_str()
                                .is_some_and(|name| name.eq_ignore_ascii_case("cookie"))
                        })
                        .and_then(|header| header["value"].as_str().map(str::to_owned))
                })
            })
        {
            details["requestCookieHeader"] = json!(cookie);
        }
        let address = call("serverAddr").await;
        details["serverIPAddress"] = address["value"]["ipAddress"].clone();
        details["serverPort"] = address["value"]["port"].clone();
        details["securityDetails"] = call("securityDetails").await["value"].clone();
        if let Some(version) = call("httpVersion").await["value"].as_str() {
            details["httpVersion"] = json!(crate::browser::har::http_version(version));
        }
        let sizes = call("sizes").await;
        for (from, to) in [
            ("requestHeadersSize", "requestHeadersSize"),
            ("responseHeadersSize", "responseHeadersSize"),
            ("responseBodySize", "responseBodySize"),
        ] {
            details[to] = sizes["sizes"][from].clone();
        }
        Ok(details)
    }
    async fn har_capture_payload(
        &self,
        context: Reference,
        request_id: Uuid,
        deadline: Instant,
    ) -> Result<Vec<u8>, Error> {
        let response = self
            .har_capture_response(context, request_id, deadline)
            .await?;
        let connection = self.connection()?;
        let initializer = connection
            .initializer(&response)
            .await?
            .ok_or("BODY_UNAVAILABLE")?;
        crate::browser::network::bounded_identity_length(
            &initializer["headers"],
            crate::browser::har::MAX_BODY,
        )?;
        let value = connection
            .retained(&response, "body", json!({}), deadline, false)
            .await?
            .ok_or("BODY_UNAVAILABLE")?;
        crate::browser::network::decode_payload(
            value["binary"].as_str().ok_or("BODY_UNAVAILABLE")?,
            true,
            crate::browser::har::MAX_BODY,
        )
    }
}
