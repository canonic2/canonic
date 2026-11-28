//! Scoped automatic HAR replies retain their tasks through driver shutdown.
use super::*;
use crate::browser::{
    contract::{CallbackResponse, HarNotFound},
    routing::{AutoRule, HarHandler, HarPhase, Rules},
};
/// The owner of an automatic archive reply: a scope rule or a chain handler.
#[derive(Clone, Copy)]
pub(super) enum AutoReply {
    Rule(AutoRule),
    Handler(HarHandler),
}
impl AutoReply {
    fn response(self) -> CallbackResponse {
        let (archive_id, not_found) = match self {
            Self::Rule(rule) => (rule.archive_id, rule.not_found),
            Self::Handler(handler) => (handler.archive_id, handler.not_found),
        };
        CallbackResponse::RouteHar {
            archive_id,
            not_found,
        }
    }
    /// Whether this owner still answers the callback: an unchanged active rule,
    /// or the same HAR handler still current in the callback's chain.
    fn current(self, rules: &Rules, reference: Reference) -> bool {
        match self {
            Self::Rule(rule) => rules
                .automatic(rule.scope)
                .is_some_and(|current| current.id == rule.id && current.phase == HarPhase::Active),
            Self::Handler(handler) => rules
                .handlers
                .current_har(reference)
                .is_some_and(|current| current.id == handler.id),
        }
    }
}
impl Host {
    pub(super) fn prune_har_rules(&self) {
        let pages = self
            .state
            .lock()
            .expect("Browser owner")
            .resources
            .values()
            .filter(|resource| matches!(resource.kind, Kind::Page | Kind::Context))
            .map(|resource| resource.reference)
            .collect::<std::collections::BTreeSet<_>>();
        self.routing
            .rules
            .lock()
            .expect("Route owner")
            .retain(|page| pages.contains(&page));
    }
    pub(super) fn har_list(&self) -> Value {
        self.prune_har_rules();
        let mut value = self.har_archives.lock().expect("HAR owner").list();
        value["routes"] = self.routing.rules.lock().expect("Route owner").summary();
        value
    }
    pub(super) fn har_remove(&self, archive_id: Uuid) -> Result<Value, Error> {
        self.prune_har_rules();
        let mut archives = self.har_archives.lock().expect("HAR owner");
        if self
            .routing
            .rules
            .lock()
            .expect("Route owner")
            .used(archive_id)
        {
            return Err("HAR_ARCHIVE_IN_USE".into());
        }
        archives.remove(archive_id)
    }
    pub(super) fn policy_routes(&self, scope: Reference) -> Vec<Reference> {
        let state = self.state.lock().expect("Browser owner");
        let owners = self.routing.owners.lock().expect("Route owners");
        let handoffs = self.routing.handoffs.lock().expect("Route handoffs");
        state
            .resources
            .values()
            .filter(|resource| {
                resource.kind == Kind::Route
                    && (owners.get(&resource.reference) == Some(&scope)
                        || handoffs
                            .values()
                            .any(|h| h.source == scope && h.target == resource.reference))
            })
            .map(|resource| resource.reference)
            .collect()
    }
    pub(super) async fn page_har_set(
        self: &Arc<Self>,
        page: Reference,
        archive_id: Option<Uuid>,
        not_found: HarNotFound,
        filter: Option<crate::browser::routing::Matcher>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.scoped_har_set(page, Kind::Page, archive_id, not_found, filter, deadline)
            .await
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Retain one scoped replay owner across subscription, event barrier, callback admission and acknowledged teardown"
    )]
    pub(super) async fn scoped_har_set(
        self: &Arc<Self>,
        scope: Reference,
        kind: Kind,
        archive_id: Option<Uuid>,
        not_found: HarNotFound,
        filter: Option<crate::browser::routing::Matcher>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if archive_id.is_none() && filter.is_some() {
            return Err("HAR_FILTER_REQUIRES_ARCHIVE".into());
        }
        let wire = self.wire(scope, kind)?;
        self.prune_har_rules();
        if self.fallback_dependency(scope) {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        if let Some(archive_id) = archive_id {
            if !self.policy_routes(scope).is_empty() {
                return Err("ROUTE_CALLBACKS_PENDING".into());
            }
            let rule = {
                let archives = self.har_archives.lock().expect("HAR owner");
                archives.get(archive_id)?;
                let mut rules = self.routing.rules.lock().expect("Route owner");
                rules.begin_filtered(
                    scope,
                    if kind == Kind::Context {
                        crate::browser::routing::HarScope::Context
                    } else {
                        crate::browser::routing::HarScope::Page
                    },
                    archive_id,
                    not_found,
                    filter,
                )?
            };
            let mut acknowledged = false;
            let result = async {
                let connection = self.connection()?;
                connection
                    .retained(
                        &wire,
                        "setNetworkInterceptionPatterns",
                        json!({"patterns":[{"regexSource":"^https?://"}]}),
                        deadline,
                        false,
                    )
                    .await?;
                acknowledged = true;
                connection.event_barrier(deadline).await?;
                self.wire(scope, kind)?;
                Ok::<_, Error>(())
            }
            .await;
            if let Err(mut error) = result {
                // The driver installs its handler before enabling the backend.
                // A remote error can therefore leave interception partly owned.
                let setup_pending = acknowledged
                    || matches!(error.code, "OUTCOME_UNKNOWN" | "BROWSER_METHOD_FAILED");
                let code = if setup_pending {
                    "OUTCOME_UNKNOWN"
                } else {
                    error.code
                };
                self.routing
                    .rules
                    .lock()
                    .expect("Route owner")
                    .setup_failed(scope, rule.id, code);
                error.partial =
                    Some(json!({"scope":scope,"ruleId":rule.id,"setupPending":setup_pending}));
                return Err(error);
            }
            let rule = self
                .routing
                .rules
                .lock()
                .expect("Route owner")
                .confirm(scope, rule.id)?;
            for reference in self.policy_routes(scope) {
                if rule.url_filter {
                    // Installation can admit a broad backend candidate before
                    // confirmation. Recheck it before scheduling archive work.
                    let Some(resource) = self
                        .state
                        .lock()
                        .expect("Browser owner")
                        .resource(reference, Some(Kind::Route))
                        .ok()
                        .cloned()
                    else {
                        continue;
                    };
                    let request = self.original_route_request(&resource.wire).await?;
                    let url = request["url"].as_str().ok_or("ROUTE_REQUEST_UNAVAILABLE")?;
                    let matches = self
                        .routing
                        .rules
                        .lock()
                        .expect("Route owner")
                        .har_matches(scope, url);
                    if !matches {
                        self.start_automatic_route_reply(reference, None).await?;
                        continue;
                    }
                }
                self.start_har_reply(reference, rule).await?;
            }
            self.wire(scope, kind)?;
            Ok(
                json!({"page":(kind==Kind::Page).then_some(scope),"context":(kind==Kind::Context).then_some(scope),"enabled":true,"rule":rule.summary(),"scope":if kind==Kind::Context {"context-and-pages-and-child-frames"}else{"page-and-child-frames"},"serviceWorkers":false}),
            )
        } else {
            self.remove_har_policy(scope, kind, true, deadline).await
        }
    }
    /// Explicit HAR disable may abort callbacks; unroute-all must preserve them.
    pub(super) async fn remove_har_policy(
        self: &Arc<Self>,
        scope: Reference,
        kind: Kind,
        abort_pending: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let wire = self.wire(scope, kind)?;
        if !abort_pending && !self.policy_routes(scope).is_empty() {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        let Some(previous) = self
            .routing
            .rules
            .lock()
            .expect("Route owner")
            .stopping(scope)
        else {
            return Ok(
                json!({"page":(kind==Kind::Page).then_some(scope),"context":(kind==Kind::Context).then_some(scope),"enabled":false}),
            );
        };
        self.changed.notify_waiters();
        let result = self
            .stop_scope_har(scope, &wire, abort_pending, deadline)
            .await;
        match result {
            Ok(()) => {
                self.routing
                    .rules
                    .lock()
                    .expect("Route owner")
                    .stopped(scope, previous.id);
                Ok(
                    json!({"page":(kind==Kind::Page).then_some(scope),"context":(kind==Kind::Context).then_some(scope),"enabled":false}),
                )
            }
            Err(mut error) => {
                self.routing
                    .rules
                    .lock()
                    .expect("Route owner")
                    .stop_failed(previous, error.code);
                error.partial = Some(
                    json!({"scope":scope,"ruleId":previous.id,"archiveId":previous.archive_id,"phase":"stopping","cleanupPending":true,"recovery":if kind==Kind::Context {"retry-stop-or-close-owned-context"}else{"retry-stop-or-close-owned-page"}}),
                );
                Err(error)
            }
        }
    }
    async fn stop_scope_har(
        self: &Arc<Self>,
        scope: Reference,
        wire: &super::super::objects::ObjectRef,
        abort_pending: bool,
        deadline: Instant,
    ) -> Result<(), Error> {
        if abort_pending {
            self.abort_har_callbacks(scope, deadline).await?;
        } else if !self.policy_routes(scope).is_empty() {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        let connection = self.connection()?;
        connection
            .retained(
                wire,
                "setNetworkInterceptionPatterns",
                json!({"patterns":[]}),
                deadline,
                false,
            )
            .await?;
        connection.event_barrier(deadline).await?;
        if abort_pending {
            self.abort_har_callbacks(scope, deadline).await?;
        } else if !self.policy_routes(scope).is_empty() {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        Ok(())
    }
    async fn abort_har_callbacks(
        self: &Arc<Self>,
        scope: Reference,
        deadline: Instant,
    ) -> Result<(), Error> {
        for reference in self.policy_routes(scope) {
            loop {
                if !self
                    .state
                    .lock()
                    .expect("Browser owner")
                    .resources
                    .contains_key(&reference.id)
                {
                    break;
                }
                if self
                    .responding
                    .lock()
                    .expect("Callback owner")
                    .contains(&reference)
                {
                    if !self
                        .routing
                        .pending
                        .lock()
                        .expect("HAR tasks")
                        .contains(&reference)
                        || Instant::now() >= deadline
                    {
                        return Err("ROUTE_CALLBACKS_PENDING".into());
                    }
                    tokio::time::sleep_until(
                        deadline.min(Instant::now() + Duration::from_millis(10)),
                    )
                    .await;
                    continue;
                }
                match self
                    .callback_resolve(
                        reference,
                        CallbackResponse::RouteAbort { error_code: None },
                        deadline,
                    )
                    .await
                {
                    Ok(_) => break,
                    Err(error)
                        if matches!(
                            error.code,
                            "STALE_REFERENCE" | "CALLBACK_ALREADY_RESOLVING"
                        ) =>
                    {
                        if !self
                            .state
                            .lock()
                            .expect("Browser owner")
                            .resources
                            .contains_key(&reference.id)
                        {
                            break;
                        }
                        if Instant::now() >= deadline {
                            return Err("ROUTE_CALLBACKS_PENDING".into());
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
    }
    pub(super) fn start_har_reply(
        self: &Arc<Self>,
        reference: Reference,
        rule: AutoRule,
    ) -> futures_util::future::BoxFuture<'_, Result<(), Error>> {
        self.start_automatic_route_reply(reference, Some(AutoReply::Rule(rule)))
    }
    /// Answer a callback whose chain reached a HAR handler from that handler's archive.
    pub(super) fn start_handler_har_reply(
        self: &Arc<Self>,
        reference: Reference,
        handler: HarHandler,
    ) -> futures_util::future::BoxFuture<'_, Result<(), Error>> {
        self.start_automatic_route_reply(reference, Some(AutoReply::Handler(handler)))
    }
    pub(super) fn start_automatic_route_reply(
        self: &Arc<Self>,
        reference: Reference,
        rule: Option<AutoReply>,
    ) -> futures_util::future::BoxFuture<'_, Result<(), Error>> {
        // Fallback may schedule another owned HAR reply through callback_reply.
        Box::pin(async move {
            if !self
                .routing
                .pending
                .lock()
                .expect("HAR tasks")
                .insert(reference)
            {
                return Ok(());
            }
            let finished = {
                let mut tasks = self.routing.tasks.lock().expect("HAR tasks");
                if self.stopping.load(Ordering::SeqCst) {
                    self.routing
                        .pending
                        .lock()
                        .expect("HAR tasks")
                        .remove(&reference);
                    return Ok(());
                }
                let mut finished = Vec::new();
                let mut index = 0;
                while index < tasks.len() {
                    if tasks[index].is_finished() {
                        finished.push(tasks.swap_remove(index));
                    } else {
                        index += 1;
                    }
                }
                if tasks.len() >= 64 {
                    self.routing
                        .pending
                        .lock()
                        .expect("HAR tasks")
                        .remove(&reference);
                    return Err("HAR_CALLBACK_CAPACITY".into());
                }
                let weak = Arc::downgrade(self);
                tasks.push(tokio::spawn(async move {
                    if let Some(host) = weak.upgrade() {
                        if let Some(rule) = rule {
                            host.automatic_har_reply(reference, rule).await;
                        } else {
                            let _ = host
                                .callback_reply(
                                    reference,
                                    CallbackResponse::RouteFallback {},
                                    Instant::now() + Duration::from_secs(30),
                                    false,
                                )
                                .await;
                        }
                        host.routing
                            .pending
                            .lock()
                            .expect("HAR tasks")
                            .remove(&reference);
                    }
                }));
                finished
            };
            for task in finished {
                task.await.map_err(|_| Error::from("HAR_OWNER_FAILED"))?;
            }
            Ok(())
        })
    }
    async fn automatic_har_reply(self: &Arc<Self>, reference: Reference, rule: AutoReply) {
        let Some(_permit) = self.har_permit(reference, rule).await else {
            return;
        };
        if self.stopping.load(Ordering::SeqCst)
            || self.state.lock().expect("Browser owner").phase != Phase::Ready
            || self
                .callbacks
                .lock()
                .expect("Callback owner")
                .get(&reference)
                .is_none_or(|expiry| *expiry <= Instant::now())
            || self
                .responding
                .lock()
                .expect("Callback owner")
                .contains(&reference)
            || !rule.current(&self.routing.rules.lock().expect("Route owner"), reference)
        {
            return;
        }
        let Some(expiry) = self
            .callbacks
            .lock()
            .expect("Callback owner")
            .get(&reference)
            .copied()
        else {
            return;
        };
        let result = self
            .callback_resolve(reference, rule.response(), expiry)
            .await;
        if result.as_ref().is_err_and(|error| {
            matches!(error.code, "STALE_REFERENCE" | "CALLBACK_ALREADY_RESOLVING")
        }) {
            return;
        }
        if let AutoReply::Rule(rule) = rule {
            self.routing
                .rules
                .lock()
                .expect("Route owner")
                .result(rule, result.as_ref().err().map(|error| error.code));
        }
        if result
            .as_ref()
            .is_err_and(|error| error.code != "OUTCOME_UNKNOWN")
            && !self
                .responding
                .lock()
                .expect("Callback owner")
                .contains(&reference)
        {
            let _ = self
                .callback_resolve(
                    reference,
                    CallbackResponse::RouteAbort { error_code: None },
                    Instant::now() + Duration::from_secs(2),
                )
                .await;
        }
    }
    async fn har_permit(
        &self,
        reference: Reference,
        rule: AutoReply,
    ) -> Option<tokio::sync::SemaphorePermit<'_>> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let expiry = self
                .callbacks
                .lock()
                .expect("Callback owner")
                .get(&reference)
                .copied()?;
            if self.stopping.load(Ordering::SeqCst)
                || self.state.lock().expect("Browser owner").phase != Phase::Ready
                || expiry <= Instant::now()
                || !rule.current(&self.routing.rules.lock().expect("Route owner"), reference)
            {
                return None;
            }
            tokio::select! {
                permit=self.routing.permits.acquire()=>return permit.ok(),
                ()=&mut changed=>{},
                ()=tokio::time::sleep_until(expiry)=>return None,
            }
        }
    }
    pub(super) async fn close_har_tasks(&self, deadline: Instant) -> Result<(), Error> {
        let mut tasks = std::mem::take(&mut *recovered(&self.routing.tasks));
        while let Some(mut task) = tasks.pop() {
            match tokio::time::timeout_at(deadline, &mut task).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    recovered(&self.routing.tasks).extend(tasks);
                    return Err("HAR_OWNER_FAILED".into());
                }
                Err(_) => {
                    tasks.push(task);
                    recovered(&self.routing.tasks).extend(tasks);
                    return Err("CLEANUP_PENDING".into());
                }
            }
        }
        recovered(&self.routing.pending).clear();
        recovered(&self.routing.owners).clear();
        recovered(&self.routing.rules).retain(|_| false);
        Ok(())
    }
}
