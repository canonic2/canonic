//! Serialized lifecycle workflows; protocol objects never enter public results.

use super::*;
use crate::browser::{contract::ContextOptions, objects::ObjectRef, values};

pub(super) fn validate_engine(
    request: &Request,
    engine: crate::browser::contract::Engine,
) -> Result<(), Error> {
    match request {
        Request::CallbackResolve {
            response:
                CallbackResponse::RouteFulfill {
                    status: Some(status),
                    ..
                }
                | CallbackResponse::RouteFulfillBinary { status, .. }
                | CallbackResponse::RouteFulfillEvidence { status, .. }
                | CallbackResponse::RouteFulfillHttpEvidence {
                    status: Some(status),
                    ..
                },
            ..
        } if engine == crate::browser::contract::Engine::Webkit && (300..400).contains(status) => {
            Err("ROUTE_STATUS_UNSUPPORTED".into())
        }
        Request::PageRouteSet {
            authentication_challenges: true,
            ..
        }
        | Request::ContextRouteSet {
            authentication_challenges: true,
            ..
        } => Err("AUTH_CHALLENGE_UNSUPPORTED".into()),
        Request::PageCss { .. }
        | Request::PageAudit { .. }
        | Request::PageDebugger { .. }
        | Request::ProfileStart { .. }
        | Request::ProfileStatus { .. }
        | Request::ProfileStop { .. }
        | Request::ProfileTakeCoverage { .. }
        | Request::ProfileQuery { .. }
        | Request::PerformanceLoad { .. }
        | Request::PerformanceQuery { .. }
        | Request::PerformanceRelease { .. }
        | Request::HeapStop { .. }
        | Request::HeapCapture { .. }
        | Request::HeapImport { .. }
        | Request::HeapLoad { .. }
        | Request::HeapQuery { .. }
        | Request::HeapCompare { .. }
        | Request::HeapRelease { .. }
        | Request::DeveloperOpen { .. }
        | Request::DeveloperStatus { .. }
        | Request::DeveloperClose { .. } => Err("NATIVE_DEVELOPER_UNSUPPORTED".into()),
        Request::Batch { requests } => {
            for request in requests {
                validate_engine(request, engine)?;
            }
            Ok(())
        }
        Request::ContextCreate { options } => options.validate_for(engine).map_err(Error::from),
        Request::PagePdf { .. } if engine != crate::browser::contract::Engine::Chrome => {
            Err("PDF_UNSUPPORTED".into())
        }
        // CPU and network throttling are native Chrome emulation only.
        Request::PageEmulateThrottling { .. } => Err("THROTTLING_UNSUPPORTED".into()),
        // The pinned driver has no Cache Storage API.
        Request::ContextCacheStorageRead { .. } | Request::ContextCacheStorageDelete { .. } => {
            Err("CACHE_STORAGE_UNSUPPORTED".into())
        }
        Request::StorageExport { options, .. } => options.validate(engine).map_err(Error::from),
        Request::ContextConfigure { setting, .. } => {
            setting.wire(engine).map(|_| ()).map_err(Error::from)
        }
        _ => Ok(()),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "One exhaustive request preflight keeps all structural refusals before operation admission"
)]
pub(super) fn validate(request: &Request) -> Result<(), Error> {
    match request {
        Request::TraceStart { options, .. } => options.validate(false).map_err(Error::from),
        Request::TraceStop {
            file_path: Some(_), ..
        }
        | Request::TraceStop {
            analysis_page: Some(_),
            ..
        }
        | Request::TraceStop {
            analysis_developer: Some(_),
            ..
        } => Err("TRACE_NATIVE_OPTIONS_UNSUPPORTED".into()),
        Request::TraceChunkStart { title, name } => {
            if let Some(name) = name {
                crate::browser::tracing::prefix(name).map_err(Error::from)?;
            }
            crate::browser::tracing::label(title).map_err(Error::from)
        }
        Request::TraceGroupStart { name, location } => {
            if let Some(location) = location {
                location.validate().map_err(Error::from)?;
            }
            crate::browser::tracing::label(name).map_err(Error::from)
        }
        Request::NetworkRoute { action, .. } => action.validate().map_err(Error::from),
        Request::HarRegister { path } => {
            if path.is_empty() || path.len() > 4096 {
                Err("HAR_PRIVATE_FILE_REQUIRED".into())
            } else {
                Ok(())
            }
        }
        Request::HttpContextCreate { options } => options.validate(),
        Request::HttpContextDispose { options, .. } => options.validate(),
        Request::HttpFetch { request, .. } => request.validate(),
        Request::HttpFetchRequest { options, .. } => options.validate(),
        Request::RouteFetch { .. } => crate::browser::http::RouteFetchOptions::of(request)
            .expect("route_fetch options")
            .validate(),
        Request::PageRouteSet {
            url: None,
            authentication_challenges: true,
            ..
        }
        | Request::ContextRouteSet {
            url: None,
            authentication_challenges: true,
            ..
        } => Err("AUTH_ROUTE_REQUIRED".into()),
        Request::CallbackResolve {
            response: CallbackResponse::AuthCredentials { username, password },
            ..
        } => crate::browser::routing::credentials(username, password).map_err(Error::from),
        Request::CallbackResolve {
            response:
                CallbackResponse::RouteContinueWith { overrides }
                | CallbackResponse::RouteFallbackWith { overrides },
            ..
        } => overrides.validate().map_err(Error::from),
        Request::BindingAdd { name, .. } => {
            crate::browser::bindings::validate(name).map_err(Error::from)
        }
        Request::PageRouteSet { url, .. } | Request::ContextRouteSet { url, .. } => url
            .as_deref()
            .map_or(Ok(()), crate::browser::routing::validate_url)
            .map_err(Error::from),
        Request::CallbackResolve {
            response:
                CallbackResponse::RouteFulfill {
                    status,
                    body,
                    json,
                    content_type,
                    headers,
                    ..
                },
            ..
        } => crate::browser::routing::text_fulfill(
            *status,
            body.as_deref(),
            json.as_ref(),
            content_type.as_deref(),
            headers,
            None,
        )
        .and_then(|(status, body, headers)| {
            crate::browser::routing::inline_response(status, &body, &headers)
        })
        .map(|_| ())
        .map_err(Error::from),
        Request::CallbackResolve {
            response:
                CallbackResponse::RouteFulfillBinary {
                    status,
                    body_base64,
                    headers,
                },
            ..
        } => crate::browser::routing::inline_response(*status, body_base64, headers)
            .map(|_| ())
            .map_err(Error::from),
        Request::CallbackResolve {
            response:
                CallbackResponse::RouteFulfillEvidence {
                    status, headers, ..
                },
            ..
        } => crate::browser::routing::binary_response(*status, "", headers)
            .map(|_| ())
            .map_err(Error::from),
        Request::CallbackResolve {
            response:
                CallbackResponse::RouteFulfillHttpEvidence {
                    status, headers, ..
                },
            ..
        } => crate::browser::routing::binary_response(status.unwrap_or(200), "", headers)
            .map(|_| ())
            .map_err(Error::from),
        Request::PageInitScriptAdd { source, .. }
        | Request::ContextInitScriptAdd { source, .. } => {
            crate::browser::scripts::validate(source).map_err(Error::from)
        }
        Request::NetworkRead { options, .. } => options.validate().map_err(Error::from),
        Request::HttpEvidenceRead { max_bytes, .. }
        | Request::HttpResponseRead { max_bytes, .. }
            if !(1..=65536).contains(max_bytes) =>
        {
            Err("HTTP_RESPONSE_RESULT_BOUNDS".into())
        }
        Request::WebSocketRouteAdd { url, .. } => {
            crate::browser::socket_routes::validate_url(url).map_err(Error::from)
        }
        Request::WebSocketRouteAddMatcher { matcher, .. } => {
            matcher.prepare().map(|_| ()).map_err(Error::from)
        }
        Request::WebSocketRouteSend {
            socket_id, message, ..
        } => crate::browser::socket_routes::validate_send(socket_id, message).map_err(Error::from),
        Request::WebSocketRouteClose {
            socket_id,
            to,
            options,
            ..
        } => crate::browser::socket_routes::validate_close(socket_id, *to, options)
            .map_err(Error::from),
        Request::WebSocketRouteHandlers {
            socket_id,
            handlers,
            ..
        } => crate::browser::socket_routes::validate_handlers(socket_id, handlers)
            .map_err(Error::from),
        Request::PageDebugger { action, .. } => action.validate().map_err(Error::from),
        Request::ContextDebugger { action, .. } => action.validate().map_err(Error::from),
        Request::ProfileStart { options, .. } => options.validate().map_err(Error::from),
        Request::EventsRead { cursor, limit, .. } => {
            canonic_mcp_kit::page::Request::from_cli(
                cursor.as_deref(),
                Some(*limit as u64),
                canonic_mcp_kit::page::Limits::STANDARD,
            )
            .map_err(|error| Error::from(error.code()))?;
            Ok(())
        }
        Request::EventsWait {
            after, conditions, ..
        } => {
            if *after > crate::browser::activity::MAX_CURSOR || !(1..=8).contains(&conditions.len())
            {
                return Err("EVENT_QUERY_BOUNDS".into());
            }
            Ok(())
        }
        // Merge is a native Chrome contract; Firefox/WebKit only replace.
        Request::StorageApply {
            mode: crate::browser::storage::ApplyMode::Merge,
            ..
        } => Err("STORAGE_MERGE_UNSUPPORTED".into()),
        Request::StorageRegister { path } if !std::path::Path::new(path).is_absolute() => {
            Err("PRIVATE_STATE_REQUIRED".into())
        }
        Request::DiagnosticsWait {
            after, conditions, ..
        } => {
            if *after > 9_007_199_254_740_991 || !(1..=8).contains(&conditions.len()) {
                return Err("INVALID_EVENT_CONDITION".into());
            }
            for condition in conditions {
                condition.validate()?;
            }
            Ok(())
        }
        Request::Batch { requests } => {
            if !(1..=8).contains(&requests.len()) {
                return Err("BATCH_BOUNDS".into());
            }
            for request in requests {
                if matches!(request, Request::Batch { .. } | Request::InputRelease {}) {
                    return Err("INVALID_BATCH_ACTION".into());
                }
                validate(request)?;
            }
            Ok(())
        }
        Request::ContextClock { action, .. } => action.validate().map_err(Error::from),
        Request::ContextCredentials { action, .. } => action.validate().map_err(Error::from),
        Request::AuthSignIn {
            broker,
            account,
            journey,
            expected_identity,
            ..
        } => crate::browser::authentication::validate_sign_in(
            broker,
            account,
            journey,
            expected_identity.as_deref(),
        )
        .map_err(Error::from),
        Request::AuthStatus {
            journey,
            expected_identity,
            ..
        } => crate::browser::authentication::validate_status(journey, expected_identity.as_deref())
            .map_err(Error::from),
        Request::ContextConfigure { setting, .. } => setting.validate().map_err(Error::from),
        Request::PageEmulateMedia { options, .. } => options.validate().map_err(Error::from),
        Request::PageSetExtraHttpHeaders { headers, .. } => {
            super::super::http::validate_headers(headers)
        }
        Request::PageInput {
            actions,
            observation,
            ..
        } => super::super::input::validate(actions, *observation).map_err(Error::from),
        Request::PageResize { viewport, .. } => {
            if !(1..=8192).contains(&viewport.width) || !(1..=8192).contains(&viewport.height) {
                return Err("INVALID_VIEWPORT".into());
            }
            Ok(())
        }
        Request::PreviewStart { options, .. } => options.validate().map_err(Error::from),
        Request::RecordingStart { options, .. } => options.validate().map_err(Error::from),
        Request::HandleProperties { query, .. } => query.validate().map_err(Error::from),
        Request::HandleProperty { name, .. } => {
            if name.len() > 1024 {
                return Err("PROPERTY_NAME_BOUNDS".into());
            }
            Ok(())
        }
        Request::DiagnosticsArgumentsRead { query, .. } => query.validate().map_err(Error::from),
        Request::DiagnosticsRead { query, .. } => query.validate().map_err(Error::from),
        Request::Evaluate {
            expression,
            argument,
            ..
        } => {
            if expression.is_empty() || expression.len() > 8192 {
                return Err("EXPRESSION_BOUNDS".into());
            }
            super::super::evaluation::validate(argument).map_err(Error::from)
        }
        Request::SelectorEvaluate { options, .. } => options.validate().map_err(Error::from),
        Request::FixtureRegister { fixture } => fixture.validate().map_err(Error::from),
        Request::DirectoryRegister { name, entries } => {
            crate::browser::directory::validate(name, entries).map_err(Error::from)
        }
        Request::PageUpload {
            locator, fixtures, ..
        } => {
            locator.selector()?;
            if fixtures.len() > 16 {
                return Err("FIXTURE_BOUNDS".into());
            }
            Ok(())
        }
        Request::ChooserResolve { fixtures, .. } if fixtures.len() > 16 => {
            Err("FIXTURE_BOUNDS".into())
        }
        Request::PageNavigate { navigation, .. } => navigation.validate().map_err(Error::from),
        Request::PageReadUrl { url, max_bytes, .. } => {
            crate::browser::page::validate_read_url(url, *max_bytes).map_err(Error::from)
        }
        Request::PageClipboard { origin, action, .. } => {
            crate::browser::clipboard::validate(origin, action).map_err(Error::from)
        }
        Request::FrameworkQuery { query, .. } => query.validate().map_err(Error::from),
        Request::DeveloperToolCall {
            tool_id,
            parameters,
            ..
        } => {
            if tool_id.is_empty() || tool_id.len() > 32 {
                return Err("DEVELOPER_TOOL_ID_BOUNDS".into());
            }
            crate::browser::developer_tools::validate_parameters(parameters).map_err(Error::from)
        }
        Request::PageStyles {
            locator,
            properties,
            max_bytes,
            ..
        } => crate::browser::page::validate_styles(locator, properties, *max_bytes)
            .map_err(Error::from),
        Request::PageRead {
            locator, max_bytes, ..
        } => {
            if !(256..=8192).contains(max_bytes) {
                return Err("READ_BOUNDS".into());
            }
            locator
                .as_ref()
                .map(|l| l.selector())
                .transpose()
                .map(|_| ())
                .map_err(Error::from)
        }
        Request::PageAction {
            locator,
            action,
            force,
            postcondition,
            ..
        } => {
            locator.selector()?;
            if let Some(condition) = postcondition {
                condition.validate()?;
            }
            action.validate(*force).map_err(Error::from)
        }
        Request::PageWait {
            locator, condition, ..
        } => {
            condition.validate()?;
            locator
                .as_ref()
                .map(|l| l.selector())
                .transpose()
                .map(|_| ())
                .map_err(Error::from)
        }
        Request::FrameEnter { locator, .. } => locator.selector().map(|_| ()).map_err(Error::from),
        Request::PageScreenshot { capture, .. } => capture.validate().map_err(Error::from),
        Request::PageSnapshot { locator, .. } => locator
            .as_ref()
            .map(|locator| locator.selector())
            .transpose()
            .map(|_| ())
            .map_err(Error::from),
        Request::PageAnnotate { markers, .. } => {
            crate::browser::evidence::validate_markers(markers).map_err(Error::from)
        }
        Request::PagePdf { options, .. } => options.validate().map_err(Error::from),
        Request::CaptureCompare {
            threshold,
            max_changed_ratio,
            ..
        } => crate::browser::capture::validate_comparison(*threshold, *max_changed_ratio)
            .map_err(Error::from),
        Request::ContextCreate { options } => options.validate().map_err(Error::from),
        Request::HandleCreate { expression, .. }
            if expression.is_empty() || expression.len() > 8192 =>
        {
            Err("EXPRESSION_BOUNDS".into())
        }
        Request::CallbackResolve {
            response:
                CallbackResponse::Dialog {
                    accept,
                    prompt_text,
                },
            ..
        } => {
            if prompt_text.as_ref().is_some_and(|text| text.len() > 8192)
                || (!accept && prompt_text.is_some())
            {
                return Err("INVALID_DIALOG_RESPONSE".into());
            }
            Ok(())
        }
        Request::CallbackResolve {
            response: CallbackResponse::BindingResolve { value },
            ..
        } => values::argument(value).map(|_| ()).map_err(Error::from),
        Request::CallbackResolve {
            response:
                CallbackResponse::BindingResolveValue { value }
                | CallbackResponse::BindingRejectValue { value },
            ..
        } => crate::browser::bindings::value_argument(value)
            .map(|_| ())
            .map_err(Error::from),
        _ => Ok(()),
    }
}

