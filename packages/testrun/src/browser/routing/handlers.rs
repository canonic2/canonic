//! Ordered manual handlers; a callback's remaining chain is fixed at admission.
use super::*;
use crate::browser::contract::Reference;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HandlerAction {
    Add {
        url: String,
        #[serde(default)]
        times: Option<u32>,
    },
    AddMatcher {
        matcher: Matcher,
        #[serde(default)]
        times: Option<u32>,
    },
    Remove {
        #[serde(rename = "handlerId")]
        handler_id: Uuid,
    },
    RemoveMatcher {
        matcher: Matcher,
    },
    /// Pinned `routeFromHAR` as one handler in this scope's ordered chain.
    AddHar {
        #[serde(rename = "archiveId")]
        archive_id: Uuid,
        #[serde(default, rename = "urlFilter")]
        url_filter: Option<Matcher>,
        #[serde(default, rename = "notFound")]
        not_found: crate::browser::har::NotFound,
        #[serde(default)]
        times: Option<u32>,
    },
    List {},
    Clear {
        /// Pinned `unrouteAll({behavior: "wait"})`. Without it, clearing while
        /// this scope owns paused callbacks refuses.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        behavior: Option<ClearBehavior>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClearBehavior {
    /// Wait until the scope's paused callbacks are answered, then clear.
    Wait,
    /// Pinned default: clear now; paused requests continue to the network.
    Default,
    /// As `Default`; agent callbacks have no handler errors to suppress.
    IgnoreErrors,
}
/// The archive reply a HAR handler gives when a request's chain reaches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HarHandler {
    pub id: Uuid,
    pub archive_id: Uuid,
    pub not_found: crate::browser::har::NotFound,
}
impl HandlerAction {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Self::Add { url, .. } = self {
            validate_url(url)?;
        }
        if let Self::AddMatcher { matcher, .. } | Self::RemoveMatcher { matcher } = self {
            matcher.compile()?;
        }
        if let Self::AddHar {
            url_filter: Some(matcher),
            ..
        } = self
        {
            matcher.compile()?;
        }
        Ok(())
    }
    pub(crate) fn matcher(&self) -> Option<Matcher> {
        match self {
            Self::Add { url, .. } => Some(Matcher::Exact { url: url.clone() }),
            Self::AddMatcher { matcher, .. } => Some(matcher.clone()),
            // Without a filter a HAR handler sees every HTTP(S) request in scope.
            Self::AddHar { url_filter, .. } => Some(url_filter.clone().unwrap_or(Matcher::Glob {
                pattern: "**".into(),
            })),
            _ => None,
        }
    }
    /// Whether the action waits for paused callbacks instead of refusing.
    pub(crate) fn waits(&self) -> bool {
        matches!(
            self,
            Self::Clear {
                behavior: Some(ClearBehavior::Wait)
            }
        )
    }
    /// Pinned `unroute`/`unrouteAll` without waiting: proceed while callbacks
    /// are paused instead of refusing.
    pub(crate) fn releases(&self) -> bool {
        matches!(
            self,
            Self::Clear {
                behavior: Some(ClearBehavior::Default | ClearBehavior::IgnoreErrors)
            } | Self::Remove { .. }
                | Self::RemoveMatcher { .. }
        )
    }
    pub(crate) fn times(&self) -> Option<u32> {
        match self {
            Self::Add { times, .. }
            | Self::AddMatcher { times, .. }
            | Self::AddHar { times, .. } => *times,
            _ => None,
        }
    }
    /// The archive and miss policy of an `add_har` action.
    pub(crate) fn har(&self) -> Option<(Uuid, crate::browser::har::NotFound)> {
        match self {
            Self::AddHar {
                archive_id,
                not_found,
                ..
            } => Some((*archive_id, *not_found)),
            _ => None,
        }
    }
}
struct Handler {
    id: Uuid,
    matcher: Matcher,
    compiled: super::matcher::Compiled,
    remaining: Option<u32>,
    har: Option<(Uuid, crate::browser::har::NotFound)>,
}
#[derive(Clone)]
struct Chain {
    scope: Reference,
    ids: Vec<Uuid>,
    position: usize,
}
#[derive(Default)]
pub(crate) struct Handlers {
    scopes: BTreeMap<Reference, Vec<Handler>>,
    calls: BTreeMap<Reference, Chain>,
    pending: BTreeSet<Reference>,
    planned: BTreeMap<Reference, Vec<Matcher>>,
}
impl Handlers {
    pub(crate) fn url(&self, scope: Reference) -> Option<&str> {
        self.scopes
            .get(&scope)?
            .first()
            .map(|handler| handler.matcher.original())
    }
    pub(crate) fn capacity(&self, scope: Reference) -> Result<(), &'static str> {
        if self.scopes.values().map(Vec::len).sum::<usize>() >= 256
            || self
                .scopes
                .get(&scope)
                .is_some_and(|items| items.len() >= 32)
        {
            return Err("ROUTE_HANDLER_CAPACITY");
        }
        Ok(())
    }
    pub(crate) fn patterns(&self, scope: Reference) -> Vec<Matcher> {
        self.scopes
            .get(&scope)
            .into_iter()
            .flatten()
            .map(|handler| handler.matcher.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    #[cfg(test)]
    pub(crate) fn planned_urls(&self, scope: Reference) -> Vec<String> {
        self.planned_patterns(scope)
            .into_iter()
            .map(|matcher| matcher.original().to_owned())
            .collect()
    }
    pub(crate) fn planned_patterns(&self, scope: Reference) -> Vec<Matcher> {
        self.planned
            .get(&scope)
            .cloned()
            .unwrap_or_else(|| self.patterns(scope))
    }
    pub(crate) fn contains_matcher(&self, scope: Reference, matcher: &Matcher) -> bool {
        self.scopes
            .get(&scope)
            .is_some_and(|items| items.iter().any(|item| &item.matcher == matcher))
    }
    pub(crate) fn requires_filtering(&self, scope: Reference) -> bool {
        self.scopes.get(&scope).is_some_and(|items| {
            items.iter().any(|item| {
                !matches!(item.matcher, Matcher::Exact { .. }) || item.remaining.is_some()
            })
        })
    }
    pub(crate) fn matches(&self, scope: Reference, url: &str) -> bool {
        self.scopes.get(&scope).is_some_and(|handlers| {
            handlers
                .iter()
                .any(|handler| handler.remaining != Some(0) && handler.compiled.is_match(url))
        })
    }
    #[cfg(test)]
    pub(crate) fn plan_remove(&mut self, scope: Reference, id: Uuid) -> bool {
        self.plan_remove_ids(scope, &[id])
    }
    pub(crate) fn matching_ids(&self, scope: Reference, matcher: &Matcher) -> Vec<Uuid> {
        self.scopes
            .get(&scope)
            .into_iter()
            .flatten()
            .filter(|handler| &handler.matcher == matcher)
            .map(|handler| handler.id)
            .collect()
    }
    pub(crate) fn removal_ids(
        &self,
        scope: Reference,
        action: &HandlerAction,
    ) -> Result<Vec<Uuid>, &'static str> {
        match action {
            HandlerAction::Remove { handler_id } => {
                if !self.contains(scope, *handler_id) {
                    return Err("UNKNOWN_ROUTE_HANDLER");
                }
                Ok(vec![*handler_id])
            }
            HandlerAction::RemoveMatcher { matcher } => Ok(self.matching_ids(scope, matcher)),
            _ => unreachable!("Removal action"),
        }
    }
    pub(crate) fn plan_remove_ids(&mut self, scope: Reference, ids: &[Uuid]) -> bool {
        let urls = self
            .scopes
            .get(&scope)
            .into_iter()
            .flatten()
            .filter(|handler| !ids.contains(&handler.id))
            .map(|handler| handler.matcher.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if urls == self.patterns(scope) {
            return false;
        }
        self.pending.insert(scope);
        self.planned.insert(scope, urls);
        true
    }
    #[cfg(test)]
    pub(crate) fn add(&mut self, scope: Reference, url: String) -> Result<Uuid, &'static str> {
        self.add_matcher(scope, Matcher::Exact { url })
    }
    #[cfg(test)]
    pub(crate) fn add_matcher(
        &mut self,
        scope: Reference,
        matcher: Matcher,
    ) -> Result<Uuid, &'static str> {
        self.add_limited(scope, matcher, None)
    }
    pub(crate) fn add_limited(
        &mut self,
        scope: Reference,
        matcher: Matcher,
        times: Option<u32>,
    ) -> Result<Uuid, &'static str> {
        if self.pending(scope) {
            return Err("ROUTE_POLICY_PENDING");
        }
        self.capacity(scope)?;
        let compiled = matcher.compile()?;
        let id = Uuid::new_v4();
        self.scopes.entry(scope).or_default().push(Handler {
            id,
            matcher,
            compiled,
            remaining: times.map(|times| times.max(1)),
            har: None,
        });
        Ok(id)
    }
    /// Make a just-added handler reply from an archive instead of a callback.
    pub(crate) fn set_har(
        &mut self,
        scope: Reference,
        id: Uuid,
        har: (Uuid, crate::browser::har::NotFound),
    ) {
        if let Some(handler) = self
            .scopes
            .get_mut(&scope)
            .into_iter()
            .flatten()
            .find(|handler| handler.id == id)
        {
            handler.har = Some(har);
        }
    }
    /// The HAR reply owed by a callback whose current handler is a HAR handler.
    pub(crate) fn current_har(&self, callback: Reference) -> Option<HarHandler> {
        let chain = self.calls.get(&callback)?;
        let id = *chain.ids.get(chain.position)?;
        let (archive_id, not_found) = self
            .scopes
            .get(&chain.scope)?
            .iter()
            .find(|handler| handler.id == id)?
            .har?;
        Some(HarHandler {
            id,
            archive_id,
            not_found,
        })
    }
    /// Whether any registered handler replies from the archive.
    pub(crate) fn uses_archive(&self, archive_id: Uuid) -> bool {
        self.scopes
            .values()
            .flatten()
            .any(|handler| handler.har.is_some_and(|(id, _)| id == archive_id))
    }
    #[cfg(test)]
    pub(crate) fn begin(&mut self, scope: Reference, url: String) -> Result<Uuid, &'static str> {
        self.begin_matcher(scope, Matcher::Exact { url })
    }
    #[cfg(test)]
    pub(crate) fn begin_matcher(
        &mut self,
        scope: Reference,
        matcher: Matcher,
    ) -> Result<Uuid, &'static str> {
        self.begin_limited(scope, matcher, None)
    }
    pub(crate) fn begin_limited(
        &mut self,
        scope: Reference,
        matcher: Matcher,
        times: Option<u32>,
    ) -> Result<Uuid, &'static str> {
        let id = self.add_limited(scope, matcher, times)?;
        self.pending.insert(scope);
        Ok(id)
    }
    pub(crate) fn pending(&self, scope: Reference) -> bool {
        self.pending.contains(&scope)
    }
    pub(crate) fn activate(&mut self, scope: Reference) {
        self.pending.remove(&scope);
        self.planned.remove(&scope);
    }
    pub(crate) fn setup_failed(&mut self, scope: Reference) {
        if self.len(scope) > 0 {
            self.pending.insert(scope);
        }
    }
    pub(crate) fn contains(&self, scope: Reference, id: Uuid) -> bool {
        self.scopes
            .get(&scope)
            .is_some_and(|items| items.iter().any(|item| item.id == id))
    }
    pub(crate) fn len(&self, scope: Reference) -> usize {
        self.scopes.get(&scope).map_or(0, Vec::len)
    }
    pub(crate) fn remove(&mut self, scope: Reference, id: Uuid) {
        if let Some(items) = self.scopes.get_mut(&scope) {
            items.retain(|item| item.id != id);
        }
        self.scopes.retain(|_, items| !items.is_empty());
        if self.len(scope) == 0 {
            self.pending.remove(&scope);
            self.planned.remove(&scope);
        }
    }
    pub(crate) fn clear(&mut self, scope: Reference) {
        self.scopes.remove(&scope);
        self.pending.remove(&scope);
        self.planned.remove(&scope);
    }
    pub(crate) fn list(&self, scope: Reference) -> Value {
        json!(self.scopes.get(&scope).into_iter().flatten().rev().map(|item| json!({"handlerId":item.id,"kind":if item.har.is_some(){"har"}else{"callback"},"archiveId":item.har.map(|(id,_)|id),"notFound":item.har.map(|(_,policy)|policy),"url":crate::browser::diagnostics::url(item.matcher.original()),"matcherKind":match item.matcher{Matcher::Exact{..}=>"exact",Matcher::Glob{..}=>"glob",Matcher::Regex{..}=>"regex",Matcher::UrlPattern(_)=>"url_pattern"},"remainingUses":item.remaining,"exhausted":item.remaining==Some(0),"setupPending":self.pending(scope)})).collect::<Vec<_>>())
    }
    pub(crate) fn admit(&mut self, callback: Reference, scope: Reference, url: &str) {
        if self.calls.contains_key(&callback) {
            return;
        }
        let ids = self
            .scopes
            .get(&scope)
            .into_iter()
            .flatten()
            .rev()
            .filter(|item| item.remaining != Some(0) && item.compiled.is_match(url))
            .map(|item| item.id)
            .collect::<Vec<_>>();
        if !ids.is_empty() {
            self.consume(scope, ids[0]);
            self.calls.insert(
                callback,
                Chain {
                    scope,
                    ids,
                    position: 0,
                },
            );
        }
    }
    pub(crate) fn current(&self, callback: Reference) -> Option<Uuid> {
        let chain = self.calls.get(&callback)?;
        chain.ids.get(chain.position).copied()
    }
    pub(crate) fn has_next(&self, callback: Reference) -> bool {
        self.calls.get(&callback).is_some_and(|chain| {
            chain
                .ids
                .iter()
                .skip(chain.position + 1)
                .any(|id| self.eligible(chain.scope, *id))
        })
    }
    pub(crate) fn advance(&mut self, original: Reference, next: Reference) {
        if let Some(mut chain) = self.calls.remove(&original) {
            chain.position += 1;
            while chain.position < chain.ids.len()
                && !self.eligible(chain.scope, chain.ids[chain.position])
            {
                chain.position += 1;
            }
            if chain.position < chain.ids.len() {
                self.consume(chain.scope, chain.ids[chain.position]);
                self.calls.insert(next, chain);
            }
        }
    }
    fn eligible(&self, scope: Reference, id: Uuid) -> bool {
        self.scopes
            .get(&scope)
            .into_iter()
            .flatten()
            .find(|handler| handler.id == id)
            .is_none_or(|handler| handler.remaining != Some(0))
    }
    fn consume(&mut self, scope: Reference, id: Uuid) {
        if let Some(handler) = self
            .scopes
            .get_mut(&scope)
            .into_iter()
            .flatten()
            .find(|handler| handler.id == id)
            && let Some(remaining) = &mut handler.remaining
        {
            *remaining = remaining.saturating_sub(1);
        }
    }
    /// Pinned `unroute` while other handlers remain: the removed handlers stop
    /// matching now and paused chains skip them; owned cleanup removes them.
    pub(crate) fn exhaust(&mut self, scope: Reference, ids: &[Uuid]) {
        for handler in self.scopes.get_mut(&scope).into_iter().flatten() {
            if ids.contains(&handler.id) {
                handler.remaining = Some(0);
            }
        }
    }
    /// Handler identities a `clear`/`remove` action selects in this scope.
    pub(crate) fn release_ids(
        &self,
        scope: Reference,
        action: &HandlerAction,
    ) -> Result<Vec<Uuid>, &'static str> {
        if matches!(action, HandlerAction::Clear { .. }) {
            return Ok(self
                .scopes
                .get(&scope)
                .into_iter()
                .flatten()
                .map(|handler| handler.id)
                .collect());
        }
        self.removal_ids(scope, action)
    }
    pub(crate) fn exhausted(&self, scope: Reference) -> Vec<Uuid> {
        self.scopes
            .get(&scope)
            .into_iter()
            .flatten()
            .filter(|handler| handler.remaining == Some(0))
            .map(|handler| handler.id)
            .collect()
    }
    pub(crate) fn forget(&mut self, callback: Reference) {
        self.calls.remove(&callback);
    }
    pub(crate) fn retain(&mut self, mut live: impl FnMut(Reference) -> bool) {
        self.scopes.retain(|scope, _| live(*scope));
        self.pending.retain(|scope| live(*scope));
        self.planned.retain(|scope, _| live(*scope));
        self.calls.retain(|_, chain| live(chain.scope));
    }
    pub(crate) fn retain_callbacks(&mut self, mut live: impl FnMut(Reference) -> bool) {
        self.calls.retain(|callback, _| live(*callback));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn har_handlers_interleave_with_callbacks_in_registration_order() {
        use crate::browser::har::NotFound;
        let scope = reference();
        let archive = Uuid::new_v4();
        let mut handlers = Handlers::default();
        let action: HandlerAction = serde_json::from_value(
            json!({"kind":"add_har","archiveId":archive,"notFound":"fallback"}),
        )
        .unwrap();
        action.validate().unwrap();
        assert_eq!(action.har(), Some((archive, NotFound::Fallback)));
        let oldest = handlers
            .add_matcher(
                scope,
                Matcher::Glob {
                    pattern: "**/acme/*".into(),
                },
            )
            .unwrap();
        let har = handlers
            .add_limited(scope, action.matcher().unwrap(), action.times())
            .unwrap();
        handlers.set_har(scope, har, action.har().unwrap());
        let newest = handlers
            .add_matcher(
                scope,
                Matcher::Exact {
                    url: "https://example.com/acme/new".into(),
                },
            )
            .unwrap();
        assert!(handlers.uses_archive(archive));
        let listed = handlers.list(scope);
        assert_eq!(listed[1]["kind"], "har");
        assert_eq!(listed[1]["archiveId"], json!(archive));
        assert_eq!(listed[0]["kind"], "callback");
        let first = reference();
        handlers.admit(first, scope, "https://example.com/acme/new");
        assert_eq!(handlers.current(first), Some(newest));
        assert!(handlers.current_har(first).is_none());
        let second = reference();
        handlers.advance(first, second);
        assert_eq!(
            handlers.current_har(second),
            Some(HarHandler {
                id: har,
                archive_id: archive,
                not_found: NotFound::Fallback,
            })
        );
        let third = reference();
        handlers.advance(second, third);
        assert_eq!(handlers.current(third), Some(oldest));
        let other = reference();
        handlers.admit(other, scope, "https://example.com/elsewhere");
        assert_eq!(
            handlers.current_har(other).map(|handler| handler.id),
            Some(har)
        );
        handlers.remove(scope, har);
        assert!(!handlers.uses_archive(archive));
        let invalid: HandlerAction = serde_json::from_value(json!({"kind":"add_har","archiveId":archive,"urlFilter":{"kind":"regex","pattern":"(?<=Acme"}})).unwrap();
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn regex_flags_removal_and_limits_keep_the_original_handler_identity() {
        let scope = reference();
        let mut handlers = Handlers::default();
        let insensitive = Matcher::Regex {
            pattern: "/Acme\\?token=Private$".into(),
            flags: "i".into(),
        };
        let sensitive = Matcher::Regex {
            pattern: "/Acme\\?token=Private$".into(),
            flags: String::new(),
        };
        let old = handlers.add_matcher(scope, sensitive.clone()).unwrap();
        let limited = handlers
            .add_limited(scope, insensitive.clone(), Some(1))
            .unwrap();
        assert!(handlers.requires_filtering(scope));
        let first = reference();
        handlers.admit(first, scope, "https://example.com/acme?token=private");
        assert_eq!(handlers.current(first), Some(limited));
        assert_eq!(handlers.exhausted(scope), vec![limited]);
        let second = reference();
        handlers.admit(second, scope, "https://example.com/acme?token=private");
        assert_eq!(handlers.current(second), None);
        handlers.admit(second, scope, "https://example.com/Acme?token=Private");
        assert_eq!(handlers.current(second), Some(old));
        assert_eq!(handlers.matching_ids(scope, &insensitive), vec![limited]);
        assert_eq!(handlers.matching_ids(scope, &sensitive), vec![old]);
        let third = reference();
        handlers.admit(third, scope, "https://example.com/other?token=Private");
        assert_eq!(handlers.current(third), None);
    }
    #[test]
    fn repeated_admission_preserves_the_selected_handler_chain_and_remaining_budget() {
        let scope = reference();
        let callback = reference();
        let url = "https://example.com/acme";
        let mut handlers = Handlers::default();
        let old = handlers.add(scope, url.into()).unwrap();
        let limited = handlers
            .add_limited(scope, Matcher::Exact { url: url.into() }, Some(2))
            .unwrap();
        handlers.admit(callback, scope, url);
        handlers.admit(callback, scope, url);
        assert_eq!(handlers.current(callback), Some(limited));
        assert_eq!(handlers.list(scope)[0]["remainingUses"], 1);
        assert!(handlers.has_next(callback));
        let next = reference();
        handlers.advance(callback, next);
        assert_eq!(handlers.current(next), Some(old));
        handlers.admit(next, scope, url);
        assert_eq!(handlers.current(next), Some(old));
        assert_eq!(handlers.list(scope)[0]["remainingUses"], 1);
    }
    #[test]
    fn limits_count_invocations_not_candidate_chains_and_skip_concurrently_spent_handlers() {
        let scope = reference();
        let url = "https://example.com/acme";
        let mut handlers = Handlers::default();
        let unlimited = handlers.add(scope, url.into()).unwrap();
        let limited = handlers
            .add_limited(scope, Matcher::Exact { url: url.into() }, Some(1))
            .unwrap();
        let newest = handlers.add(scope, url.into()).unwrap();
        let first = reference();
        let second = reference();
        handlers.admit(first, scope, url);
        handlers.admit(second, scope, url);
        assert_eq!(handlers.current(first), Some(newest));
        assert_eq!(handlers.exhausted(scope), Vec::<Uuid>::new());
        let first_next = reference();
        handlers.advance(first, first_next);
        assert_eq!(handlers.current(first_next), Some(limited));
        assert_eq!(handlers.exhausted(scope), vec![limited]);
        let second_next = reference();
        handlers.advance(second, second_next);
        assert_eq!(handlers.current(second_next), Some(unlimited));
        assert!(!handlers.has_next(second_next));
        assert!(handlers.has_next(first_next));
        let last = reference();
        handlers.advance(first_next, last);
        assert_eq!(handlers.current(last), Some(unlimited));
        let fresh = reference();
        handlers.admit(fresh, scope, url);
        let next = reference();
        handlers.advance(fresh, next);
        assert_eq!(handlers.current(next), Some(unlimited));
    }
    #[test]
    fn zero_and_repeated_limits_expire_once_without_consuming_nonmatches() {
        for times in [0, 1, 2] {
            let scope = reference();
            let mut handlers = Handlers::default();
            let id = handlers
                .add_limited(
                    scope,
                    Matcher::Glob {
                        pattern: "**/acme".into(),
                    },
                    Some(times),
                )
                .unwrap();
            handlers.admit(reference(), scope, "https://example.com/other");
            assert!(handlers.exhausted(scope).is_empty());
            for _ in 0..times.max(1) {
                let callback = reference();
                handlers.admit(callback, scope, "https://example.com/acme");
                assert_eq!(handlers.current(callback), Some(id));
                handlers.forget(callback);
            }
            assert!(!handlers.matches(scope, "https://example.com/acme"));
            assert!(handlers.requires_filtering(scope));
            let callback = reference();
            handlers.admit(callback, scope, "https://example.com/acme");
            assert_eq!(handlers.current(callback), None);
            assert_eq!(handlers.exhausted(scope), vec![id]);
            handlers.remove(scope, id);
            assert_eq!(handlers.len(scope), 0);
        }
    }
    #[test]
    fn matcher_removal_selects_all_equal_registrations_without_retiring_before_ack() {
        let scope = reference();
        let sibling = reference();
        let mut handlers = Handlers::default();
        let exact = Matcher::Exact {
            url: "https://example.com/acme".into(),
        };
        let glob = Matcher::Glob {
            pattern: "https://example.com/acme".into(),
        };
        let first = handlers.add_matcher(scope, glob.clone()).unwrap();
        let second = handlers.add_matcher(scope, glob.clone()).unwrap();
        let retained = handlers.add_matcher(scope, exact.clone()).unwrap();
        let other = handlers.add_matcher(sibling, glob.clone()).unwrap();
        let action = HandlerAction::RemoveMatcher {
            matcher: glob.clone(),
        };
        let ids = handlers.removal_ids(scope, &action).unwrap();
        assert_eq!(ids, vec![first, second]);
        assert!(handlers.plan_remove_ids(scope, &ids));
        assert_eq!(handlers.planned_patterns(scope), vec![exact]);
        assert!(handlers.contains(scope, first));
        assert!(handlers.contains(scope, second));
        handlers.setup_failed(scope);
        assert!(handlers.pending(scope));
        assert_eq!(handlers.matching_ids(scope, &glob), ids);
        for id in ids {
            handlers.remove(scope, id);
        }
        handlers.activate(scope);
        assert!(handlers.contains(scope, retained));
        assert!(handlers.contains(sibling, other));
        assert!(handlers.removal_ids(scope, &action).unwrap().is_empty());
        assert_eq!(
            handlers
                .removal_ids(scope, &HandlerAction::Remove { handler_id: first })
                .unwrap_err(),
            "UNKNOWN_ROUTE_HANDLER"
        );
    }
    #[test]
    fn typed_glob_removal_preserves_exact_handlers_and_admitted_fallback_chains() {
        let scope = reference();
        let callback = reference();
        let next = reference();
        let mut handlers = Handlers::default();
        let glob = Matcher::Glob {
            pattern: "https://example.com/acme".into(),
        };
        let broad = handlers.add_matcher(scope, glob.clone()).unwrap();
        let exact = handlers
            .add(scope, "https://example.com/acme".into())
            .unwrap();
        assert!(handlers.contains_matcher(scope, &glob));
        assert_eq!(handlers.patterns(scope).len(), 2);
        handlers.admit(callback, scope, "https://example.com/acme");
        assert_eq!(handlers.current(callback), Some(exact));
        assert!(handlers.plan_remove(scope, broad));
        assert_eq!(
            handlers.planned_patterns(scope),
            vec![Matcher::Exact {
                url: "https://example.com/acme".into()
            }]
        );
        assert!(handlers.contains(scope, broad));
        handlers.remove(scope, broad);
        handlers.activate(scope);
        handlers.advance(callback, next);
        assert_eq!(handlers.current(next), Some(broad));
        assert!(handlers.matches(scope, "https://example.com/acme"));
        assert!(!handlers.matches(scope, "https://example.com/other"));
    }
    #[test]
    fn handler_setup_is_owned_before_ack_and_failure_requires_cleanup() {
        let scope = reference();
        let other = reference();
        let callback = reference();
        let mut handlers = Handlers::default();
        let id = handlers
            .begin(scope, "https://example.com/acme".into())
            .unwrap();
        handlers.admit(callback, scope, "https://example.com/acme");
        assert_eq!(handlers.current(callback), Some(id));
        assert!(handlers.pending(scope));
        assert_eq!(
            handlers
                .add(scope, "https://example.com/acme".into())
                .unwrap_err(),
            "ROUTE_POLICY_PENDING"
        );
        handlers.activate(scope);
        let other_id = handlers
            .add(scope, "https://example.com/other".into())
            .unwrap();
        assert!(handlers.plan_remove(scope, other_id));
        assert!(handlers.contains(scope, other_id));
        assert_eq!(
            handlers.planned_urls(scope),
            vec!["https://example.com/acme"]
        );
        assert!(!handlers.contains(other, id));
        handlers.setup_failed(scope);
        assert_eq!(handlers.list(scope)[0]["setupPending"], true);
        handlers.forget(callback);
        handlers.clear(scope);
        assert!(!handlers.pending(scope));
        assert_eq!(handlers.len(scope), 0);
        assert!(
            handlers
                .add(scope, "https://example.com/other".into())
                .is_ok()
        );
    }
    #[test]
    fn handler_capacity_is_bounded_per_scope_and_connection() {
        let mut handlers = Handlers::default();
        for _ in 0..8 {
            let scope = reference();
            for _ in 0..32 {
                handlers
                    .add(scope, "https://example.com/acme".into())
                    .unwrap();
            }
            assert_eq!(
                handlers
                    .add(scope, "https://example.com/acme".into())
                    .unwrap_err(),
                "ROUTE_HANDLER_CAPACITY"
            );
        }
        assert_eq!(
            handlers
                .add(reference(), "https://example.com/acme".into())
                .unwrap_err(),
            "ROUTE_HANDLER_CAPACITY"
        );
    }
    fn reference() -> Reference {
        Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    #[test]
    fn newest_handler_runs_first_and_fallback_keeps_its_admitted_chain() {
        let scope = reference();
        let first = reference();
        let second = reference();
        let mut handlers = Handlers::default();
        let old = handlers
            .add(scope, "https://example.com/acme?item=1".into())
            .unwrap();
        let new = handlers
            .add(scope, "https://example.com/acme?item=1".into())
            .unwrap();
        handlers.admit(first, scope, "https://example.com/acme?item=1");
        assert_eq!(handlers.current(first), Some(new));
        handlers.remove(scope, old);
        handlers.advance(first, second);
        assert_eq!(handlers.current(first), None);
        assert_eq!(handlers.current(second), Some(old));
        assert!(!handlers.has_next(second));
        assert!(!handlers.list(scope).to_string().contains("item=1"));
        handlers.forget(second);
        assert_eq!(handlers.current(second), None);
    }
}
