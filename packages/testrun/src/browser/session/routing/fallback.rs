//! Local handler handoff without continuing the backend's paused request.
use super::*;
use crate::browser::{lifecycle::Resource, routing::AutoRule};

pub(in crate::browser::session) struct Target {
    context: Reference,
    pub(in crate::browser::session) rule: Option<AutoRule>,
    url: Option<String>,
    same_scope: bool,
}

impl Host {
    #[expect(
        clippy::too_many_lines,
        reason = "one fallback decision: its resource, context and target checks share their locals"
    )]
    pub(in crate::browser::session) async fn route_fallback_target(
        &self,
        resource: &Resource,
    ) -> Result<Option<Target>, Error> {
        let (page, context) = {
            let state = self.state.lock().expect("Browser owner");
            state.resource(resource.reference, Some(Kind::Route))?;
            (
                state.ancestor(resource.reference, Kind::Page),
                state.ancestor(resource.reference, Kind::Context),
            )
        };
        let owner = self
            .routing
            .owners
            .lock()
            .expect("Route owners")
            .get(&resource.reference)
            .copied();
        if self
            .routing
            .rules
            .lock()
            .expect("Route owner")
            .handlers
            .has_next(resource.reference)
        {
            return self
                .same_scope_fallback_target(resource, owner.ok_or("STALE_REFERENCE")?)
                .await;
        }
        let (Some(page), Some(context)) = (page, context) else {
            return Ok(None);
        };
        if self
            .routing
            .owners
            .lock()
            .expect("Route owners")
            .get(&resource.reference)
            != Some(&page)
        {
            return Ok(None);
        }
        if self
            .routing
            .rules
            .lock()
            .expect("Route owner")
            .automatic(page)
            .is_some_and(|rule| rule.phase != routing::HarPhase::Active)
        {
            return Err("ROUTE_POLICY_PENDING".into());
        }
        if self
            .routing
            .updating
            .lock()
            .expect("Route updates")
            .contains(&context)
        {
            return Err("ROUTE_POLICY_PENDING".into());
        }
        let rule = self
            .routing
            .rules
            .lock()
            .expect("Route owner")
            .automatic(context);
        if let Some(rule) = rule {
            if rule.phase != routing::HarPhase::Active {
                return Err("ROUTE_POLICY_PENDING".into());
            }
            if rule.url_filter {
                let request = self.original_route_request(&resource.wire).await?;
                let url = request["url"].as_str().ok_or("ROUTE_REQUEST_UNAVAILABLE")?;
                if !self
                    .routing
                    .rules
                    .lock()
                    .expect("Route owner")
                    .har_matches(context, url)
                {
                    return Ok(None);
                }
            }
            return Ok(Some(Target {
                context,
                rule: Some(rule),
                url: None,
                same_scope: false,
            }));
        }
        if !self
            .routing
            .rules
            .lock()
            .expect("Route owner")
            .manual(context)
        {
            return Ok(None);
        }
        self.context_fallback_target(resource, context).await
    }
    async fn context_fallback_target(
        &self,
        resource: &Resource,
        context: Reference,
    ) -> Result<Option<Target>, Error> {
        let request = self.original_route_request(&resource.wire).await?;
        let url = self
            .routing
            .manual_urls
            .lock()
            .expect("Route patterns")
            .get(&context)
            .cloned()
            .ok_or("ROUTE_POLICY_PENDING")?;
        let source_url = request["url"].as_str().ok_or("ROUTE_REQUEST_UNAVAILABLE")?;
        let matches = {
            let rules = self.routing.rules.lock().expect("Route owner");
            if rules.handlers.len(context) > 0 {
                rules.handlers.matches(context, source_url)
            } else {
                source_url == url
            }
        };
        Ok(matches.then_some(Target {
            context,
            rule: None,
            url: Some(source_url.to_owned()),
            same_scope: false,
        }))
    }
    async fn same_scope_fallback_target(
        &self,
        resource: &Resource,
        scope: Reference,
    ) -> Result<Option<Target>, Error> {
        self.routing
            .manual_urls
            .lock()
            .expect("Route patterns")
            .get(&scope)
            .cloned()
            .ok_or("ROUTE_POLICY_PENDING")?;
        let request = self.original_route_request(&resource.wire).await?;
        let url = request["url"]
            .as_str()
            .ok_or("ROUTE_REQUEST_UNAVAILABLE")?
            .to_owned();
        Ok(Some(Target {
            context: scope,
            rule: None,
            url: Some(url),
            same_scope: true,
        }))
    }
    pub(in crate::browser::session) fn handoff_route(
        &self,
        original: Reference,
        target: &Target,
        overrides: &routing::Overrides,
    ) -> Result<Reference, Error> {
        let context = target.context;
        let mut state = self.state.lock().expect("Browser owner");
        let resource = state.resource(original, Some(Kind::Route))?.clone();
        let target_kind = state.resource(context, None)?.kind;
        if !matches!(target_kind, Kind::Page | Kind::Context)
            || (target_kind == Kind::Context
                && state.ancestor(original, Kind::Context) != Some(context))
            || (target_kind == Kind::Page && state.ancestor(original, Kind::Page) != Some(context))
        {
            return Err("FOREIGN_CALLBACK".into());
        }
        let page = state.ancestor(original, Kind::Page).unwrap_or(context);
        let mut callbacks = self.callbacks.lock().expect("Callback owner");
        let expiry = callbacks
            .get(&original)
            .copied()
            .filter(|expiry| *expiry > Instant::now())
            .ok_or("CALLBACK_EXPIRED")?;
        let responding = self.responding.lock().expect("Callback response owner");
        if responding.contains(&original) {
            return Err("CALLBACK_ALREADY_RESOLVING".into());
        }
        let updating = self.routing.updating.lock().expect("Route updates");
        if updating.contains(&context) {
            return Err("ROUTE_POLICY_PENDING".into());
        }
        let mut rules = self.routing.rules.lock().expect("Route owner");
        if target.same_scope && !rules.handlers.has_next(original) {
            return Err("ROUTE_POLICY_CHANGED".into());
        }
        if rules
            .automatic(page)
            .is_some_and(|rule| rule.phase != routing::HarPhase::Active)
        {
            return Err("ROUTE_POLICY_PENDING".into());
        }
        let urls = self.routing.manual_urls.lock().expect("Route patterns");
        let current = if let Some(expected) = target.rule {
            rules.automatic(context).is_some_and(|rule| {
                rule.id == expected.id && rule.phase == routing::HarPhase::Active
            })
        } else {
            rules.manual(context)
                && urls.contains_key(&context)
                && target.url.as_ref().is_some_and(|url| {
                    if rules.handlers.len(context) > 0 {
                        rules.handlers.matches(context, url)
                    } else {
                        urls.get(&context) == Some(url)
                    }
                })
        };
        if !current {
            return Err("ROUTE_POLICY_CHANGED".into());
        }
        let mut owners = self.routing.owners.lock().expect("Route owners");
        if owners.get(&original) != Some(&if target.same_scope { context } else { page }) {
            return Err("STALE_REFERENCE".into());
        }
        let mut handoffs = self.routing.handoffs.lock().expect("Route handoffs");
        handoffs.retain(|_, handoff| state.resource(handoff.target, Some(Kind::Route)).is_ok());
        state.retire(original);
        let next = state.issue(Kind::Route, resource.wire.clone(), resource.parent)?;
        self.routing
            .overrides
            .lock()
            .expect("Route overrides")
            .insert(resource.wire, overrides.clone());
        callbacks.remove(&original);
        callbacks.insert(next, expiry);
        owners.remove(&original);
        owners.insert(next, context);
        if target.same_scope {
            rules.handlers.advance(original, next);
        } else {
            rules.handlers.forget(original);
            if let Some(url) = &target.url {
                rules.handlers.admit(next, context, url);
            }
        }
        for handoff in handoffs.values_mut() {
            if handoff.target == original {
                handoff.target = next;
            }
        }
        handoffs.insert(
            original,
            Handoff {
                target: next,
                source: if target.same_scope { context } else { page },
            },
        );
        self.changed.notify_waiters();
        Ok(next)
    }
    pub(in crate::browser::session) fn fallback_dependency(&self, scope: Reference) -> bool {
        let state = self.state.lock().expect("Browser owner");
        self.routing
            .handoffs
            .lock()
            .expect("Route handoffs")
            .values()
            .any(|handoff| {
                handoff.source == scope && state.resource(handoff.target, Some(Kind::Route)).is_ok()
            })
    }
}
