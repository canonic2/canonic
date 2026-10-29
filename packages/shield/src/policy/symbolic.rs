//! Comparison-only effect overapproximation through the ordinary authority resolver.

use super::{CustomRules, Decision, Error, Knowledge, Pack, Profile, Request};
use crate::effects::{Effect, Hints};
use std::collections::BTreeSet;

pub(crate) fn effects(packs: &[(&Pack, Profile)], customs: &[&CustomRules]) -> Vec<Effect> {
    packs
        .iter()
        .flat_map(|(pack, _)| {
            pack.document
                .rules
                .iter()
                .filter_map(|rule| rule.matcher.as_ref())
                .flat_map(super::model::Matcher::effects)
        })
        .chain(customs.iter().flat_map(|custom| {
            custom
                .document
                .rules
                .iter()
                .filter_map(|rule| rule.matcher.as_ref())
                .flat_map(super::model::Matcher::effects)
        }))
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Never exposed as request evaluation: only the decision participates in a proof.
pub(crate) fn decision(
    packs: &[(&Pack, Profile)],
    customs: &[&CustomRules],
    request: &Request,
    effects: &[Effect],
) -> Result<Decision, Error> {
    request.validate()?;
    let hints = Hints::symbolic(request, effects);
    Ok(super::evaluate_evidence(
        packs,
        customs,
        request,
        true,
        &|| Ok(()),
        Knowledge {
            cwd: true,
            arguments: true,
        },
        Some(hints),
    )?
    .decision)
}
