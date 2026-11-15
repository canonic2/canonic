//! Comparison-only effect overapproximation through the ordinary authority resolver.

use crate::effects::{Effect, Hints};
use crate::policy::{CustomRules, Decision, Error, Knowledge, Pack, Profile, Request};
use std::collections::BTreeSet;

pub(crate) fn effects(packs: &[(&Pack, Profile)], customs: &[&CustomRules]) -> Vec<Effect> {
    packs
        .iter()
        .flat_map(|(pack, _)| {
            pack.document
                .rules
                .iter()
                .filter_map(|rule| rule.matcher.as_ref())
                .flat_map(crate::policy::model::Matcher::effects)
        })
        .chain(customs.iter().flat_map(|custom| {
            custom
                .document
                .rules
                .iter()
                .filter_map(|rule| rule.matcher.as_ref())
                .flat_map(crate::policy::model::Matcher::effects)
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
    Ok(crate::policy::evaluate_evidence(
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
