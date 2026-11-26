//! Pack variable values and rule overrides for one selection: the guided walk,
//! command-line edits and their parsing. Values are only proposed here; the
//! ordinary review and confirmation activate them.
use super::super::terminal::{Step, Terminal};
use crate::{
    policy::{self, model::variables::Declaration},
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
    /// Return to the last step of a completed walk.
    pub(in crate::review::operator) resume: bool,
}

/// Answers and the questions actually reached in this selection's walk.
#[derive(Default)]
pub(in crate::review::operator) struct Navigation {
    answers: Answers,
    history: Vec<usize>,
}

impl Navigation {
    pub(in crate::review::operator) fn has_questions(&self) -> bool {
        !self.history.is_empty()
    }
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
    navigation: &mut Navigation,
) -> Result<Step<()>, Error> {
    let flow = Flow::new(artifact, settings.effective().variables, options.scope)?;
    let Navigation { answers, history } = navigation;
    let mut next = 0;
    let mut stage = Stage::Questions;
    if options.resume {
        if history.is_empty() {
            return Ok(Step::Back);
        }
        // Land on the last step this walk shows.
        next = flow.declarations.len();
        stage = if history.len() >= 2 {
            Stage::Review
        } else if options.scope == Walk::All
            && !options.local
            && asks_place(settings, &flow.declarations, answers)
        {
            Stage::Place
        } else {
            next = history.pop().unwrap_or_default();
            Stage::Questions
        };
    } else {
        history.clear();
    }
    loop {
        stage =
            match stage {
                Stage::Questions => {
                    while next < flow.declarations.len() {
                        let (name, declaration) = &flow.declarations[next];
                        if flow.asks(name, declaration, answers) {
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
                Stage::Review => {
                    let reviewed = flow.review(tty, project, pack, answers)?;
                    // Editing an earlier answer can add or remove later questions.
                    history.clear();
                    history.extend(flow.declarations.iter().enumerate().filter_map(
                        |(index, (name, _))| answers.contains_key(name).then_some(index),
                    ));
                    match reviewed {
                        Step::Done(()) => Stage::Place,
                        Step::Back => {
                            next = history.pop().unwrap_or_default();
                            Stage::Questions
                        }
                    }
                }
                Stage::Place if answers.is_empty() => return Ok(Step::Done(())),
                Stage::Place => {
                    let place = if options.local {
                        Step::Done(Place::Local)
                    } else if options.scope == Walk::Missing {
                        Step::Done(Place::Suggested)
                    } else {
                        placement(tty, settings, &flow.declarations, answers)?
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
    answers: &Answers,
    place: Place,
) -> Result<(), Error> {
    for (name, declaration) in declarations {
        let Some(answer) = answers.get(name) else {
            continue;
        };
        let local = answer.is_some()
            && (declaration.executable_paths()
                || match place {
                    Place::Suggested => suggests_local(settings, name, declaration),
                    Place::Project => false,
                    Place::Local => true,
                });
        settings.edit(local, |binding| {
            match answer {
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
    /// Read saved values in their declared shape, as resolution does: a list
    /// item saved alone, before the variable became a list, is a one-item list.
    fn new(
        artifact: &str,
        mut current: BTreeMap<String, Value>,
        scope: Walk,
    ) -> Result<Self, Error> {
        let declarations = policy::ordered_declarations(artifact)?;
        for (name, declaration) in &declarations {
            if let Some(value) = current.get_mut(name) {
                *value = declaration.normalize(value);
            }
        }
        Ok(Self {
            declarations,
            current,
            scope,
        })
    }

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
                        policy::model::variables::render(value)
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

/// One question for shareable answers. Executable path lists always stay local.
fn placement(
    tty: &mut Terminal,
    settings: &Settings,
    declarations: &[(String, Declaration)],
    answers: &Answers,
) -> Result<Step<Place>, Error> {
    if !asks_place(settings, declarations, answers) {
        return Ok(Step::Done(Place::Project));
    }
    let executable_paths = declarations.iter().any(|(name, declaration)| {
        declaration.executable_paths() && answers.get(name).is_some_and(Option::is_some)
    });
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
        format!("In the local layer: {}", local.join(", "))
    };
    let (prompt, description, share) = if executable_paths {
        (
            "Where to save the other answers",
            "Executable path lists use config.local.yml. Git ignore rules control what Git includes by default.",
            "Share the other answers with the project",
        )
    } else {
        (
            "Where to save these answers",
            "Shared values use config.yml. Local values use config.local.yml. Git ignore rules control what Git includes by default.",
            "All shared with the project",
        )
    };
    let chosen = tty.choose_step(
        prompt,
        description,
        &[
            ("suggested", "As the pack suggests", &suggested),
            ("project", share, "config.yml, shared with the team"),
            (
                "local",
                "All in the local layer",
                "config.local.yml, in this checkout",
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
fn asks_place(
    settings: &Settings,
    declarations: &[(String, Declaration)],
    answers: &Answers,
) -> bool {
    settings.local.is_some()
        && declarations.iter().any(|(name, declaration)| {
            !declaration.executable_paths() && answers.get(name).is_some_and(Option::is_some)
        })
}

fn suggests_local(settings: &Settings, name: &str, declaration: &Declaration) -> bool {
    let current = settings.layer(name);
    declaration.executable_paths()
        || current == "local"
        || (current != "project" && declaration.layer() == policy::model::variables::Layer::Local)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sharing_guided_answers_keeps_executable_paths_local() {
        let declarations: Vec<(String, Declaration)> = serde_json::from_value(json!([
            ["tools", {"type":"list","description":"Tool paths","layer":"local",
                "items":{"type":"string"},"discover":[{"path_search":"tool"}]}],
            ["profile", {"type":"string","description":"Profile"}],
        ]))
        .unwrap();
        let mut settings = Settings {
            shared: policy::Binding::default(),
            local: Some(crate::configuration::layers::Overlay::default()),
        };
        let paths = json!([crate::test_support::absolute("/opt/acme/bin/tool")]);
        let mut answers = Answers::from([("tools".into(), Some(paths.clone()))]);
        assert!(!asks_place(&settings, &declarations, &answers));
        answers.insert("profile".into(), Some(json!("development")));
        assert!(asks_place(&settings, &declarations, &answers));
        bind_answers(&mut settings, &declarations, &answers, Place::Project).unwrap();
        assert!(!settings.shared.variables.contains_key("tools"));
        assert_eq!(settings.shared.variables["profile"], "development");
        let local = settings.local.unwrap();
        assert_eq!(local.variables["tools"], paths);
        assert!(!local.variables.contains_key("profile"));
    }

    #[test]
    fn a_list_item_saved_alone_shows_the_same_dependent_questions_as_a_list() {
        let pack = json!({"schema_version":1,"kind":"pack","id":"acme/build","version":"0.1.0",
            "description":"Acme build","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":"*"},
            "capabilities":["structured-rules-v1","argument-rules-v1","variables-v1"],
            "variables":{
                "tools":{"type":"list","description":"Build tools","items":{"type":"string"},"max_items":4},
                "target":{"type":"string","description":"Make target",
                    "setup":{"ask_when":[{"var":"tools","contains":"make"}]}}},
            "rules":[{"id":"tools","decision":"allow",
                "conditions":{"domain":"argv","executable":{"var":"tools"},"arguments":{"kind":"any"}}}]})
        .to_string();
        let (name, declaration) = {
            let flow = Flow::new(&pack, BTreeMap::new(), Walk::All).unwrap();
            flow.declarations
                .into_iter()
                .find(|(name, _)| name == "target")
                .unwrap()
        };
        for saved in [json!("make"), json!(["make"])] {
            let flow = Flow::new(
                &pack,
                BTreeMap::from([("tools".into(), saved.clone())]),
                Walk::All,
            )
            .unwrap();
            assert!(flow.asks(&name, &declaration, &Answers::new()), "{saved}");
        }
        let flow = Flow::new(
            &pack,
            BTreeMap::from([("tools".into(), json!("cargo"))]),
            Walk::All,
        )
        .unwrap();
        assert!(!flow.asks(&name, &declaration, &Answers::new()));
    }
}
