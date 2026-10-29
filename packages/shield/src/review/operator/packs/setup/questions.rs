//! One variable's setup question: its shape for the declared type, the pack's
//! wording and preselection, and turning the reply into a checked value.
use super::super::super::terminal::{Question, Reply, Step, Terminal};
use crate::{
    policy::variables::{Declaration, Kind, render},
    review::Error,
};
use serde_json::{Value, json};
use std::{fmt::Write, path::Path};

/// Choice ID for leaving the variable unset.
const UNSET: &str = "-";
/// Choice ID for keeping the preselected current, suggested or default value.
const KNOWN: &str = "=";

/// What a question knows before it is asked.
pub(super) struct Known<'a> {
    /// The variable's value before this walk.
    pub(super) current: Option<&'a Value>,
    /// This walk's earlier answer when the operator came back to the question.
    pub(super) answered: Option<&'a Option<Value>>,
    /// An earlier step exists, so Escape reads "back".
    pub(super) back: bool,
}

/// Ask for one variable; `Done(None)` leaves it unset.
pub(super) fn ask(
    tty: &mut Terminal,
    project: &Path,
    pack: &str,
    name: &str,
    declaration: &Declaration,
    known: &Known<'_>,
) -> Result<Step<Option<Value>>, Error> {
    let setup = declaration.setup.clone().unwrap_or_default();
    let current = known.current;
    let suggested = suggestion(project, declaration);
    let context = context(declaration, current, suggested.as_ref());
    let required = declaration.required();
    let preferred = match known.answered {
        Some(answer) => answer.clone(),
        None => current
            .or(suggested.as_ref())
            .or(declaration.default.as_ref())
            .cloned(),
    };
    let mut question = Question {
        title: format!("{pack}: {name}"),
        prompt: setup
            .question
            .clone()
            .unwrap_or_else(|| declaration.description.clone()),
        context: context.trim().to_owned(),
        choices: Vec::new(),
        multiple: false,
        typed: None,
        default: None,
        back: known.back,
    };
    if !required {
        let detail = if declaration.default.is_some() {
            ""
        } else {
            "rules that use it stay off"
        };
        question
            .choices
            .push((UNSET.into(), unset_label(declaration), detail.into()));
        question.default = Some(UNSET.into());
    }
    let declared = declaration.choices().unwrap_or_default();
    match declaration.kind {
        Kind::Choice | Kind::Boolean => pick(tty, question, declaration, preferred.as_ref()),
        Kind::List if !declaration.typed() => {
            question.choices = declared
                .iter()
                .map(|v| (v.clone(), v.clone(), String::new()))
                .collect();
            question.multiple = true;
            question.default = None;
            let mut check = |reply: &Reply| match reply {
                Reply::Choices(ids) if ids.is_empty() && !required => Ok(()),
                Reply::Choices(ids) => declaration.check(&json!(ids)).map_err(|e| e.to_string()),
                Reply::Typed(_) => Err("pick from the list".into()),
            };
            Ok(match tty.question(&question, &mut check)? {
                Step::Back => Step::Back,
                Step::Done(Reply::Choices(ids)) if ids.is_empty() => Step::Done(None),
                Step::Done(Reply::Choices(ids)) => Step::Done(Some(json!(ids))),
                Step::Done(Reply::Typed(_)) => {
                    return Err(Error::Invalid("this question has no typed answer"));
                }
            })
        }
        _ => {
            if let Some(value) = &preferred {
                let detail = if known.answered.is_some() {
                    "your answer"
                } else if current == Some(value) {
                    "current value"
                } else if suggested.as_ref() == Some(value) {
                    "suggested for this project"
                } else {
                    "pack default"
                };
                question
                    .choices
                    .push((KNOWN.into(), render(value), detail.into()));
                question.default = Some(KNOWN.into());
            }
            typed(tty, question, declaration, preferred)
        }
    }
}

/// A choice or boolean: one of its values, after any unset answer.
fn pick(
    tty: &mut Terminal,
    mut question: Question,
    declaration: &Declaration,
    preferred: Option<&Value>,
) -> Result<Step<Option<Value>>, Error> {
    let values: Vec<(String, String)> = if declaration.kind == Kind::Boolean {
        vec![("true".into(), "Yes".into()), ("false".into(), "No".into())]
    } else {
        declaration
            .choices()
            .unwrap_or_default()
            .iter()
            .map(|v| (v.clone(), v.clone()))
            .collect()
    };
    for (id, label) in values {
        question.choices.push((id, label, String::new()));
    }
    if let Some(value) = preferred {
        question.default = Some(render(value));
    }
    Ok(match tty.question(&question, &mut |_| Ok(()))? {
        Step::Back => Step::Back,
        Step::Done(Reply::Choices(ids)) => match ids.first().map(String::as_str) {
            None | Some(UNSET) => Step::Done(None),
            Some(id) => Step::Done(Some(declaration.parse(id)?)),
        },
        Step::Done(Reply::Typed(_)) => {
            return Err(Error::Invalid("this question has no typed answer"));
        }
    })
}

/// A string, path or free-text list: listed answers plus the typed row.
fn typed(
    tty: &mut Terminal,
    mut question: Question,
    declaration: &Declaration,
    preferred: Option<Value>,
) -> Result<Step<Option<Value>>, Error> {
    let setup = declaration.setup.clone().unwrap_or_default();
    let label = match declaration.kind {
        Kind::Path => "Enter a project-relative path",
        Kind::List => "Enter comma-separated items",
        _ => "Enter a value",
    };
    question.typed = Some((
        setup.value.unwrap_or_else(|| label.into()),
        setup.placeholder,
    ));
    let mut check = |reply: &Reply| match reply {
        Reply::Typed(text) => declaration
            .parse(text)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Reply::Choices(_) => Ok(()),
    };
    Ok(match tty.question(&question, &mut check)? {
        Step::Back => Step::Back,
        Step::Done(Reply::Typed(text)) => Step::Done(Some(declaration.parse(&text)?)),
        Step::Done(Reply::Choices(ids)) if ids.first().map(String::as_str) == Some(KNOWN) => {
            Step::Done(preferred)
        }
        Step::Done(Reply::Choices(_)) => Step::Done(None),
    })
}

/// The description under a pack-written question, then the known values.
fn context(
    declaration: &Declaration,
    current: Option<&Value>,
    suggested: Option<&Value>,
) -> String {
    let mut context = if declaration
        .setup
        .as_ref()
        .is_some_and(|s| s.question.is_some())
    {
        declaration.description.clone()
    } else {
        String::new()
    };
    for (label, value) in [
        ("Default", declaration.default.as_ref()),
        ("Current", current),
        ("Suggested for this project", suggested),
    ] {
        if let Some(value) = value {
            let _ = write!(context, "\n{label}: {}", render(value));
        }
    }
    context
}

/// How an answer reads in the setup review.
pub(super) fn describe(declaration: &Declaration, answer: Option<&Value>) -> String {
    answer.map_or_else(|| unset_label(declaration), render)
}

fn unset_label(declaration: &Declaration) -> String {
    declaration
        .setup
        .as_ref()
        .and_then(|s| s.unset.clone())
        .unwrap_or_else(|| {
            if declaration.default.is_some() {
                "Use the pack default".into()
            } else {
                "No check".into()
            }
        })
}

fn suggestion(project: &Path, declaration: &Declaration) -> Option<Value> {
    declaration
        .suggest
        .iter()
        .flatten()
        .find(|s| project.join(&s.file).is_file())
        .map(|s| s.value.clone())
}
