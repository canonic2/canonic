//! Host HTTP composition keeps response bytes owned across caller expiry and I/O failure.
use super::*;
use crate::browser::contract::Engine;
use crate::browser::http::{self, Ready};
use crate::browser::objects::ObjectRef;
struct BrowserCookieAccess {
    connection: Connection,
    context: ObjectRef,
    runtime: tokio::runtime::Handle,
    engine: Engine,
}
impl http::browser::BrowserCookies for BrowserCookieAccess {
    fn partition_identity_available(&self) -> bool {
        self.engine != Engine::Firefox
    }
    fn partition_model(&self) -> http::browser::PartitionModel {
        if self.engine == Engine::Webkit {
            http::browser::PartitionModel::Site
        } else {
            http::browser::PartitionModel::Unsupported
        }
    }
    fn read(&self, deadline: Instant) -> Result<Value, Error> {
        self.runtime
            .block_on(self.connection.retained(
                &self.context,
                "cookies",
                json!({"urls":[]}),
                deadline,
                false,
            ))?
            .map(|reply| reply["cookies"].clone())
            .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL".into())
    }
    fn write(&self, cookies: Value, deadline: Instant) -> Result<(), Error> {
        self.runtime.block_on(self.connection.retained(
            &self.context,
            "addCookies",
            json!({"cookies":cookies}),
            deadline,
            false,
        ))?;
        Ok(())
    }
}
impl Host {
    #[expect(clippy::too_many_lines, reason = "one match over every HTTP operation")]
    pub(super) async fn http_operation(
        &self,
        operation: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        // `route_fetch` is an `http_fetch` of the pending callback's private request.
        let options = http::RouteFetchOptions::of(&request);
        let request = match request {
            Request::HttpFetchRequest {
                http_context,
                source_request_id,
                options,
            } => {
                let connection = self.state.lock().expect("Browser owner").connection;
                let source = self
                    .request_inputs
                    .lock()
                    .expect("Request input owner")
                    .get(connection, source_request_id)?;
                Request::HttpFetch {
                    http_context,
                    request: source.fetch(options)?,
                }
            }
            Request::RouteFetch {
                callback,
                http_context,
                ..
            } => {
                let wire = self
                    .state
                    .lock()
                    .expect("Browser owner")
                    .resource(callback, Some(Kind::Route))?
                    .wire
                    .clone();
                let (_, paused) = self.route_request_copy(&wire).await?;
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
                self.wire(context, Kind::Context)?;
                self.admissible(operation, deadline)?;
                let connection = self.state.lock().expect("Browser owner").connection;
                let mut sources = self.request_inputs.lock().expect("Request input owner");
                sources.set(connection, context, enabled)?;
                Ok(json!({"context":context,"enabled":enabled,"requestInputs":sources.summary()}))
            }
            Request::HttpRequestInputRemove { source_request_id } => {
                let connection = self.state.lock().expect("Browser owner").connection;
                let removed = self
                    .request_inputs
                    .lock()
                    .expect("Request input owner")
                    .remove(connection, source_request_id)?;
                Ok(json!({"sourceRequestId":source_request_id,"removed":removed,"replayed":false}))
            }
            Request::HttpContextCreate { options } => {
                self.http_context_create(operation, options, deadline).await
            }
            Request::HttpContextList {} => {
                Ok(self.http_contexts.lock().expect("HTTP owner").list())
            }
            Request::HttpContextDispose {
                http_context,
                options,
            } => self
                .http_contexts
                .lock()
                .expect("HTTP owner")
                .dispose(http_context, options),
            Request::HttpContextRead {
                http_context,
                include_reason,
                max_bytes,
            } => self.http_contexts.lock().expect("HTTP owner").context_read(
                http_context,
                include_reason,
                max_bytes,
            ),
            Request::HttpResponseBodyRead {
                request_id,
                format,
                max_bytes,
            } => self
                .http_contexts
                .lock()
                .expect("HTTP owner")
                .response_body_read(request_id, format, max_bytes),
            Request::HttpResponseDispose { request_id } => self
                .http_contexts
                .lock()
                .expect("HTTP owner")
                .response_dispose(request_id),
            Request::HttpContextStorageState {
                http_context,
                options,
            } => {
                options.validate()?;
                let path = options.path.clone();
                let receipt = self
                    .http_storage_state(http_context, options, deadline)
                    .await?;
                let artifact = self
                    .http_contexts
                    .lock()
                    .expect("HTTP owner")
                    .published_storage_state(&receipt)?;
                http::storage_state_copy(artifact, receipt, path, deadline).await
            }
            Request::HttpEvidenceFinalize { capture_id } => self
                .http_contexts
                .lock()
                .expect("HTTP owner")
                .finalize_evidence(self.journal.directory(), capture_id),
            Request::HttpContextRemove { http_context } => self
                .http_contexts
                .lock()
                .expect("HTTP owner")
                .remove(http_context),
            Request::HttpContextCookiesClear {
                http_context,
                options,
            } => {
                self.http_cookies_clear(operation, http_context, options, deadline)
                    .await
            }
            Request::HttpContextCookiesAdd {
                http_context,
                cookies,
            } => {
                let context = self
                    .http_contexts
                    .lock()
                    .expect("HTTP owner")
                    .associated_context(http_context)?;
                if let Some(context) = context {
                    let cookies = http::cookie_admin::normalize_browser(cookies)?;
                    let context = self
                        .state
                        .lock()
                        .expect("Browser owner")
                        .resource(context, Some(Kind::Context))?
                        .wire
                        .clone();
                    self.connection()?
                        .retained(
                            &context,
                            "addCookies",
                            json!({"cookies":cookies}),
                            deadline,
                            false,
                        )
                        .await?;
                    Ok(
                        json!({"httpContext":http_context,"cookiesAdded":true,"browserAssociated":true}),
                    )
                } else {
                    self.http_contexts
                        .lock()
                        .expect("HTTP owner")
                        .cookies_add(http_context, cookies)
                }
            }
            Request::HttpContextCookiesRead { http_context, urls } => {
                let urls = urls.parse()?;
                let context = self
                    .http_contexts
                    .lock()
                    .expect("HTTP owner")
                    .associated_context(http_context)?;
                if let Some(context) = context {
                    let context = self
                        .state
                        .lock()
                        .expect("Browser owner")
                        .resource(context, Some(Kind::Context))?
                        .wire
                        .clone();
                    let reply = self
                        .connection()?
                        .retained(&context, "cookies", json!({"urls":[]}), deadline, false)
                        .await?
                        .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
                    http::cookie_admin::select(reply["cookies"].clone(), &urls)
                } else {
                    self.http_contexts
                        .lock()
                        .expect("HTTP owner")
                        .cookies_read(http_context, &urls)
                }
            }
            Request::HttpEvidenceList {} => Ok(self
                .http_contexts
                .lock()
                .expect("HTTP owner")
                .evidence_list()),
            Request::HttpEvidenceRead {
                evidence_id,
                format,
                max_bytes,
            } => {
                let artifact = self
                    .http_contexts
                    .lock()
                    .expect("HTTP owner")
                    .evidence_get(evidence_id)?;
                http::read_response(&artifact, format, max_bytes)
            }
            Request::HttpEvidenceRemove { evidence_id } => {
                let artifact = self
                    .http_contexts
                    .lock()
                    .expect("HTTP owner")
                    .evidence_get(evidence_id)?;
                artifact.remove()?;
                self.http_contexts
                    .lock()
                    .expect("HTTP owner")
                    .evidence_forget(evidence_id);
                Ok(json!({"evidenceId":evidence_id,"removed":true}))
            }
            Request::HttpResponseFinalize { request_id } => self.http_finalize(request_id),
            Request::HttpResponseRead {
                request_id,
                max_bytes,
                include_headers_object,
            } => self
                .http_contexts
                .lock()
                .expect("HTTP owner")
                .response_read_options(request_id, max_bytes, include_headers_object),
            Request::HttpResponseRemove { request_id } => Ok(self
                .http_contexts
                .lock()
                .expect("HTTP owner")
                .response_remove(request_id)),
            Request::HttpResponseDiscard { request_id } => self
                .http_contexts
                .lock()
                .expect("HTTP owner")
                .discard(request_id),
            Request::HttpFetch {
                http_context,
                request,
            } => {
                self.http_callers
                    .lock()
                    .expect("HTTP caller owner")
                    .insert(operation);
                let context = self
                    .http_contexts
                    .lock()
                    .expect("HTTP owner")
                    .associated_context(http_context)?;
                if let Some(context) = context {
                    self.state
                        .lock()
                        .expect("Browser owner")
                        .resource(context, Some(Kind::Context))?;
                }
                let request = http::prepare(request, deadline).await?;
                self.admissible(operation, deadline)?;
                if let Some(context) = context {
                    self.state
                        .lock()
                        .expect("Browser owner")
                        .resource(context, Some(Kind::Context))?;
                }
                let (id, disposed) = {
                    let mut contexts = self.http_contexts.lock().expect("HTTP owner");
                    let id = contexts.start(http_context, request, deadline)?;
                    (id, contexts.disposal_watch(id)?)
                };
                let mut delay = Duration::from_millis(5);
                loop {
                    if disposed.load(Ordering::Acquire) {
                        let mut error = http::pending(http_context, id);
                        error.code = "HTTP_CONTEXT_DISPOSED";
                        return Err(error);
                    }
                    match self.http_finalize(id) {
                        Err(error) if error.code == "HTTP_RESPONSE_PENDING" => {}
                        result => return result,
                    }
                    let cancelled = self
                        .cancelling
                        .lock()
                        .expect("Cancellation owner")
                        .contains(&operation);
                    if cancelled
                        || self
                            .http_contexts
                            .lock()
                            .expect("HTTP owner")
                            .deadline(id)
                            .is_some_and(|deadline| Instant::now() >= deadline)
                    {
                        if cancelled {
                            self.http_contexts
                                .lock()
                                .expect("HTTP owner")
                                .cancel_request(id);
                        }
                        let mut error = http::pending(http_context, id);
                        if cancelled {
                            error.code = "CANCELLED";
                        }
                        return Err(error);
                    }
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_millis(250));
                }
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
    async fn http_storage_state(
        &self,
        http_context: Reference,
        options: http::StorageStateOptions,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let context = self
            .http_contexts
            .lock()
            .expect("HTTP owner")
            .associated_context(http_context)?;
        let Some(context) = context else {
            return self
                .http_contexts
                .lock()
                .expect("HTTP owner")
                .storage_state(http_context, options, self.journal.directory());
        };
        let engine = self.state.lock().expect("Browser owner").engine;
        crate::browser::storage::Options {
            indexed_db: options.indexed_db,
            opfs: options.opfs,
            ..Default::default()
        }
        .validate(engine)?;
        let connection = self.connection()?;
        connection.event_barrier(deadline).await?;
        let wire = self.wire(context, Kind::Context)?;
        let mut captured = connection
            .retained(
                &wire,
                "storageState",
                json!({"indexedDB":options.indexed_db,"opfs":options.opfs,"credentials":false}),
                deadline,
                false,
            )
            .await?
            .ok_or("MISSING_RESULT")?;
        if let Some(credentials) = captured.get("credentials") {
            if !credentials.as_array().is_some_and(Vec::is_empty) {
                return Err("UNREQUESTED_CREDENTIAL_STATE".into());
            }
            captured
                .as_object_mut()
                .ok_or("INVALID_STORAGE_STATE")?
                .remove("credentials");
        }
        connection.event_barrier(deadline).await?;
        // Keep the browser witness through HTTP-client revalidation and staging:
        // neither a closed context nor a removed client can publish a late read.
        let state = self.state.lock().expect("Browser owner");
        state.resource(context, Some(Kind::Context))?;
        self.http_contexts
            .lock()
            .expect("HTTP owner")
            .publish_storage_state(http_context, options, captured, self.journal.directory())
    }

    async fn http_context_create(
        &self,
        operation: Uuid,
        mut options: http::Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options = http::prepare_options(options, deadline).await?;
        self.admissible(operation, deadline)?;
        let connection = self.state.lock().expect("Browser owner").connection;
        let inherit_credentials = options.http_credentials.is_none();
        if let Some(context) = options.browser_context {
            self.connection()?.event_barrier(deadline).await?;
            {
                let state = self.state.lock().expect("Browser owner");
                state.resource(context, Some(Kind::Context))?;
                options.inherit(state.context_options.get(&context).ok_or("INVALID_SCOPE")?);
                options.validate()?;
                options.validate_browser_partition(
                    if state.engine == Engine::Webkit {
                        http::browser::PartitionModel::Site
                    } else {
                        http::browser::PartitionModel::Unsupported
                    },
                    state.engine != Engine::Firefox,
                )?;
                if inherit_credentials {
                    self.http_contexts
                        .lock()
                        .expect("HTTP owner")
                        .require_context_credentials(context)?;
                }
            }
            options = http::client_certificates::prepare_options(options, deadline).await?;
            if options.user_agent.is_none() {
                options.user_agent = Some(
                    self.http_browser_user_agent
                        .get_or_try_init(|| async {
                            let browser = self
                                .state
                                .lock()
                                .expect("Browser owner")
                                .resources
                                .values()
                                .find(|resource| resource.kind == Kind::Browser)
                                .map(|resource| resource.wire.clone())
                                .ok_or("HTTP_BROWSER_DEFAULTS_UNAVAILABLE")?;
                            let reply = self
                                .connection()?
                                .retained_internal(
                                    &browser,
                                    "defaultUserAgentForTest",
                                    json!({}),
                                    deadline,
                                    false,
                                )
                                .await?
                                .ok_or("MISSING_RESULT")?;
                            let agent = reply["userAgent"].as_str().ok_or("BACKEND_PROTOCOL")?;
                            http::validate_user_agent(agent)?;
                            Ok::<String, Error>(agent.into())
                        })
                        .await?
                        .clone(),
                );
                self.connection()?.event_barrier(deadline).await?;
                self.state
                    .lock()
                    .expect("Browser owner")
                    .resource(context, Some(Kind::Context))?;
            }
            let browser_connection = self.connection()?;
            self.admissible(operation, deadline)?;
            // Keep the current context witness through defaults publication and
            // client allocation. A close event cannot retire it between them
            // and leave a newly inserted default owner behind that close.
            let state = self.state.lock().expect("Browser owner");
            let wire = state.resource(context, Some(Kind::Context))?.wire.clone();
            let initial = state.context_options.get(&context).ok_or("INVALID_SCOPE")?;
            let mut owner = self.http_contexts.lock().expect("HTTP owner");
            owner.inherit_headers(
                &mut options,
                context,
                initial.extra_http_headers.as_deref().unwrap_or_default(),
            );
            if inherit_credentials {
                owner.inherit_credentials(&mut options, context, initial.http_credentials.as_ref());
            }
            let browser = Arc::new(BrowserCookieAccess {
                connection: browser_connection,
                context: wire,
                runtime: tokio::runtime::Handle::current(),
                engine: state.engine,
            }) as Arc<dyn http::browser::BrowserCookies>;
            return owner.create_with_browser(connection, options, Some(browser));
        }
        self.http_contexts
            .lock()
            .expect("HTTP owner")
            .create_with_browser(connection, options, None)
    }
    async fn http_cookies_clear(
        &self,
        operation: Uuid,
        http_context: Reference,
        options: http::CookieFilter,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        let context = {
            let owner = self.http_contexts.lock().expect("HTTP owner");
            owner.require_idle(http_context)?;
            owner.associated_context(http_context)?
        };
        if let Some(context) = context {
            let context_reference = context;
            let context = self
                .state
                .lock()
                .expect("Browser owner")
                .resource(context, Some(Kind::Context))?
                .wire
                .clone();
            if !options.is_empty() {
                let reply = self
                    .connection()?
                    .retained(&context, "cookies", json!({"urls":[]}), deadline, false)
                    .await?
                    .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
                let values = reply["cookies"]
                    .as_array()
                    .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?
                    .clone();
                let mut selected = self
                    .http_cookie_filter(operation, http_context, values, options, deadline)
                    .await?;
                let count = selected.len();
                self.admissible(operation, deadline)?;
                // The exact context object is revalidated after host matching.
                self.state
                    .lock()
                    .expect("Browser owner")
                    .resource(context_reference, Some(Kind::Context))?;
                if !selected.is_empty() {
                    for value in &mut selected {
                        value["expires"] = json!(0);
                    }
                    self.connection()?
                        .retained(
                            &context,
                            "addCookies",
                            json!({"cookies":selected}),
                            deadline,
                            false,
                        )
                        .await?;
                }
                return Ok(
                    json!({"httpContext":http_context,"cookiesCleared":true,"browserAssociated":true,"matchedCookies":count}),
                );
            }
            self.connection()?
                .retained(&context, "clearCookies", json!({}), deadline, false)
                .await?;
            Ok(json!({"httpContext":http_context,"cookiesCleared":true,"browserAssociated":true}))
        } else {
            if !options.is_empty() {
                let values = self
                    .http_contexts
                    .lock()
                    .expect("HTTP owner")
                    .cookie_filter_snapshot(http_context)?;
                let selected = self
                    .http_cookie_filter(operation, http_context, values, options, deadline)
                    .await?;
                self.admissible(operation, deadline)?;
                return self
                    .http_contexts
                    .lock()
                    .expect("HTTP owner")
                    .cookies_remove_selected(http_context, &selected);
            }
            self.http_contexts
                .lock()
                .expect("HTTP owner")
                .clear_cookies(http_context)
        }
    }
    async fn http_cookie_filter(
        &self,
        operation: Uuid,
        http_context: Reference,
        values: Vec<Value>,
        options: http::CookieFilter,
        deadline: Instant,
    ) -> Result<Vec<Value>, Error> {
        let indices = {
            let lease = self
                .http_contexts
                .lock()
                .expect("HTTP owner")
                .cookie_filter_lease(http_context)?;
            let work = http::cookie_filter::select_owned(&values, options, deadline, lease);
            tokio::pin!(work);
            loop {
                self.admissible(operation, deadline)?;
                tokio::select! {
                    result=&mut work=>break result?,
                    _=tokio::time::sleep(Duration::from_millis(25))=>{}
                }
            }
        };
        self.admissible(operation, deadline)?;
        http::cookie_filter::selected(values, indices)
    }
    pub(super) fn http_finalize(&self, id: Uuid) -> Result<Value, Error> {
        let ready = self
            .http_contexts
            .lock()
            .expect("HTTP owner")
            .ready(id)?
            .ok_or("HTTP_RESPONSE_PENDING")?;
        match ready {
            Ready::Complete(value) => Ok(value),
            Ready::Reply(reply) => match http::materialize(self.journal.directory(), &reply) {
                Ok(artifact) => self.http_contexts.lock().expect("HTTP owner").complete(
                    id,
                    reply.metadata,
                    artifact,
                ),
                Err(mut error) => {
                    error.partial = Some(
                        json!({"requestId":id,"response":reply.metadata,"responseRetained":true,"nextAction":"http_response_finalize","replayed":false}),
                    );
                    self.http_contexts
                        .lock()
                        .expect("HTTP owner")
                        .restore(id, reply);
                    Err(error)
                }
            },
        }
    }
    pub(super) fn close_http(&self) -> Result<(), Error> {
        let pending = recovered(&self.http_contexts).pending();
        for id in pending {
            if self.http_finalize(id).is_err() && recovered(&self.http_contexts).has_pending(id) {
                return Err("CLEANUP_PENDING".into());
            }
        }
        let mut owner = recovered(&self.http_contexts);
        owner.finalize_all_evidence(self.journal.directory())?;
        owner.close()
    }
    pub(super) async fn close_http_owned(&self, deadline: Instant) -> Result<(), Error> {
        loop {
            match self.close_http() {
                Err(error) if error.code == "CLEANUP_PENDING" && Instant::now() < deadline => {
                    tokio::time::sleep_until(
                        deadline.min(Instant::now() + Duration::from_millis(25)),
                    )
                    .await;
                }
                result => return result,
            }
        }
    }
    pub(super) async fn close_http_callers(&self, deadline: Instant) -> Result<(), Error> {
        loop {
            if recovered(&self.http_callers).is_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("CLEANUP_PENDING".into());
            }
            tokio::select! {
                ()=self.changed.notified()=>{},
                ()=tokio::time::sleep_until(deadline.min(Instant::now()+Duration::from_millis(25)))=>{},
            }
        }
    }
}
