//! Owned native context/page allocation, setup and reference-scoped workflows.

use super::*;

fn native_headers(headers: &[crate::browser::http::Header]) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|header| (header.name.to_ascii_lowercase(), header.value.clone()))
        .collect()
}

impl Worker {
    pub(super) fn headers_for_session(&self, session: &str) -> Option<BTreeMap<String, String>> {
        let (context, page, worker) = self.index.diagnostic_scope(session)?;
        // Pinned Chromium never attaches shared workers, so they get no headers.
        if worker.is_some_and(|worker| {
            self.index.worker_scope(worker).is_ok_and(|(_, _, kind)| {
                kind == crate::browser::cdp::resources::WorkerKind::Shared
            })
        }) {
            return None;
        }
        let context_headers = self
            .context_layout
            .get(&context)
            .and_then(|options| options.extra_http_headers.as_deref());
        let page_headers = page.and_then(|page| self.page_headers.get(&page));
        if context_headers.is_none() && page_headers.is_none() {
            return None;
        }
        let mut headers = native_headers(context_headers.unwrap_or_default());
        if let Some(page_headers) = page_headers {
            headers.extend(native_headers(page_headers));
        }
        Some(headers)
    }
    async fn configure_page_headers(
        &mut self,
        id: Uuid,
        page: Reference,
        headers: Vec<crate::browser::http::Header>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.index
            .native(page, Kind::Page)
            .map_err(resource_error)?;
        crate::browser::http::validate_headers(&headers)?;
        if Instant::now() >= deadline {
            return Err("NOT_DISPATCHED".into());
        }
        self.page_headers.insert(page, headers.clone());
        let mut sessions = BTreeSet::new();
        for kind in [Kind::Page, Kind::Frame] {
            for (reference, _) in self.index.list(kind) {
                if let Ok(route) = self.index.route(reference, Some(kind))
                    && self
                        .index
                        .diagnostic_scope(&route.session_id)
                        .is_some_and(|(_, owner, _)| owner == Some(page))
                {
                    sessions.insert(route.session_id);
                }
            }
        }
        let mut completed = 0;
        for session in &sessions {
            let result = async {
                self.call(id, Some(session), "Network.enable", json!({}), deadline)
                    .await?;
                let merged = self.headers_for_session(session).ok_or("STALE_REFERENCE")?;
                self.call(
                    id,
                    Some(session),
                    "Network.setExtraHTTPHeaders",
                    json!({"headers":merged}),
                    deadline,
                )
                .await
            }
            .await;
            if let Err(mut error) = result {
                error.partial = Some(
                    json!({"page":page,"headerCount":headers.len(),"configurationComplete":false,"completedAttachments":completed,"selectedAttachments":sessions.len(),"stateMayHaveChanged":true,"replayed":false,"restoration":"explicit-reconfiguration-or-close-owned-page","cause":error.partial}),
                );
                return Err(error);
            }
            completed += 1;
        }
        self.index
            .native(page, Kind::Page)
            .map_err(resource_error)?;
        Ok(
            json!({"page":page,"headerCount":headers.len(),"configuredAttachments":completed,"dispatchCompleted":true,"effectsObserved":false,"futureAttachments":true,"backend":"chrome_cdp","replayed":false}),
        )
    }
    /// Best-effort reapplication of each session's restored merged headers.
    /// The boxed trait object bounds the worker future's auto-trait depth.
    fn restore_context_headers<'a>(
        &'a mut self,
        id: Uuid,
        sessions: &'a BTreeSet<String>,
        deadline: Instant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = usize> + Send + 'a>> {
        Box::pin(async move {
            let mut restored = 0;
            for session in sessions {
                let list = self.headers_for_session(session).unwrap_or_default();
                if self
                    .call(
                        id,
                        Some(session),
                        "Network.setExtraHTTPHeaders",
                        json!({"headers":list}),
                        deadline,
                    )
                    .await
                    .is_ok()
                {
                    restored += 1;
                }
            }
            restored
        })
    }
    async fn configure_context_headers(
        &mut self,
        id: Uuid,
        context: Reference,
        headers: Vec<crate::browser::http::Header>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        crate::browser::http::validate_headers(&headers)?;
        let previous = self
            .context_layout
            .get_mut(&context)
            .ok_or("INVALID_SCOPE")?
            .extra_http_headers
            .replace(headers.clone());
        let _ = self.http_contexts.context_headers_pending(context);
        // Store the future-target setting before native awaits. Existing
        // attachments are selected by their exact owned context and deduplicated.
        let mut sessions = BTreeSet::new();
        for kind in [Kind::Page, Kind::Frame, Kind::Worker] {
            for (reference, _) in self.index.list(kind) {
                if let Ok(route) = self.index.route(reference, Some(kind))
                    && self
                        .index
                        .diagnostic_scope(&route.session_id)
                        .is_some_and(|(owner, _, _)| owner == context)
                    && self.headers_for_session(&route.session_id).is_some()
                {
                    sessions.insert(route.session_id);
                }
            }
        }
        let mut completed = 0;
        for session in &sessions {
            let configured = async {
                self.call(id, Some(session), "Network.enable", json!({}), deadline)
                    .await?;
                self.call(
                    id,
                    Some(session),
                    "Network.setExtraHTTPHeaders",
                    json!({"headers":self.headers_for_session(session).ok_or("STALE_REFERENCE")?}),
                    deadline,
                )
                .await
            }
            .await;
            if let Err(mut error) = configured {
                // Pinned setExtraHTTPHeaders restores the previous option and
                // reapplies it best effort. HTTP defaults follow the restored list
                // only when no attachment can still hold the new one: the failure
                // was a definite reply and every reapplication succeeded.
                if let Some(layout) = self.context_layout.get_mut(&context) {
                    layout.extra_http_headers.clone_from(&previous);
                }
                let restored = self.restore_context_headers(id, &sessions, deadline).await;
                let clean = matches!(error.code, "BROWSER_METHOD_FAILED" | "NOT_DISPATCHED")
                    && restored == sessions.len()
                    && self.context_layout.contains_key(&context);
                if clean {
                    self.http_contexts
                        .context_headers_applied(context, previous.as_deref().unwrap_or(&[]));
                }
                error.partial = Some(
                    json!({"context":context,"headerCount":headers.len(),"configurationComplete":false,"completedAttachments":completed,"selectedAttachments":sessions.len(),"rolledBack":true,"restoredAttachments":restored,"stateMayHaveChanged":!clean,"httpDefaults":if clean {"previous-headers"} else {"refused-until-acknowledged-reconfiguration"},"replayed":false,"cause":error.partial}),
                );
                return Err(error);
            }
            completed += 1;
        }
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        self.http_contexts
            .context_headers_applied(context, &headers);
        Ok(
            json!({"context":context,"headerCount":headers.len(),"dispatchCompleted":true,"effectsObserved":false,"configuredAttachments":completed,"futureAttachments":true,"backend":"chrome_cdp"}),
        )
    }
    async fn execute_lifecycle(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match request {
            Request::ContextCreate { options } => {
                self.create_context_with_state(id, options, deadline).await
            }
            Request::ContextConfigure {
                context,
                setting: crate::browser::settings::ContextSetting::ExtraHttpHeaders { headers },
            } => {
                // A trait object bounds the worker future's auto-trait depth.
                let configured: std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + '_>,
                > = Box::pin(self.configure_context_headers(id, context, headers, deadline));
                configured.await
            }
            Request::ContextConfigure {
                context,
                setting: crate::browser::settings::ContextSetting::HttpCredentials { credentials },
            } => {
                self.configure_context_credentials(id, context, credentials, deadline)
                    .await
            }
            Request::ContextConfigure { context, setting } => {
                self.configure_environment(id, context, setting, deadline)
                    .await
            }
            Request::ContextClock { context, action } => {
                self.context_clock(id, context, action, deadline).await
            }
            Request::ContextCredentials { context, action } => {
                self.context_credentials(id, context, action, deadline)
                    .await
            }
            Request::PageEmulateMedia { page, options } => {
                self.page_emulate_media(id, page, options, deadline).await
            }
            Request::PageEmulateThrottling { page, options } => {
                self.page_emulate_throttling(id, page, options, deadline)
                    .await
            }
            Request::PageSetExtraHttpHeaders { page, headers } => {
                self.configure_page_headers(id, page, headers, deadline)
                    .await
            }
            // Pinned SDK Chromium: collect garbage in the page's main session.
            Request::PageRequestGc { page } => {
                let session = self
                    .index
                    .route(page, Some(Kind::Page))
                    .map_err(resource_error)?
                    .session_id;
                self.call(
                    id,
                    Some(&session),
                    "HeapProfiler.collectGarbage",
                    json!({}),
                    deadline,
                )
                .await?;
                Ok(json!({"page":page,"requested":true}))
            }
            Request::PageCreate { context, select } => {
                let page = self.page_create(id, context, deadline).await?;
                if select {
                    let mut state = self.shared.state.lock().expect("Chrome lifecycle");
                    state.selected_context = Some(context);
                    state.selected_page = Some(page);
                }
                Ok(json!({"page":page}))
            }
            Request::ContextList {} | Request::PageList {} => {
                let kind = if matches!(request, Request::ContextList {}) {
                    Kind::Context
                } else {
                    Kind::Page
                };
                Ok(
                    json!({"items":self.index.list(kind).iter().map(|(reference,parent)| json!({"reference":reference,"parent":parent})).collect::<Vec<_>>()}),
                )
            }
            request @ (Request::ContextSelect { .. } | Request::PageSelect { .. }) => {
                self.select_lifecycle(request)
            }
            Request::ContextClose { context } => {
                // Publish a recordHar capture while its responses are still readable.
                let recorded = if self.har_archives.capture_status(context).is_ok() {
                    Some(self.har_capture_stop(id, context, deadline).await?)
                } else {
                    None
                };
                let mut closed = self.close_context(id, context, deadline).await?;
                if let Some(recorded) = recorded {
                    closed["har"] = recorded;
                }
                Ok(closed)
            }
            Request::PageClose { page } => self.close_page(id, page, deadline).await,
            request => self.execute_storage(id, request, deadline).await,
        }
    }
    async fn create_context_with_state(
        &mut self,
        id: Uuid,
        options: contract::ContextOptions,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let explicit_proxy = options.proxy.is_some();
        let initial_state = self
            .native_load_initial_storage(id, options.storage_state.as_deref().cloned(), deadline)
            .await?;
        let options = options.effective_for(
            contract::Engine::Chrome,
            self.shared
                .state
                .lock()
                .expect("Chrome state")
                .launch_proxy
                .as_ref(),
        )?;
        let (context, options) = self
            .context_create_configured(id, options, explicit_proxy, deadline)
            .await?;
        self.context_layout.insert(context, options.clone());
        self.http_contexts
            .context_credentials_initial(context, options.http_credentials.as_ref());
        if options.has_touch {
            self.touch_contexts.insert(context);
        }
        self.configure_downloads(id, context, options.accept_downloads, deadline)
            .await?;
        self.initialize_context_emulation(id, context, &options, deadline)
            .await?;
        if let Some(record) = &options.record_har {
            self.har_capture_start(
                id,
                context,
                None,
                record.options()?,
                None,
                record.filter(&options),
                deadline,
            )
            .await
            .map_err(|mut error| {
                error.partial =
                    Some(json!({"context":context,"recordHarPending":true,"cause":error.partial}));
                error
            })?;
        }
        if let Some(state) = initial_state {
            self.native_initial_storage(id, context, state, deadline)
                .await?;
        }
        Ok(json!({"context":context}))
    }

    async fn execute_storage(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match request {
            Request::StorageRegister { path } => self.native_storage_register(path),
            Request::StorageExport { context, options } => {
                self.native_storage_export(id, context, options, deadline)
                    .await
            }
            Request::StorageApply {
                context,
                snapshot,
                mode,
                install_authenticator,
            } => {
                self.native_storage_apply(
                    id,
                    context,
                    snapshot,
                    mode,
                    install_authenticator,
                    deadline,
                )
                .await
            }
            Request::StorageForget { snapshot } => self.native_storage_forget(snapshot),
            Request::ContextCacheStorageRead {
                context,
                origin,
                cache_name,
                url,
                cursor,
                limit,
            } => {
                self.native_cache_storage_read(
                    id, context, origin, cache_name, url, cursor, limit, deadline,
                )
                .await
            }
            Request::ContextCacheStorageDelete {
                context,
                origin,
                cache_name,
                url,
            } => {
                self.native_cache_storage_delete(id, context, origin, cache_name, url, deadline)
                    .await
            }
            Request::ContextCookiesRead {
                context,
                urls,
                cursor,
                limit,
            } => {
                self.native_context_cookies_read(id, context, urls, cursor, limit, deadline)
                    .await
            }
            Request::ContextCookiesAdd { context, cookies } => {
                self.native_context_cookies_add(id, context, cookies, deadline)
                    .await
            }
            Request::ContextCookiesClear { context, options } => {
                self.native_context_cookies_clear(id, context, options, deadline)
                    .await
            }
            Request::PageWebStorageRead {
                page,
                kind,
                name,
                cursor,
                limit,
            } => {
                self.native_page_web_storage_read(id, page, kind, name, cursor, limit, deadline)
                    .await
            }
            Request::PageWebStorageUpdate { page, kind, update } => {
                self.native_page_web_storage_update(id, page, kind, update, deadline)
                    .await
            }
            _ => Err("CAPABILITY_UNAVAILABLE".into()),
        }
    }

    fn select_lifecycle(&mut self, request: Request) -> Result<Value, Error> {
        match request {
            Request::ContextSelect { context } => {
                self.index
                    .native(context, Kind::Context)
                    .map_err(resource_error)?;
                let mut state = self.shared.state.lock().expect("Chrome lifecycle");
                if state.selected_context != Some(context) {
                    state.selected_page = None;
                }
                state.selected_context = Some(context);
                Ok(json!({"context":context}))
            }
            Request::PageSelect { page } => {
                self.index
                    .native(page, Kind::Page)
                    .map_err(resource_error)?;
                let parent = self
                    .index
                    .list(Kind::Page)
                    .into_iter()
                    .find(|(r, _)| *r == page)
                    .and_then(|(_, p)| p);
                let mut state = self.shared.state.lock().expect("Chrome lifecycle");
                state.selected_page = Some(page);
                state.selected_context = parent;
                Ok(json!({"page":page}))
            }
            _ => Err("CAPABILITY_UNAVAILABLE".into()),
        }
    }
    #[cfg(test)]
    pub(super) async fn context_create(
        &mut self,
        id: Uuid,
        deadline: Instant,
    ) -> Result<Reference, Error> {
        self.context_create_with_proxy_bound(id, None, None, deadline)
            .await
    }
    pub(super) async fn context_create_with_proxy_bound(
        &mut self,
        id: Uuid,
        proxy: Option<&contract::ContextProxy>,
        tunnel: Option<Uuid>,
        deadline: Instant,
    ) -> Result<Reference, Error> {
        let mut params = json!({"disposeOnDetach":true});
        if let Some(proxy) = proxy {
            params
                .as_object_mut()
                .expect("Context creation parameters")
                .extend(
                    proxy
                        .native_fields()
                        .as_object()
                        .expect("Private proxy parameters")
                        .clone(),
                );
        }
        let value = self
            .call(id, None, "Target.createBrowserContext", params, deadline)
            .await?;
        let native = value["browserContextId"]
            .as_str()
            .ok_or("BACKEND_PROTOCOL")?;
        let context = self
            .index
            .register_context(native)
            .map_err(resource_error)?;
        if let Some(tunnel) = tunnel {
            self.certificate_tunnels.bind(tunnel, context);
        }
        self.call(
            id,
            None,
            "Browser.setDownloadBehavior",
            json!({"behavior":"deny","browserContextId":native}),
            deadline,
        )
        .await?;
        Ok(context)
    }
    pub(super) async fn page_create(
        &mut self,
        id: Uuid,
        context: Reference,
        deadline: Instant,
    ) -> Result<Reference, Error> {
        if self
            .adoption
            .as_ref()
            .is_some_and(|(borrowed, _)| *borrowed == context)
        {
            return Err("ATTACHMENT_ADOPTED_CONTEXT_READ_ONLY_ALLOCATION".into());
        }
        let context = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        let value = self
            .call(
                id,
                None,
                "Target.createTarget",
                json!({"url":"about:blank","browserContextId":context}),
                deadline,
            )
            .await?;
        let target = value["targetId"]
            .as_str()
            .ok_or("BACKEND_PROTOCOL")?
            .to_owned();
        let info = self
            .call(
                id,
                None,
                "Target.getTargetInfo",
                json!({"targetId":target}),
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
        let page = self.index.target_reference(&target).ok_or("PAGE_CLOSED")?;
        let session = if self.startup.enabled {
            self.startup_attachment(id, page, deadline).await?
        } else {
            let attach = self
                .call(
                    id,
                    None,
                    "Target.attachToTarget",
                    json!({"targetId":target,"flatten":true}),
                    deadline,
                )
                .await?;
            let session = attach["sessionId"]
                .as_str()
                .ok_or("BACKEND_PROTOCOL")?
                .to_owned();
            self.index
                .register_attachment(page, &session)
                .map_err(resource_error)?;
            session
        };
        if self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?
            .session_id
            != session
        {
            return Err("BACKEND_PROTOCOL".into());
        }
        self.setup(id, page, Kind::Page, &session, deadline).await?;
        self.record_activity(page, crate::browser::activity::Event::PageCreated, page);
        self.publish();
        Ok(page)
    }
    pub(super) async fn setup(
        &mut self,
        id: Uuid,
        reference: Reference,
        kind: Kind,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        if self.configured.contains(session) {
            return Ok(());
        }
        if matches!(kind, Kind::Page | Kind::Frame) {
            self.call(id, Some(session), "Page.enable", json!({}), deadline)
                .await?;
            let tree = self
                .call(id, Some(session), "Page.getFrameTree", json!({}), deadline)
                .await?;
            self.index
                .seed_frame_tree(session, &tree)
                .map_err(resource_error)?;
            self.http_state.seed(&self.index, session, &tree);
        }
        self.call(id, Some(session), "Runtime.enable", json!({}), deadline)
            .await?;
        self.setup_network_environment(id, kind, session, deadline)
            .await?;
        if kind == Kind::Page {
            let context = self
                .index
                .list(Kind::Page)
                .into_iter()
                .find(|(page, _)| *page == reference)
                .and_then(|(_, parent)| parent);
            if context.is_some_and(|context| self.touch_contexts.contains(&context)) {
                self.call(
                    id,
                    Some(session),
                    "Emulation.setTouchEmulationEnabled",
                    json!({"enabled":true,"maxTouchPoints":1}),
                    deadline,
                )
                .await?;
            }
            let layout = context
                .and_then(|context| self.context_layout.get(&context))
                .cloned()
                .unwrap_or_default();
            self.call(
                id,
                Some(session),
                "Emulation.setDeviceMetricsOverride",
                Self::device_metrics(
                    &layout,
                    layout.viewport,
                    layout.screen.unwrap_or(layout.viewport),
                ),
                deadline,
            )
            .await?;
        }
        self.setup_emulation(id, reference, kind, session, deadline)
            .await?;
        self.call(
            id,
            Some(session),
            "Target.setAutoAttach",
            json!({"autoAttach":true,"waitForDebuggerOnStart":self.startup.enabled,"flatten":true,"filter":[{"type":"shared_worker","exclude":true},{"type":"service_worker","exclude":true},{}]}),
            deadline,
        )
        .await?;
        if matches!(kind, Kind::Page | Kind::Frame) {
            self.call(id, Some(session), "Page.addScriptToEvaluateOnNewDocument",
                json!({"source":page::INITIALIZATION,"worldName":page::WORLD,"runImmediately":true}), deadline).await?;
            self.call(
                id,
                Some(session),
                "Page.setInterceptFileChooserDialog",
                json!({"enabled":true}),
                deadline,
            )
            .await?;
        }
        self.index
            .attachment_current(reference, kind, session)
            .map_err(resource_error)?;
        self.configure_bindings(id, kind, session, deadline).await?;
        self.configure_fetch(id, session, deadline).await?;
        self.configure_clock(id, kind, session, deadline).await?;
        self.configure_credentials(id, kind, session, deadline)
            .await?;
        self.configure_init_scripts(id, kind, session, deadline)
            .await?;
        self.configured.insert(session.into());
        if let Some((context, _, _)) = self.index.diagnostic_scope(session)
            && (self.diagnostics.history.active(context) || self.har_archives.capturing(context))
        {
            self.configure_diagnostics(id, session, deadline).await?;
        }
        if self.configured.len() > 4096 {
            return Err("RESOURCE_CAPACITY".into());
        }
        Ok(())
    }
    async fn setup_network_environment(
        &mut self,
        id: Uuid,
        kind: Kind,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        if matches!(kind, Kind::Page | Kind::Frame)
            && self
                .index
                .diagnostic_scope(session)
                .is_some_and(|(context, _, _)| {
                    self.certificate_tunnels.active(context)
                        || self
                            .context_layout
                            .get(&context)
                            .is_some_and(|options| options.ignore_https_errors == Some(true))
                })
        {
            // The explicit ignoreHTTPSErrors option, or the SDK's internal
            // browser-facing proxy TLS override; with client certificates the
            // Rust relay independently verifies every configured upstream.
            self.call(
                id,
                Some(session),
                "Security.setIgnoreCertificateErrors",
                json!({"ignore":true}),
                deadline,
            )
            .await?;
        }
        if let Some(headers) = self.headers_for_session(session) {
            self.call(id, Some(session), "Network.enable", json!({}), deadline)
                .await?;
            self.call(
                id,
                Some(session),
                "Network.setExtraHTTPHeaders",
                json!({"headers":headers}),
                deadline,
            )
            .await?;
        }
        Ok(())
    }

    pub(super) async fn reconcile(&mut self, id: Uuid, deadline: Instant) -> Result<(), Error> {
        if !matches!(
            self.shared.state.lock().expect("Chrome lifecycle").phase,
            Phase::Ready | Phase::Starting
        ) {
            return Ok(());
        }
        if self.startup.busy {
            return Ok(());
        }
        if self.startup.failed.is_some() {
            let mut error = Error::from("TARGET_STARTUP_PENDING");
            error.partial = Some(
                json!({"targetStartup":self.startup_summary(),"nextAction":"browser_disconnect"}),
            );
            return Err(error);
        }
        self.startup.busy = true;
        let result = async {
            self.reconcile_targets(id, deadline).await?;
            self.resume_startup(deadline).await
        }
        .await;
        self.startup.busy = false;
        if self.startup.enabled
            && let Err(error) = &result
        {
            self.startup.failed = Some(error.code);
        }
        if self.startup.enabled {
            self.publish();
        }
        result
    }
    async fn reconcile_targets(&mut self, id: Uuid, deadline: Instant) -> Result<(), Error> {
        if !matches!(
            self.shared.state.lock().expect("Chrome lifecycle").phase,
            Phase::Ready | Phase::Starting
        ) {
            return Ok(());
        }
        loop {
            let next = self
                .index
                .targets()
                .into_iter()
                .find(|(reference, kind, _, session)| {
                    (session.is_some()
                        || (*kind == Kind::Page && !self.startup.enabled)
                        || self.index.context_worker(*reference))
                        && session
                            .as_ref()
                            .is_none_or(|session| !self.configured.contains(session))
                });
            let Some((reference, kind, native, session)) = next else {
                return Ok(());
            };
            let session = if let Some(session) = session {
                session
            } else {
                // Dedicated workers/frames use their parent attachment. Shared
                // and service workers belong to the owned context, independent
                // of any one creating or controlled page.
                if kind != Kind::Page && !self.index.context_worker(reference) {
                    return Ok(());
                }
                let value = self
                    .call(
                        id,
                        None,
                        "Target.attachToTarget",
                        json!({"targetId":native,"flatten":true}),
                        deadline,
                    )
                    .await?;
                let session = value["sessionId"]
                    .as_str()
                    .ok_or("BACKEND_PROTOCOL")?
                    .to_owned();
                self.index
                    .register_attachment(reference, &session)
                    .map_err(resource_error)?;
                if self
                    .index
                    .route(reference, Some(kind))
                    .map_err(resource_error)?
                    .session_id
                    != session
                {
                    return Err("BACKEND_PROTOCOL".into());
                }
                session
            };
            if let Err(error) = self.setup(id, reference, kind, &session, deadline).await {
                let current = self.index.attachment_current(reference, kind, &session);
                if matches!(
                    error.code,
                    "INVALID_SCOPE" | "STALE_REFERENCE" | "BROWSER_METHOD_FAILED"
                ) && matches!(
                    current,
                    Err(super::super::ResourceError::Scope | super::super::ResourceError::Stale)
                ) {
                    // A setup read can race a witnessed detach/navigation.
                    // Keep the old references invalid and configure only the
                    // next currently indexed attachment; never replay input.
                    continue;
                }
                return Err(error);
            }
        }
    }
    pub(super) async fn execute(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if let Request::Batch { requests } = request {
            let mut results = Vec::new();
            for request in requests {
                let cancellation = {
                    let state = self.shared.state.lock().expect("Chrome lifecycle");
                    if state.phase != Phase::Ready {
                        Some("SESSION_NOT_READY")
                    } else if state
                        .operations
                        .get(&id)
                        .is_some_and(|operation| operation.cancel_requested)
                    {
                        Some("CANCELLED")
                    } else if Instant::now() >= deadline {
                        Some("NOT_DISPATCHED")
                    } else {
                        None
                    }
                };
                let result = if let Some(code) = cancellation {
                    Err(code.into())
                } else {
                    match self.controller_current(id, true) {
                        Ok(()) => self.execute_one(id, request, deadline).await,
                        Err(error) => Err(error),
                    }
                };
                match result {
                    Ok(result) => {
                        results.push(result);
                        if let Some(operation) = self
                            .shared
                            .state
                            .lock()
                            .expect("Chrome lifecycle")
                            .operations
                            .get_mut(&id)
                        {
                            operation.result =
                                Some(json!({"completedActions":results.len(),"results":results}));
                        }
                        self.shared.changed.notify_waiters();
                    }
                    Err(error) => return Err(error.with_batch_result(results, None)),
                }
            }
            return Ok(json!({"completedActions":results.len(),"results":results}));
        }
        self.execute_one(id, request, deadline).await
    }
    #[expect(
        clippy::too_many_lines,
        reason = "The request router delegates capability families while keeping lifecycle and typed dispatch exhaustive"
    )]
    pub(super) async fn execute_one(
        &mut self,
        id: Uuid,
        mut request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.shared.control.input_current(id)?;
        if request.uses_base() {
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
            super::supported(&request)?;
        }
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
                self.http_operation(id, request, deadline).await
            }
            request @ (Request::WebSocketRouteAdd { .. }
            | Request::WebSocketRouteAddMatcher { .. }
            | Request::WebSocketRouteSend { .. }
            | Request::WebSocketRouteHandlers { .. }
            | Request::WebSocketRouteClose { .. }
            | Request::WebSocketRouteList { .. }
            | Request::WebSocketRouteRemove { .. }) => {
                self.socket_route(id, request, deadline).await
            }
            request @ (Request::BindingAdd { .. }
            | Request::BindingList { .. }
            | Request::BindingRead { .. }
            | Request::BindingRemove { .. }) => self.binding_operation(id, request, deadline).await,
            Request::NetworkRoute { scope, action } => {
                self.network_route(id, scope, action, deadline).await
            }
            Request::PageRouteSet {
                page,
                url,
                authentication_challenges,
            } => {
                self.page_route_set(id, page, url, authentication_challenges, deadline)
                    .await
            }
            Request::ContextRouteSet {
                context,
                url,
                authentication_challenges,
            } => {
                self.scoped_route_set(
                    id,
                    context,
                    Kind::Context,
                    url,
                    authentication_challenges,
                    deadline,
                )
                .await
            }
            Request::DiagnosticsStart { context } => {
                self.diagnostics_subscribe(id, context, true, deadline)
                    .await
            }
            Request::DiagnosticsStop { context } => {
                self.diagnostics_subscribe(id, context, false, deadline)
                    .await
            }
            Request::DiagnosticsRead { context, query } => self.diagnostics_read(context, &query),
            request @ (Request::DiagnosticsArgumentsSet { .. }
            | Request::DiagnosticsArgumentsList { .. }
            | Request::DiagnosticsArgumentsRead { .. }
            | Request::DiagnosticsArgumentsRemove { .. }) => {
                self.diagnostic_arguments_operation(id, request, deadline)
                    .await
            }
            Request::DeveloperOpen { page, panel } => {
                self.developer_open(id, page, panel, deadline).await
            }
            Request::DeveloperStatus { developer } => self.developer_status(developer),
            Request::PageAudit { page, options } => {
                self.page_audit(id, page, options, deadline).await
            }
            Request::ProfileStart { page, options } => {
                self.profile_start(id, page, options, deadline).await
            }
            Request::ProfileStatus { page } => self.profile_status(page),
            Request::ProfileTakeCoverage { page } => {
                self.profile_take_coverage(id, page, deadline).await
            }
            Request::ProfileQuery { evidence_id, query } => self.profile_query(evidence_id, &query),
            request @ (Request::PerformanceLoad { .. }
            | Request::PerformanceQuery { .. }
            | Request::PerformanceRelease { .. }) => {
                self.performance_operation(id, request, deadline).await
            }
            Request::HeapStop { page } => self.heap_stop(page, deadline).await,
            request @ (Request::HeapCapture { .. }
            | Request::HeapImport { .. }
            | Request::HeapLoad { .. }
            | Request::HeapQuery { .. }
            | Request::HeapCompare { .. }
            | Request::HeapRelease { .. }) => self.heap_operation(id, request, deadline).await,
            Request::TraceStart { context, options } => {
                self.trace_start(id, context, options, deadline).await
            }
            Request::TraceStatus {} => Ok(self.trace_status()),
            Request::PageDebugger { page, action } => {
                self.page_debugger(id, page, action, deadline).await
            }
            Request::TraceStop {
                file_path,
                analysis_developer,
                analysis_page,
            } => match analysis_page {
                Some(page) => {
                    self.trace_stop_for_analysis_page(id, file_path, page, deadline)
                        .await
                }
                None => {
                    self.trace_stop_with_analysis(id, file_path, analysis_developer, deadline)
                        .await
                }
            },
            Request::ProfileStop { page } => self.profile_stop(page, deadline).await,
            Request::DeveloperEvidenceList {} => Ok(
                json!({"evidence":self.developers.evidence.list(),"pending":self.developers.evidence.pending_summary(),"capacity":self.developers.evidence.capacity_summary()}),
            ),
            Request::DeveloperEvidenceFinalize { capture_id } => {
                match self.coverage_finalize_capture(capture_id) {
                    Some(result) => result,
                    None => self
                        .developers
                        .evidence
                        .finalize(self.journal.directory(), capture_id),
                }
            }
            Request::DeveloperEvidenceRemove { evidence_id } => {
                let artifact = self.developers.evidence.get(evidence_id)?;
                artifact.remove()?;
                self.developers.evidence.forget(evidence_id);
                Ok(json!({"evidenceId":evidence_id,"removed":true}))
            }
            Request::DeveloperClose { developer } => {
                self.developer_close(developer, deadline).await
            }
            request @ (Request::PageInitScriptAdd { .. }
            | Request::PageInitScriptList { .. }
            | Request::PageInitScriptRemove { .. }
            | Request::ContextInitScriptAdd { .. }
            | Request::ContextInitScriptList { .. }
            | Request::ContextInitScriptRemove { .. }) => {
                self.init_script(id, request, deadline).await
            }
            Request::NetworkRead {
                context,
                request_id,
                options,
            } => {
                self.network_read(id, context, request_id, options, deadline)
                    .await
            }
            Request::PageHarSet {
                page,
                archive_id,
                not_found,
                url_filter,
            } => {
                self.page_har_set(id, page, archive_id, not_found, url_filter, deadline)
                    .await
            }
            Request::ContextHarSet {
                context,
                archive_id,
                not_found,
                url_filter,
            } => {
                self.scoped_har_set(
                    id,
                    context,
                    Kind::Context,
                    archive_id,
                    not_found,
                    url_filter,
                    deadline,
                )
                .await
            }
            Request::HarRegister { path } => self
                .har_archives
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
                    id,
                    context,
                    page,
                    (content, mode, export),
                    replaces_archive_id,
                    url_filter,
                    deadline,
                )
                .await
            }
            Request::HarCaptureStatus { context } => self.har_archives.capture_status(context),
            Request::HarCaptureStop { context } => {
                self.har_capture_stop(id, context, deadline).await
            }
            Request::HarFinalize { archive_id } => self
                .har_archives
                .finalize(self.journal.directory(), archive_id),
            Request::HarRemove { archive_id } => self.har_remove(archive_id),
            Request::NetworkEvidenceList {} => Ok(json!({"evidence":self.network_evidence.list()})),
            Request::NetworkEvidenceRemove { evidence_id } => {
                let artifact = self.network_evidence.get(evidence_id)?;
                crate::browser::network::remove_body(&artifact)?;
                self.network_evidence.forget(evidence_id);
                Ok(json!({"evidenceId":evidence_id,"removed":true}))
            }
            request @ (Request::FrameworkPrepare { .. }
            | Request::ContextFrameworkPrepare { .. }
            | Request::FrameworkOpen { .. }
            | Request::FrameworkQuery { .. }
            | Request::FrameworkRelease { .. }) => {
                self.framework_operation(id, request, deadline).await
            }
            Request::WebMcpDebug { scope, action } => {
                self.webmcp_debug_operation(id, scope, action, deadline)
                    .await
            }
            Request::WebMcp { scope, action } => {
                self.webmcp_page_operation(id, scope, action, deadline)
                    .await
            }
            Request::BrowserExtension { action } => {
                self.extension_operation(id, action, deadline).await
            }
            Request::BrowserPwa { action } => self.pwa_operation(id, action, deadline).await,
            request @ (Request::DeveloperToolsDiscover { .. }
            | Request::DeveloperToolCall { .. }
            | Request::DeveloperToolResultRead { .. }
            | Request::DeveloperToolsRelease { .. }) => {
                self.developer_tools_operation(id, request, deadline).await
            }
            Request::HandleDispose { handle } if self.frameworks.require(handle).is_ok() => {
                Err("FRAMEWORK_RELEASE_REQUIRED".into())
            }
            request @ (Request::Evaluate { .. }
            | Request::SelectorEvaluate { .. }
            | Request::HandleRead { .. }
            | Request::HandleOwnerFrame { .. }
            | Request::HandleContentFrame { .. }
            | Request::FrameElement { .. }
            | Request::FrameRetain { .. }
            | Request::FrameOwnerRelease { .. }
            | Request::HandleProperty { .. }
            | Request::HandleProperties { .. }
            | Request::HandleDispose { .. }
            | Request::WorkerList { .. }
            | Request::ContextWorkerList { .. }) => {
                self.execute_evaluation(id, request, deadline).await
            }
            Request::DiagnosticsWait {
                context,
                after,
                conditions,
            } => loop {
                self.observation_ready(id, deadline)?;
                if let Some(value) = self.probe_diagnostics(context, after, &conditions)? {
                    return Ok(value);
                }
                self.observation_tick(deadline).await?;
            },
            Request::PageResize { page, viewport } => {
                let scope = self.page_scope(id, page, None, deadline).await?;
                let context = self
                    .index
                    .list(Kind::Page)
                    .into_iter()
                    .find(|(reference, _)| *reference == page)
                    .and_then(|(_, parent)| parent)
                    .ok_or("STALE_REFERENCE")?;
                let layout = self
                    .context_layout
                    .get(&context)
                    .cloned()
                    .unwrap_or_default();
                self.inputs.invalidate(page);
                // Pinned setViewportSize: the emulated screen follows the viewport.
                self.call(
                    id,
                    Some(&scope.session),
                    "Emulation.setDeviceMetricsOverride",
                    Self::device_metrics(&layout, viewport, viewport),
                    deadline,
                )
                .await?;
                if let Some(connection) = &self.connection {
                    connection.frame_lane().fence(&scope.session);
                }
                Ok(json!({"page":page,"viewport":viewport}))
            }
            request @ (Request::PageScreenshot { .. }
            | Request::PageSnapshot { .. }
            | Request::PageAnnotate { .. }
            | Request::PagePdf { .. }
            | Request::PreviewStart { .. }
            | Request::PreviewStatus { .. }
            | Request::PreviewStop { .. }
            | Request::RecordingStart { .. }
            | Request::RecordingStatus { .. }
            | Request::RecordingStop { .. }
            | Request::CaptureCompare { .. }) => self.execute_evidence(id, request, deadline).await,
            request @ (Request::AuthBrokerList { .. }
            | Request::AuthSignIn { .. }
            | Request::AuthStatus { .. }) => {
                Box::pin(self.execute_authentication(id, request, deadline)).await
            }
            request @ (Request::ContextCreate { .. }
            | Request::ContextClock { .. }
            | Request::ContextCredentials { .. }
            | Request::ContextConfigure { .. }
            | Request::PageEmulateMedia { .. }
            | Request::PageEmulateThrottling { .. }
            | Request::StorageRegister { .. }
            | Request::StorageExport { .. }
            | Request::StorageApply { .. }
            | Request::StorageForget { .. }
            | Request::ContextCookiesRead { .. }
            | Request::ContextCookiesAdd { .. }
            | Request::ContextCookiesClear { .. }
            | Request::PageWebStorageRead { .. }
            | Request::PageWebStorageUpdate { .. }
            | Request::ContextCacheStorageRead { .. }
            | Request::ContextCacheStorageDelete { .. }
            | Request::PageSetExtraHttpHeaders { .. }
            | Request::PageRequestGc { .. }
            | Request::ContextList {}
            | Request::ContextSelect { .. }
            | Request::ContextClose { .. }
            | Request::PageCreate { .. }
            | Request::PageList {}
            | Request::PageSelect { .. }
            | Request::PageClose { .. }) => self.execute_lifecycle(id, request, deadline).await,
            request @ Request::PageRead { .. } => self.page_read(id, request, deadline).await,
            request @ Request::PageAction { .. } => self.page_action(id, request, deadline).await,
            Request::PageInput {
                page,
                observation,
                actions,
            } => {
                self.page_input(id, page, observation, actions, deadline)
                    .await
            }
            Request::InputRelease {} => self.release_inputs().await,
            Request::ChooserResolve { chooser, fixtures } => {
                self.resolve_chooser(id, chooser, fixtures, deadline).await
            }
            Request::DownloadList {} => Ok(
                json!({"items":self.transfers.downloads.values().map(|download|download.summary.clone()).collect::<Vec<_>>()}),
            ),
            Request::DownloadStatus { download } => self.download_status(download),
            Request::DownloadCancel { download } => self.cancel_download(download, deadline).await,
            Request::DownloadDelete { download } => self.delete_download(download).await,
            Request::FixtureRegister { fixture } => self.fixture_register(fixture).await,
            Request::DirectoryRegister { name, entries } => {
                self.directory_register(name, entries).await
            }
            Request::FixtureList {} => Ok(
                json!({"items":self.transfers.fixtures.values().map(|fixture|fixture.summary.clone()).chain(self.transfers.directories.values().map(|directory|directory.summary.clone())).collect::<Vec<_>>()}),
            ),
            Request::FixtureVerify { fixture } => {
                if let Some(directory) = self.transfers.directories.get(&fixture).cloned() {
                    for source in &directory.sources {
                        self.fixture_bytes(*source).await?;
                    }
                    crate::browser::directory::verify(directory.clone()).await?;
                    return Ok(json!({"fixture":directory.summary,"verified":true}));
                }
                let (summary, _) = self.fixture_bytes(fixture).await?;
                Ok(json!({"fixture":summary,"verified":true}))
            }
            Request::PageUpload {
                page,
                frame,
                locator,
                fixtures,
            } => {
                self.page_upload(id, page, frame, locator, fixtures, deadline)
                    .await
            }
            request @ Request::PageWait { .. } => self.page_wait(id, request, deadline).await,
            Request::CallbackResolve { callback, response } => {
                self.resolve_callback(id, callback, response, deadline)
                    .await
            }
            Request::EventsRead {
                context,
                cursor,
                limit,
            } => self.events_read(context, cursor.as_deref(), limit),
            Request::EventsWait {
                context,
                after,
                conditions,
            } => {
                self.events_wait(id, context, after, &conditions, deadline)
                    .await
            }
            Request::PageNavigate { page, navigation } => {
                self.navigate(id, page, navigation, deadline).await
            }
            Request::PageReadUrl {
                page,
                url,
                max_bytes,
            } => self.read_url(id, page, url, max_bytes, deadline).await,
            Request::PageClipboard {
                page,
                origin,
                action,
            } => {
                self.page_clipboard(id, page, origin, action, deadline)
                    .await
            }
            request @ Request::PageCss { .. } => self.page_css(id, request, deadline).await,
            request @ Request::PageStyles { .. } => self.page_styles(id, request, deadline).await,
            Request::FrameEnter {
                page,
                frame,
                locator,
            } => self.frame_enter(id, page, frame, locator, deadline).await,
            _ => Err("CAPABILITY_UNAVAILABLE".into()),
        }
    }
    async fn close_context(
        &mut self,
        id: Uuid,
        context: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.persistent_context == Some(context) {
            return Err("DISCONNECT_PROFILE_CONTEXT".into());
        }
        if self
            .adoption
            .as_ref()
            .is_some_and(|(borrowed, _)| *borrowed == context)
        {
            return Err("ATTACHMENT_BORROWED_CONTEXT".into());
        }
        if self.closed.get(&context) == Some(&Kind::Context) {
            self.certificate_tunnels
                .close(Some(context), deadline)
                .await?;
            return Ok(json!({"closed":true}));
        }
        if self.closed.len() >= 4096 {
            return Err("RESOURCE_CAPACITY".into());
        }
        let native = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        let pages: Vec<_> = self
            .index
            .list(Kind::Page)
            .into_iter()
            .filter(|(_, parent)| *parent == Some(context))
            .map(|(reference, _)| reference)
            .collect();
        if self.closed.len() + pages.len() + 1 > 4096 {
            return Err("RESOURCE_CAPACITY".into());
        }
        let mut sessions = self
            .index
            .context_sessions(context)
            .map_err(resource_error)?;
        if let Some(session) = self.http_state.retirement_session(&native) {
            sessions.push(session);
        }
        let retirement = self
            .connection
            .as_ref()
            .ok_or("BACKEND_LOST")?
            .reserve_retirement(sessions)
            .map_err(protocol_error)?;
        self.index.mark_closing(context).map_err(resource_error)?;
        self.call(
            id,
            None,
            "Target.disposeBrowserContext",
            json!({"browserContextId":native}),
            deadline,
        )
        .await?;
        self.index.retire(context).map_err(resource_error)?;
        self.fence_clocks();
        self.prune_routes();
        self.page_headers
            .retain(|page, _| self.index.native(*page, Kind::Page).is_ok());
        self.diagnostics.stop(context);
        self.websockets.close_context(context);
        self.http_contexts.context_defaults_closed(context);
        for page in pages {
            self.remember_close(page, Kind::Page)?;
        }
        self.remember_close(context, Kind::Context)?;
        retirement.commit_after_close();
        self.context_layout.remove(&context);
        self.emulation.prune(&self.index);
        self.certificate_tunnels.close(Some(context), deadline).await.map_err(|mut error| {
            error.partial = Some(json!({"context":context,"closed":true,"clientCertificateTunnelCleanupPending":true})); error
        })?;
        Ok(json!({"closed":true}))
    }

    async fn close_page(
        &mut self,
        id: Uuid,
        page: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.closed.get(&page) == Some(&Kind::Page) {
            return Ok(json!({"closed":true}));
        }
        if self.closed.len() >= 4096 {
            return Err("RESOURCE_CAPACITY".into());
        }
        let native = self
            .index
            .native(page, Kind::Page)
            .map_err(resource_error)?;
        let context = self
            .index
            .retained_page_context(page)
            .ok_or("INVALID_SCOPE")?;
        let sessions = self.index.page_sessions(page).map_err(resource_error)?;
        let retirement = self
            .connection
            .as_ref()
            .ok_or("BACKEND_LOST")?
            .reserve_retirement(sessions)
            .map_err(protocol_error)?;
        self.index.mark_closing(page).map_err(resource_error)?;
        let result = self
            .call(
                id,
                None,
                "Target.closeTarget",
                json!({"targetId":native}),
                deadline,
            )
            .await?;
        if result["success"] != true {
            return Err("BROWSER_METHOD_FAILED".into());
        }
        // Retain the acknowledged close before retiring its context association.
        // If targetDestroyed arrived first, History deduplicates that witness.
        self.interactions.history.push(
            context,
            page,
            crate::browser::activity::Event::PageClosed,
            page,
        );
        // A targetDestroyed event may already have retired it.
        if self.index.target_reference(&native).is_some() {
            self.index.retire(page).map_err(resource_error)?;
            self.fence_clocks();
            self.prune_routes();
        }
        self.page_headers.remove(&page);
        self.emulation.prune(&self.index);
        self.remember_close(page, Kind::Page)?;
        retirement.commit_after_close();
        Ok(json!({"closed":true}))
    }
}
