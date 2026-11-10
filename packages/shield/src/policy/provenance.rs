//! Bounded branch decision origins retained independently of resolved rules.
use super::{Decision, Override, Profile, model::PackDocument, rules::Decisions};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

/// The configuration layer supplying an effective override.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Layer {
    Project,
    Local,
}

/// The authored pack default or the layer that supplied the selected branch.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Source {
    Pack,
    Project,
    Local,
}

/// Decision evidence for one matched branch, including unchanged defaults.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Evidence {
    profile: Profile,
    source: Source,
    default_decision: Decision,
    before_override: Option<Decision>,
    final_decision: Decision,
    #[serde(skip_serializing_if = "Option::is_none")]
    override_layer: Option<Layer>,
}

/// Original branches for one overridden source entry. At most one per override.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Original {
    met: Option<Decisions>,
    unmet: Option<Decisions>,
    effective: Override,
    project: Option<Override>,
    layer: Layer,
}

pub(super) fn capture(
    document: &PackDocument,
    overrides: &BTreeMap<String, Override>,
) -> BTreeMap<String, Original> {
    let rules = document.rules.iter().map(|rule| {
        (
            rule.id.as_str(),
            Some(rule.decision.clone()),
            rule.on_unmet.as_ref().map(|branch| branch.decision.clone()),
        )
    });
    let gates = document
        .preconditions
        .iter()
        .map(|gate| (gate.id.as_str(), None, Some(gate.on_unmet.decision.clone())));
    rules
        .chain(gates)
        .filter_map(|(id, met, unmet)| {
            overrides.get(id).map(|effective| {
                (
                    id.to_owned(),
                    Original {
                        met,
                        unmet,
                        effective: effective.clone(),
                        project: None,
                        layer: Layer::Project,
                    },
                )
            })
        })
        .collect()
}

pub(crate) fn layers(
    originals: &mut BTreeMap<String, Original>,
    local_selection: bool,
    project: &BTreeMap<String, Override>,
    local: Option<&BTreeMap<String, Override>>,
) {
    for (id, original) in originals {
        original.layer =
            if local_selection || local.is_some_and(|overrides| overrides.contains_key(id)) {
                Layer::Local
            } else {
                Layer::Project
            };
        original.project = if matches!(original.layer, Layer::Local) && !local_selection {
            project.get(id).cloned()
        } else {
            None
        };
    }
}

fn branch(overridden: &Override, unmet: bool) -> Option<Decision> {
    match overridden {
        Override::Decision(decision) if !unmet => Some(*decision),
        Override::Branches { decision, on_unmet } => {
            if unmet {
                *on_unmet
            } else {
                *decision
            }
        }
        _ => None,
    }
}

pub(super) fn evidence(
    original: Option<&Original>,
    profile: Profile,
    unmet: bool,
    final_decision: Decision,
) -> Evidence {
    let default_decision = original
        .and_then(|original| {
            if unmet {
                original.unmet.as_ref()
            } else {
                original.met.as_ref()
            }
        })
        .map_or(final_decision, |decisions| decisions.selected(profile));
    let project = original.and_then(|original| original.project.as_ref());
    let before_override = if project == Some(&Override::Off) {
        None
    } else {
        Some(
            project
                .and_then(|project| branch(project, unmet))
                .unwrap_or(default_decision),
        )
    };
    let source = original
        .and_then(|original| {
            branch(&original.effective, unmet).map(|_| match original.layer {
                Layer::Project => Source::Project,
                Layer::Local => Source::Local,
            })
        })
        .unwrap_or(Source::Pack);
    Evidence {
        profile,
        source,
        default_decision,
        before_override,
        final_decision,
        override_layer: original.map(|original| original.layer),
    }
}