pub(super) async fn channel(
    connection: &Connection,
    result: Option<Value>,
    field: &str,
    kind: &str,
) -> Result<ObjectRef, Error> {
    let result = result.ok_or("MISSING_RESULT")?;
    connection
        .resolve(
            result[field]["guid"].as_str().ok_or("MISSING_CHANNEL")?,
            kind,
        )
        .await
        .map_err(Error::from)
}

async fn page_main_frame(connection: &Connection, initializer: &Value) -> Result<ObjectRef, Error> {
    let guid = initializer["mainFrame"]["guid"]
        .as_str()
        .ok_or("MISSING_FRAME")?;
    Ok(connection.resolve(guid, "Frame").await?)
}

impl Host {
    /// Resolve URL fields against the scope's context `baseURL`, then revalidate.
    /// Every driver dispatch path that can carry such fields calls this first.
    pub(super) fn resolve_request_base(&self, request: &mut Request) -> Result<(), Error> {
        if request.uses_base() {
            request.resolve_base(&mut |scope| {
                let state = self.state.lock().expect("Browser owner");
                let context = state.ancestor(scope, Kind::Context)?;
                state.context_options.get(&context)?.base()
            });
            validate(request)?;
        }
        Ok(())
    }
    pub(super) async fn execute_page_control(
        self: &Arc<Self>,
        id: Uuid,
        mut request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.resolve_request_base(&mut request)?;
        match request {
            Request::PageWait {
                page,
                frame,
                locator,
                condition,
            } => {
                self.wait_condition(Some(id), page, frame, locator, condition, deadline)
                    .await
            }
            Request::PageAction {
                page,
                frame,
                locator,
                action,
                force,
                postcondition,
            } => {
                self.page_action(
                    page,
                    frame,
                    locator,
                    action,
                    super::page::ActionOptions {
                        operation: Some(id),
                        force,
                        postcondition,
                    },
                    deadline,
                )
                .await
            }
            // Only page waits and actions are routed here; refuse anything else
            // before a driver call instead of panicking the operation task.
            _ => Err("CAPABILITY_UNAVAILABLE".into()),
        }
    }
    pub(super) async fn context_create(
        &self,
        operation: Uuid,
        browser: Reference,
        mut options: ContextOptions,
        deadline: Instant,
    ) -> Result<Reference, Error> {
        let engine = self.state.lock().expect("Browser owner").engine;
        if self.attached.load(Ordering::SeqCst) {
            contract::validate_attachment_context(engine, &options)?;
        }
        options.validate_for(engine)?;
        if let Some(certificates) = options.client_certificates.take() {
            let prepared = crate::browser::http::client_certificates::prepare_options(
                crate::browser::http::Options {
                    client_certificates: Some(certificates),
                    ..Default::default()
                },
                deadline,
            )
            .await?;
            options.client_certificates = prepared.client_certificates;
        }
        options.storage_state = super::storage::initial_storage_state(
            options.storage_state.take().map(|state| *state),
            engine,
            deadline,
        )
        .await?
        .map(Box::new);
        self.admissible(operation, deadline)?;
        let effective = {
            let state = self.state.lock().expect("Browser owner");
            options.effective_for(state.engine, state.launch_proxy.as_ref())?
        };
        let wire = self.wire(browser, Kind::Browser)?;
        let connection = self.connection()?;
        let context = channel(
            &connection,
            connection
                .retained(&wire, "newContext", options.wire(), deadline, false)
                .await?,
            "context",
            "BrowserContext",
        )
        .await?;
        self.register_launched_context(&connection, browser, context, effective, deadline)
            .await
    }
    pub(super) async fn register_launched_context(
        &self,
        connection: &crate::browser::connection::Connection,
        browser: Reference,
        context: crate::browser::objects::ObjectRef,
        effective: ContextOptions,
        deadline: Instant,
    ) -> Result<Reference, Error> {
        let reference = {
            let mut state = self.state.lock().expect("Browser owner");
            let reference = state.issue(Kind::Context, context.clone(), Some(browser))?;
            state.context_options.insert(reference, effective);
            reference
        };
        connection.retained(&context, "registerSelectorEngine", json!({"selectorEngine":{"name":"testrun","source":crate::browser::reading::REFERENCES,"contentScript":false}}), deadline, false).await?;
        connection
            .retained(
                &context,
                "updateSubscription",
                json!({"event":"dialog","enabled":true}),
                deadline,
                false,
            )
            .await?;
        connection
            .retained(
                &context,
                "addInitScript",
                json!({"source":super::page::INITIALIZATION}),
                deadline,
                false,
            )
            .await?;
        let record = {
            let state = self.state.lock().expect("Browser owner");
            state
                .context_options
                .get(&reference)
                .and_then(|options| Some((options.record_har.clone()?, options.clone())))
        };
        if let Some((record, options)) = record {
            let started = match record.options() {
                Ok(capture) => {
                    self.har_capture_start(
                        reference,
                        None,
                        capture,
                        None,
                        record.filter(&options),
                        deadline,
                    )
                    .await
                }
                Err(error) => Err(error),
            };
            started.map_err(|mut error| {
                error.partial = Some(
                    json!({"context":reference,"recordHarPending":true,"cause":error.partial}),
                );
                error
            })?;
        }
        connection.event_barrier(deadline).await?;
        self.state
            .lock()
            .expect("Browser owner")
            .resource(reference, Some(Kind::Context))?;
        Ok(reference)
    }
    pub(super) async fn profile_startup_page(
        &self,
        context: Reference,
        deadline: Instant,
    ) -> Result<Option<Reference>, Error> {
        let connection = self.connection()?;
        let pages = connection
            .children(&self.wire(context, Kind::Context)?, "Page")
            .await?;
        let [page] = pages.as_slice() else {
            return Ok(None);
        };
        let initializer = connection
            .initializer(page)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        if initializer["isClosed"] == true || initializer["opener"].is_object() {
            return Ok(None);
        }
        let frame = page_main_frame(&connection, &initializer).await?;
        // addInitScript covers subsequent documents. Install the same hooks in
        // the existing blank document without navigating or touching a restored page.
        let initialize = format!(
            "() => {{ if (location.href !== 'about:blank' || document.body?.hasChildNodes()) return false;\n{}\nreturn true; }}",
            super::page::INITIALIZATION
        );
        if self
            .evaluate_internal(&frame, &initialize, Value::Null, deadline)
            .await?
            != true
        {
            return Ok(None);
        }
        self.index_page(&connection, page.clone(), context)
            .await
            .map(Some)
    }

