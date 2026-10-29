//! Pack variable values and rule overrides for one selection: the guided walk,
//! command-line edits and their parsing. Values are only proposed here; the
//! ordinary review and confirmation activate them.
use super::super::terminal::{Step, Terminal};
use crate::{
    policy::{self, variables::Declaration},
    review::Error,
    review::inspection::Settings,
};
use serde_json::Value;
use std::{collections::BTreeMap, fmt::Write, path::Path};
mod overrides;
mod questions;
pub(in crate::review::operator) use overrides::walk as walk_overrides;

pub(in crate::review::operator) use crate::review::packs::settings::{Edits, apply};

/// Which variables the guided walk asks for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::review::operator) enum Walk {
    /// Every declared variable whose question applies, in declaration order.
    All,
    /// Only required variables that have no value.
    Missing,
}

/// How one selection's walk runs.
#[derive(Clone, Copy)]
pub(in crate::review::operator) struct Options {
    pub(in crate::review::operator) scope: Walk,
    /// Save every answer on this machine without asking where.
    pub(in crate::review::operator) local: bool,
    /// A step comes before this walk, so Escape on its first question goes back.
    pub(in crate::review::operator) earlier: bool,
    /// Start at the walk's last step with the selection's current values as
    /// its answers, for a later step that went back into it.
    pub(in crate::review::operator) resume: bool,
}

/// Where the walk is.
#[derive(Clone, Copy)]
enum Stage {
    Questions,
    Review,
    Place,
}

/// Ask a selection's setup questions in the order the pack declares its
/// variables, review the answers, then choose where they are saved. Escape goes
/// back one step with answers kept; on the first step the walk returns
/// [`Step::Back`]. Nothing is bound until the last step is answered.
pub(in crate::review::operator) fn walk(
    tty: &mut Terminal,
    project: &Path,
    pack: &str,
    artifact: &str,
    settings: &mut Settings,
    options: Options,
) -> Result<Step<()>, Error> {
    let flow = Flow {
        declarations: policy::ordered_declarations(artifact)?,
        current: settings.effective().variables,
        scope: options.scope,
    };
    let mut answers = Answers::new();
    // Declarations asked in this walk, in order; going back pops one.
    let mut history: Vec<usize> = Vec::new();
    let mut next = 0;
    let mut stage = Stage::Questions;
    if options.resume {
        for (index, (name, declaration)) in flow.declarations.iter().enumerate() {
            if flow.asks(name, declaration, &answers) {
                answers.insert(name.clone(), flow.current.get(name).cloned());
                history.push(index);
            }
        }
        if history.is_empty() {
            return Ok(Step::Back);
        }
        // Land on the last step this walk shows.
        next = flow.declarations.len();
        stage = if history.len() >= 2 {
            Stage::Review
        } else if !options.local && asks_place(settings, &answers) {
            Stage::Place
        } else {
            next = history.pop().unwrap_or_default();
            Stage::Questions
        };
    }
    loop {
        stage = match stage {
            Stage::Questions => {
                while next < flow.declarations.len() {
                    let (name, declaration) = &flow.declarations[next];
                    if flow.asks(name, declaration, &answers) {
                        break;
                    }
                    answers.remove(name);
                    next += 1;
                }
                let Some((name, declaration)) = flow.declarations.get(next) else {
                    stage = Stage::Review;
                    continue;
                };
                let known = questions::Known {
                    current: flow.current.get(name),
                    answered: answers.get(name),
                    back: options.earlier || !history.is_empty(),
                };
                match questions::ask(tty, project, pack, name, declaration, &known)? {
                    Step::Done(answer) => {
                        answers.insert(name.clone(), answer);
                        history.push(next);
                        next += 1;
                    }
                    Step::Back => match history.pop() {
                        Some(previous) => next = previous,
                        None => return Ok(Step::Back),
                    },
                }
                Stage::Questions
            }
            Stage::Review if history.len() < 2 => Stage::Place,
            Stage::Review => match flow.review(tty, project, pack, &mut answers)? {
                Step::Done(()) => Stage::Place,
                Step::Back => {
                    next = history.pop().unwrap_or_default();
                    Stage::Questions
                }
            },
            Stage::Place if answers.is_empty() => return Ok(Step::Done(())),
            Stage::Place => {
                let place = if options.local {
                    Step::Done(Place::Local)
                } else {
                    placement(tty, settings, &flow.declarations, &answers)?
                };
                match place {
                    Step::Done(place) => {
                        bind_answers(settings, &flow.declarations, answers, place)?;
                        return Ok(Step::Done(()));
                    }
                    Step::Back if history.len() >= 2 => Stage::Review,
                    Step::Back => match history.pop() {
                        Some(previous) => {
                            next = previous;
                            Stage::Questions
                        }
                        None => return Ok(Step::Back),
                    },
                }
            }
        };
    }
}

/// Bind every answer in the layer `place` chooses for it.
fn bind_answers(
    settings: &mut Settings,
    declarations: &[(String, Declaration)],
    mut answers: Answers,
    place: Place,
) -> Result<(), Error> {
    for (name, declaration) in declarations {
        let Some(answer) = answers.remove(name) else {
            continue;
        };
        let local = answer.is_some()
            && match place {
                Place::Suggested => suggests_local(settings, name, declaration),
                Place::Project => false,
                Place::Local => true,
            };
        settings.edit(local, |binding| {
            match &answer {
                Some(value) => binding.variables.insert(name.clone(), value.clone()),
                None => binding.variables.remove(name),
            };
            Ok(())
        })?;
        // A project value, or no value, replaces any personal one.
        if (!local || answer.is_none())
            && let Some(overlay) = settings.local.as_mut()
        {
            overlay.variables.remove(name);
            overlay.unset.retain(|n| n != name);
        }
    }
    Ok(())
}

