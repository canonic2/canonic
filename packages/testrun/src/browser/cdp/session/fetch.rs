//! Desired page interception and acknowledged per-renderer Fetch configuration.
use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Configuration {
    patterns: Vec<String>,
    authentication: bool,
}
impl Configuration {
    pub(super) fn manual_patterns(patterns: Vec<String>, authentication: bool) -> Self {
        Self {
            patterns,
            authentication,
        }
    }
    pub(super) fn manual(url: String, authentication: bool) -> Self {
        Self {
            patterns: vec![url],
            authentication,
        }
    }
    pub(super) fn har() -> Self {
        Self {
            patterns: vec!["http://*".into(), "https://*".into()],
            authentication: false,
        }
    }
}
#[derive(Default)]
struct Attachment {
    acknowledged: Option<Configuration>,
    pending: Option<Pending>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    Enable,
    Disable,
}
#[derive(Default)]
pub(super) struct Policies {
    desired: BTreeMap<Reference, Configuration>,
    attachments: BTreeMap<String, Attachment>,
    updating: BTreeSet<Reference>,
    pub(super) authentication: BTreeMap<(String, String), super::authentication::Attempt>,
    pub(super) authentication_lost: bool,
}
impl Worker {
    pub(super) fn context_fetch_fallback(
        &self,
        page: Reference,
        current: Reference,
        url: Option<&str>,
    ) -> Result<Option<Reference>, Error> {
        if current != page {
            return Ok(None);
        }
        if self
            .routing_rules
            .automatic(page)
            .is_some_and(|rule| rule.phase != crate::browser::routing::HarPhase::Active)
        {
            return Err("ROUTE_POLICY_PENDING".into());
        }
        let route = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?;
        let (context, _, _) = self
            .index
            .diagnostic_scope(&route.session_id)
            .ok_or("INVALID_SCOPE")?;
        let Some(configuration) = self.fetch.desired.get(&context) else {
            return Ok(None);
        };
        if self.fetch.updating.contains(&context) {
            return Err("ROUTE_POLICY_PENDING".into());
        }
        let url = url.ok_or("ROUTE_URL_BOUNDS")?;
        if self.routing_rules.handlers.len(context) > 0 {
            return Ok(self
                .routing_rules
                .handlers
                .matches(context, url)
                .then_some(context));
        }
        if self.routing_rules.automatic(context).is_some() {
            return Ok(self
                .routing_rules
                .har_matches(context, url)
                .then_some(context));
        }
        Ok(configuration
            .patterns
            .iter()
            .any(|pattern| {
                pattern == url
                    || (pattern == "http://*" && url.starts_with("http://"))
                    || (pattern == "https://*" && url.starts_with("https://"))
            })
            .then_some(context))
    }
    fn effective_fetch(
        &self,
        context: Reference,
        page: Option<Reference>,
    ) -> Option<Configuration> {
        let mut configuration = self.fetch.desired.get(&context).cloned();
        if let Some(page) = page.and_then(|page| self.fetch.desired.get(&page)) {
            if let Some(configuration) = &mut configuration {
                configuration.patterns.extend(page.patterns.iter().cloned());
                configuration.patterns.sort();
                configuration.patterns.dedup();
                configuration.authentication |= page.authentication;
            } else {
                configuration = Some(page.clone());
            }
        }
        if self.automatic_authentication(context) {
            let configuration =
                configuration.get_or_insert_with(|| Configuration::manual("*".into(), true));
            configuration.authentication = true;
            configuration.patterns = vec!["*".into()];
        }
        configuration
    }
    pub(super) fn logical_route_filter(&self, context: Reference, page: Option<Reference>) -> bool {
        page.into_iter().chain(Some(context)).any(|scope| {
            if self
                .routing_rules
                .automatic(scope)
                .is_some_and(|rule| rule.url_filter)
            {
                return true;
            }
            (self.fetch.desired.contains_key(&scope)
                || !self.routing_rules.handlers.exhausted(scope).is_empty())
                && self.routing_rules.handlers.requires_filtering(scope)
        })
    }
    /// Whether this attachment's interception is being or has been turned off.
    /// A request it pauses then belongs to no policy: a teardown race.
    pub(super) fn fetch_released(&self, session: &str) -> bool {
        self.fetch.attachments.get(session).is_some_and(|target| {
            target.pending == Some(Pending::Disable)
                || target.pending.is_none() && target.acknowledged.is_none()
        })
    }
    pub(super) fn fetch_owner(
        &self,
        context: Reference,
        page: Option<Reference>,
        url: &str,
        authentication: bool,
    ) -> Option<Reference> {
        page.into_iter().chain(Some(context)).find(|scope| {
            self.fetch.desired.get(scope).is_some_and(|configuration| {
                if authentication {
                    return configuration.authentication;
                }
                if self.routing_rules.handlers.len(*scope) > 0 {
                    return self.routing_rules.handlers.matches(*scope, url);
                }
                if self.routing_rules.automatic(*scope).is_some() {
                    return self.routing_rules.har_matches(*scope, url);
                }
                configuration.patterns.iter().any(|pattern| {
                    pattern == url
                        || (pattern == "http://*" && url.starts_with("http://"))
                        || (pattern == "https://*" && url.starts_with("https://"))
                })
            })
        })
    }
    fn fence_fetch(&mut self) {
        self.fetch.desired.retain(|scope, _| {
            self.index.retained(*scope, Kind::Page) || self.index.retained(*scope, Kind::Context)
        });
        self.fetch
            .attachments
            .retain(|session, _| self.index.retained_attachment(session));
        self.fetch
            .authentication
            .retain(|(session, _), _| self.index.retained_attachment(session));
    }
    pub(super) async fn refresh_context_fetch(
        &mut self,
        id: Uuid,
        context: Reference,
        deadline: Instant,
    ) -> Result<(), Error> {
        let configuration = self.fetch.desired.get(&context).cloned();
        self.set_scoped_fetch(id, context, Kind::Context, configuration, deadline)
            .await
    }
    pub(super) fn context_fetch_pending(&self, context: Reference) -> bool {
        self.fetch.attachments.iter().any(|(session, attachment)| {
            attachment.pending.is_some()
                && self
                    .index
                    .diagnostic_scope(session)
                    .is_some_and(|(owner, _, _)| owner == context)
        })
    }
    pub(super) async fn set_scoped_fetch(
        &mut self,
        id: Uuid,
        scope: Reference,
        kind: Kind,
        configuration: Option<Configuration>,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.fence_fetch();
        self.index.native(scope, kind).map_err(resource_error)?;
        let sessions = self
            .index
            .targets()
            .into_iter()
            .filter_map(|(_, _, _, session)| session)
            .filter(|session| {
                self.route_scope(session).is_some_and(|(context, owner)| {
                    if kind == Kind::Context {
                        context == scope
                    } else {
                        owner.page() == Some(scope)
                    }
                })
            })
            .collect::<BTreeSet<_>>();
        if sessions.iter().any(|session| {
            self.fetch.attachments.get(session).is_some_and(|target| {
                target.pending.is_some_and(|pending| {
                    configuration.is_some()
                        || pending == Pending::Disable
                        || self
                            .index
                            .diagnostic_scope(session)
                            .is_some_and(|(context, _, _)| self.automatic_authentication(context))
                })
            })
        }) {
            return Err("INTERCEPTION_OUTCOME_UNKNOWN".into());
        }
        if let Some(configuration) = configuration {
            self.fetch.desired.insert(scope, configuration);
        } else {
            // Stop installation on future renderers before releasing existing ones.
            self.fetch.desired.remove(&scope);
        }
        self.fetch.updating.insert(scope);
        let result = async {
            for session in sessions {
                self.apply_fetch(id, &session, deadline).await?;
                self.index.native(scope, kind).map_err(resource_error)?;
            }
            Ok(())
        }
        .await;
        self.fetch.updating.remove(&scope);
        result
    }
    pub(super) async fn configure_fetch(
        &mut self,
        id: Uuid,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.fence_fetch();
        let Some((context, owner)) = self.route_scope(session) else {
            return Ok(());
        };
        let page = owner.page();
        let desired = self.effective_fetch(context, page);
        if desired.is_some()
            && !page.is_some_and(|page| self.fetch.updating.contains(&page))
            && !self.fetch.updating.contains(&context)
            && (page.into_iter().chain(Some(context)).any(|scope| {
                self.routing_rules
                    .automatic(scope)
                    .is_some_and(|rule| rule.phase != crate::browser::routing::HarPhase::Active)
            }) || self.fetch.attachments.iter().any(|(owner, target)| {
                self.route_scope(owner)
                    .is_some_and(|(owner_context, route_owner)| {
                        let owner_page = route_owner.page();
                        ((owner_page == page && page.is_some())
                            || (owner_context == context
                                && self.fetch.desired.contains_key(&context)))
                            && (target.pending.is_some()
                                || target.acknowledged
                                    != self.effective_fetch(owner_context, owner_page))
                    })
            }))
        {
            return Err("INTERCEPTION_SETUP_PENDING".into());
        }
        self.apply_fetch(id, session, deadline).await
    }
    async fn apply_fetch(
        &mut self,
        id: Uuid,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.fence_fetch();
        let Some((context, owner)) = self.route_scope(session) else {
            return Ok(());
        };
        let desired = self.effective_fetch(context, owner.page());
        let previous_pending = self
            .fetch
            .attachments
            .get(session)
            .and_then(|target| target.pending);
        if previous_pending.is_some_and(|pending| desired.is_some() || pending == Pending::Disable)
        {
            return Err("INTERCEPTION_OUTCOME_UNKNOWN".into());
        }
        if self
            .fetch
            .attachments
            .get(session)
            .and_then(|target| target.acknowledged.as_ref())
            == desired.as_ref()
            && previous_pending.is_none()
        {
            return Ok(());
        }
        if !self.fetch.attachments.contains_key(session) && self.fetch.attachments.len() >= 512 {
            return Err("INTERCEPTION_TARGET_CAPACITY".into());
        }
        let (method, params) = if let Some(configuration) = &desired {
            (
                "Fetch.enable",
                json!({"patterns":configuration.patterns.iter().map(|url|json!({"urlPattern":url.replace('?', "\\?"),"requestStage":"Request"})).collect::<Vec<_>>(),"handleAuthRequests":configuration.authentication}),
            )
        } else {
            ("Fetch.disable", json!({}))
        };
        self.fetch
            .attachments
            .entry(session.into())
            .or_default()
            .pending = Some(if desired.is_some() {
            Pending::Enable
        } else {
            Pending::Disable
        });
        let result = async {
            if desired
                .as_ref()
                .is_some_and(|configuration| configuration.authentication)
                && self.automatic_authentication(context)
            {
                self.call(id, Some(session), "Network.enable", json!({}), deadline)
                    .await?;
            }
            self.call(id, Some(session), method, params, deadline).await
        }
        .await;
        if !self.index.retained_attachment(session) {
            self.fetch.attachments.remove(session);
            return Ok(());
        }
        let target = self
            .fetch
            .attachments
            .get_mut(session)
            .ok_or("STALE_REFERENCE")?;
        match result {
            Ok(_) => {
                target.pending = None;
                target.acknowledged = desired;
                Ok(())
            }
            Err(mut error) => {
                if !matches!(
                    error.code,
                    "OUTCOME_UNKNOWN" | "BACKEND_LOST" | "BACKEND_PROTOCOL"
                ) {
                    target.pending = previous_pending;
                }
                error.partial = Some(
                    json!({"page":owner.page(),"worker":owner.worker(),"context":context,"interceptionSetupPending":true,"nextAction":if owner.page().is_some(){"page_close"}else{"context_close"},"replayed":false}),
                );
                Err(error)
            }
        }
    }
}
