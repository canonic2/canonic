//! Scoped interception owners retained across uncertain setup and teardown.
use crate::browser::{contract::Reference, har::NotFound};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HarPhase {
    Installing,
    Active,
    Stopping,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct AutoRule {
    pub id: Uuid,
    pub scope: Reference,
    scope_kind: ScopeKind,
    pub archive_id: Uuid,
    pub not_found: NotFound,
    pub phase: HarPhase,
    pub replies: u64,
    pub failures: u64,
    pub last_error: Option<&'static str>,
    pub url_filter: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScopeKind {
    Page,
    Context,
}
impl AutoRule {
    pub fn summary(self) -> Value {
        json!({"ruleId":self.id,"scope":self.scope,"scopeKind":self.scope_kind,"page":(self.scope_kind==ScopeKind::Page).then_some(self.scope),"context":(self.scope_kind==ScopeKind::Context).then_some(self.scope),"archiveId":self.archive_id,"notFound":self.not_found,"urlFilterActive":self.url_filter,"phase":self.phase,"replies":self.replies,"failures":self.failures,"lastError":self.last_error})
    }
}
#[derive(Default)]
pub(crate) struct Rules {
    pub(crate) handlers: super::Handlers,
    manual: BTreeSet<Reference>,
    automatic: BTreeMap<Reference, AutoRule>,
    filters: BTreeMap<Reference, super::Compiled>,
}
impl Rules {
    pub fn begin_filtered(
        &mut self,
        scope: Reference,
        kind: ScopeKind,
        archive_id: Uuid,
        not_found: NotFound,
        filter: Option<super::Matcher>,
    ) -> Result<AutoRule, &'static str> {
        let compiled = filter.as_ref().map(super::Matcher::compile).transpose()?;
        let mut rule = match kind {
            ScopeKind::Page => self.begin(scope, archive_id, not_found)?,
            ScopeKind::Context => self.begin_context(scope, archive_id, not_found)?,
        };
        if let Some(compiled) = compiled {
            self.filters.insert(scope, compiled);
            rule.url_filter = true;
            self.automatic.get_mut(&scope).expect("New rule").url_filter = true;
        }
        Ok(rule)
    }
    pub fn har_matches(&self, scope: Reference, url: &str) -> bool {
        self.automatic.contains_key(&scope)
            && self
                .filters
                .get(&scope)
                .is_none_or(|filter| filter.is_match(url))
    }
    pub fn manual(&self, page: Reference) -> bool {
        self.manual.contains(&page)
    }
    pub fn mark_manual(&mut self, page: Reference, enabled: bool) {
        if enabled {
            self.manual.insert(page);
        } else {
            self.manual.remove(&page);
        }
    }
    pub fn automatic(&self, page: Reference) -> Option<AutoRule> {
        self.automatic.get(&page).copied()
    }
    pub fn begin(
        &mut self,
        page: Reference,
        archive_id: Uuid,
        not_found: NotFound,
    ) -> Result<AutoRule, &'static str> {
        self.begin_scoped(page, ScopeKind::Page, archive_id, not_found)
    }
    pub fn begin_context(
        &mut self,
        context: Reference,
        archive_id: Uuid,
        not_found: NotFound,
    ) -> Result<AutoRule, &'static str> {
        self.begin_scoped(context, ScopeKind::Context, archive_id, not_found)
    }
    fn begin_scoped(
        &mut self,
        scope: Reference,
        scope_kind: ScopeKind,
        archive_id: Uuid,
        not_found: NotFound,
    ) -> Result<AutoRule, &'static str> {
        if self.manual(scope) {
            return Err("MANUAL_ROUTE_ACTIVE");
        }
        if self.automatic.contains_key(&scope) {
            return Err("HAR_ROUTE_ACTIVE");
        }
        if self.automatic.len() >= 64 {
            return Err("HAR_ROUTE_CAPACITY");
        }
        let rule = AutoRule {
            id: Uuid::new_v4(),
            scope,
            scope_kind,
            archive_id,
            not_found,
            phase: HarPhase::Installing,
            replies: 0,
            failures: 0,
            last_error: None,
            url_filter: false,
        };
        self.automatic.insert(scope, rule);
        Ok(rule)
    }
    pub fn confirm(&mut self, page: Reference, id: Uuid) -> Result<AutoRule, &'static str> {
        let rule = self
            .automatic
            .get_mut(&page)
            .filter(|rule| rule.id == id)
            .ok_or("HAR_ROUTE_LOST")?;
        rule.phase = HarPhase::Active;
        rule.last_error = None;
        Ok(*rule)
    }
    pub fn setup_failed(&mut self, page: Reference, id: Uuid, code: &'static str) {
        if self.automatic(page).is_none_or(|rule| rule.id != id) {
            return;
        }
        if code == "OUTCOME_UNKNOWN" {
            self.automatic
                .get_mut(&page)
                .expect("Rule owner")
                .last_error = Some(code);
        } else {
            self.automatic.remove(&page);
            self.filters.remove(&page);
        }
    }
    pub fn stopping(&mut self, page: Reference) -> Option<AutoRule> {
        let rule = self.automatic.get_mut(&page)?;
        let previous = *rule;
        rule.phase = HarPhase::Stopping;
        Some(previous)
    }
    pub fn stop_failed(&mut self, previous: AutoRule, code: &'static str) {
        if let Some(rule) = self
            .automatic
            .get_mut(&previous.scope)
            .filter(|rule| rule.id == previous.id)
        {
            rule.phase = HarPhase::Stopping;
            rule.last_error = Some(code);
        }
    }
    pub fn stopped(&mut self, page: Reference, id: Uuid) {
        if self.automatic(page).is_some_and(|rule| rule.id == id) {
            self.automatic.remove(&page);
            self.filters.remove(&page);
        }
    }
    pub fn used(&self, archive_id: Uuid) -> bool {
        self.automatic
            .values()
            .any(|rule| rule.archive_id == archive_id)
            || self.handlers.uses_archive(archive_id)
    }
    pub fn result(&mut self, rule: AutoRule, error: Option<&'static str>) {
        if let Some(current) = self
            .automatic
            .get_mut(&rule.scope)
            .filter(|current| current.id == rule.id)
        {
            if let Some(code) = error {
                current.failures = current.failures.saturating_add(1);
                current.last_error = Some(code);
            } else {
                current.replies = current.replies.saturating_add(1);
            }
        }
    }
    pub fn retain(&mut self, mut live: impl FnMut(Reference) -> bool) {
        self.manual.retain(|page| live(*page));
        self.automatic.retain(|page, _| live(*page));
        self.filters
            .retain(|scope, _| self.automatic.contains_key(scope));
        self.handlers.retain(live);
    }
    pub fn summary(&self) -> Value {
        json!(
            self.automatic
                .values()
                .map(|rule| rule.summary())
                .collect::<Vec<_>>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page() -> Reference {
        Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    #[test]
    fn har_filters_compile_before_ownership_and_follow_the_original_rule_lifetime() {
        use crate::browser::routing::Matcher;
        let scope = page();
        let archive = Uuid::new_v4();
        let mut rules = Rules::default();
        let invalid = Matcher::Regex {
            pattern: "(?=secret".into(),
            flags: String::new(),
        };
        assert!(
            rules
                .begin_filtered(
                    scope,
                    ScopeKind::Page,
                    archive,
                    NotFound::Abort,
                    Some(invalid)
                )
                .is_err()
        );
        assert!(!rules.used(archive));
        assert!(rules.filters.is_empty());
        for filter in [
            Matcher::Exact {
                url: "https://example.com/acme?item=one".into(),
            },
            Matcher::Glob {
                pattern: "**/acme?item=*".into(),
            },
            Matcher::Regex {
                pattern: r"/ACME\?item=one$".into(),
                flags: "i".into(),
            },
        ] {
            let rule = rules
                .begin_filtered(
                    scope,
                    ScopeKind::Page,
                    archive,
                    NotFound::Abort,
                    Some(filter),
                )
                .unwrap();
            assert!(rules.har_matches(scope, "https://example.com/acme?item=one"));
            assert!(!rules.har_matches(scope, "https://example.com/acme?other=one"));
            assert_eq!(rule.summary()["urlFilterActive"], true);
            assert!(!rule.summary().to_string().contains("item="));
            rules.setup_failed(scope, rule.id, "OUTCOME_UNKNOWN");
            assert_eq!(rules.filters.len(), 1);
            rules.confirm(scope, rule.id).unwrap();
            let stopping = rules.stopping(scope).unwrap();
            rules.stop_failed(stopping, "OUTCOME_UNKNOWN");
            rules.stopped(scope, Uuid::new_v4());
            assert_eq!(rules.filters.len(), 1);
            rules.stopped(scope, rule.id);
            assert!(rules.filters.is_empty());
        }
        let rule = rules
            .begin_filtered(
                scope,
                ScopeKind::Context,
                archive,
                NotFound::Abort,
                Some(Matcher::Glob {
                    pattern: "**".into(),
                }),
            )
            .unwrap();
        rules.setup_failed(scope, rule.id, "BROWSER_METHOD_FAILED");
        assert!(rules.filters.is_empty());
        rules
            .begin_filtered(
                scope,
                ScopeKind::Context,
                archive,
                NotFound::Abort,
                Some(Matcher::Glob {
                    pattern: "**".into(),
                }),
            )
            .unwrap();
        rules.retain(|_| false);
        assert!(rules.filters.is_empty());
    }
    #[test]
    fn manual_interception_requires_confirmed_release_before_automatic_installation() {
        let page = page();
        let mut rules = Rules::default();
        rules.mark_manual(page, true);
        assert_eq!(
            rules
                .begin(page, Uuid::new_v4(), NotFound::Abort)
                .unwrap_err(),
            "MANUAL_ROUTE_ACTIVE"
        );
        rules.mark_manual(page, false);
        let rule = rules.begin(page, Uuid::new_v4(), NotFound::Abort).unwrap();
        assert_eq!(rule.phase, HarPhase::Installing);
        assert_eq!(
            rules.confirm(page, rule.id).unwrap().phase,
            HarPhase::Active
        );
    }
    #[test]
    fn uncertain_setup_and_stop_retain_the_archive_owner_until_confirmed_teardown() {
        let page = page();
        let archive = Uuid::new_v4();
        let mut rules = Rules::default();
        let rule = rules.begin(page, archive, NotFound::Abort).unwrap();
        rules.setup_failed(page, rule.id, "OUTCOME_UNKNOWN");
        assert!(rules.used(archive));
        assert_eq!(rules.automatic(page).unwrap().phase, HarPhase::Installing);
        rules.confirm(page, rule.id).unwrap();
        let previous = rules.stopping(page).unwrap();
        rules.stop_failed(previous, "BROWSER_METHOD_FAILED");
        assert_eq!(rules.automatic(page).unwrap().phase, HarPhase::Stopping);
        let previous = rules.stopping(page).unwrap();
        rules.stop_failed(previous, "OUTCOME_UNKNOWN");
        assert_eq!(rules.automatic(page).unwrap().phase, HarPhase::Stopping);
        assert!(rules.used(archive));
        rules.stopped(page, rule.id);
        assert!(!rules.used(archive));
    }
    #[test]
    fn stale_acknowledgements_cannot_confirm_or_remove_a_replacement_rule() {
        let page = page();
        let mut rules = Rules::default();
        let old = rules.begin(page, Uuid::new_v4(), NotFound::Abort).unwrap();
        rules.setup_failed(page, old.id, "BROWSER_METHOD_FAILED");
        let current = rules
            .begin(page, Uuid::new_v4(), NotFound::Continue)
            .unwrap();
        assert_eq!(rules.confirm(page, old.id).unwrap_err(), "HAR_ROUTE_LOST");
        rules.setup_failed(page, old.id, "OUTCOME_UNKNOWN");
        rules.stopped(page, old.id);
        rules.result(old, Some("BACKEND_LOST"));
        assert_eq!(rules.automatic(page).unwrap().id, current.id);
        assert_eq!(rules.automatic(page).unwrap().failures, 0);
        rules.retain(|_| false);
        assert!(rules.automatic(page).is_none());
    }
    #[test]
    fn context_and_page_replay_keep_independent_archive_owners_and_counters() {
        let context = page();
        let page = page();
        let archive = Uuid::new_v4();
        let mut rules = Rules::default();
        let context_rule = rules
            .begin_context(context, archive, NotFound::Abort)
            .unwrap();
        let page_rule = rules.begin(page, archive, NotFound::Continue).unwrap();
        assert_eq!(context_rule.summary()["scopeKind"], "context");
        assert_eq!(context_rule.summary()["context"], json!(context));
        assert!(context_rule.summary()["page"].is_null());
        rules.result(context_rule, None);
        assert_eq!(rules.automatic(context).unwrap().replies, 1);
        assert_eq!(rules.automatic(page).unwrap().replies, 0);
        rules.stopped(context, context_rule.id);
        assert!(rules.used(archive));
        rules.stopped(page, page_rule.id);
        assert!(!rules.used(archive));
    }
}
