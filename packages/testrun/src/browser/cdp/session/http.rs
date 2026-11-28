//! Independent HTTP tasks preserve callback responsiveness and retained response finalization.
use super::*;
use crate::browser::http::{self, Ready};
use futures_util::FutureExt;
mod pending;
pub(super) use pending::PendingFetch;
struct BrowserCookieAccess {
    connection: Connection,
    context: String,
    runtime: tokio::runtime::Handle,
}
impl http::browser::BrowserCookies for BrowserCookieAccess {
    fn partition_identity_available(&self) -> bool {
        true
    }
    fn partition_model(&self) -> http::browser::PartitionModel {
        http::browser::PartitionModel::SiteAndAncestor
    }
    fn read(&self, deadline: Instant) -> Result<Value, Error> {
        self.runtime
            .block_on(self.connection.call(
                None,
                "Storage.getCookies",
                json!({"browserContextId":self.context}),
                Lane::Ordinary,
                deadline,
            ))
            .map(|reply| reply["cookies"].clone())
            .map_err(protocol_error)
            .and_then(normalize_cookie_keys)
    }
    fn write(&self, cookies: Value, deadline: Instant) -> Result<(), Error> {
        let cookies = cookies_for_write(cookies)?;
        self.runtime
            .block_on(self.connection.call(
                None,
                "Storage.setCookies",
                json!({"browserContextId":self.context,"cookies":cookies}),
                Lane::Ordinary,
                deadline,
            ))
            .map(|_| ())
            .map_err(protocol_error)
    }
}
pub(super) fn cookies_for_write(mut cookies: Value) -> Result<Value, Error> {
    for cookie in cookies
        .as_array_mut()
        .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?
    {
        if let Some(site) = cookie["partitionKey"].as_str().map(str::to_owned) {
            let ancestor = cookie["_crHasCrossSiteAncestor"].as_bool().unwrap_or(true);
            cookie["partitionKey"] = json!({"topLevelSite":site,"hasCrossSiteAncestor":ancestor});
            cookie
                .as_object_mut()
                .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?
                .remove("_crHasCrossSiteAncestor");
        }
        if cookie["expires"].as_f64() == Some(-1.0) {
            cookie
                .as_object_mut()
                .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?
                .remove("expires");
        }
    }
    Ok(cookies)
}
fn normalize_cookie_keys(mut cookies: Value) -> Result<Value, Error> {
    for cookie in cookies
        .as_array_mut()
        .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?
    {
        if let Some(key) = cookie
            .get("partitionKey")
            .filter(|key| !key.is_null())
            .cloned()
        {
            let site = key["topLevelSite"]
                .as_str()
                .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
            let ancestor = key["hasCrossSiteAncestor"]
                .as_bool()
                .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
            cookie["partitionKey"] = json!(site);
            cookie["_crHasCrossSiteAncestor"] = json!(ancestor);
        }
    }
    Ok(cookies)
}
pub(super) fn cookie_view(cookies: Value) -> Result<Vec<Value>, Error> {
    let Value::Array(mut cookies) = normalize_cookie_keys(cookies)? else {
        return Err("HTTP_BROWSER_COOKIE_PROTOCOL".into());
    };
    for cookie in &mut cookies {
        let fields = cookie
            .as_object_mut()
            .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
        fields.retain(|key, _| {
            [
                "name",
                "value",
                "domain",
                "path",
                "expires",
                "httpOnly",
                "secure",
                "sameSite",
                "partitionKey",
                "_crHasCrossSiteAncestor",
            ]
            .contains(&key.as_str())
        });
        fields.entry("sameSite").or_insert(json!("Lax"));
    }
    Ok(cookies)
}
impl Worker {
    pub(super) fn http_request_input(
        &self,
        id: Uuid,
        options: http::request_inputs::Overrides,
    ) -> Result<http::Fetch, Error> {
        let connection = self
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .connection;
        self.request_inputs.get(connection, id)?.fetch(options)
    }
    #[expect(clippy::too_many_lines, reason = "one match over every HTTP operation")]
    pub(super) fn http_operation(
        &mut self,
        operation: Uuid,
        request: Request,
        deadline: Instant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + '_>>
    {
        Box::pin(async move {
            // `route_fetch` is an `http_fetch` of the pending callback's private request.
            let options = http::RouteFetchOptions::of(&request);
            let request = match request {
                Request::HttpFetchRequest {
                    http_context,
                    source_request_id,
                    options,
                } => Request::HttpFetch {
                    http_context,
                    request: self.http_request_input(source_request_id, options)?,
                },
                Request::RouteFetch {
                    callback,
                    http_context,
                    ..
                } => {
                    let paused = self
                        .interactions
                        .routes
                        .get(&callback)
                        .ok_or("STALE_REFERENCE")?
                        .effective_request()?;
                    Request::HttpFetch {
                        http_context,
                        request: http::route_fetch_request(
                            &paused,
                            options.as_ref().expect("route_fetch options"),
                        )?,
                    }
                }
                request => request,
            };
            match request {
                Request::HttpRequestInputsSet { context, enabled } => {
                    self.index
                        .native(context, Kind::Context)
                        .map_err(resource_error)?;
                    self.observation_ready(operation, deadline)?;
                    let connection = self
                        .shared
                        .state
                        .lock()
                        .expect("Chrome lifecycle")
                        .connection;
                    self.request_inputs.set(connection, context, enabled)?;
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
                        if let Err(mut error) = self
                            .configure_diagnostics(operation, &session, deadline)
                            .await
                        {
                            error.partial = Some(
                                json!({"context":context,"enabled":enabled,"requestInputs":self.request_inputs.summary(),"budgetConfigured":false,"replayed":false}),
                            );
                            return Err(error);
                        }
                    }
                    Ok(
                        json!({"context":context,"enabled":enabled,"requestInputs":self.request_inputs.summary(),"budgetConfigured":true}),
                    )
                }
                Request::HttpRequestInputRemove { source_request_id } => {
                    let connection = self
                        .shared
                        .state
                        .lock()
                        .expect("Chrome lifecycle")
                        .connection;
                    let removed = self.request_inputs.remove(connection, source_request_id)?;
                    Ok(
                        json!({"sourceRequestId":source_request_id,"removed":removed,"replayed":false}),
                    )
                }
                Request::HttpContextCreate { options } => {
                    let options = self
                        .await_http_preparation(
                            operation,
                            http::prepare_options(options, deadline),
                            deadline,
                        )
                        .await?;
                    self.http_context_create(options)
                }
                Request::HttpContextStorageState {
                    http_context,
                    options,
                } => {
                    options.validate()?;
                    let path = options.path.clone();
                    let receipt = Box::pin(self.native_http_storage_state(
                        operation,
                        http_context,
                        options,
                        deadline,
                    ))
                    .await?;
                    let artifact = self.http_contexts.published_storage_state(&receipt)?;
                    http::storage_state_copy(artifact, receipt, path, deadline).await
                }
                Request::HttpEvidenceFinalize { capture_id } => {
                    Box::pin(self.native_http_state_finalize(capture_id, deadline)).await
                }
                Request::HttpContextList {} => Ok(self.http_contexts.list()),
                Request::HttpContextDispose {
                    http_context,
                    options,
                } => self.http_contexts.dispose(http_context, options),
                Request::HttpContextRead {
                    http_context,
                    include_reason,
                    max_bytes,
                } => self
                    .http_contexts
                    .context_read(http_context, include_reason, max_bytes),
                Request::HttpResponseBodyRead {
                    request_id,
                    format,
                    max_bytes,
                } => self
                    .http_contexts
                    .response_body_read(request_id, format, max_bytes),
                Request::HttpResponseDispose { request_id } => {
                    self.http_contexts.response_dispose(request_id)
                }
                Request::HttpContextRemove { http_context } => {
                    self.http_contexts.remove(http_context)
                }
                Request::HttpContextCookiesClear {
                    http_context,
                    options,
                } => {
                    options.validate()?;
                    self.http_contexts.require_idle(http_context)?;
                    if let Some(context) = self.http_contexts.associated_context(http_context)? {
                        let context_reference = context;
                        let context = self
                            .index
                            .native(context, Kind::Context)
                            .map_err(resource_error)?;
                        if !options.is_empty() {
                            let reply = self
                                .call(
                                    operation,
                                    None,
                                    "Storage.getCookies",
                                    json!({"browserContextId":context}),
                                    deadline,
                                )
                                .await?;
                            let values = cookie_view(reply["cookies"].clone())?;
                            let lease = self.http_contexts.cookie_filter_lease(http_context)?;
                            let indices = self
                                .await_http_preparation(
                                    operation,
                                    http::cookie_filter::select_owned(
                                        &values, options, deadline, lease,
                                    ),
                                    deadline,
                                )
                                .await?;
                            self.index
                                .native(context_reference, Kind::Context)
                                .map_err(resource_error)?;
                            let mut selected = http::cookie_filter::selected(values, indices)?;
                            let count = selected.len();
                            if !selected.is_empty() {
                                for value in &mut selected {
                                    value["expires"] = json!(0);
                                }
                                let cookies = cookies_for_write(json!(selected))?;
                                self.call(
                                    operation,
                                    None,
                                    "Storage.setCookies",
                                    json!({"browserContextId":context,"cookies":cookies}),
                                    deadline,
                                )
                                .await?;
                            }
                            return Ok(
                                json!({"httpContext":http_context,"cookiesCleared":true,"browserAssociated":true,"matchedCookies":count}),
                            );
                        }
                        self.call(
                            operation,
                            None,
                            "Storage.clearCookies",
                            json!({"browserContextId":context}),
                            deadline,
                        )
                        .await?;
                        Ok(
                            json!({"httpContext":http_context,"cookiesCleared":true,"browserAssociated":true}),
                        )
                    } else {
                        if !options.is_empty() {
                            let values = self.http_contexts.cookie_filter_snapshot(http_context)?;
                            let lease = self.http_contexts.cookie_filter_lease(http_context)?;
                            let indices = self
                                .await_http_preparation(
                                    operation,
                                    http::cookie_filter::select_owned(
                                        &values, options, deadline, lease,
                                    ),
                                    deadline,
                                )
                                .await?;
                            let selected = http::cookie_filter::selected(values, indices)?;
                            return self
                                .http_contexts
                                .cookies_remove_selected(http_context, &selected);
                        }
                        self.http_contexts.clear_cookies(http_context)
                    }
                }
                Request::HttpContextCookiesAdd {
                    http_context,
                    cookies,
                } => {
                    if let Some(context) = self.http_contexts.associated_context(http_context)? {
                        let cookies = cookies_for_write(json!(
                            http::cookie_admin::normalize_browser(cookies)?
                        ))?;
                        let context = self
                            .index
                            .native(context, Kind::Context)
                            .map_err(resource_error)?;
                        self.call(
                            operation,
                            None,
                            "Storage.setCookies",
                            json!({"browserContextId":context,"cookies":cookies}),
                            deadline,
                        )
                        .await?;
                        Ok(
                            json!({"httpContext":http_context,"cookiesAdded":true,"browserAssociated":true}),
                        )
                    } else {
                        self.http_contexts.cookies_add(http_context, cookies)
                    }
                }
                Request::HttpContextCookiesRead { http_context, urls } => {
                    let urls = urls.parse()?;
                    if let Some(context) = self.http_contexts.associated_context(http_context)? {
                        let context = self
                            .index
                            .native(context, Kind::Context)
                            .map_err(resource_error)?;
                        let reply = self
                            .call(
                                operation,
                                None,
                                "Storage.getCookies",
                                json!({"browserContextId":context}),
                                deadline,
                            )
                            .await?;
                        http::cookie_admin::select(
                            json!(cookie_view(reply["cookies"].clone())?),
                            &urls,
                        )
                    } else {
                        self.http_contexts.cookies_read(http_context, &urls)
                    }
                }
                Request::HttpEvidenceList {} => Ok(self.http_contexts.evidence_list()),
                Request::HttpEvidenceRead {
                    evidence_id,
                    format,
                    max_bytes,
                } => http::read_response(
                    &self.http_contexts.evidence_get(evidence_id)?,
                    format,
                    max_bytes,
                ),
                Request::HttpEvidenceRemove { evidence_id } => {
                    self.http_contexts.evidence_get(evidence_id)?.remove()?;
                    self.http_contexts.evidence_forget(evidence_id);
                    Ok(json!({"evidenceId":evidence_id,"removed":true}))
                }
                Request::HttpResponseFinalize { request_id } => self.http_finalize(request_id),
                Request::HttpResponseRead {
                    request_id,
                    max_bytes,
                    include_headers_object,
                } => self.http_contexts.response_read_options(
                    request_id,
                    max_bytes,
                    include_headers_object,
                ),
                Request::HttpResponseRemove { request_id } => {
                    Ok(self.http_contexts.response_remove(request_id))
                }
                Request::HttpResponseDiscard { request_id } => {
                    self.http_contexts.discard(request_id)
                }
                Request::HttpFetch {
                    http_context,
                    request,
                } => {
                    if let Some(context) = self.http_contexts.associated_context(http_context)? {
                        self.index
                            .native(context, Kind::Context)
                            .map_err(resource_error)?;
                    }
                    let request = self.prepare_http(operation, request, deadline).await?;
                    if let Some(context) = self.http_contexts.associated_context(http_context)? {
                        self.index
                            .native(context, Kind::Context)
                            .map_err(resource_error)?;
                    }
                    let id = self.http_contexts.start(http_context, request, deadline)?;
                    loop {
                        match self.http_finalize(id) {
                            Err(error) if error.code == "HTTP_RESPONSE_PENDING" => {}
                            result => return result,
                        }
                        let cancelled = self
                            .shared
                            .state
                            .lock()
                            .expect("Chrome lifecycle")
                            .operations
                            .get(&operation)
                            .is_some_and(|operation| operation.cancel_requested);
                        if cancelled
                            || self
                                .http_contexts
                                .deadline(id)
                                .is_some_and(|deadline| Instant::now() >= deadline)
                        {
                            if cancelled {
                                self.http_contexts.cancel_request(id);
                            }
                            let mut error = http::pending(http_context, id);
                            if cancelled {
                                error.code = "CANCELLED";
                            }
                            return Err(error);
                        }
                        if let Err(error) = self
                            .observation_tick(Instant::now() + Duration::from_millis(25))
                            .await
                        {
                            self.events = None;
                            self.lose(error.code);
                        }
                    }
                }
                _ => Err("INVALID_SCOPE".into()),
            }
        })
    }
    pub(super) async fn prepare_http(
        &mut self,
        operation: Uuid,
        request: http::Fetch,
        deadline: Instant,
    ) -> Result<http::Fetch, Error> {
        self.await_http_preparation(operation, http::prepare(request, deadline), deadline)
            .await
    }
    pub(super) async fn await_http_preparation<T: Send>(
        &mut self,
        operation: Uuid,
        preparation: impl std::future::Future<Output = Result<T, Error>> + Send,
        deadline: Instant,
    ) -> Result<T, Error> {
        tokio::pin!(preparation);
        loop {
            if self
                .shared
                .state
                .lock()
                .expect("Chrome lifecycle")
                .operations
                .get(&operation)
                .is_some_and(|operation| operation.cancel_requested)
            {
                return Err("CANCELLED".into());
            }
            if Instant::now() >= deadline {
                return Err("HTTP_DEADLINE".into());
            }
            // Already-ready preparation needs no housekeeping. Once a tick
            // starts it may own a dispatched private reply, so its completion
            // cannot be cancelled merely because preparation becomes ready.
            let (prepared, observed) = if let Some(result) = preparation.as_mut().now_or_never() {
                (Some(result), Ok(()))
            } else {
                let tick = self.observation_tick(Instant::now() + Duration::from_millis(25));
                tokio::pin!(tick);
                tokio::select! {
                    result=&mut preparation=>(Some(result),tick.await),
                    result=&mut tick=>(None,result),
                }
            };
            if let Err(error) = observed {
                self.events = None;
                self.lose(error.code);
                return Err(error);
            }
            if let Some(result) = prepared {
                if self
                    .shared
                    .state
                    .lock()
                    .expect("Chrome lifecycle")
                    .operations
                    .get(&operation)
                    .is_some_and(|operation| operation.cancel_requested)
                {
                    return Err("CANCELLED".into());
                }
                if Instant::now() >= deadline {
                    return Err("HTTP_DEADLINE".into());
                }
                return result;
            }
        }
    }
    fn http_context_create(&mut self, mut options: http::Options) -> Result<Value, Error> {
        let connection = self.shared.state.lock().expect("Chrome owner").connection;
        let browser = if let Some(context) = options.browser_context {
            self.index
                .native(context, Kind::Context)
                .map_err(resource_error)?;
            let inherit_credentials = options.http_credentials.is_none();
            if inherit_credentials {
                self.http_contexts.require_context_credentials(context)?;
            }
            let layout = self.context_layout.get(&context).ok_or("INVALID_SCOPE")?;
            let inherit_certificates = options.client_certificates.is_none();
            options.inherit(layout);
            if inherit_certificates {
                options.prepared_client_certificates = self.certificate_tunnels.cached(context);
            }
            if inherit_credentials {
                self.http_contexts.inherit_credentials(
                    &mut options,
                    context,
                    layout.http_credentials.as_ref(),
                );
            }
            self.http_contexts.inherit_headers(
                &mut options,
                context,
                self.context_layout
                    .get(&context)
                    .ok_or("INVALID_SCOPE")?
                    .extra_http_headers
                    .as_deref()
                    .unwrap_or_default(),
            );
            if options.user_agent.is_none() {
                options.user_agent = Some(
                    self.shared
                        .state
                        .lock()
                        .expect("Chrome owner")
                        .identity
                        .as_ref()
                        .and_then(|identity| identity["browser"]["userAgent"].as_str())
                        .ok_or("HTTP_BROWSER_DEFAULTS_UNAVAILABLE")?
                        .to_owned(),
                );
            }
            let context = self
                .index
                .native(context, Kind::Context)
                .map_err(resource_error)?;
            Some(Arc::new(BrowserCookieAccess {
                connection: self.connection.clone().ok_or("BACKEND_LOST")?,
                context,
                runtime: tokio::runtime::Handle::current(),
            }) as Arc<dyn http::browser::BrowserCookies>)
        } else {
            None
        };
        self.http_contexts
            .create_with_browser(connection, options, browser)
    }
    pub(super) fn http_finalize(&mut self, id: Uuid) -> Result<Value, Error> {
        match self
            .http_contexts
            .ready(id)?
            .ok_or("HTTP_RESPONSE_PENDING")?
        {
            Ready::Complete(value) => Ok(value),
            Ready::Reply(reply) => match http::materialize(self.journal.directory(), &reply) {
                Ok(artifact) => self.http_contexts.complete(id, reply.metadata, artifact),
                Err(mut error) => {
                    error.partial = Some(
                        json!({"requestId":id,"response":reply.metadata,"responseRetained":true,"nextAction":"http_response_finalize","replayed":false}),
                    );
                    self.http_contexts.restore(id, reply);
                    Err(error)
                }
            },
        }
    }
    pub(super) fn close_http(&mut self) -> Result<(), Error> {
        for id in self.http_contexts.pending() {
            if self.http_finalize(id).is_err() && self.http_contexts.has_pending(id) {
                return Err("CLEANUP_PENDING".into());
            }
        }
        self.http_contexts
            .finalize_all_evidence(self.journal.directory())?;
        self.http_contexts.close()
    }
}

#[cfg(test)]
mod cookie_tests {
    use super::*;

    #[test]
    fn native_cookie_partition_normalization_preserves_ancestry_and_refuses_missing_identity() {
        let cookies = normalize_cookie_keys(json!([
            {"name":"plain","value":"Acme"},
            {"name":"part","partitionKey":{"topLevelSite":"https://example.com","hasCrossSiteAncestor":false}}
        ])).unwrap();
        assert!(cookies[0].get("partitionKey").is_none());
        assert_eq!(cookies[1]["partitionKey"], "https://example.com");
        assert_eq!(cookies[1]["_crHasCrossSiteAncestor"], false);
        for key in [
            json!({"topLevelSite":"https://example.com"}),
            json!({"hasCrossSiteAncestor":true}),
        ] {
            assert_eq!(
                normalize_cookie_keys(json!([{"partitionKey":key}]))
                    .unwrap_err()
                    .code,
                "HTTP_BROWSER_COOKIE_PROTOCOL"
            );
        }
    }
}
