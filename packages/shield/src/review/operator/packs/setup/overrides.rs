//! Guided rule overrides: mark the pack entries to change in one list, then
//! choose a change for each marked entry. Changes are only proposed; review
//! shows any that widen.
use super::super::super::terminal::{Step, Terminal};
use crate::{
    policy::{
        self, Binding, Decision, Override,
        resolution::targets::{Kind, Target},
    },
    review::Error,
};
use std::collections::BTreeMap;

/// List the pack's rules with the overridden ones marked. Space marks the rules
/// to change and Enter continues to one step per marked rule; unmarked rules use
/// the pack's decision. Escape in a rule's step returns to the list with its
/// marks kept; Escape on the list returns [`Step::Back`], labeled "back" when an
/// `earlier` step exists and keeps the changes made so far. Nothing is applied
/// before the final review.
pub(in crate::review::operator) fn walk(
    tty: &mut Terminal,
    artifact: &str,
    binding: &mut Binding,
    earlier: bool,
) -> Result<Step<()>, Error> {
    let targets = policy::override_targets(artifact)?;
    if targets.is_empty() {
        return Ok(Step::Done(()));
    }
    let mut overrides = binding.overrides.clone();
    let mut marked: Vec<String> = targets
        .iter()
        .filter(|t| overrides.contains_key(&t.id))
        .map(|t| t.id.clone())
        .collect();
    'list: loop {
        let rows: Vec<(String, String)> = targets
            .iter()
            .map(|target| {
                let detail = match overrides.get(&target.id) {
                    Some(change) => format!("currently {} · {}", describe(change), target.detail),
                    None => target.detail.clone(),
                };
                (target.id.clone(), detail)
            })
            .collect();
        let choices: Vec<(&str, &str, &str)> = rows
            .iter()
            .map(|(id, detail)| (id.as_str(), id.as_str(), detail.as_str()))
            .collect();
        let marks: Vec<&str> = marked.iter().map(String::as_str).collect();
        let Step::Done(chosen) = tty.choose_many_step(
            "Rule overrides",
            "Mark the rules to turn off or decide differently for this project; unmarked rules use the pack's decision.",
            &choices,
            &marks,
            "Keep every rule",
            earlier,
        )?
        else {
            // Going back keeps the changes made so far, like any other answer;
            // unmarking only removes an override once its pass completes.
            binding.overrides = overrides;
            return Ok(Step::Back);
        };
        marked = chosen;
        // The working map keeps every existing override until the pass ends,
        // so a step reached again after going back still finds its current
        // value instead of starting from "off".
        for target in targets.iter().filter(|t| marked.contains(&t.id)) {
            if change(tty, target, &mut overrides)? == Step::Back {
                continue 'list;
            }
        }
        overrides.retain(|id, _| marked.contains(id));
        binding.overrides = overrides;
        return Ok(Step::Done(()));
    }
}

/// Choose the change for one marked entry; an entry that can only be turned
/// off takes that change without a step.
fn change(
    tty: &mut Terminal,
    target: &Target,
    overrides: &mut BTreeMap<String, Override>,
) -> Result<Step<()>, Error> {
    if target.kind == Kind::Member {
        overrides.insert(target.id.clone(), Override::Off);
        return Ok(Step::Done(()));
    }
    let mut actions = vec![("off", "Turn it off", "it decides nothing")];
    if matches!(target.kind, Kind::Rule { .. }) {
        actions.extend([
            ("allow", "Allow", "when it matches"),
            ("ask", "Ask", "when it matches"),
            ("deny", "Deny", "when it matches"),
        ]);
    }
    if matches!(target.kind, Kind::Rule { unmet: true } | Kind::Precondition) {
        actions.extend([
            ("unmet-ask", "Ask when its check fails", ""),
            ("unmet-deny", "Deny when its check fails", ""),
        ]);
    }
    let existing = overrides.get(&target.id);
    let preselected = existing
        .and_then(action)
        .filter(|current| actions.iter().any(|(id, _, _)| id == current));
    // A combination no action here produces stays selectable, so Enter keeps it.
    let keep = existing
        .filter(|_| preselected.is_none())
        .map(|change| format!("Keep current ({})", describe(change)));
    if let Some(label) = &keep {
        actions.insert(0, ("keep", label.as_str(), "leave this override unchanged"));
    }
    let current = preselected.unwrap_or(if keep.is_some() { "keep" } else { "off" });
    let Step::Done(chosen) =
        tty.choose_step(&target.id, &target.detail, &actions, current, true)?
    else {
        return Ok(Step::Back);
    };
    if chosen == "keep" {
        return Ok(Step::Done(()));
    }
    let decision = |name: &str| match name {
        "allow" => Decision::Allow,
        "ask" => Decision::Ask,
        _ => Decision::Deny,
    };
    let change = match chosen.as_str() {
        "off" => Override::Off,
        unmet if unmet.starts_with("unmet-") => Override::Branches {
            decision: None,
            on_unmet: Some(decision(&unmet["unmet-".len()..])),
        },
        other => Override::Decision(decision(other)),
    };
    overrides.insert(target.id.clone(), change);
    Ok(Step::Done(()))
}

/// The action that produced an override, to preselect it; `None` for a
/// combination no single action produces.
fn action(change: &Override) -> Option<&'static str> {
    Some(match change {
        Override::Off => "off",
        Override::Decision(Decision::Allow) => "allow",
        Override::Decision(Decision::Ask) => "ask",
        Override::Decision(Decision::Deny) => "deny",
        Override::Branches {
            decision: None,
            on_unmet: Some(Decision::Ask),
        } => "unmet-ask",
        Override::Branches {
            decision: None,
            on_unmet: Some(Decision::Deny),
        } => "unmet-deny",
        Override::Branches { .. } => return None,
    })
}

fn describe(change: &Override) -> String {
    let name = |d: &Decision| match d {
        Decision::Allow => "allow",
        Decision::Ask => "ask",
        Decision::Deny => "deny",
    };
    match change {
        Override::Off => "off".into(),
        Override::Decision(d) => name(d).into(),
        Override::Branches { decision, on_unmet } => [
            decision.as_ref().map(|d| name(d).to_owned()),
            on_unmet
                .as_ref()
                .map(|d| format!("{} when its check fails", name(d))),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", "),
    }
}
