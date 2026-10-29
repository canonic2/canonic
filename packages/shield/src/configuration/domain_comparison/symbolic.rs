//! Universal overapproximation proofs; synthetic effects are never witnesses.

use super::{Prepared, partition, profiles};
use crate::policy::{Error, profile_comparison::Bound, symbolic};

pub(super) fn prepare(
    first: &Prepared<'_>,
    second: &Prepared<'_>,
    fixed: Option<(&str, &str)>,
    regions: &mut partition::Partition,
) -> Result<Option<(Bound, usize)>, Error> {
    if regions.incomplete != Some(partition::Incomplete::UnsupportedSemanticDomain)
        || first.effective.access_paths() != second.effective.access_paths()
    {
        return Ok(None);
    }
    let packs: Vec<_> = first.packs.iter().chain(&second.packs).copied().collect();
    let customs: Vec<_> = first.custom.iter().chain(&second.custom).copied().collect();
    let mut shapes = partition::shapes(&packs, &customs, fixed)?;
    if shapes.incomplete.is_some() {
        regions.incomplete = shapes.incomplete;
        return Ok(None);
    }
    let effects = symbolic::effects(&packs, &customs);
    let Some(states) = 1usize.checked_shl(u32::try_from(effects.len()).map_err(|_| Error::Limit)?)
    else {
        regions.incomplete = Some(partition::Incomplete::EvaluationBudget);
        return Ok(None);
    };
    let Some(count) = shapes.requests.len().checked_mul(states) else {
        regions.incomplete = Some(partition::Incomplete::EvaluationBudget);
        return Ok(None);
    };
    let limit = partition::witness_budget(&packs, &customs);
    // Reserve the remaining sweep budget for real witness evaluation.
    if count > limit / 2 {
        regions.incomplete = Some(partition::Incomplete::EvaluationBudget);
        return Ok(None);
    }
    let mut expansion = false;
    let mut restriction = false;
    for request in &shapes.requests {
        for state in 0..states {
            let possible: Vec<_> = effects
                .iter()
                .enumerate()
                .filter_map(|(bit, effect)| ((state & (1 << bit)) != 0).then_some(*effect))
                .collect();
            let previous = symbolic::decision(&first.packs, &first.custom, request, &possible)?;
            let next = symbolic::decision(&second.packs, &second.custom, request, &possible)?;
            expansion |= next < previous;
            restriction |= next > previous;
        }
    }
    let bound = match (expansion, restriction) {
        (false, false) => Bound::Equivalent,
        (false, true) => Bound::NonExpansive,
        (true, false) => Bound::NonRestrictive,
        // Impossible truth vectors can contain both directions. They cannot
        // establish real mixed behavior or be emitted as request witnesses.
        (true, true) => return Ok(None),
    };
    if bound == Bound::Equivalent {
        shapes.requests.clear();
        shapes.incomplete = None;
    } else {
        profiles::augment(&mut shapes.requests, limit - count);
        shapes.incomplete = Some(partition::Incomplete::StrictWitnessNotFound);
    }
    *regions = shapes;
    Ok(Some((bound, count)))
}
