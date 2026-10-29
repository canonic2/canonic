//! Universal profile bounds over identical selected matcher/authority artifacts.

use super::{CustomRules, Decision, Pack, Profile};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bound {
    Equivalent,
    NonExpansive,
    NonRestrictive,
}

/// Semantic evaluations have an ask floor. Identical deny truth vectors
/// therefore imply identical final decisions, including conflicts/overrides.
/// Monotone rule decisions preserve the resolver's direction for every request.
pub(crate) fn profiles(
    before: &[(&Pack, Profile)],
    after: &[(&Pack, Profile)],
    before_custom: &[&CustomRules],
    after_custom: &[&CustomRules],
) -> Option<Bound> {
    if !super::semantic_selection(before, before_custom) {
        return None;
    }
    let mut left_custom: Vec<_> = before_custom.iter().map(|custom| &custom.digest).collect();
    let mut right_custom: Vec<_> = after_custom.iter().map(|custom| &custom.digest).collect();
    left_custom.sort();
    right_custom.sort();
    if left_custom != right_custom || before.len() != after.len() {
        return None;
    }
    let mut tightened = false;
    let mut relaxed = false;
    let mut identical_denials = true;
    for (pack, previous) in before {
        let (next_pack, next) = after.iter().find(|(next, _)| next.id() == pack.id())?;
        if pack.digest != next_pack.digest {
            return None;
        }
        let rules = pack.document.rules.iter().flat_map(|rule| {
            std::iter::once(&rule.decision).chain(rule.on_unmet.as_ref().map(|u| &u.decision))
        });
        let gates = pack
            .document
            .preconditions
            .iter()
            .map(|p| &p.on_unmet.decision);
        for decisions in rules.chain(gates) {
            let a = decisions.selected(*previous);
            let b = decisions.selected(*next);
            identical_denials &= (a == Decision::Deny) == (b == Decision::Deny);
            tightened |= b > a;
            relaxed |= b < a;
        }
    }
    if identical_denials {
        Some(Bound::Equivalent)
    } else if !relaxed {
        Some(Bound::NonExpansive)
    } else if !tightened {
        Some(Bound::NonRestrictive)
    } else {
        None
    }
}