/// Answers by variable; `None` leaves a variable unset.
type Answers = BTreeMap<String, Option<Value>>;

struct Flow {
    declarations: Vec<(String, Declaration)>,
    current: BTreeMap<String, Value>,
    scope: Walk,
}

impl Flow {
    /// Whether this walk asks the variable now: in scope, and every `ask_when`
    /// test holds for the answers so far, or current values where unanswered.
    fn asks(&self, name: &str, declaration: &Declaration, answers: &Answers) -> bool {
        let in_scope =
            self.scope == Walk::All || (declaration.required() && !self.current.contains_key(name));
        let tests = declaration
            .setup
            .as_ref()
            .and_then(|s| s.ask_when.as_deref())
            .unwrap_or_default();
        in_scope
            && tests.iter().all(|test| {
                let variable = test.variable();
                let value = answers
                    .get(variable)
                    .map_or_else(|| self.current.get(variable), Option::as_ref);
                test.holds(value)
            })
    }

    /// Show every answer; choosing one asks it again and re-checks the
    /// questions after it, asking those that now apply and dropping the rest.
    /// Escape here returns to the last question; Escape in a re-asked question
    /// returns here.
    fn review(
        &self,
        tty: &mut Terminal,
        project: &Path,
        pack: &str,
        answers: &mut Answers,
    ) -> Result<Step<()>, Error> {
        loop {
            let mut choices = vec![(
                "save".to_owned(),
                "Use these answers".to_owned(),
                String::new(),
            )];
            let mut kept = String::new();
            for (name, declaration) in &self.declarations {
                if let Some(answer) = answers.get(name) {
                    choices.push((
                        name.clone(),
                        name.clone(),
                        questions::describe(declaration, answer.as_ref()),
                    ));
                } else if let Some(value) = self.current.get(name) {
                    let _ = write!(
                        kept,
                        "\n{name} keeps {} because its question does not apply.",
                        policy::variables::render(value)
                    );
                }
            }
            let rows: Vec<(&str, &str, &str)> = choices
                .iter()
                .map(|(id, label, detail)| (id.as_str(), label.as_str(), detail.as_str()))
                .collect();
            let context = format!("Choose an answer to change it.{kept}");
            let title = format!("{pack}: setup answers");
            let Step::Done(chosen) = tty.choose_step(&title, &context, &rows, "save", true)? else {
                return Ok(Step::Back);
            };
            if chosen == "save" {
                return Ok(Step::Done(()));
            }
            let Some(position) = self.declarations.iter().position(|(n, _)| *n == chosen) else {
                continue;
            };
            let (name, declaration) = &self.declarations[position];
            let known = questions::Known {
                current: self.current.get(name),
                answered: answers.get(name),
                back: true,
            };
            let Step::Done(answer) = questions::ask(tty, project, pack, name, declaration, &known)?
            else {
                continue;
            };
            answers.insert(name.clone(), answer);
            for (later, declaration) in &self.declarations[position + 1..] {
                match (
                    self.asks(later, declaration, answers),
                    answers.contains_key(later),
                ) {
                    (true, false) => {
                        let known = questions::Known {
                            current: self.current.get(later),
                            answered: None,
                            back: true,
                        };
                        // Going back from a newly applicable question leaves it unanswered.
                        if let Step::Done(answer) =
                            questions::ask(tty, project, pack, later, declaration, &known)?
                        {
                            answers.insert(later.clone(), answer);
                        }
                    }
                    (false, true) => {
                        answers.remove(later);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Where the walk saves its answers.
#[derive(Clone, Copy)]
enum Place {
    /// Each value in its current layer, otherwise the layer its pack suggests.
    Suggested,
    Project,
    Local,
}

/// One question for all answers. Local-only selections and walks that only
/// unset values have a single place and skip it.
fn placement(
    tty: &mut Terminal,
    settings: &Settings,
    declarations: &[(String, Declaration)],
    answers: &Answers,
) -> Result<Step<Place>, Error> {
    if !asks_place(settings, answers) {
        return Ok(Step::Done(Place::Project));
    }
    let local: Vec<&str> = declarations
        .iter()
        .filter(|(name, d)| {
            answers.get(name).is_some_and(Option::is_some) && suggests_local(settings, name, d)
        })
        .map(|(name, _)| name.as_str())
        .collect();
    let suggested = if local.is_empty() {
        "Every value shared with the project".to_owned()
    } else {
        format!("Only on this machine: {}", local.join(", "))
    };
    let chosen = tty.choose_step(
        "Where to save these answers",
        "Project values are committed and shared; local values stay on this machine.",
        &[
            ("suggested", "As the pack suggests", &suggested),
            (
                "project",
                "All shared with the project",
                "config.yml, shared with the team",
            ),
            (
                "local",
                "All only on this machine",
                "config.local.yml, ignored by Git",
            ),
        ],
        "suggested",
        true,
    )?;
    let Step::Done(chosen) = chosen else {
        return Ok(Step::Back);
    };
    Ok(Step::Done(match chosen.as_str() {
        "project" => Place::Project,
        "local" => Place::Local,
        _ => Place::Suggested,
    }))
}

/// Whether the save-location question has a choice to offer.
fn asks_place(settings: &Settings, answers: &Answers) -> bool {
    settings.local.is_some() && answers.values().any(Option::is_some)
}

fn suggests_local(settings: &Settings, name: &str, declaration: &Declaration) -> bool {
    let current = settings.layer(name);
    current == "local"
        || (current != "project" && declaration.layer() == policy::variables::Layer::Local)
}
