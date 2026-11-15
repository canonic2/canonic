//! Same-predicate custom outcomes: source identity never substitutes for consent.
use super::{CustomDocument, CustomRules, Hash, Rule, SourceIdentity};
use crate::policy::{Profile, dependencies::profile_comparison::Bound, rules::Unmet};
use serde::{Serialize, Serializer};

struct Predicate<'a>(&'a Rule);
impl Serialize for Predicate<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Rule {
            id,
            legacy,
            legacy_normalized,
            invocation,
            command_set,
            decision: _,
            conditions,
            requirements,
            on_unmet,
            reason: _,
            guidance,
            expires_at,
            fallback,
            when,
            agents,
            matcher: _,
        } = self.0;
        if let Some(crate::policy::rules::Guidance {
            message: _,
            alternatives: _,
        }) = guidance
        {}
        if let Some(Unmet {
            decision: _,
            message: _,
            alternatives: _,
        }) = on_unmet
        {}
        (
            id,
            legacy,
            legacy_normalized,
            invocation,
            command_set,
            conditions,
            requirements,
            on_unmet.is_some(),
            expires_at,
            fallback,
            when,
            agents,
        )
            .serialize(serializer)
    }
}

fn identity(custom: &CustomRules, limit: usize) -> Option<SourceIdentity> {
    if custom.comparison_sources.as_ref()?.len() != 1 {
        return None;
    }
    let CustomDocument {
        schema_version,
        kind,
        capabilities,
        rules,
        directory_options,
    } = &custom.document;
    if rules.iter().any(|rule| {
        rule.legacy.is_some() || rule.legacy_normalized.is_some() || rule.expires_at.is_some()
    }) {
        return None;
    }
    let mut hash = Hash::new(limit);
    if !hash.value(&(
        "same-custom-predicates-v1",
        schema_version,
        kind,
        capabilities,
        directory_options,
        rules.len(),
    )) {
        return None;
    }
    for rule in rules {
        if !hash.value(&Predicate(rule)) {
            return None;
        }
    }
    Some(hash.finish())
}

/// Prove a direction for every common request/fact assignment. A strict
/// relation still requires a real resolver witness; predicates may be empty.
pub(crate) fn custom_bound(
    before: &CustomRules,
    after: &CustomRules,
    limit: usize,
) -> Option<Bound> {
    let first = identity(before, limit)?;
    let second = identity(after, limit.checked_sub(first.bytes)?)?;
    crate::diagnostics::trace::count(
        "comparison.custom_decision_bound_bytes",
        (first.bytes + second.bytes) as u64,
    );
    if !first.same(&second) {
        return None;
    }
    let mut tighter = false;
    let mut looser = false;
    for (first, second) in before.document.rules.iter().zip(&after.document.rules) {
        let previous = std::iter::once(&first.decision)
            .chain(first.on_unmet.as_ref().map(|unmet| &unmet.decision));
        let next = std::iter::once(&second.decision)
            .chain(second.on_unmet.as_ref().map(|unmet| &unmet.decision));
        for (a, b) in previous.zip(next) {
            tighter |= b.selected(Profile::Recommended) > a.selected(Profile::Recommended);
            looser |= b.selected(Profile::Recommended) < a.selected(Profile::Recommended);
        }
    }
    match (tighter, looser) {
        (true, false) => Some(Bound::NonExpansive),
        (false, true) => Some(Bound::NonRestrictive),
        _ => None,
    }
}