    pub(super) async fn page_create(
        &self,
        context: Reference,
        select: bool,
        deadline: Instant,
    ) -> Result<Reference, Error> {
        let wire = self.wire(context, Kind::Context)?;
        let connection = self.connection()?;
        let page = channel(
            &connection,
            connection
                .retained(&wire, "newPage", json!({}), deadline, false)
                .await?,
            "page",
            "Page",
        )
        .await?;
        let reference = self.index_page(&connection, page, context).await?;
        if select {
            let mut state = self.state.lock().expect("Browser owner");
            state.resource(reference, Some(Kind::Page))?;
            state.selected_context = Some(context);
            state.selected_page = Some(reference);
        }
        Ok(reference)
    }
    pub(super) async fn index_page(
        &self,
        connection: &Connection,
        page: ObjectRef,
        context: Reference,
    ) -> Result<Reference, Error> {
        let initializer = connection
            .initializer(&page)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        if initializer["isClosed"] == true {
            return Err("PAGE_CLOSED".into());
        }
        let frame = page_main_frame(connection, &initializer).await?;
        let (reference, created) = {
            let mut state = self.state.lock().expect("Browser owner");
            let created = state.by_wire(&page).is_none();
            let reference = state.issue(Kind::Page, page.clone(), Some(context))?;
            state.issue(Kind::Frame, frame, Some(reference))?;
            (reference, created)
        };
        if created {
            self.record_activity(
                reference,
                crate::browser::activity::Event::PageCreated,
                reference,
            )?;
        }
        connection
            .retained(
                &page,
                "updateSubscription",
                json!({"event":"fileChooser","enabled":true}),
                Instant::now() + Duration::from_secs(5),
                false,
            )
            .await?;
        Ok(reference)
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Typed dispatch delegates each operation to its capability owner without rewriting the request"
    )]
    pub(super) async fn execute(
        self: &Arc<Self>,
        operation: Uuid,
        mut request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.resolve_request_base(&mut request)?;
        match request {
            request @ (Request::HttpContextCreate { .. }
            | Request::HttpRequestInputsSet { .. }
            | Request::HttpFetchRequest { .. }
            | Request::HttpRequestInputRemove { .. }
            | Request::HttpContextList {}
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
            | Request::HttpFetch { .. }
            | Request::RouteFetch { .. }
            | Request::HttpResponseFinalize { .. }
            | Request::HttpResponseDiscard { .. }
            | Request::HttpResponseRead { .. }
            | Request::HttpResponseRemove { .. }
            | Request::HttpEvidenceList {}
            | Request::HttpEvidenceRead { .. }
            | Request::HttpEvidenceRemove { .. }) => {
                self.http_operation(operation, request, deadline).await
            }
            Request::PageCss { .. }
            | Request::PageAudit { .. }
            | Request::PageDebugger { .. }
            | Request::ProfileStart { .. }
            | Request::ProfileStatus { .. }
            | Request::ProfileStop { .. }
            | Request::ProfileTakeCoverage { .. }
            | Request::ProfileQuery { .. }
            | Request::PerformanceLoad { .. }
            | Request::PerformanceQuery { .. }
            | Request::PerformanceRelease { .. }
            | Request::HeapStop { .. }
            | Request::HeapCapture { .. }
            | Request::HeapImport { .. }
            | Request::HeapLoad { .. }
            | Request::HeapQuery { .. }
            | Request::HeapCompare { .. }
            | Request::HeapRelease { .. } => Err("NATIVE_DEVELOPER_UNSUPPORTED".into()),
            Request::TraceStart { context, options } => {
                self.trace_start(context, options, deadline).await
            }
            Request::TraceStatus {} => Ok(self.trace_status()),
            Request::ContextDebugger { context, action } => {
                self.context_debugger(context, action, deadline).await
            }
            request @ (Request::TraceStop { .. }
            | Request::TraceChunkStart { .. }
            | Request::TraceChunkStop {}
            | Request::TraceGroupStart { .. }
            | Request::TraceGroupEnd {}) => self.trace_command(request, deadline).await,
            request @ (Request::DeveloperEvidenceList {}
            | Request::DeveloperEvidenceFinalize { .. }
            | Request::DeveloperEvidenceRemove { .. }) => self.trace_evidence_operation(request),
            Request::DeveloperOpen { .. }
            | Request::DeveloperStatus { .. }
            | Request::DeveloperClose { .. } => Err("NATIVE_DEVELOPER_UNSUPPORTED".into()),
            // CPU and network throttling are native Chrome emulation only.
            Request::PageEmulateThrottling { .. } => Err("THROTTLING_UNSUPPORTED".into()),
            Request::ContextCacheStorageRead { .. } | Request::ContextCacheStorageDelete { .. } => {
                Err("CACHE_STORAGE_UNSUPPORTED".into())
            }
            Request::EventsRead {
                context,
                cursor,
                limit,
            } => self.events_read(context, cursor, limit, deadline).await,
            Request::StorageRegister { path } => self.storage_register(path),
            Request::StorageExport { context, options } => {
                self.storage_export(context, options, deadline).await
            }
            Request::StorageApply {
                context,
                snapshot,
                mode,
                install_authenticator,
            } => {
                self.storage_apply(context, snapshot, mode, install_authenticator, deadline)
                    .await
            }
            Request::StorageForget { snapshot } => self.storage_forget(snapshot),
            Request::ContextCookiesRead {
                context,
                urls,
                cursor,
                limit,
            } => {
                self.context_cookies_read(context, urls, cursor, limit, deadline)
                    .await
            }
            Request::ContextCookiesAdd { context, cookies } => {
                self.context_cookies_add(context, cookies, deadline).await
            }
            Request::ContextCookiesClear { context, options } => {
                self.context_cookies_clear(operation, context, options, deadline)
                    .await
            }
            Request::PageWebStorageRead {
                page,
                kind,
                name,
                cursor,
                limit,
            } => {
                self.page_web_storage_read(page, kind, name, cursor, limit, deadline)
                    .await
            }
            Request::PageWebStorageUpdate { page, kind, update } => {
                self.page_web_storage_update(page, kind, update, deadline)
                    .await
            }
            Request::ContextClock { context, action } => {
                self.context_clock(context, action, deadline).await
            }
            Request::ContextCredentials { context, action } => {
                self.context_credentials(context, action, deadline).await
            }
            request @ (Request::AuthBrokerList { .. }
            | Request::AuthSignIn { .. }
            | Request::AuthStatus { .. }) => {
                Box::pin(self.execute_authentication(operation, request, deadline)).await
            }
            Request::ContextConfigure { context, setting } => {
                self.context_configure(context, setting, deadline).await
            }
            Request::PageEmulateMedia { page, options } => {
                self.page_emulate_media(page, options, deadline).await
            }
            Request::PageSetExtraHttpHeaders { page, headers } => {
                self.page_set_extra_http_headers(page, headers, deadline)
                    .await
            }
            Request::PageRequestGc { page } => {
                let wire = self
                    .state
                    .lock()
                    .expect("Browser owner")
                    .resource(page, Some(Kind::Page))?
                    .wire
                    .clone();
                self.connection()?
                    .retained(&wire, "requestGC", json!({}), deadline, false)
                    .await?;
                Ok(json!({"page":page,"requested":true}))
            }
            Request::PageResize { page, viewport } => {
                self.page_resize(page, viewport, deadline).await
            }
            Request::PreviewStart { page, options } => {
                self.preview_start(page, options, deadline).await
            }
            Request::PreviewStatus { preview } => self.preview_status(preview),
            Request::PreviewStop { preview } => self.preview_stop(preview, deadline).await,
            Request::RecordingStart { page, options } => {
                self.recording_start(page, options, deadline).await
            }
            Request::RecordingStatus { recording } => {
                self.recording_result(recording, false, deadline).await
            }
            Request::RecordingStop { recording } => {
                self.recording_result(recording, true, deadline).await
            }
            Request::DiagnosticsStart { context } => {
                self.diagnostics_subscribe(context, true, deadline).await
            }
            Request::DiagnosticsStop { context } => {
                self.diagnostics_subscribe(context, false, deadline).await
            }
            Request::DiagnosticsRead { context, query } => {
                self.diagnostics_read(context, query, deadline).await
            }
            request @ (Request::BindingAdd { .. }
            | Request::BindingList { .. }
            | Request::BindingRemove { .. }
            | Request::BindingRead { .. }) => self.binding_operation(request, deadline).await,
            Request::NetworkRoute { scope, action } => {
                self.network_route(scope, action, deadline).await
            }
            Request::PageRouteSet {
                page,
                url,
                authentication_challenges,
            } => {
                self.page_route_set(page, url, authentication_challenges, deadline)
                    .await
            }
            Request::ContextRouteSet {
                context,
                url,
                authentication_challenges,
            } => {
                self.scoped_route_set(
                    context,
                    Kind::Context,
                    url,
                    authentication_challenges,
                    deadline,
                )
                .await
            }
            Request::PageHarSet {
                page,
                archive_id,
                not_found,
                url_filter,
            } => {
                self.page_har_set(page, archive_id, not_found, url_filter, deadline)
                    .await
            }
            Request::ContextHarSet {
                context,
                archive_id,
                not_found,
                url_filter,
            } => {
                self.scoped_har_set(
                    context,
                    Kind::Context,
                    archive_id,
                    not_found,
                    url_filter,
                    deadline,
                )
                .await
            }
            request @ (Request::WebSocketRouteAdd { .. }
            | Request::WebSocketRouteAddMatcher { .. }
            | Request::WebSocketRouteSend { .. }
            | Request::WebSocketRouteHandlers { .. }
            | Request::WebSocketRouteClose { .. }
            | Request::WebSocketRouteList { .. }
            | Request::WebSocketRouteRemove { .. }) => self.socket_route(request, deadline).await,
            request @ (Request::PageInitScriptAdd { .. }
            | Request::PageInitScriptList { .. }
            | Request::PageInitScriptRemove { .. }
            | Request::ContextInitScriptAdd { .. }
            | Request::ContextInitScriptList { .. }
            | Request::ContextInitScriptRemove { .. }) => self.init_script(request, deadline).await,
            Request::NetworkRead {
                context,
                request_id,
                options,
            } => {
                self.network_read(context, request_id, options, deadline)
                    .await
            }
            Request::HarRegister { path } => self
                .har_archives
                .lock()
                .expect("HAR owner")
                .register(std::path::Path::new(&path), self.journal.directory()),
            Request::HarList {} => Ok(self.har_list()),
            Request::HarCaptureStart {
                context,
                page,
                content,
                mode,
                path,
                resources_dir,
                replaces_archive_id,
                url_filter,
            } => {
                let content =
                    crate::browser::har::CaptureContent::for_capture(content, replaces_archive_id);
                let export = path
                    .as_deref()
                    .map(|path| {
                        crate::browser::har::Export::new(path, resources_dir.as_deref(), content)
                    })
                    .transpose()?;
                self.har_capture_start(
                    context,
                    page,
                    (content, mode, export),
                    replaces_archive_id,
                    url_filter,
                    deadline,
                )
                .await
            }
            Request::HarCaptureStatus { context } => self
                .har_archives
                .lock()
                .expect("HAR owner")
                .capture_status(context),
            Request::HarCaptureStop { context } => self.har_capture_stop(context, deadline).await,
            Request::HarFinalize { archive_id } => self
                .har_archives
                .lock()
                .expect("HAR owner")
                .finalize(self.journal.directory(), archive_id),
            Request::HarRemove { archive_id } => self.har_remove(archive_id),
            Request::NetworkEvidenceList {} => Ok(
                json!({"evidence":self.network_evidence.lock().expect("Network evidence").list()}),
            ),
            Request::NetworkEvidenceRemove { evidence_id } => {
                self.network_evidence_remove(evidence_id)
            }
            Request::Evaluate {
                scope,
                expression,
                is_function,
                argument,
                return_handle,
            } => {
                self.evaluate_scoped(
                    scope,
                    expression,
                    is_function,
                    argument,
                    return_handle,
                    deadline,
                )
                .await
            }
            Request::SelectorEvaluate { scope, options } => {
                self.selector_evaluate(scope, options, deadline).await
            }
            Request::HandleRead { handle } => self.handle_read(handle, deadline).await,
            Request::FrameElement { frame } => self.frame_element(frame, deadline).await,
            Request::FrameRetain { frame } => {
                self.connection()?.event_barrier(deadline).await?;
                let mut state = self.state.lock().expect("Browser owner");
                let owner = state.retain_frame(frame)?;
                let page = state.ancestor(owner, Kind::Page).ok_or("INVALID_SCOPE")?;
                Ok(
                    json!({"frame":owner,"documentFrame":frame,"page":page,"retained":true,"lifetime":"frame"}),
                )
            }
            Request::FrameOwnerRelease { frame } => {
                self.connection()?.event_barrier(deadline).await?;
                self.state
                    .lock()
                    .expect("Browser owner")
                    .release_frame(frame)?;
                Ok(json!({"frame":frame,"released":true,"nativeScopeClosed":false}))
            }
            Request::HandleOwnerFrame { handle } => self.handle_frame(handle, true, deadline).await,
            Request::HandleContentFrame { handle } => {
                self.handle_frame(handle, false, deadline).await
            }
            Request::WorkerList { page } => self.worker_list(page, deadline).await,
            Request::ContextWorkerList { context } => {
                self.context_worker_list(context, deadline).await
            }
            Request::HandleProperty { handle, name } => {
                self.handle_property(handle, name, deadline).await
            }
            Request::HandleProperties { handle, query } => {
                self.handle_properties(handle, query, deadline).await
            }
            Request::FixtureRegister { fixture } => self.fixture_register(fixture).await,
            Request::DirectoryRegister { name, entries } => {
                self.directory_register(name, entries).await
            }
            Request::FixtureList {} => Ok(
                json!({"items":self.transfers.lock().expect("Transfers").snapshot()["fixtures"]}),
            ),
            Request::FixtureVerify { fixture } => self.fixture_verify(fixture).await,
            Request::PageUpload {
                page,
                frame,
                locator,
                fixtures,
            } => self.upload(page, frame, locator, fixtures, deadline).await,
            Request::ChooserResolve { chooser, fixtures } => {
                self.chooser_resolve(chooser, fixtures, deadline).await
            }
            Request::DownloadList {} => Ok(
                json!({"items":self.transfers.lock().expect("Transfers").snapshot()["downloads"]}),
            ),
            Request::DownloadStatus { download } => self.download_status(download),
            Request::DownloadCancel { download } => self.download_cancel(download, deadline).await,
            Request::DownloadDelete { download } => self.download_delete(download, deadline).await,
            request => self.execute_page(operation, request, deadline).await,
        }
    }
    async fn page_clipboard(
        &self,
        page: Reference,
        origin: String,
        action: crate::browser::clipboard::Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if matches!(action, crate::browser::clipboard::Action::Read {})
            && self.state.lock().expect("Browser owner").engine == contract::Engine::Firefox
        {
            return Err("CLIPBOARD_READ_UNSUPPORTED".into());
        }
        let scope = self.page_scope(page, None, deadline).await?;
        let result = self
            .evaluate(
                &scope.wire,
                crate::browser::clipboard::SCRIPT,
                json!({"origin":origin,"action":action}),
                deadline,
            )
            .await?;
        let result = crate::browser::clipboard::result(result)?;
        if self.frame_current(&scope.wire, deadline).await? != scope.reference {
            return Err("DOCUMENT_CHANGED".into());
        }
        Ok(json!({"page":page,"result":result,"replayed":false}))
    }
    async fn execute_page(
        self: &Arc<Self>,
        operation: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match request {
            request @ (Request::PagePdf { .. }
            | Request::PageSnapshot { .. }
            | Request::PageAnnotate { .. }
            | Request::CaptureCompare { .. }) => self.execute_evidence(request, deadline).await,
            Request::PageClipboard {
                page,
                origin,
                action,
            } => self.page_clipboard(page, origin, action, deadline).await,
            Request::PageNavigate { page, navigation } => {
                self.navigate(page, navigation, deadline).await
            }
            Request::PageReadUrl {
                page,
                url,
                max_bytes,
            } => self.read_url(page, url, max_bytes, deadline).await,
            Request::PageStyles {
                page,
                frame,
                locator,
                properties,
                max_bytes,
            } => {
                self.page_styles(page, frame, locator, properties, max_bytes, deadline)
                    .await
            }
            Request::PageRead {
                page,
                frame,
                locator,
                read,
                max_bytes,
            } => {
                self.page_read(page, frame, locator, read, max_bytes, deadline)
                    .await
            }
            Request::PageAction {
                page,
                frame,
                locator,
                action,
                force,
                postcondition,
            } => {
                self.page_action(
                    page,
                    frame,
                    locator,
                    action,
                    super::page::ActionOptions {
                        operation: None,
                        force,
                        postcondition,
                    },
                    deadline,
                )
                .await
            }
            Request::PageWait {
                page,
                frame,
                locator,
                condition,
            } => {
                self.page_wait(page, frame, locator, condition, deadline)
                    .await
            }
            Request::FrameEnter {
                page,
                frame,
                locator,
            } => self.frame_enter(page, frame, locator, deadline).await,
            Request::PageScreenshot { page, capture } => {
                self.screenshot(page, capture, deadline).await
            }
            request => self.execute_resource(operation, request, deadline).await,
        }
    }
    #[expect(
        clippy::too_many_lines,
        reason = "one match over every resource request"
    )]
    async fn execute_resource(
        self: &Arc<Self>,
        operation: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match request {
            Request::ContextCreate { options } => {
                let browser = self
                    .state
                    .lock()
                    .expect("Browser owner")
                    .resources
                    .values()
                    .find(|r| r.kind == Kind::Browser)
                    .map(|r| r.reference)
                    .ok_or("BROWSER_CLOSED")?;
                Ok(
                    json!({"context":self.context_create(operation,browser,options,deadline).await?}),
                )
            }
            Request::ContextList {} | Request::PageList {} => {
                let kind = if matches!(request, Request::ContextList {}) {
                    Kind::Context
                } else {
                    Kind::Page
                };
                let state = self.state.lock().expect("Browser owner");
                Ok(
                    json!({"items":state.resources.values().filter(|r|r.kind==kind).map(|r|json!({"reference":r.reference,"parent":r.parent})).collect::<Vec<_>>()}),
                )
            }
            Request::ContextSelect { context } => {
                let mut state = self.state.lock().expect("Browser owner");
                state.resource(context, Some(Kind::Context))?;
                if state.selected_context != Some(context) {
                    state.selected_page = None;
                }
                state.selected_context = Some(context);
                Ok(json!({"context":context}))
            }
            Request::PageSelect { page } => {
                let mut state = self.state.lock().expect("Browser owner");
                state.resource(page, Some(Kind::Page))?;
                state.selected_context = state.ancestor(page, Kind::Context);
                state.selected_page = Some(page);
                Ok(json!({"page":page}))
            }
            Request::PageCreate { context, select } => {
                Ok(json!({"page":self.page_create(context,select,deadline).await?}))
            }
            Request::ContextClose { context } => {
                if *self.persistent_context.lock().expect("Profile context") == Some(context) {
                    return Err("DISCONNECT_PROFILE_CONTEXT".into());
                }
                self.close_trace(Some(context), deadline).await?;
                // Publish a recordHar capture while its responses are still readable.
                let recorded = if self
                    .har_archives
                    .lock()
                    .expect("HAR owner")
                    .capture_status(context)
                    .is_ok()
                {
                    Some(self.har_capture_stop(context, deadline).await?)
                } else {
                    None
                };
                let mut closed = self
                    .close_resource(context, Kind::Context, "close", deadline)
                    .await?;
                if let Some(recorded) = recorded {
                    closed["har"] = recorded;
                }
                Ok(closed)
            }
            Request::PageClose { page } => {
                self.close_resource(page, Kind::Page, "close", deadline)
                    .await
            }
            Request::FrameworkPrepare { .. }
            | Request::ContextFrameworkPrepare { .. }
            | Request::FrameworkOpen { .. }
            | Request::FrameworkQuery { .. }
            | Request::FrameworkRelease { .. } => self.framework_operation(request, deadline).await,
            Request::DeveloperToolsDiscover { .. }
            | Request::DeveloperToolCall { .. }
            | Request::DeveloperToolResultRead { .. }
            | Request::DeveloperToolsRelease { .. } => {
                self.developer_tools_operation(request, deadline).await
            }
            Request::WebMcpDebug { .. } | Request::WebMcp { .. } => {
                Err("NATIVE_WEB_MCP_UNSUPPORTED".into())
            }
            Request::BrowserExtension { .. } => Err("NATIVE_EXTENSION_UNSUPPORTED".into()),
            Request::BrowserPwa { .. } => Err("NATIVE_PWA_UNSUPPORTED".into()),
            Request::HandleDispose { handle } => {
                if self
                    .frameworks
                    .lock()
                    .expect("Frameworks")
                    .require(handle)
                    .is_ok()
                {
                    return Err("FRAMEWORK_RELEASE_REQUIRED".into());
                }
                self.close_resource(handle, Kind::Handle, "dispose", deadline)
                    .await
            }
            Request::HandleCreate { page, expression } => {
                self.handle_create(page, expression, deadline).await
            }
            Request::CallbackResolve { callback, response } => {
                self.callback_resolve(callback, response, deadline).await
            }
            // A request no earlier dispatch arm handled is unsupported on this
            // backend; it refuses before a driver call.
            _ => Err("CAPABILITY_UNAVAILABLE".into()),
        }
    }
    pub(super) async fn close_resource(
        &self,
        reference: Reference,
        kind: Kind,
        method: &str,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let retired = self
            .state
            .lock()
            .expect("Browser owner")
            .retired
            .get(&reference)
            .copied();
        if let Some(retired) = retired {
            return if retired == kind && kind != Kind::Handle {
                if kind == Kind::Context {
                    self.finish_closed_debugger(reference, deadline).await?;
                }
                Ok(json!({"closed":true}))
            } else if retired == kind {
                Err("STALE_REFERENCE".into())
            } else {
                Err("INVALID_SCOPE".into())
            };
        }
        let wire = self.wire(reference, kind)?;
        if kind == Kind::Context {
            self.automation_debuggers
                .lock()
                .unwrap()
                .prepare_close(reference);
        }
        let pages = {
            let state = self.state.lock().expect("Browser owner");
            state
                .subtree(reference)
                .into_iter()
                .filter(|resource| resource.kind == Kind::Page)
                .map(|resource| {
                    (
                        resource.reference,
                        state.retained_ancestor(resource.reference, Kind::Context),
                    )
                })
                .collect::<Vec<_>>()
        };
        self.state
            .lock()
            .expect("Browser owner")
            .closing
            .insert(reference);
        let connection = self.connection()?;
        let result = connection
            .retained(&wire, method, json!({}), deadline, true)
            .await;
        if let Err(error) = result {
            let error = Error::from(error);
            if error.code != "OUTCOME_UNKNOWN" {
                self.state
                    .lock()
                    .expect("Browser owner")
                    .closing
                    .remove(&reference);
                if kind == Kind::Context
                    && !self.state.lock().unwrap().retired.contains_key(&reference)
                {
                    self.automation_debuggers
                        .lock()
                        .unwrap()
                        .cancel_close(reference);
                }
            }
            return Err(error);
        }
        self.acknowledge_resource_close(reference, kind, &pages);
        self.changed.notify_waiters();
        for (page, _) in pages {
            self.end_page_previews(page, false).await?;
        }
        if kind == Kind::Context {
            self.request_inputs
                .lock()
                .expect("Request input owner")
                .close_context(reference);
            self.diagnostics
                .lock()
                .expect("Diagnostics")
                .enable(reference, false);
            self.finish_closed_debugger(reference, deadline).await?;
        }
        Ok(json!({"closed":true}))
    }
    fn acknowledge_resource_close(
        &self,
        reference: Reference,
        kind: Kind,
        pages: &[(Reference, Option<Reference>)],
    ) {
        let mut state = self.state.lock().expect("Browser owner");
        // Native close completion is authoritative even when its queued
        // event has not reached the lifetime consumer. Keep the witness
        // before retiring identities; late close events cannot revive them.
        let mut history = self.activity.lock().expect("Browser event history");
        for (page, context) in pages {
            if let Some(context) = context {
                history.push(
                    *context,
                    *page,
                    crate::browser::activity::Event::PageClosed,
                    *page,
                );
            }
        }
        if kind == Kind::Context {
            // Close acknowledgement can precede the queued close/dispose
            // event. Retire associated HTTP work while its owner is still
            // indexed; a late event cannot recover an already-retired owner.
            self.http_contexts
                .lock()
                .expect("HTTP owner")
                .context_defaults_closed(reference);
        }
        state.retire(reference);
    }

    async fn handle_create(
        &self,
        page: Reference,
        expression: String,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let frame = {
            let state = self.state.lock().expect("Browser owner");
            state.resource(page, Some(Kind::Page))?;
            state
                .resources
                .values()
                .find(|r| r.kind == Kind::Frame && r.parent == Some(page))
                .cloned()
                .ok_or("FRAME_UNAVAILABLE")?
        };
        let connection = self.connection()?;
        let result = connection
            .retained(
                &frame.wire,
                "evaluateExpressionHandle",
                json!({"expression":expression,"arg":values::argument(&Value::Null)?}),
                deadline,
                false,
            )
            .await?;
        let result = result.ok_or("MISSING_RESULT")?;
        let guid = result["handle"]["guid"].as_str().ok_or("MISSING_HANDLE")?;
        let handle = match connection.resolve(guid, "JSHandle").await {
            Ok(handle) => handle,
            Err(_) => connection.resolve(guid, "ElementHandle").await?,
        };
        let reference = {
            let mut state = self.state.lock().expect("Browser owner");
            match state.resource(frame.reference, Some(Kind::Frame)) {
                Ok(_) => state.issue(Kind::Handle, handle.clone(), Some(frame.reference)),
                Err(error) => Err(error),
            }
        };
        match reference {
            Ok(reference) => Ok(json!({"handle":reference})),
            Err(error) => {
                if connection
                    .retained(
                        &handle,
                        "dispose",
                        json!({}),
                        Instant::now() + Duration::from_secs(30),
                        true,
                    )
                    .await
                    .is_err()
                {
                    self.set_failure("HANDLE_CLEANUP_PENDING");
                }
                Err(error.into())
            }
        }
    }
    /// `route_fulfill_http_evidence` becomes `route_fulfill_evidence` over a private
    /// network-evidence copy of the HTTP response body.
    fn http_evidence_reply(
        &self,
        reference: Reference,
        response: CallbackResponse,
    ) -> Result<CallbackResponse, Error> {
        let CallbackResponse::RouteFulfillHttpEvidence {
            status,
            http_evidence_id,
            headers,
        } = response
        else {
            return Ok(response);
        };
        let context = {
            let state = self.state.lock().expect("Browser owner");
            state.resource(reference, Some(Kind::Route))?;
            state
                .ancestor(reference, Kind::Context)
                .ok_or("STALE_REFERENCE")?
        };
        let artifact = self
            .http_contexts
            .lock()
            .expect("HTTP owner")
            .evidence_get(http_evidence_id)?;
        let (bytes, status) = crate::browser::http::fulfill_source(&artifact, status)?;
        let copy = crate::browser::network::write_body(
            self.journal.directory(),
            context,
            Uuid::new_v4(),
            crate::browser::network::BodySide::Response,
            &bytes,
            crate::browser::network::MAX_BODY,
        )?;
        let summary = self
            .network_evidence
            .lock()
            .expect("Network evidence")
            .insert(copy)?;
        Ok(CallbackResponse::RouteFulfillEvidence {
            status,
            network_evidence_id: serde_json::from_value(summary["evidenceId"].clone())
                .map_err(|_| Error::from("BACKEND_PROTOCOL"))?,
            headers,
        })
    }
    pub(super) async fn callback_resolve(
        self: &Arc<Self>,
        reference: Reference,
        response: CallbackResponse,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.callback_reply(reference, response, deadline, false)
            .await
    }
    async fn callback_parameters(
        &self,
        resource: &crate::browser::lifecycle::Resource,
        response: CallbackResponse,
    ) -> Result<(&'static str, Value, Option<Value>), Error> {
        let (method, params) = match (resource.kind, response) {
            (
                Kind::Dialog,
                CallbackResponse::Dialog {
                    accept,
                    prompt_text,
                },
            ) => {
                if accept {
                    let mut params = json!({});
                    if let Some(text) = prompt_text {
                        params["promptText"] = json!(text);
                    }
                    ("accept", params)
                } else {
                    ("dismiss", json!({}))
                }
            }
            (Kind::Route, CallbackResponse::RouteAbort { error_code }) => (
                "abort",
                json!({"errorCode":error_code.unwrap_or_default().code()}),
            ),
            (Kind::Route, CallbackResponse::RouteContinue {}) => {
                ("continue", json!({"isFallback":false}))
            }
            (
                Kind::Route,
                CallbackResponse::RouteFallback {} | CallbackResponse::RouteFallbackWith { .. },
            ) => ("continue", json!({"isFallback":true})),
            (Kind::Route, CallbackResponse::RouteContinueWith { .. }) => {
                ("continue", json!({"isFallback":false}))
            }
            (
                Kind::Route,
                CallbackResponse::RouteFulfill {
                    status,
                    body,
                    json,
                    content_type,
                    headers,
                    file,
                    ..
                },
            ) => {
                let (status, body_base64, headers) = crate::browser::routing::text_fulfill(
                    status,
                    body.as_deref(),
                    json.as_ref(),
                    content_type.as_deref(),
                    &headers,
                    file.as_ref()
                        .map(crate::browser::routing::FulfillFile::parts),
                )?;
                if file.is_some() {
                    let binary = CallbackResponse::RouteFulfillBinary {
                        status,
                        body_base64,
                        headers,
                    };
                    return self.artifact_callback_parameters(resource, binary).await;
                }
                // The pinned client sends string and JSON bodies as text; WebKit
                // then defaults a missing content type to text/plain.
                let params = self
                    .text_route_response(&resource.wire, status, &body_base64, &headers)
                    .await?;
                ("fulfill", params)
            }
            (
                Kind::Route,
                response @ (CallbackResponse::RouteFulfillBinary { .. }
                | CallbackResponse::RouteFulfillEvidence { .. }
                | CallbackResponse::RouteHar { .. }),
            ) => return self.artifact_callback_parameters(resource, response).await,
            (Kind::Binding, CallbackResponse::BindingReject {}) => (
                "reject",
                json!({"error":{"error":{"name":"Error","message":"Testrun callback expired or scope closed"}}}),
            ),
            (Kind::Binding, CallbackResponse::BindingResolve { value }) => {
                ("resolve", json!({"result":values::argument(&value)?}))
            }
            (Kind::Binding, CallbackResponse::BindingResolveValue { value }) => (
                "resolve",
                json!({"result":crate::browser::bindings::value_argument(&value)?}),
            ),
            (Kind::Binding, CallbackResponse::BindingRejectValue { value }) => (
                "reject",
                json!({"error":{"value":crate::browser::bindings::value_argument(&value)?["value"]}}),
            ),
            _ => return Err("CALLBACK_RESPONSE_MISMATCH".into()),
        };
        Ok((method, params, None))
    }
    /// Hand a paused route to its next handler; `None` means no handler remains.
    async fn route_fallback_handoff(
        self: &Arc<Self>,
        reference: Reference,
        resource: &crate::browser::lifecycle::Resource,
        overrides: &crate::browser::routing::Overrides,
    ) -> Result<Option<Value>, Error> {
        let Some(target) = self.route_fallback_target(resource).await? else {
            return Ok(None);
        };
        let next = self.handoff_route(reference, &target, overrides)?;
        let handler = self
            .routing
            .rules
            .lock()
            .expect("Route owner")
            .handlers
            .current_har(next);
        let started = match (target.rule, handler) {
            (Some(rule), _) => Some(self.start_har_reply(next, rule).await),
            // The next handler in this chain replies from its archive.
            (None, Some(handler)) => Some(self.start_handler_har_reply(next, handler).await),
            (None, None) => None,
        };
        if let Some(started) = started {
            started.map_err(|mut error| {
                error.partial =
                    Some(json!({"fallback":true,"nextCallback":next,"cleanupPending":true}));
                error
            })?;
        }
        Ok(Some(
            json!({"resolved":true,"fallback":true,"nextCallback":next}),
        ))
    }
    async fn artifact_callback_parameters(
        &self,
        resource: &crate::browser::lifecycle::Resource,
        response: CallbackResponse,
    ) -> Result<(&'static str, Value, Option<Value>), Error> {
        let reference = resource.reference;
        let mut har_summary = None;
        let (method, params) = match response {
            CallbackResponse::RouteFulfillBinary {
                status,
                body_base64,
                headers,
            } => {
                crate::browser::routing::inline_response(status, &body_base64, &headers)?;
                let params = self
                    .binary_route_response(&resource.wire, status, &body_base64, &headers)
                    .await?;
                self.state
                    .lock()
                    .expect("Browser owner")
                    .resource(reference, Some(Kind::Route))?;
                ("fulfill", params)
            }
            CallbackResponse::RouteFulfillEvidence {
                status,
                network_evidence_id,
                headers,
            } => {
                use base64::Engine as _;
                let artifact = self
                    .network_evidence
                    .lock()
                    .expect("Network evidence")
                    .get(network_evidence_id)?;
                let body = base64::engine::general_purpose::STANDARD
                    .encode(crate::browser::network::evidence_body(&artifact)?);
                let params = self
                    .binary_route_response(&resource.wire, status, &body, &headers)
                    .await?;
                self.state
                    .lock()
                    .expect("Browser owner")
                    .resource(reference, Some(Kind::Route))?;
                ("fulfill", params)
            }
            CallbackResponse::RouteHar {
                archive_id,
                not_found,
            } => {
                let (method, params, summary) = self
                    .har_route_response(&resource.wire, archive_id, not_found)
                    .await?;
                self.state
                    .lock()
                    .expect("Browser owner")
                    .resource(reference, Some(Kind::Route))?;
                har_summary = Some(summary);
                (method, params)
            }
            _ => return Err("CALLBACK_RESPONSE_MISMATCH".into()),
        };
        Ok((method, params, har_summary))
    }
    fn callback_policy_scopes(&self, reference: Reference, kind: Kind) -> Vec<Reference> {
        if kind == Kind::Route {
            let mut scopes = self
                .routing
                .owners
                .lock()
                .expect("Route owners")
                .get(&reference)
                .copied()
                .into_iter()
                .collect::<Vec<_>>();
            scopes.extend(
                self.routing
                    .handoffs
                    .lock()
                    .expect("Route handoffs")
                    .values()
                    .filter(|handoff| handoff.target == reference)
                    .map(|handoff| handoff.source),
            );
            scopes
        } else {
            Vec::new()
        }
    }
    fn settle_callback(
        &self,
        resource: &crate::browser::lifecycle::Resource,
        succeeded: bool,
    ) -> Vec<Reference> {
        let reference = resource.reference;
        let scopes = self.callback_policy_scopes(reference, resource.kind);
        self.callbacks
            .lock()
            .expect("Callback owner")
            .remove(&reference);
        if succeeded {
            self.routing
                .overrides
                .lock()
                .expect("Route overrides")
                .remove(&resource.wire);
            self.routing
                .handoffs
                .lock()
                .expect("Route handoffs")
                .retain(|_, handoff| handoff.target != reference);
            self.routing
                .owners
                .lock()
                .expect("Route owners")
                .remove(&reference);
            self.binding_calls
                .lock()
                .expect("Binding calls")
                .remove(&reference);
            self.state.lock().expect("Browser owner").retire(reference);
            self.responding
                .lock()
                .expect("Callback response owner")
                .remove(&reference);
        } else {
            self.set_failure("CALLBACK_OUTCOME_UNKNOWN");
        }
        scopes
    }
    pub(super) async fn callback_reply(
        self: &Arc<Self>,
        reference: Reference,
        response: CallbackResponse,
        deadline: Instant,
        expiry_cleanup: bool,
    ) -> Result<Value, Error> {
        let response = self.http_evidence_reply(reference, response)?;
        let resource = self
            .state
            .lock()
            .expect("Browser owner")
            .resource(reference, None)?
            .clone();
        let mut overrides = self
            .routing
            .overrides
            .lock()
            .expect("Route overrides")
            .get(&resource.wire)
            .cloned()
            .unwrap_or_default();
        if let CallbackResponse::RouteContinueWith { overrides: next }
        | CallbackResponse::RouteFallbackWith { overrides: next } = &response
        {
            if resource.kind != Kind::Route {
                return Err("CALLBACK_RESPONSE_MISMATCH".into());
            }
            overrides = overrides.overlay(next);
            self.route_overrides(&resource.wire, &overrides).await?;
            self.update_route_request_input(&resource.wire, &overrides)
                .await?;
        }
        if resource.kind == Kind::Route
            && matches!(
                response,
                CallbackResponse::RouteFallback {} | CallbackResponse::RouteFallbackWith { .. }
            )
            && let Some(result) = self
                .route_fallback_handoff(reference, &resource, &overrides)
                .await?
        {
            return Ok(result);
        }
        let response = crate::browser::routing::load_fulfill_file(response, deadline).await?;
        let (mut method, mut params, har_summary) =
            self.callback_parameters(&resource, response).await?;
        if method == "fallback" {
            if let Some(mut result) = self
                .route_fallback_handoff(reference, &resource, &overrides)
                .await?
            {
                result["har"] = har_summary.unwrap_or(Value::Null);
                return Ok(result);
            }
            method = "continue";
        }
        if resource.kind == Kind::Route && method == "continue" && !overrides.is_empty() {
            let changes = self.route_overrides(&resource.wire, &overrides).await?;
            params
                .as_object_mut()
                .expect("Continue parameters")
                .extend(changes.as_object().expect("Validated overrides").clone());
        }
        self.admit_callback_reply(reference, resource.kind, deadline, expiry_cleanup)?;
        self.record(json!({"callback":reference,"kind":resource.kind,"stage":"resolving"}))
            .await?;
        let result = self
            .connection()?
            .retained(&resource.wire, method, params, deadline, true)
            .await;
        let scopes = self.settle_callback(&resource, result.is_ok());
        self.record(json!({"callback":reference,"stage":"settled","succeeded":result.is_ok()}))
            .await?;
        result?;
        let mut result = json!({"resolved":true});
        if let Some(summary) = har_summary {
            result["har"] = summary;
        }
        let failures = Box::pin(self.cleanup_exhausted_handlers(scopes, deadline)).await;
        if !failures.is_empty() {
            result["handlerCleanupPending"] = json!(failures);
        }
        Ok(result)
    }
}
impl Host {
    fn admit_callback_reply(
        &self,
        reference: Reference,
        kind: Kind,
        deadline: Instant,
        expiry_cleanup: bool,
    ) -> Result<(), Error> {
        let state = self.state.lock().expect("Browser owner");
        state.resource(reference, Some(kind))?;
        if !expiry_cleanup
            && self
                .callbacks
                .lock()
                .expect("Callback owner")
                .get(&reference)
                .is_none_or(|expiry| *expiry <= Instant::now())
        {
            return Err("CALLBACK_EXPIRED".into());
        }
        if Instant::now() >= deadline {
            return Err("NOT_DISPATCHED".into());
        }
        if !self
            .responding
            .lock()
            .expect("Callback response owner")
            .insert(reference)
        {
            return Err("CALLBACK_ALREADY_RESOLVING".into());
        }
        Ok(())
    }
}
