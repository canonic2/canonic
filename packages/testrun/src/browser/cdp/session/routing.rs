//! Paused request ownership on the independent callback lane.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use contract::CallbackResponse;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RouteOwner {
    Page(Reference),
    ServiceWorker(Reference),
}
impl RouteOwner {
    pub(super) fn reference(self) -> Reference {
        match self {
            Self::Page(value) | Self::ServiceWorker(value) => value,
        }
    }
    pub(super) fn kind(self) -> Kind {
        match self {
            Self::Page(_) => Kind::Page,
            Self::ServiceWorker(_) => Kind::Worker,
        }
    }
    pub(super) fn page(self) -> Option<Reference> {
        match self {
            Self::Page(value) => Some(value),
            Self::ServiceWorker(_) => None,
        }
    }
    pub(super) fn worker(self) -> Option<Reference> {
        match self {
            Self::ServiceWorker(value) => Some(value),
            Self::Page(_) => None,
        }
    }
}
pub(super) struct Route {
    pub(super) owner: RouteOwner,
    pub(super) policy_scope: Reference,
    fallback_from: Option<Reference>,
    match_url: Option<String>,
    session: String,
    native: String,
    network_id: Option<String>,
    request_input_id: Option<Uuid>,
    pub(super) expires: Instant,
    pub(super) retry_at: Instant,
    pub(super) responding: bool,
    pub(super) auto_pass: bool,
    pub(super) last_failure: Option<&'static str>,
    /// Paused while interception was turned off; Chrome may already have released it.
    pub(super) released: bool,
    summary: Value,
    authentication: bool,
    automatic_auth: Option<Value>,
    har_request: Option<crate::browser::har::Request>,
    overrides: crate::browser::routing::Overrides,
    navigation: bool,
}
impl Worker {
    pub(super) fn route_scope(&self, session: &str) -> Option<(Reference, RouteOwner)> {
        let (context, page, worker) = self.index.diagnostic_scope(session)?;
        let owner = if let Some(worker) = worker {
            // Dedicated-worker requests pause in their page's interception, so
            // worker sessions never own routes; shared workers get none at all.
            self.index
                .worker_scope(worker)
                .ok()
                .filter(|(_, page, kind)| {
                    page.is_none() && *kind == crate::browser::cdp::resources::WorkerKind::Service
                })?;
            RouteOwner::ServiceWorker(worker)
        } else {
            RouteOwner::Page(page?)
        };
        Some((context, owner))
    }
    pub(super) fn route_context(&self, route: &Route) -> Option<Reference> {
        self.index
            .route(route.owner.reference(), Some(route.owner.kind()))
            .ok()?;
        self.route_scope(&route.session)
            .filter(|(_, owner)| *owner == route.owner)
            .map(|(context, _)| context)
    }
    /// `clear` with `behavior: "wait"` retries until its deadline: requests the
    /// scope privately continues can pause between its wait and its teardown.
    pub(super) async fn network_route(
        &mut self,
        id: Uuid,
        scope: Reference,
        action: crate::browser::routing::HandlerAction,
        deadline: Instant,
    ) -> Result<Value, Error> {
        loop {
            match Box::pin(self.network_route_once(id, scope, action.clone(), deadline)).await {
                Err(error)
                    if action.waits()
                        && error.code == "ROUTE_CALLBACKS_PENDING"
                        && Instant::now() < deadline =>
                {
                    self.observation_tick(deadline.min(Instant::now() + Duration::from_millis(25)))
                        .await?;
                }
                result => return result,
            }
        }
    }
    async fn network_route_once(
        &mut self,
        id: Uuid,
        scope: Reference,
        action: crate::browser::routing::HandlerAction,
        deadline: Instant,
    ) -> Result<Value, Error> {
        use crate::browser::routing::HandlerAction;
        action.validate()?;
        let matcher = action.matcher();
        let times = action.times();
        let kind = if self.index.native(scope, Kind::Page).is_ok() {
            Kind::Page
        } else {
            Kind::Context
        };
        self.index.native(scope, kind).map_err(resource_error)?;
        self.prune_har_routes();
        // `clear` with `behavior: "wait"` serves callback replies while it waits.
        while action.waits()
            && self
                .interactions
                .routes
                .values()
                .any(|route| route.uses_policy(scope))
        {
            if Instant::now() >= deadline {
                return Err("ROUTE_CALLBACKS_PENDING".into());
            }
            self.observation_tick(deadline.min(Instant::now() + Duration::from_millis(25)))
                .await?;
            self.prune_har_routes();
        }
        let paused = |session: &Self| -> Vec<Reference> {
            session
                .interactions
                .routes
                .iter()
                .filter(|(_, route)| route.uses_policy(scope))
                .map(|(reference, _)| *reference)
                .collect()
        };
        // Pinned unroute/unrouteAll without waiting. Removing the last handler
        // ends interception: paused requests continue to the network now and a
        // late reply refuses like any answered callback. With handlers left,
        // paused requests stay paused and skip the removed ones.
        if action.releases() && !paused(self).is_empty() {
            let ids = self.routing_rules.handlers.release_ids(scope, &action)?;
            let last = matches!(action, HandlerAction::Clear { .. })
                || self.routing_rules.handlers.len(scope) == ids.len();
            if !last {
                self.routing_rules.handlers.exhaust(scope, &ids);
                return Ok(
                    json!({"scope":scope,"handlers":self.routing_rules.handlers.list(scope)}),
                );
            }
            for callback in paused(self) {
                Box::pin(self.resolve_route(
                    callback,
                    CallbackResponse::RouteContinue {},
                    deadline,
                ))
                .await?;
            }
        }
        if !matches!(action, HandlerAction::List {}) && !paused(self).is_empty() {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        match action {
            HandlerAction::List {} => {}
            HandlerAction::Add { .. }
            | HandlerAction::AddMatcher { .. }
            | HandlerAction::AddHar { .. } => {
                let har = action.har();
                if let Some((archive_id, _)) = har {
                    self.har_archives.get(archive_id)?;
                }
                return self
                    .add_route_handler(
                        id,
                        scope,
                        kind,
                        (matcher.expect("Validated matcher"), har),
                        times,
                        deadline,
                    )
                    .await;
            }
            action @ (HandlerAction::Remove { .. } | HandlerAction::RemoveMatcher { .. }) => {
                self.remove_route_handlers(id, scope, kind, &action, deadline)
                    .await?;
            }
            HandlerAction::Clear { .. } => {
                self.remove_har_policy(id, scope, kind, false, deadline)
                    .await?;
                if self.routing_rules.manual(scope) || self.routing_rules.handlers.len(scope) > 0 {
                    self.scoped_route_set(id, scope, kind, None, false, deadline)
                        .await?;
                }
                self.routing_rules.handlers.clear(scope);
            }
        }
        Ok(json!({"scope":scope,"handlers":self.routing_rules.handlers.list(scope)}))
    }
    async fn remove_route_handlers(
        &mut self,
        id: Uuid,
        scope: Reference,
        kind: Kind,
        action: &crate::browser::routing::HandlerAction,
        deadline: Instant,
    ) -> Result<(), Error> {
        let ids = self.routing_rules.handlers.removal_ids(scope, action)?;
        if ids.is_empty() {
            return Ok(());
        }
        if self.routing_rules.handlers.len(scope) == ids.len() {
            self.scoped_route_set(id, scope, kind, None, false, deadline)
                .await?;
        } else if self.routing_rules.handlers.pending(scope) {
            return Err("ROUTE_POLICY_PENDING".into());
        } else if self.routing_rules.handlers.plan_remove_ids(scope, &ids) {
            let primary = self
                .routing_rules
                .handlers
                .url(scope)
                .expect("Owned handler")
                .to_owned();
            self.scoped_route_set(id, scope, kind, Some(primary), false, deadline)
                .await?;
        }
        for id in ids {
            self.routing_rules.handlers.remove(scope, id);
        }
        self.routing_rules.handlers.activate(scope);
        Ok(())
    }

    async fn add_route_handler(
        &mut self,
        id: Uuid,
        scope: Reference,
        kind: Kind,
        (matcher, har): (
            crate::browser::routing::Matcher,
            Option<(Uuid, contract::HarNotFound)>,
        ),
        times: Option<u32>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.routing_rules.handlers.capacity(scope)?;
        let first = self.routing_rules.handlers.len(scope) == 0;
        let update = first
            || !self
                .routing_rules
                .handlers
                .contains_matcher(scope, &matcher);
        if first {
            if self.routing_rules.manual(scope) {
                return Err("MANUAL_ROUTE_ACTIVE".into());
            }
            if self.routing_rules.automatic(scope).is_some() {
                return Err("HAR_ROUTE_ACTIVE".into());
            }
        }
        let handler = if first {
            self.routing_rules
                .handlers
                .begin_limited(scope, matcher, times)?
        } else {
            self.routing_rules
                .handlers
                .add_limited(scope, matcher, times)?
        };
        if let Some(har) = har {
            self.routing_rules.handlers.set_har(scope, handler, har);
        }
        if update {
            self.routing_rules.handlers.setup_failed(scope);
            let primary = self
                .routing_rules
                .handlers
                .url(scope)
                .expect("Owned handler")
                .to_owned();
            if let Err(mut error) = self
                .scoped_route_set(id, scope, kind, Some(primary), false, deadline)
                .await
            {
                error.partial = Some(
                    json!({"scope":scope,"handlerId":handler,"setupPending":true,"cause":error.partial}),
                );
                return Err(error);
            }
            self.routing_rules.handlers.activate(scope);
        }
        Ok(
            json!({"scope":scope,"handlerId":handler,"handlers":self.routing_rules.handlers.list(scope)}),
        )
    }
    pub(super) async fn cleanup_exhausted_handlers(
        &mut self,
        scopes: Vec<Reference>,
        deadline: Instant,
    ) -> Vec<Value> {
        let mut failures = Vec::new();
        for scope in scopes.into_iter().collect::<BTreeSet<_>>() {
            if !self.index.retained(scope, Kind::Page) && !self.index.retained(scope, Kind::Context)
            {
                continue;
            }
            let ids = self.routing_rules.handlers.exhausted(scope);
            if ids.is_empty() {
                continue;
            }
            if self
                .interactions
                .routes
                .values()
                .any(|route| route.uses_policy(scope))
            {
                continue;
            }
            for handler_id in ids {
                if self.routing_rules.handlers.pending(scope) {
                    failures.push(json!({"scope":scope,"code":"ROUTE_POLICY_PENDING","nextAction":{"operation":"network_route","scope":scope,"action":{"kind":"clear"}},"replayed":false}));
                    break;
                }
                if let Err(error) = self
                    .network_route(
                        Uuid::new_v4(),
                        scope,
                        crate::browser::routing::HandlerAction::Remove { handler_id },
                        deadline,
                    )
                    .await
                {
                    failures.push(json!({"scope":scope,"code":error.code,"nextAction":{"operation":"network_route","scope":scope,"action":{"kind":"clear"}},"replayed":false}));
                    break;
                }
            }
        }
        failures
    }
    pub(super) async fn page_route_set(
        &mut self,
        id: Uuid,
        page: Reference,
        url: Option<String>,
        authentication: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.scoped_route_set(id, page, Kind::Page, url, authentication, deadline)
            .await
    }
    pub(super) async fn scoped_route_set(
        &mut self,
        id: Uuid,
        scope: Reference,
        kind: Kind,
        url: Option<String>,
        authentication: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if authentication && url.is_none() {
            return Err("AUTH_ROUTE_REQUIRED".into());
        }
        if let Some(url) = &url {
            if !self.routing_rules.handlers.pending(scope) {
                crate::browser::routing::validate_url(url)?;
            }
            if self.routing_rules.handlers.pending(scope)
                && self.routing_rules.handlers.url(scope) != Some(url.as_str())
            {
                return Err("ROUTE_POLICY_PENDING".into());
            }
        }
        self.index.native(scope, kind).map_err(resource_error)?;
        self.prune_har_routes();
        if self.routing_rules.automatic(scope).is_some() {
            return Err("HAR_ROUTE_ACTIVE".into());
        }
        if self
            .interactions
            .routes
            .values()
            .any(|route| route.uses_policy(scope))
        {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        let enabled = url.is_some();
        let configuration = url.map(|url| {
            if self.routing_rules.handlers.pending(scope) {
                fetch::Configuration::manual_patterns(
                    self.routing_rules
                        .handlers
                        .planned_patterns(scope)
                        .iter()
                        .flat_map(crate::browser::routing::Matcher::native_patterns)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    authentication,
                )
            } else {
                fetch::Configuration::manual(url, authentication)
            }
        });
        self.routing_rules.mark_manual(scope, true);
        if let Err(error) = self
            .set_scoped_fetch(id, scope, kind, configuration, deadline)
            .await
        {
            self.routing_rules.handlers.setup_failed(scope);
            return Err(error);
        }
        self.index.native(scope, kind).map_err(resource_error)?;
        self.routing_rules.mark_manual(scope, enabled);
        if !enabled || !self.routing_rules.handlers.pending(scope) {
            self.routing_rules.handlers.clear(scope);
        }
        Ok(
            json!({"page":(kind==Kind::Page).then_some(scope),"context":(kind==Kind::Context).then_some(scope),"enabled":enabled,"authenticationChallenges":authentication,"scope":if kind==Kind::Context {"context-pages-child-frames-and-service-workers"}else{"page-and-child-frames"},"serviceWorkers":kind==Kind::Context,"serviceWorkerScope":"context-manual-routes","serviceWorkerHar":false,"serviceWorkerAuthentication":false,"crossProcessFrames":true}),
        )
    }
    pub(super) fn route_event(&mut self, event: &Event) -> Result<(), Error> {
        self.authentication_finished(event);
        if !matches!(
            event.method.as_str(),
            "Fetch.requestPaused" | "Fetch.authRequired"
        ) {
            return Ok(());
        }
        let Some(session) = event.session_id.as_deref() else {
            return Err("BACKEND_PROTOCOL".into());
        };
        let Some((context, route_owner)) = self.route_scope(session) else {
            // Closing or foreign attachments cannot create callback authority.
            // Their target owner retains responsibility for native release.
            return Ok(());
        };
        let Some((native, authentication)) = self.route_callback_identity(event, session)? else {
            return Ok(());
        };
        let page = route_owner.page();
        let reference = Reference {
            connection_id: route_owner.reference().connection_id,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let request = &event.params["request"];
        if self.logical_route_filter(context, page) && request["url"].as_str().is_none() {
            return Err("ROUTE_REQUEST_UNAVAILABLE".into());
        }
        let automatic = self.automatic_authentication(context);
        if automatic && !authentication {
            self.authentication_request(session, &native, event.params["networkId"].as_str())?;
        }
        let owner = self.fetch_owner(
            context,
            page,
            request["url"].as_str().unwrap_or(""),
            authentication,
        );
        let policy_scope = owner.unwrap_or(page.unwrap_or(context));
        let automatic_auth = if authentication && owner.is_none() && automatic {
            Some(self.authentication_response(
                context,
                session,
                &native,
                request["url"].as_str().ok_or("ROUTE_REQUEST_UNAVAILABLE")?,
            )?)
        } else {
            None
        };
        self.routing_rules
            .handlers
            .retain_callbacks(|callback| self.interactions.routes.contains_key(&callback));
        if !authentication {
            self.routing_rules.handlers.admit(
                reference,
                policy_scope,
                request["url"].as_str().unwrap_or(""),
            );
        }
        let mut summary = json!({"url":crate::browser::diagnostics::url(request["url"].as_str().unwrap_or("")),"method":crate::browser::diagnostics::text(request["method"].as_str().unwrap_or("")).0,"headersIncluded":false,"bodyIncluded":false,"handlerId":self.routing_rules.handlers.current(reference),"phase":if authentication{"authentication"}else{"request"}});
        if authentication {
            let challenge = &event.params["authChallenge"];
            summary["challenge"] = json!({"origin":crate::browser::diagnostics::url(challenge["origin"].as_str().unwrap_or("")),"scheme":crate::browser::diagnostics::text(challenge["scheme"].as_str().unwrap_or("")).0,"source":challenge["source"],"credentialsIncluded":false});
        }
        self.interactions.routes.insert(
            reference,
            Route {
                owner: route_owner,
                policy_scope,
                fallback_from: None,
                overrides: Default::default(),
                match_url: request["url"]
                    .as_str()
                    .filter(|url| url.len() <= 8192)
                    .map(str::to_owned),
                session: session.into(),
                network_id: event.params["networkId"].as_str().map(str::to_owned),
                request_input_id: event.params["networkId"].as_str().and_then(|network| {
                    self.diagnostics
                        .take_request_input_id(context, session, network)
                }),
                native,
                expires: Instant::now() + Duration::from_secs(30),
                retry_at: Instant::now(),
                responding: false,
                last_failure: None,
                // Logical nonmatches continue privately, and so do requests
                // paused while this attachment's interception was turned off:
                // no handler remains to answer them.
                auto_pass: owner.is_none()
                    && (automatic
                        || self.logical_route_filter(context, page)
                        || self.fetch_released(session)),
                released: owner.is_none() && self.fetch_released(session),
                summary,
                authentication,
                automatic_auth,
                har_request: har_request(request).ok(),
                navigation: event.params["resourceType"] == "Document",
            },
        );
        Ok(())
    }
    fn route_callback_identity(
        &self,
        event: &Event,
        session: &str,
    ) -> Result<Option<(String, bool)>, Error> {
        if self.interactions.routes.len() + self.interactions.dialogs.len() + self.binding_count()
            >= 64
        {
            return Err("CALLBACK_CAPACITY".into());
        }
        let native = event.params["requestId"]
            .as_str()
            .ok_or("BACKEND_PROTOCOL")?
            .to_owned();
        let authentication = event.method == "Fetch.authRequired";
        if self.interactions.routes.values().any(|r| {
            r.session == session && r.native == native && r.authentication == authentication
        }) {
            return Ok(None);
        }
        Ok(Some((native, authentication)))
    }

    fn route_response_overrides(
        &self,
        reference: Reference,
        response: &CallbackResponse,
    ) -> Result<crate::browser::routing::Overrides, Error> {
        if reference.connection_id != self.shared.state.lock().expect("Chrome state").connection {
            return Err("FOREIGN_REFERENCE".into());
        }
        let route = self
            .interactions
            .routes
            .get(&reference)
            .ok_or("STALE_REFERENCE")?;
        if route.responding {
            return Err("CALLBACK_ALREADY_RESOLVING".into());
        }
        if route.expires <= Instant::now() {
            return Err("CALLBACK_EXPIRED".into());
        }
        self.route_context(route).ok_or("STALE_REFERENCE")?;
        let auth_response = matches!(
            response,
            CallbackResponse::AuthCancel {}
                | CallbackResponse::AuthDefault {}
                | CallbackResponse::AuthCredentials { .. }
        );
        if auth_response != route.authentication {
            return Err("CALLBACK_RESPONSE_MISMATCH".into());
        }
        let overrides = match response {
            CallbackResponse::RouteContinueWith { overrides }
            | CallbackResponse::RouteFallbackWith { overrides } => {
                route.overrides.overlay(overrides)
            }
            _ => route.overrides.clone(),
        };
        Ok(overrides)
    }
    fn fallback_route_callback(
        &mut self,
        reference: Reference,
        overrides: &crate::browser::routing::Overrides,
    ) -> Result<Option<Value>, Error> {
        let route = self
            .interactions
            .routes
            .get(&reference)
            .ok_or("STALE_REFERENCE")?;
        if !overrides.is_empty() {
            let request = route
                .har_request
                .as_ref()
                .ok_or("ROUTE_REQUEST_UNAVAILABLE")?;
            overrides.params(&request.url, &request.headers)?;
        }
        let same_scope = self.routing_rules.handlers.has_next(reference);
        let target = if same_scope {
            Some(route.policy_scope)
        } else if let Some(page) = route.owner.page() {
            self.context_fetch_fallback(page, route.policy_scope, route.match_url.as_deref())?
        } else {
            None
        };
        if let Some(scope) = target {
            let mut next = self
                .interactions
                .routes
                .remove(&reference)
                .ok_or("STALE_REFERENCE")?;
            if !same_scope {
                next.fallback_from = Some(next.policy_scope);
            }
            next.policy_scope = scope;
            next.overrides = overrides.clone();
            if let Some(url) = &next.overrides.url {
                next.summary["url"] = json!(crate::browser::diagnostics::url(url));
            }
            if let Some(method) = &next.overrides.method {
                next.summary["method"] = json!(method);
            }
            let next_reference = Reference {
                connection_id: reference.connection_id,
                id: Uuid::new_v4(),
                generation: Uuid::new_v4(),
            };
            if same_scope {
                self.routing_rules
                    .handlers
                    .advance(reference, next_reference);
            } else {
                self.routing_rules.handlers.forget(reference);
                self.routing_rules.handlers.admit(
                    next_reference,
                    scope,
                    next.match_url.as_deref().unwrap_or(""),
                );
            }
            next.summary["handlerId"] = json!(self.routing_rules.handlers.current(next_reference));
            self.interactions.routes.insert(next_reference, next);
            self.publish();
            return Ok(Some(
                json!({"resolved":true,"fallback":true,"nextCallback":next_reference}),
            ));
        }
        Ok(None)
    }
    fn route_callback_parameters(
        &self,
        route: &Route,
        response: CallbackResponse,
    ) -> Result<(&'static str, Value, Option<Value>), Error> {
        if matches!(response, CallbackResponse::AuthDefault {})
            && route.auto_pass
            && let Some(response) = &route.automatic_auth
        {
            return Ok((
                "Fetch.continueWithAuth",
                json!({"requestId":route.native,"authChallengeResponse":if self.fetch.authentication_lost {json!({"response":"CancelAuth"})}else{response.clone()}}),
                None,
            ));
        }
        let (method, params) = match response {
            CallbackResponse::AuthCancel {} => (
                "Fetch.continueWithAuth",
                json!({"requestId":route.native,"authChallengeResponse":{"response":"CancelAuth"}}),
            ),
            CallbackResponse::AuthDefault {} => (
                "Fetch.continueWithAuth",
                json!({"requestId":route.native,"authChallengeResponse":{"response":"Default"}}),
            ),
            CallbackResponse::AuthCredentials { username, password } => {
                crate::browser::routing::credentials(&username, &password)?;
                (
                    "Fetch.continueWithAuth",
                    json!({"requestId":route.native,"authChallengeResponse":{"response":"ProvideCredentials","username":username,"password":password}}),
                )
            }
            CallbackResponse::RouteAbort { error_code } => (
                "Fetch.failRequest",
                json!({"requestId":route.native,"errorReason":error_code.unwrap_or_default().chrome_reason()}),
            ),
            CallbackResponse::RouteContinue {}
            | CallbackResponse::RouteFallback {}
            | CallbackResponse::RouteFallbackWith { .. } => {
                ("Fetch.continueRequest", json!({"requestId":route.native}))
            }
            CallbackResponse::RouteContinueWith { .. } => {
                ("Fetch.continueRequest", json!({"requestId":route.native}))
            }
            CallbackResponse::RouteFulfill {
                status,
                body,
                json,
                content_type,
                headers,
                file,
                ..
            } => {
                let (status, body_base64, headers) = crate::browser::routing::text_fulfill(
                    status,
                    body.as_deref(),
                    json.as_ref(),
                    content_type.as_deref(),
                    &headers,
                    file.as_ref()
                        .map(crate::browser::routing::FulfillFile::parts),
                )?;
                let method = route.summary["method"].as_str().ok_or("BACKEND_PROTOCOL")?;
                // A caller file may use the full binary bound; inline text stays 64 KiB.
                let prepared = if file.is_some() {
                    crate::browser::routing::binary_response(status, &body_base64, &headers)?
                } else {
                    crate::browser::routing::inline_response(status, &body_base64, &headers)?
                };
                let headers = prepared.headers_for(method)?;
                (
                    "Fetch.fulfillRequest",
                    json!({"requestId":route.native,"responseCode":status,"body":if method=="HEAD"{""}else{&body_base64},"responseHeaders":headers}),
                )
            }
            CallbackResponse::RouteFulfillBinary {
                status,
                body_base64,
                headers,
            } => {
                let method = route.summary["method"].as_str().ok_or("BACKEND_PROTOCOL")?;
                let headers =
                    crate::browser::routing::inline_response(status, &body_base64, &headers)?
                        .headers_for(method)?;
                (
                    "Fetch.fulfillRequest",
                    json!({"requestId":route.native,"responseCode":status,"body":if method=="HEAD"{""}else{&body_base64},"responseHeaders":headers}),
                )
            }
            CallbackResponse::RouteFulfillEvidence {
                status,
                network_evidence_id,
                headers,
            } => self.route_evidence_fulfill(route, status, network_evidence_id, headers)?,
            CallbackResponse::RouteHar {
                archive_id,
                not_found,
            } => return self.har_callback_parameters(route, archive_id, not_found),
            _ => return Err("CALLBACK_RESPONSE_MISMATCH".into()),
        };
        Ok((method, params, None))
    }
    fn route_evidence_fulfill(
        &self,
        route: &Route,
        status: u16,
        network_evidence_id: Uuid,
        headers: Vec<crate::browser::routing::Header>,
    ) -> Result<(&'static str, Value), Error> {
        let artifact = self.network_evidence.get(network_evidence_id)?;
        let body = STANDARD.encode(crate::browser::network::evidence_body(&artifact)?);
        let method = route.summary["method"].as_str().ok_or("BACKEND_PROTOCOL")?;
        let headers = crate::browser::routing::binary_response(status, &body, &headers)?
            .headers_for(method)?;
        Ok((
            "Fetch.fulfillRequest",
            json!({"requestId":route.native,"responseCode":status,"body":if method=="HEAD"{""}else{&body},"responseHeaders":headers}),
        ))
    }

    fn har_callback_parameters(
        &self,
        route: &Route,
        archive_id: Uuid,
        not_found: contract::HarNotFound,
    ) -> Result<(&'static str, Value, Option<Value>), Error> {
        let har_summary;
        let (method, params) = {
            let request = route.effective_request()?;
            if let Some(reply) = self.har_archives.reply(archive_id, &request)? {
                har_summary = Some(
                    json!({"archiveId":archive_id,"matched":true,"redirectsFollowed":reply.redirects}),
                );
                if route.navigation && reply.url != request.url {
                    if !self
                        .routing_rules
                        .automatic(route.policy_scope)
                        .is_some_and(|rule| {
                            rule.archive_id == archive_id
                                && rule.phase == crate::browser::routing::HarPhase::Active
                        })
                    {
                        return Err("HAR_NAVIGATION_REDIRECT_REQUIRED".into());
                    }
                    (
                        "Fetch.fulfillRequest",
                        json!({"requestId":route.native,"responseCode":303,"body":"","responseHeaders":[{"name":"location","value":reply.url},{"name":"content-length","value":"0"}]}),
                    )
                } else {
                    let headers = crate::browser::routing::binary_response(
                        reply.status,
                        &reply.body_base64,
                        &reply.headers,
                    )?
                    .headers_for(&request.method)?;
                    (
                        "Fetch.fulfillRequest",
                        json!({"requestId":route.native,"responseCode":reply.status,"body":if request.method=="HEAD"{""}else{&reply.body_base64},"responseHeaders":headers}),
                    )
                }
            } else if matches!(
                not_found,
                contract::HarNotFound::Continue | contract::HarNotFound::Fallback
            ) {
                // A fallback miss reaches here only when no later handler remains.
                har_summary =
                    Some(json!({"archiveId":archive_id,"matched":false,"notFound":not_found}));
                ("Fetch.continueRequest", json!({"requestId":route.native}))
            } else {
                har_summary =
                    Some(json!({"archiveId":archive_id,"matched":false,"notFound":"abort"}));
                // Pinned HarRouter calls route.abort() without a code.
                (
                    "Fetch.failRequest",
                    json!({"requestId":route.native,"errorReason":crate::browser::routing::AbortCode::Failed.chrome_reason()}),
                )
            }
        };
        Ok((method, params, har_summary))
    }
    /// `route_fulfill_http_evidence` becomes `route_fulfill_evidence` over a private
    /// network-evidence copy of the HTTP response body.
    fn http_evidence_reply(
        &mut self,
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
        let route = self
            .interactions
            .routes
            .get(&reference)
            .ok_or("STALE_REFERENCE")?;
        let context = self.route_context(route).ok_or("STALE_REFERENCE")?;
        let artifact = self.http_contexts.evidence_get(http_evidence_id)?;
        let (bytes, status) = crate::browser::http::fulfill_source(&artifact, status)?;
        let copy = crate::browser::network::write_body(
            self.journal.directory(),
            context,
            Uuid::new_v4(),
            crate::browser::network::BodySide::Response,
            &bytes,
            crate::browser::network::MAX_BODY,
        )?;
        let summary = self.network_evidence.insert(copy)?;
        Ok(CallbackResponse::RouteFulfillEvidence {
            status,
            network_evidence_id: serde_json::from_value(summary["evidenceId"].clone())
                .map_err(|_| Error::from("BACKEND_PROTOCOL"))?,
            headers,
        })
    }
    fn update_route_request_input(
        &mut self,
        reference: Reference,
        overrides: &crate::browser::routing::Overrides,
    ) -> Result<(), Error> {
        let route = self
            .interactions
            .routes
            .get(&reference)
            .ok_or("STALE_REFERENCE")?;
        let context = self.route_context(route).ok_or("STALE_REFERENCE")?;
        let source = route
            .har_request
            .as_ref()
            .ok_or("ROUTE_REQUEST_UNAVAILABLE")?;
        overrides.params(&source.url, &source.headers)?;
        let existing = route.request_input_id;
        let session = route.session.clone();
        let network = route.network_id.clone();
        let id = existing.or_else(|| {
            network.as_deref().and_then(|native| {
                self.diagnostics
                    .take_request_input_id(context, &session, native)
            })
        });
        if let Some(id) = id {
            self.interactions
                .routes
                .get_mut(&reference)
                .expect("Validated callback")
                .request_input_id = Some(id);
            let _ = self
                .request_inputs
                .update(reference.connection_id, id, overrides);
        } else if self.diagnostics.history.active(context) && self.request_inputs.enabled(context) {
            return Err("HTTP_REQUEST_INPUT_NOT_READY".into());
        }
        Ok(())
    }

    fn route_continue_overrides(
        route: &Route,
        overrides: &crate::browser::routing::Overrides,
        params: &mut Value,
    ) -> Result<(), Error> {
        let request = route
            .har_request
            .as_ref()
            .ok_or("ROUTE_REQUEST_UNAVAILABLE")?;
        let mut changes = overrides.params(&request.url, &request.headers)?;
        if let Some(headers) = changes.get_mut("headers").and_then(Value::as_array_mut) {
            headers.retain(|header| {
                !header["name"]
                    .as_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case("cookie"))
            });
        }
        params
            .as_object_mut()
            .expect("Continue parameters")
            .extend(changes.as_object().expect("Validated overrides").clone());
        Ok(())
    }

    pub(super) async fn resolve_route(
        &mut self,
        reference: Reference,
        response: CallbackResponse,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let response = crate::browser::routing::load_fulfill_file(response, deadline).await?;
        let response = self.http_evidence_reply(reference, response)?;
        let overrides = self.route_response_overrides(reference, &response)?;
        if matches!(
            response,
            CallbackResponse::RouteContinueWith { .. } | CallbackResponse::RouteFallbackWith { .. }
        ) {
            self.update_route_request_input(reference, &overrides)?;
        }
        if matches!(
            response,
            CallbackResponse::RouteFallback {} | CallbackResponse::RouteFallbackWith { .. }
        ) && let Some(result) = self.fallback_route_callback(reference, &overrides)?
        {
            return Ok(result);
        }
        if let CallbackResponse::RouteHar {
            archive_id,
            not_found: contract::HarNotFound::Fallback,
        } = response
        {
            let route = self
                .interactions
                .routes
                .get(&reference)
                .ok_or("STALE_REFERENCE")?;
            if !self
                .har_archives
                .matches(archive_id, &route.effective_request()?)?
                && let Some(mut result) = self.fallback_route_callback(reference, &overrides)?
            {
                result["har"] =
                    json!({"archiveId":archive_id,"matched":false,"notFound":"fallback"});
                return Ok(result);
            }
        }
        let route = self
            .interactions
            .routes
            .get(&reference)
            .ok_or("STALE_REFERENCE")?;
        let (method, mut params, har_summary) = self.route_callback_parameters(route, response)?;
        if method == "Fetch.continueRequest" && !overrides.is_empty() {
            Self::route_continue_overrides(route, &overrides, &mut params)?;
        }
        let session = route.session.clone();
        let owner = route.owner;
        let scopes = std::iter::once(route.policy_scope)
            .chain(route.fallback_from)
            .collect();
        if route.expires <= Instant::now() {
            return Err("CALLBACK_EXPIRED".into());
        }
        self.drain()?;
        let current = self
            .interactions
            .routes
            .get(&reference)
            .ok_or("STALE_REFERENCE")?;
        if current.owner != owner {
            return Err("STALE_REFERENCE".into());
        }
        self.route_context(current).ok_or("STALE_REFERENCE")?;
        self.interactions
            .routes
            .get_mut(&reference)
            .ok_or("STALE_REFERENCE")?
            .responding = true;
        let result = if matches!(
            method,
            "Fetch.continueRequest" | "Fetch.failRequest" | "Fetch.fulfillRequest"
        ) {
            self.route_reply_call(Some(&session), method, params, deadline)
                .await
        } else {
            self.cleanup_call(Some(&session), method, params, deadline)
                .await
        };
        match result {
            Ok(_) => {
                self.interactions.routes.remove(&reference);
                let mut result = json!({"callback":reference,"resolved":true,"replayed":false});
                if let Some(summary) = har_summary {
                    result["har"] = summary;
                }
                let failures = Box::pin(self.cleanup_exhausted_handlers(scopes, deadline)).await;
                if !failures.is_empty() {
                    result["handlerCleanupPending"] = json!(failures);
                }
                Ok(result)
            }
            Err(error) => Err(self.route_reply_error(reference, error)),
        }
    }
    fn route_reply_error(&mut self, reference: Reference, error: Error) -> Error {
        if error.code != "OUTCOME_UNKNOWN"
            && let Some(route) = self.interactions.routes.get_mut(&reference)
            && (route.automatic_auth.is_none() || error.code == "NOT_DISPATCHED")
        {
            route.responding = false;
        }
        error
    }
    pub(super) fn prune_routes(&mut self) {
        self.interactions.routes.retain(|_, route| {
            self.index
                .retained(route.owner.reference(), route.owner.kind())
                && self.index.retained_attachment(&route.session)
        });
    }
    pub(super) async fn expire_routes(&mut self) {
        self.prune_routes();
        let expired = self
            .interactions
            .routes
            .iter()
            .filter(|(_, r)| {
                !r.responding && r.expires <= Instant::now() && r.retry_at <= Instant::now()
            })
            .map(|(id, r)| {
                (
                    *id,
                    r.session.clone(),
                    r.native.clone(),
                    r.authentication,
                    std::iter::once(r.policy_scope)
                        .chain(r.fallback_from)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        for (id, session, native, authentication, scopes) in expired {
            let Some(connection) = self.connection.as_ref() else {
                break;
            };
            match connection
                .call(
                    Some(&session),
                    if authentication {"Fetch.continueWithAuth"} else {"Fetch.failRequest"},
                    if authentication {json!({"requestId":native,"authChallengeResponse":{"response":"CancelAuth"}})} else {json!({"requestId":native,"errorReason":"Aborted"})},
                    Lane::Control,
                    Instant::now() + Duration::from_secs(2),
                )
                .await
            {
                Ok(_) => {
                    self.interactions.routes.remove(&id);
                    Box::pin(self.cleanup_exhausted_handlers(scopes,Instant::now()+Duration::from_secs(2))).await;
                }
                Err(error) => {
                    let error = protocol_error(error);
                    if error.code == "OUTCOME_UNKNOWN" {
                        self.lose("CALLBACK_OUTCOME_UNKNOWN");
                        break;
                    }
                    if let Some(route) = self.interactions.routes.get_mut(&id) {
                        route.retry_at = Instant::now() + Duration::from_secs(2);
                    }
                }
            }
        }
    }
}
fn har_request(request: &Value) -> Result<crate::browser::har::Request, Error> {
    let fields = request["headers"]
        .as_object()
        .ok_or("HAR_REQUEST_UNAVAILABLE")?
        .iter()
        .map(|(name, value)| json!({"name":name,"value":value}))
        .collect::<Vec<_>>();
    crate::browser::har::Request::new(
        request["url"].as_str().ok_or("HAR_REQUEST_UNAVAILABLE")?,
        request["method"]
            .as_str()
            .ok_or("HAR_REQUEST_UNAVAILABLE")?,
        json!(fields),
        request_body(request)?,
    )
}
pub(super) fn request_body(request: &Value) -> Result<Option<Vec<u8>>, Error> {
    let body = if let Some(entries) = request["postDataEntries"].as_array() {
        let mut body = Vec::new();
        for entry in entries {
            let encoded = entry["bytes"].as_str().ok_or("HAR_REQUEST_UNAVAILABLE")?;
            if encoded.len() > crate::browser::har::MAX_BODY.div_ceil(3) * 4 {
                return Err("HAR_REQUEST_BOUNDS".into());
            }
            body.extend(
                STANDARD
                    .decode(encoded)
                    .map_err(|_| Error::from("HAR_REQUEST_UNAVAILABLE"))?,
            );
            if body.len() > crate::browser::har::MAX_BODY {
                return Err("HAR_REQUEST_BOUNDS".into());
            }
        }
        Some(body)
    } else if let Some(text) = request["postData"].as_str() {
        if text.len() > crate::browser::har::MAX_BODY {
            return Err("HAR_REQUEST_BOUNDS".into());
        }
        Some(text.as_bytes().to_vec())
    } else if request["hasPostData"] == true {
        return Err("HAR_REQUEST_UNAVAILABLE".into());
    } else {
        None
    };
    Ok(body)
}
impl Route {
    pub(super) fn unmatched_response(&self) -> CallbackResponse {
        if self.authentication {
            CallbackResponse::AuthDefault {}
        } else {
            CallbackResponse::RouteContinue {}
        }
    }
    pub(super) fn effective_request(&self) -> Result<crate::browser::har::Request, Error> {
        let original = self.har_request.as_ref().ok_or("HAR_REQUEST_UNAVAILABLE")?;
        let mut request = original.clone();
        let source =
            json!({"url":original.url,"method":original.method,"headers":original.headers});
        let effective = self.overrides.apply(&source)?;
        request.url = effective["url"]
            .as_str()
            .ok_or("ROUTE_REQUEST_UNAVAILABLE")?
            .into();
        request.method = effective["method"]
            .as_str()
            .ok_or("ROUTE_REQUEST_UNAVAILABLE")?
            .into();
        request.headers = serde_json::from_value(effective["headers"].clone())
            .map_err(|_| Error::from("ROUTE_REQUEST_UNAVAILABLE"))?;
        if let Some(body) = &self.overrides.body_base64 {
            request.body = Some(
                STANDARD
                    .decode(body)
                    .map_err(|_| Error::from("ROUTE_BODY_INVALID"))?,
            );
        }
        Ok(request)
    }
    pub(super) fn uses_policy(&self, scope: Reference) -> bool {
        self.policy_scope == scope || self.fallback_from == Some(scope)
    }
    pub(super) fn snapshot(&self, reference: Reference) -> Value {
        let mut value = json!({"reference":reference,"page":self.owner.page(),"policyScope":self.policy_scope,"handlerId":self.summary["handlerId"],"kind":"route","details":self.summary,"responding":self.responding,"expiresInMs":self.expires.saturating_duration_since(Instant::now()).as_millis()});
        if let Some(worker) = self.owner.worker() {
            value["worker"] = json!(worker);
        }
        value
    }
}
