//! Request-aware synthetic body framing before a route callback is consumed.
use super::*;
use crate::browser::{
    objects::ObjectRef,
    routing::{self, Header},
};
mod fallback;
#[derive(Clone, Copy)]
pub(super) struct Handoff {
    pub(super) target: Reference,
    pub(super) source: Reference,
}
pub(super) struct Runtime {
    pub(super) overrides: Mutex<std::collections::HashMap<ObjectRef, routing::Overrides>>,
    pub(super) updating: Mutex<std::collections::BTreeSet<Reference>>,
    pub(super) manual_urls: Mutex<std::collections::BTreeMap<Reference, String>>,
    pub(super) handoffs: Mutex<std::collections::BTreeMap<Reference, Handoff>>,
    pub(super) rules: Mutex<routing::Rules>,
    pub(super) owners: Mutex<std::collections::BTreeMap<Reference, Reference>>,
    pub(super) pending: Mutex<std::collections::BTreeSet<Reference>>,
    pub(super) tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    pub(super) permits: tokio::sync::Semaphore,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            overrides: Mutex::default(),
            updating: Mutex::default(),
            manual_urls: Mutex::default(),
            handoffs: Mutex::default(),
            rules: Mutex::default(),
            owners: Mutex::default(),
            pending: Mutex::default(),
            tasks: Mutex::default(),
            permits: tokio::sync::Semaphore::new(4),
        }
    }
}
impl Host {
    /// `clear` with `behavior: "wait"` retries until its deadline: requests the
    /// scope privately continues can pause between its wait and its teardown.
    pub(super) async fn network_route(
        self: &Arc<Self>,
        scope: Reference,
        action: routing::HandlerAction,
        deadline: Instant,
    ) -> Result<Value, Error> {
        loop {
            match Box::pin(self.network_route_once(scope, action.clone(), deadline)).await {
                Err(error)
                    if action.waits()
                        && error.code == "ROUTE_CALLBACKS_PENDING"
                        && Instant::now() < deadline =>
                {
                    tokio::time::sleep_until(
                        deadline.min(Instant::now() + Duration::from_millis(25)),
                    )
                    .await;
                }
                result => return result,
            }
        }
    }
    async fn network_route_once(
        self: &Arc<Self>,
        scope: Reference,
        action: routing::HandlerAction,
        deadline: Instant,
    ) -> Result<Value, Error> {
        use routing::HandlerAction;
        action.validate()?;
        let matcher = action.matcher();
        let times = action.times();
        let kind = self
            .state
            .lock()
            .expect("Browser owner")
            .resource(scope, None)?
            .kind;
        if !matches!(kind, Kind::Page | Kind::Context) {
            return Err("INVALID_SCOPE".into());
        }
        self.prune_har_rules();
        // `clear` with `behavior: "wait"` lets callers answer callbacks first.
        while action.waits() && !self.policy_routes(scope).is_empty() {
            if Instant::now() >= deadline {
                return Err("ROUTE_CALLBACKS_PENDING".into());
            }
            let changed = self.changed.notified();
            tokio::select! {
                () = changed => {},
                () = tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(25))) => {},
            }
            self.prune_har_rules();
        }
        // Pinned unroute/unrouteAll without waiting. Removing the last handler
        // ends interception: paused requests continue to the network now and a
        // late reply refuses like any answered callback. With handlers left,
        // paused requests stay paused and skip the removed ones.
        if action.releases() && !self.policy_routes(scope).is_empty() {
            let (ids, last) = {
                let rules = self.routing.rules.lock().expect("Route owner");
                let ids = rules.handlers.release_ids(scope, &action)?;
                let last = matches!(action, HandlerAction::Clear { .. })
                    || rules.handlers.len(scope) == ids.len();
                (ids, last)
            };
            if !last {
                let mut rules = self.routing.rules.lock().expect("Route owner");
                rules.handlers.exhaust(scope, &ids);
                return Ok(json!({"scope":scope,"handlers":rules.handlers.list(scope)}));
            }
            for callback in self.policy_routes(scope) {
                Box::pin(self.callback_resolve(
                    callback,
                    contract::CallbackResponse::RouteContinue {},
                    deadline,
                ))
                .await?;
            }
        }
        if !matches!(action, HandlerAction::List {}) && !self.policy_routes(scope).is_empty() {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        match action {
            HandlerAction::List {} => {}
            HandlerAction::Add { .. }
            | HandlerAction::AddMatcher { .. }
            | HandlerAction::AddHar { .. } => {
                let har = action.har();
                if let Some((archive_id, _)) = har {
                    self.har_archives
                        .lock()
                        .expect("HAR owner")
                        .get(archive_id)?;
                }
                return self
                    .add_route_handler(
                        scope,
                        kind,
                        (matcher.expect("Validated matcher"), har),
                        times,
                        deadline,
                    )
                    .await;
            }
            action @ (HandlerAction::Remove { .. } | HandlerAction::RemoveMatcher { .. }) => {
                self.remove_route_handlers(scope, kind, &action, deadline)
                    .await?;
            }
            HandlerAction::Clear { .. } => {
                // Scope-wide replay is exclusive with chain handlers, but clear
                // owns both forms and must retain the archive until teardown ACK.
                self.remove_har_policy(scope, kind, false, deadline).await?;
                let enabled = {
                    let rules = self.routing.rules.lock().expect("Route owner");
                    rules.manual(scope) || rules.handlers.len(scope) > 0
                };
                if enabled {
                    self.scoped_route_set(scope, kind, None, false, deadline)
                        .await?;
                }
                self.routing
                    .rules
                    .lock()
                    .expect("Route owner")
                    .handlers
                    .clear(scope);
            }
        }
        Ok(
            json!({"scope":scope,"handlers":self.routing.rules.lock().expect("Route owner").handlers.list(scope)}),
        )
    }
    async fn remove_route_handlers(
        &self,
        scope: Reference,
        kind: Kind,
        action: &routing::HandlerAction,
        deadline: Instant,
    ) -> Result<(), Error> {
        let (ids, last) = {
            let rules = self.routing.rules.lock().expect("Route owner");
            let ids = rules.handlers.removal_ids(scope, action)?;
            if ids.is_empty() {
                return Ok(());
            }
            let last = rules.handlers.len(scope) == ids.len();
            if !last && rules.handlers.pending(scope) {
                return Err("ROUTE_POLICY_PENDING".into());
            }
            (ids, last)
        };
        if last {
            self.scoped_route_set(scope, kind, None, false, deadline)
                .await?;
        } else {
            let primary = {
                let mut rules = self.routing.rules.lock().expect("Route owner");
                rules
                    .handlers
                    .plan_remove_ids(scope, &ids)
                    .then(|| rules.handlers.url(scope).expect("Owned handler").to_owned())
            };
            if let Some(primary) = primary {
                self.scoped_route_set(scope, kind, Some(primary), false, deadline)
                    .await?;
            }
        }
        let mut rules = self.routing.rules.lock().expect("Route owner");
        for id in ids {
            rules.handlers.remove(scope, id);
        }
        rules.handlers.activate(scope);
        Ok(())
    }

    async fn add_route_handler(
        &self,
        scope: Reference,
        kind: Kind,
        (matcher, har): (
            crate::browser::routing::Matcher,
            Option<(Uuid, contract::HarNotFound)>,
        ),
        times: Option<u32>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let (first, update) = {
            let rules = self.routing.rules.lock().expect("Route owner");
            rules.handlers.capacity(scope)?;
            let first = rules.handlers.len(scope) == 0;
            if first && rules.manual(scope) {
                return Err("MANUAL_ROUTE_ACTIVE".into());
            }
            if first && rules.automatic(scope).is_some() {
                return Err("HAR_ROUTE_ACTIVE".into());
            }
            (
                first,
                first || !rules.handlers.contains_matcher(scope, &matcher),
            )
        };
        let handler = {
            let mut rules = self.routing.rules.lock().expect("Route owner");
            let handler = if first {
                rules.handlers.begin_limited(scope, matcher, times)?
            } else {
                rules.handlers.add_limited(scope, matcher, times)?
            };
            // Same lock as registration: no callback can admit it without its archive.
            if let Some(har) = har {
                rules.handlers.set_har(scope, handler, har);
            }
            handler
        };
        if update {
            let primary = {
                let mut rules = self.routing.rules.lock().expect("Route owner");
                rules.handlers.setup_failed(scope);
                rules.handlers.url(scope).expect("Owned handler").to_owned()
            };
            if let Err(mut error) = self
                .scoped_route_set(scope, kind, Some(primary), false, deadline)
                .await
            {
                error.partial = Some(
                    json!({"scope":scope,"handlerId":handler,"setupPending":true,"cause":error.partial}),
                );
                return Err(error);
            }
            self.routing
                .rules
                .lock()
                .expect("Route owner")
                .handlers
                .activate(scope);
        }
        let rules = self.routing.rules.lock().expect("Route owner");
        Ok(json!({"scope":scope,"handlerId":handler,"handlers":rules.handlers.list(scope)}))
    }
    pub(super) async fn cleanup_exhausted_handlers(
        self: &Arc<Self>,
        scopes: Vec<Reference>,
        deadline: Instant,
    ) -> Vec<Value> {
        let mut failures = Vec::new();
        for scope in scopes
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
        {
            if self
                .state
                .lock()
                .expect("Browser owner")
                .resource(scope, None)
                .is_err()
            {
                continue;
            }
            let ids = self
                .routing
                .rules
                .lock()
                .expect("Route owner")
                .handlers
                .exhausted(scope);
            if ids.is_empty() || !self.policy_routes(scope).is_empty() {
                continue;
            }
            for id in ids {
                if self
                    .routing
                    .rules
                    .lock()
                    .expect("Route owner")
                    .handlers
                    .pending(scope)
                {
                    failures.push(json!({"scope":scope,"code":"ROUTE_POLICY_PENDING","nextAction":{"operation":"network_route","scope":scope,"action":{"kind":"clear"}},"replayed":false}));
                    break;
                }
                if let Err(error) = self
                    .network_route(
                        scope,
                        routing::HandlerAction::Remove { handler_id: id },
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
        &self,
        page: Reference,
        url: Option<String>,
        authentication: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.scoped_route_set(page, Kind::Page, url, authentication, deadline)
            .await
    }
    #[expect(
        clippy::too_many_lines,
        reason = "One serialized policy update retains its backend receipt and scope fence through late acknowledgement and cleanup"
    )]
    pub(super) async fn scoped_route_set(
        &self,
        scope: Reference,
        kind: Kind,
        url: Option<String>,
        authentication: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if authentication {
            return Err("AUTH_CHALLENGE_UNSUPPORTED".into());
        }
        if let Some(url) = &url {
            let rules = self.routing.rules.lock().expect("Route owner");
            if rules.handlers.pending(scope) && rules.handlers.url(scope) != Some(url.as_str()) {
                return Err("ROUTE_POLICY_PENDING".into());
            }
        }
        let wire = self.wire(scope, kind)?;
        self.prune_har_rules();
        {
            let state = self.state.lock().expect("Browser owner");
            self.routing
                .manual_urls
                .lock()
                .expect("Route patterns")
                .retain(|scope, _| state.resource(*scope, None).is_ok());
        }
        if self
            .routing
            .rules
            .lock()
            .expect("Route owner")
            .automatic(scope)
            .is_some()
        {
            return Err("HAR_ROUTE_ACTIVE".into());
        }
        if !self.policy_routes(scope).is_empty() {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        if !self
            .routing
            .updating
            .lock()
            .expect("Route updates")
            .insert(scope)
        {
            return Err("ROUTE_POLICY_PENDING".into());
        }
        if !self.policy_routes(scope).is_empty() {
            self.routing
                .updating
                .lock()
                .expect("Route updates")
                .remove(&scope);
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        let previous = self
            .routing
            .rules
            .lock()
            .expect("Route owner")
            .manual(scope);
        self.routing
            .rules
            .lock()
            .expect("Route owner")
            .mark_manual(scope, true);
        let enabled = url.is_some();
        let source_url = url.clone();
        let matchers = {
            let rules = self.routing.rules.lock().expect("Route owner");
            if enabled && rules.handlers.pending(scope) {
                rules.handlers.planned_patterns(scope)
            } else {
                url.into_iter()
                    .map(|url| routing::Matcher::Exact { url })
                    .collect()
            }
        };
        let patterns = matchers
            .into_iter()
            .map(|matcher| matcher.source().map(|source| json!({"regexSource":source})))
            .collect::<Result<Vec<_>, _>>()?;
        let mut acknowledged = false;
        let result = async {
            let connection = self.connection()?;
            connection
                .retained(
                    &wire,
                    "setNetworkInterceptionPatterns",
                    json!({"patterns":patterns}),
                    deadline,
                    false,
                )
                .await?;
            acknowledged = true;
            // A retained pattern write can acknowledge after the public
            // deadline. Verify its actual receipt without replaying the write.
            connection
                .event_barrier(deadline.max(Instant::now() + Duration::from_secs(2)))
                .await?;
            self.wire(scope, kind)?;
            Ok::<_, Error>(())
        }
        .await;
        if let Err(mut error) = result {
            self.routing
                .rules
                .lock()
                .expect("Route owner")
                .handlers
                .setup_failed(scope);
            let mut updating = self.routing.updating.lock().expect("Route updates");
            if !acknowledged && !matches!(error.code, "OUTCOME_UNKNOWN" | "BROWSER_METHOD_FAILED") {
                self.routing
                    .rules
                    .lock()
                    .expect("Route owner")
                    .mark_manual(scope, previous);
            }
            if acknowledged {
                error.partial =
                    Some(json!({"scope":scope,"enabled":enabled,"verificationPending":true}));
            }
            self.routing
                .manual_urls
                .lock()
                .expect("Route patterns")
                .remove(&scope);
            updating.remove(&scope);
            return Err(error);
        }
        let mut updating = self.routing.updating.lock().expect("Route updates");
        let mut rules = self.routing.rules.lock().expect("Route owner");
        let mut urls = self.routing.manual_urls.lock().expect("Route patterns");
        rules.mark_manual(scope, enabled);
        if !enabled || !rules.handlers.pending(scope) {
            rules.handlers.clear(scope);
        }
        if let Some(url) = source_url {
            urls.insert(scope, url);
        } else {
            urls.remove(&scope);
        }
        updating.remove(&scope);
        Ok(
            json!({"page":(kind==Kind::Page).then_some(scope),"context":(kind==Kind::Context).then_some(scope),"enabled":enabled,"scope":if kind==Kind::Context {"context-and-pages-and-child-frames"}else{"page-and-child-frames"},"serviceWorkers":false}),
        )
    }
    pub(super) async fn binary_route_response(
        &self,
        wire: &ObjectRef,
        status: u16,
        body: &str,
        headers: &[Header],
    ) -> Result<Value, Error> {
        let request = self.route_request(wire).await?;
        self.framed_response(&request, status, body, headers)
    }
    pub(super) async fn route_overrides(
        &self,
        wire: &ObjectRef,
        overrides: &routing::Overrides,
    ) -> Result<Value, Error> {
        let source = self.route_request(wire).await?;
        let request = crate::browser::har::Request::new(
            source["url"].as_str().ok_or("ROUTE_REQUEST_UNAVAILABLE")?,
            source["method"]
                .as_str()
                .ok_or("ROUTE_REQUEST_UNAVAILABLE")?,
            source["headers"].clone(),
            None,
        )?;
        overrides
            .params(&request.url, &request.headers)
            .map_err(Error::from)
    }
    pub(super) async fn original_route_request(&self, wire: &ObjectRef) -> Result<Value, Error> {
        let connection = self.connection()?;
        let route = connection
            .initializer(wire)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        let request = connection
            .resolve(
                route["request"]["guid"].as_str().ok_or("MISSING_REQUEST")?,
                "Request",
            )
            .await?;
        connection
            .initializer(&request)
            .await?
            .ok_or("MISSING_INITIALIZER".into())
    }
    pub(super) async fn update_route_request_input(
        &self,
        wire: &ObjectRef,
        overrides: &routing::Overrides,
    ) -> Result<(), Error> {
        let connection = self.connection()?;
        let route = connection
            .initializer(wire)
            .await?
            .ok_or("MISSING_INITIALIZER")?;
        let request = connection
            .resolve(
                route["request"]["guid"].as_str().ok_or("MISSING_REQUEST")?,
                "Request",
            )
            .await?;
        let id = self
            .diagnostics
            .lock()
            .expect("Diagnostics")
            .known_request(&request);
        let context = {
            let state = self.state.lock().expect("Browser owner");
            let mut scope = state.by_wire(wire).map(|resource| resource.reference);
            let mut found = None;
            for _ in 0..contract::MAX_RESOURCES {
                let Some(resource) =
                    scope.and_then(|reference| state.resource(reference, None).ok())
                else {
                    break;
                };
                if resource.kind == Kind::Context {
                    found = Some(resource.reference);
                    break;
                }
                scope = resource.parent;
            }
            found
        };
        if let Some(id) = id {
            let owner = self.state.lock().expect("Browser owner").connection;
            let _ = self
                .request_inputs
                .lock()
                .expect("Request input owner")
                .update(owner, id, overrides);
        } else if let Some(context) = context
            && self
                .diagnostics
                .lock()
                .expect("Diagnostics")
                .active(context)
            && self
                .request_inputs
                .lock()
                .expect("Request input owner")
                .enabled(context)
        {
            return Err("HTTP_REQUEST_INPUT_NOT_READY".into());
        }
        Ok(())
    }
    async fn route_request(&self, wire: &ObjectRef) -> Result<Value, Error> {
        let source = self.original_route_request(wire).await?;
        let overrides = self
            .routing
            .overrides
            .lock()
            .expect("Route overrides")
            .get(wire)
            .cloned()
            .unwrap_or_default();
        overrides.apply(&source).map_err(Error::from)
    }
    /// As [`Self::binary_route_response`] for a UTF-8 text body sent as text.
    pub(super) async fn text_route_response(
        &self,
        wire: &ObjectRef,
        status: u16,
        body: &str,
        headers: &[Header],
    ) -> Result<Value, Error> {
        use base64::Engine as _;
        let mut params = self
            .binary_route_response(wire, status, body, headers)
            .await?;
        if params["body"] != "" {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(body)
                .map_err(|_| Error::from("ROUTE_BODY_ENCODING"))?;
            params["body"] =
                json!(String::from_utf8(bytes).map_err(|_| Error::from("ROUTE_BODY_ENCODING"))?);
        }
        params["isBase64"] = json!(false);
        Ok(params)
    }
    fn framed_response(
        &self,
        request: &Value,
        status: u16,
        body: &str,
        headers: &[Header],
    ) -> Result<Value, Error> {
        let engine = self.state.lock().expect("Browser owner").engine;
        if engine == contract::Engine::Webkit && (300..400).contains(&status) {
            return Err("ROUTE_STATUS_UNSUPPORTED".into());
        }
        let prepared = routing::binary_response(status, body, headers)?;
        let method = request["method"].as_str().ok_or("BACKEND_PROTOCOL")?;
        let headers = prepared.headers_for(method)?;
        let headers = if engine == contract::Engine::Webkit {
            routing::dictionary_headers(headers)
        } else {
            headers
        };
        Ok(
            json!({"status":status,"body":if method=="HEAD"{""}else{body},"isBase64":true,"headers":headers}),
        )
    }
    /// A paused route's request as bounded private bytes, with its raw initializer.
    pub(super) async fn route_request_copy(
        &self,
        wire: &ObjectRef,
    ) -> Result<(Value, crate::browser::har::Request), Error> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let source = self.route_request(wire).await?;
        let body = source
            .get("postData")
            .map(|body| {
                let encoded = body.as_str().ok_or("HAR_REQUEST_UNAVAILABLE")?;
                if encoded.len() > crate::browser::har::MAX_BODY.div_ceil(3) * 4 {
                    return Err(Error::from("HAR_REQUEST_BOUNDS"));
                }
                STANDARD
                    .decode(encoded)
                    .map_err(|_| Error::from("HAR_REQUEST_UNAVAILABLE"))
            })
            .transpose()?;
        let request = crate::browser::har::Request::new(
            source["url"].as_str().ok_or("HAR_REQUEST_UNAVAILABLE")?,
            source["method"].as_str().ok_or("HAR_REQUEST_UNAVAILABLE")?,
            source["headers"].clone(),
            body,
        )?;
        Ok((source, request))
    }
    pub(super) async fn har_route_response(
        &self,
        wire: &ObjectRef,
        archive_id: Uuid,
        not_found: contract::HarNotFound,
    ) -> Result<(&'static str, Value, Value), Error> {
        let (source, request) = self.route_request_copy(wire).await?;
        let reply = self
            .har_archives
            .lock()
            .expect("HAR owner")
            .reply(archive_id, &request)?;
        if let Some(reply) = reply {
            if source["isNavigationRequest"] == true && reply.url != request.url {
                let callback = {
                    let state = self.state.lock().expect("Browser owner");
                    state.by_wire(wire).map(|resource| resource.reference)
                };
                let scope = callback.and_then(|callback| {
                    self.routing
                        .owners
                        .lock()
                        .expect("Route owners")
                        .get(&callback)
                        .copied()
                });
                let automatic = scope
                    .and_then(|scope| {
                        self.routing
                            .rules
                            .lock()
                            .expect("Route owner")
                            .automatic(scope)
                    })
                    .is_some_and(|rule| {
                        rule.archive_id == archive_id && rule.phase == routing::HarPhase::Active
                    });
                if !automatic {
                    return Err("HAR_NAVIGATION_REDIRECT_REQUIRED".into());
                }
                return Ok((
                    "redirectNavigationRequest",
                    json!({"url":reply.url}),
                    json!({"archiveId":archive_id,"matched":true,"redirectsFollowed":reply.redirects}),
                ));
            }
            Ok((
                "fulfill",
                self.framed_response(&source, reply.status, &reply.body_base64, &reply.headers)?,
                json!({"archiveId":archive_id,"matched":true,"redirectsFollowed":reply.redirects}),
            ))
        } else if matches!(not_found, contract::HarNotFound::Continue) {
            Ok((
                "continue",
                json!({"isFallback":false}),
                json!({"archiveId":archive_id,"matched":false,"notFound":"continue"}),
            ))
        } else if matches!(not_found, contract::HarNotFound::Fallback) {
            // The caller hands this request to the next handler, or continues at chain end.
            Ok((
                "fallback",
                json!({"isFallback":true}),
                json!({"archiveId":archive_id,"matched":false,"notFound":"fallback"}),
            ))
        } else {
            Ok((
                "abort",
                // Pinned HarRouter calls route.abort() without a code.
                json!({"errorCode":crate::browser::routing::AbortCode::Failed.code()}),
                json!({"archiveId":archive_id,"matched":false,"notFound":"abort"}),
            ))
        }
    }
}
