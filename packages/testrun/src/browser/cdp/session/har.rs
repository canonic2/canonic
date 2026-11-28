//! Automatic page replay runs on the callback lane during ordinary browser waits.
use super::*;
use crate::browser::{
    contract::{CallbackResponse, HarNotFound},
    routing::HarPhase,
};
impl Worker {
    pub(super) fn prune_har_routes(&mut self) {
        self.routing_rules.retain(|scope| {
            self.index.retained(scope, Kind::Page) || self.index.retained(scope, Kind::Context)
        });
    }
    pub(super) fn har_list(&mut self) -> Value {
        self.prune_har_routes();
        let mut value = self.har_archives.list();
        value["routes"] = self.routing_rules.summary();
        value
    }
    pub(super) fn har_remove(&mut self, archive_id: Uuid) -> Result<Value, Error> {
        self.prune_har_routes();
        if self.routing_rules.used(archive_id) {
            return Err("HAR_ARCHIVE_IN_USE".into());
        }
        self.har_archives.remove(archive_id)
    }
    pub(super) async fn page_har_set(
        &mut self,
        id: Uuid,
        page: Reference,
        archive_id: Option<Uuid>,
        not_found: HarNotFound,
        filter: Option<crate::browser::routing::Matcher>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.scoped_har_set(
            id,
            page,
            Kind::Page,
            archive_id,
            not_found,
            filter,
            deadline,
        )
        .await
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Keep native request identity, replay scope, options and deadline explicit across setup and teardown"
    )]
    pub(super) async fn scoped_har_set(
        &mut self,
        id: Uuid,
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
        self.index.native(scope, kind).map_err(resource_error)?;
        self.prune_har_routes();
        if self
            .interactions
            .routes
            .values()
            .any(|route| route.policy_scope != scope && route.uses_policy(scope))
        {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        if let Some(archive_id) = archive_id {
            self.har_archives.get(archive_id)?;
            if self
                .interactions
                .routes
                .values()
                .any(|route| route.uses_policy(scope))
            {
                return Err("ROUTE_CALLBACKS_PENDING".into());
            }
            let rule = self.routing_rules.begin_filtered(
                scope,
                if kind == Kind::Context {
                    crate::browser::routing::HarScope::Context
                } else {
                    crate::browser::routing::HarScope::Page
                },
                archive_id,
                not_found,
                filter,
            )?;
            let result = self
                .set_scoped_fetch(id, scope, kind, Some(fetch::Configuration::har()), deadline)
                .await;
            if let Err(mut error) = result {
                // A sibling renderer may already have acknowledged setup.
                self.routing_rules
                    .setup_failed(scope, rule.id, "OUTCOME_UNKNOWN");
                error.partial = Some(
                    json!({"scope":scope,"ruleId":rule.id,"setupPending":true,"nextAction":if kind==Kind::Context {"context_har_set with archiveId null or context_close"}else{"page_har_set with archiveId null or page_close"}}),
                );
                return Err(error);
            }
            self.index.native(scope, kind).map_err(resource_error)?;
            let rule = self.routing_rules.confirm(scope, rule.id)?;
            Ok(
                json!({"page":(kind==Kind::Page).then_some(scope),"context":(kind==Kind::Context).then_some(scope),"enabled":true,"rule":rule.summary(),"scope":if kind==Kind::Context {"context-and-pages-and-child-frames"}else{"page-and-child-frames"},"crossProcessFrames":true}),
            )
        } else {
            self.remove_har_policy(id, scope, kind, true, deadline)
                .await
        }
    }
    /// Explicit HAR disable may abort callbacks; unroute-all must preserve them.
    pub(super) async fn remove_har_policy(
        &mut self,
        id: Uuid,
        scope: Reference,
        kind: Kind,
        abort_pending: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if !abort_pending
            && self
                .interactions
                .routes
                .values()
                .any(|route| route.uses_policy(scope))
        {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        let Some(previous) = self.routing_rules.stopping(scope) else {
            return Ok(
                json!({"page":(kind==Kind::Page).then_some(scope),"context":(kind==Kind::Context).then_some(scope),"enabled":false}),
            );
        };
        let result = self
            .stop_scope_har(id, scope, kind, abort_pending, deadline)
            .await;
        match result {
            Ok(()) => {
                self.routing_rules.stopped(scope, previous.id);
                Ok(
                    json!({"page":(kind==Kind::Page).then_some(scope),"context":(kind==Kind::Context).then_some(scope),"enabled":false}),
                )
            }
            Err(error) => {
                self.routing_rules.stop_failed(previous, error.code);
                Err(error)
            }
        }
    }
    async fn stop_scope_har(
        &mut self,
        id: Uuid,
        scope: Reference,
        kind: Kind,
        abort_pending: bool,
        deadline: Instant,
    ) -> Result<(), Error> {
        let pending = self
            .interactions
            .routes
            .iter()
            .filter(|(_, route)| route.uses_policy(scope))
            .map(|(reference, route)| (*reference, route.responding, route.expires))
            .collect::<Vec<_>>();
        for (reference, responding, expires) in pending {
            if !abort_pending || responding || expires <= Instant::now() {
                return Err("ROUTE_CALLBACKS_PENDING".into());
            }
            if let Err(error) = self
                .resolve_route(
                    reference,
                    CallbackResponse::RouteAbort { error_code: None },
                    deadline,
                )
                .await
            {
                if !self.interactions.routes.contains_key(&reference) {
                    continue;
                }
                return Err(error);
            }
        }
        self.set_scoped_fetch(id, scope, kind, None, deadline)
            .await?;
        if self
            .interactions
            .routes
            .values()
            .any(|route| route.uses_policy(scope))
        {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        Ok(())
    }
    pub(super) async fn service_automatic_route(
        &mut self,
        reference: Reference,
        response: CallbackResponse,
        deadline: Instant,
    ) -> Result<(), Error> {
        if self.har_servicing {
            return Err("ROUTE_CALLBACKS_PENDING".into());
        }
        self.har_servicing = true;
        let result = self.resolve_route(reference, response, deadline).await;
        self.har_servicing = false;
        if let Err(error) = &result
            && let Some(route) = self.interactions.routes.get_mut(&reference)
        {
            route.last_failure = Some(error.code);
            if route.released {
                self.interactions.routes.remove(&reference);
            } else {
                route.retry_at = Instant::now() + Duration::from_millis(250);
            }
        }
        result.map(|_| ())
    }
    pub(super) async fn poll_har_routes(&mut self) {
        self.service_storage_requests().await;
        if self.har_servicing
            || self.shared.state.lock().expect("Chrome state").phase != Phase::Ready
        {
            return;
        }
        // CDP wildcard filtering admits candidates for the precise logical
        // matcher. A proven nonmatch is continued privately, never offered as
        // an application callback or handed to a fallback handler.
        if let Some((reference, deadline, response)) = self
            .interactions
            .routes
            .iter()
            .find(|(_, route)| {
                route.auto_pass
                    && !route.responding
                    && route.expires > Instant::now()
                    && route.retry_at <= Instant::now()
            })
            .map(|(reference, route)| (*reference, route.expires, route.unmatched_response()))
        {
            let _ = self
                .service_automatic_route(reference, response, deadline)
                .await;
            return;
        }
        let next = self
            .interactions
            .routes
            .iter()
            .filter(|(_, route)| {
                !route.responding
                    && route.expires > Instant::now()
                    && route.retry_at <= Instant::now()
            })
            .find_map(|(reference, route)| {
                // A HAR handler reached in this callback's chain answers first;
                // otherwise the scope's automatic rule, if any.
                if let Some(handler) = self.routing_rules.handlers.current_har(*reference) {
                    return Some((
                        *reference,
                        None,
                        handler.archive_id,
                        handler.not_found,
                        route.expires,
                    ));
                }
                self.routing_rules
                    .automatic(route.policy_scope)
                    .filter(|rule| rule.phase == HarPhase::Active)
                    .map(|rule| {
                        (
                            *reference,
                            Some(rule),
                            rule.archive_id,
                            rule.not_found,
                            route.expires,
                        )
                    })
            });
        let Some((reference, rule, archive_id, not_found, expires)) = next else {
            return;
        };
        self.har_servicing = true;
        let result = self
            .resolve_route(
                reference,
                CallbackResponse::RouteHar {
                    archive_id,
                    not_found,
                },
                expires,
            )
            .await;
        if let Some(rule) = rule {
            self.routing_rules
                .result(rule, result.as_ref().err().map(|error| error.code));
        }
        if result
            .as_ref()
            .is_err_and(|error| error.code != "OUTCOME_UNKNOWN")
            && self
                .interactions
                .routes
                .get(&reference)
                .is_some_and(|route| !route.responding)
            && self
                .resolve_route(
                    reference,
                    CallbackResponse::RouteAbort { error_code: None },
                    Instant::now() + Duration::from_secs(2),
                )
                .await
                .is_err()
            && let Some(route) = self.interactions.routes.get_mut(&reference)
        {
            route.retry_at = Instant::now() + Duration::from_secs(2);
        }
        self.har_servicing = false;
    }
}
