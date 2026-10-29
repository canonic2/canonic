//! The pack entries an override can name, described for people choosing one.
use super::super::{
    Decision, Profile,
    model::PackDocument,
    rules::{Decisions, Rule},
};

/// What an override can change on one entry, matching its validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Off or a decision; `unmet` also allows Ask or Deny for failed requirements.
    Rule { unmet: bool },
    /// Off or an Ask or Deny for its unmet branch.
    Precondition,
    /// A command member of a shared rule body: off only.
    Member,
}

/// One overridable entry with a short description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) id: String,
    pub(crate) kind: Kind,
    pub(crate) detail: String,
}

/// Rules, then preconditions, then command members, each in source order.
pub(in crate::policy) fn list(document: &PackDocument) -> Vec<Target> {
    let rules = document.rules.iter().map(|rule| Target {
        id: rule.id.clone(),
        kind: Kind::Rule {
            unmet: rule.on_unmet.is_some(),
        },
        detail: rule_detail(rule),
    });
    let preconditions = document.preconditions.iter().map(|p| Target {
        id: p.id.clone(),
        kind: Kind::Precondition,
        detail: format!(
            "pack-wide check · otherwise {}",
            decisions(&p.on_unmet.decision)
        ),
    });
    let members = document.command_sets.iter().flat_map(|(set, members)| {
        members.iter().map(move |(command, id)| Target {
            id: id.clone(),
            kind: Kind::Member,
            detail: format!("{command} in {set}"),
        })
    });
    rules.chain(preconditions).chain(members).collect()
}

fn rule_detail(rule: &Rule) -> String {
    let mut detail = decisions(&rule.decision);
    if let Some(unmet) = &rule.on_unmet {
        detail = format!("{detail}, otherwise {}", decisions(&unmet.decision));
    }
    let about = rule.reason.clone().or_else(|| {
        rule.conditions["executable"]
            .as_str()
            .map(|e| format!("runs {e}"))
    });
    match about {
        Some(about) => format!("{detail} · {about}"),
        None => detail,
    }
}

/// One decision, or each profile's when they differ.
fn decisions(decisions: &Decisions) -> String {
    let name = |d: Decision| match d {
        Decision::Allow => "allow",
        Decision::Ask => "ask",
        Decision::Deny => "deny",
    };
    let [loose, recommended, strict] =
        [Profile::Loose, Profile::Recommended, Profile::Strict].map(|p| decisions.selected(p));
    if loose == recommended && recommended == strict {
        name(recommended).into()
    } else {
        format!(
            "loose {} · recommended {} · strict {}",
            name(loose),
            name(recommended),
            name(strict)
        )
    }
}
