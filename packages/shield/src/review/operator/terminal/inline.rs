//! Shield's single inline terminal host; views return intent and never authorize policy.
use super::{Card, Step};
use crate::review::Error;
use canonic_tui::{
    editor::{Editor, EditorView},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    permission_review::{self, Kind, PermissionReview, Resource},
    picker::{Choice, ChoicePicker, Filter, Intent, PickerView, SelectionMode},
    runtime::{Capabilities, InlineOptions, InlineSession},
    theme::{ColorMode, Presentation},
    tool_call_card::Output,
};
use std::{io::IsTerminal, time::Duration};
mod signals;

pub(super) struct Host {
    session: InlineSession,
    signals: signals::Signals,
}
/// Acquire the required shared UI or report the terminal prerequisite.
pub(super) fn open(height: u16) -> Result<Host, Error> {
    let term = std::env::var("TERM").unwrap_or_default();
    if !std::io::stdin().is_terminal()
        || !std::io::stdout().is_terminal()
        || !["xterm", "screen", "tmux", "linux", "vt", "ansi"]
            .iter()
            .any(|p| term.starts_with(p))
    {
        return Err(Error::Invalid(
            "interactive TUI requires terminal stdin/stdout and a cursor-capable TERM",
        ));
    }
    let signals = signals::Signals::new()?;
    let session = InlineSession::open(InlineOptions {
        height,
        capabilities: Capabilities {
            cursor: true,
            alternate: false,
        },
    });
    match session {
        Ok(session) => Ok(Host { session, signals }),
        Err(_) if signals.cancelled() => Err(Error::Cancelled),
        Err(error) => Err(error.into()),
    }
}
fn presentation() -> Presentation {
    Presentation {
        colors: if std::env::var_os("NO_COLOR").is_some() {
            ColorMode::NoColor
        } else {
            ColorMode::Ansi16
        },
        ..Presentation::default()
    }
}
fn entered(event: &Event) -> bool {
    matches!(event, Event::Key(k) if k.kind == KeyEventKind::Press && k.code == KeyCode::Enter && k.modifiers.is_empty())
}
fn cancelled(event: &Event) -> bool {
    escaped(event) || interrupted(event)
}
fn escaped(event: &Event) -> bool {
    matches!(event, Event::Key(k) if k.kind != KeyEventKind::Release && k.code == KeyCode::Esc)
}
fn interrupted(event: &Event) -> bool {
    matches!(event, Event::Key(k) if k.kind != KeyEventKind::Release && k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL))
}
#[cfg(test)]
fn accepted(picker: &mut ChoicePicker, event: &Event, available: bool) -> Option<String> {
    collected(picker, event, available).and_then(|ids| ids.into_iter().next())
}
fn collected(picker: &mut ChoicePicker, event: &Event, available: bool) -> Option<Vec<String>> {
    match picker.update(event) {
        Intent::Selected(id) if entered(event) && available => Some(vec![id]),
        Intent::Confirmed(ids) if entered(event) && available => Some(ids),
        _ => None,
    }
}
pub(super) fn choose(
    host: Host,
    title: &str,
    context: &str,
    choices: &[(&str, &str, &str)],
    default: &str,
) -> Result<String, Error> {
    let menu = Menu {
        title,
        context,
        choices,
        default: Some(default),
        mode: SelectionMode::Single,
        escape: None,
        marked: &[],
        empty: "",
    };
    match select(host, &menu)? {
        Step::Done(ids) => ids
            .into_iter()
            .next()
            .ok_or(Error::Invalid("prompt choice missing")),
        Step::Back => Err(Error::Cancelled),
    }
}
/// One step of a guided flow: Escape returns [`Step::Back`] and the footer
/// reads `esc <escape>`; Ctrl-C still cancels.
pub(super) fn choose_step(
    host: Host,
    title: &str,
    context: &str,
    choices: &[(&str, &str, &str)],
    default: &str,
    escape: &str,
) -> Result<Step<String>, Error> {
    let menu = Menu {
        title,
        context,
        choices,
        default: Some(default),
        mode: SelectionMode::Single,
        escape: Some(escape),
        marked: &[],
        empty: "",
    };
    Ok(match select(host, &menu)? {
        Step::Done(ids) => Step::Done(
            ids.into_iter()
                .next()
                .ok_or(Error::Invalid("prompt choice missing"))?,
        ),
        Step::Back => Step::Back,
    })
}
/// A multiple choice starting with `marked` rows marked; `empty` is the
/// settled answer when the operator confirms nothing.
pub(super) fn choose_many(
    host: Host,
    title: &str,
    context: &str,
    choices: &[(&str, &str, &str)],
    marked: &[&str],
    empty: &str,
) -> Result<Vec<String>, Error> {
    let menu = Menu {
        title,
        context,
        choices,
        default: None,
        mode: SelectionMode::Multiple,
        escape: None,
        marked,
        empty,
    };
    match select(host, &menu)? {
        Step::Done(ids) => Ok(ids),
        Step::Back => Err(Error::Cancelled),
    }
}
/// A multiple choice as one step of a guided flow: Escape returns
/// [`Step::Back`] and the footer reads `esc <escape>`; Ctrl-C still cancels.
pub(super) fn choose_many_step(
    host: Host,
    title: &str,
    context: &str,
    choices: &[(&str, &str, &str)],
    marked: &[&str],
    empty: &str,
    escape: &str,
) -> Result<Step<Vec<String>>, Error> {
    let menu = Menu {
        title,
        context,
        choices,
        default: None,
        mode: SelectionMode::Multiple,
        escape: Some(escape),
        marked,
        empty,
    };
    select(host, &menu)
}
/// A picker prompt; `escape` makes Escape a step back instead of a cancel.
struct Menu<'a> {
    title: &'a str,
    context: &'a str,
    choices: &'a [(&'a str, &'a str, &'a str)],
    default: Option<&'a str>,
    mode: SelectionMode,
    escape: Option<&'a str>,
    /// Rows marked when a multiple choice opens.
    marked: &'a [&'a str],
    /// The settled answer when nothing is marked.
    empty: &'a str,
}
fn select(host: Host, menu: &Menu<'_>) -> Result<Step<Vec<String>>, Error> {
    let Menu {
        title,
        context,
        choices,
        default,
        mode,
        escape,
        marked,
        empty,
    } = *menu;
    let signals = host.signals;
    let mut session = host.session;
    let mut picker = ChoicePicker::new(
        choices
            .iter()
            .map(|(id, label, detail)| {
                let mut c = Choice::new(*id, *label);
                c.detail = (*detail).into();
                c
            })
            .collect(),
        default,
        mode,
        // Long lists, such as a pack's rules, get a search row; short ones keep digit shortcuts.
        if choices.len() > 9 {
            Filter::Auto
        } else {
            Filter::Off
        },
    )
    .map_err(|_| Error::Invalid("duplicate prompt choices"))?;
    picker.set_marked(marked);
    // The footer names the empty answer, such as `enter skip for now`.
    let nothing = if empty.is_empty() {
        "continue".to_owned()
    } else {
        empty.to_lowercase()
    };
    loop {
        session = signals.check(session)?;
        let area = session.area();
        session.draw(|frame| {
            picker.render(
                frame,
                area,
                PickerView {
                    title,
                    context,
                    source: "Shield",
                    max_height: area.height,
                    presentation: presentation(),
                    escape: escape.unwrap_or("cancel"),
                    empty: &nothing,
                },
                true,
            );
        })?;
        let Some(event) = session.poll(Duration::from_millis(100))? else {
            continue;
        };
        // Escape clears a search query before it closes this step.
        let leaves = interrupted(&event) || (escaped(&event) && picker.query().is_empty());
        if leaves && escape.is_some() && !interrupted(&event) {
            // The first step of a flow cancels; its settled line says so.
            let settled = if escape == Some("back") {
                "back"
            } else {
                "cancelled"
            };
            session.finish(&format!("{title} → {settled}"))?;
            return Ok(Step::Back);
        }
        if leaves {
            session.finish(&format!("{title} → cancelled"))?;
            return Err(Error::Cancelled);
        }
        // Numbers highlight. Only a deliberate Enter can confirm an operator choice.
        if let Some(ids) = collected(&mut picker, &event, session.confirmation_available()) {
            let label = if ids.is_empty() {
                empty.into()
            } else if ids.len() > 4 {
                format!("{} of {} marked", ids.len(), choices.len())
            } else {
                ids.iter()
                    .map(|id| {
                        choices
                            .iter()
                            .find(|(key, _, _)| *key == id)
                            .map_or(id.as_str(), |(_, label, _)| *label)
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            session.finish(&format!("{title} → {label}"))?;
            return Ok(Step::Done(ids));
        }
    }
}

/// One proposal as a permission review. Accept takes a second check that
/// quotes the change; Esc leaves it pending.
pub(super) fn review(host: Host, card: &Card, waiting: usize) -> Result<String, Error> {
    let signals = host.signals;
    let mut session = host.session;
    let mut review = PermissionReview::new(request(card))
        .map_err(|_| Error::Invalid("proposal review exceeds limits"))?;
    loop {
        session = signals.check(session)?;
        let width = session.area().width;
        // Leave room for Details to open without scrolling the transcript.
        // The Accept check draws over the card and needs its own rows.
        let check = if review.confirming() { 8 } else { 1 };
        session.set_height(review.height(width, presentation()).saturating_add(check))?;
        let area = session.area();
        session.draw(|frame| {
            review.render_with(frame, area, presentation(), waiting);
            review.render_overlay(frame, area, presentation());
        })?;
        let Some(event) = session.poll(Duration::from_millis(100))? else {
            continue;
        };
        if matches!(&event, Event::Key(k) if k.kind != KeyEventKind::Release && k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL))
        {
            session.finish(&format!("{} → cancelled", card.title))?;
            return Err(Error::Cancelled);
        }
        review.update(&event);
        if let Some(intent) = review.take_intents().into_iter().next() {
            let choice = match intent {
                permission_review::Intent::Respond(response) => response.choice,
                permission_review::Intent::Dismissed { .. } => "leave".into(),
            };
            let outcome = match choice.as_str() {
                "accept" => "accepted",
                "reject" => "rejected",
                _ => "left pending",
            };
            session.finish(&format!("{} → {outcome} · {}", card.title, card.summary))?;
            return Ok(choice);
        }
    }
}

fn request(card: &Card) -> permission_review::Request {
    let choice = |id: &str, label: &str, detail: &str, kind| {
        let mut choice = permission_review::Choice::new(id, label, kind);
        choice.detail = detail.into();
        choice
    };
    let mut accept = choice("accept", "Accept", "activate this change now", Kind::Allow);
    accept.wider = true;
    accept.scope.clone_from(&card.summary);
    accept.prompt = "This changes the project's active policy right away:".into();
    accept.disabled.clone_from(&card.stale);
    permission_review::Request {
        id: "proposal".into(),
        revision: 1,
        run: "shield policy review".into(),
        title: card.title.clone(),
        summary: card.summary.clone(),
        reason: card.reason.clone(),
        resources: card
            .resources
            .iter()
            .map(|(name, detail)| Resource {
                name: name.clone(),
                detail: detail.clone(),
            })
            .collect(),
        details_label: "complete change and rules".into(),
        details: Output::Text(card.details.clone()),
        choices: vec![
            accept,
            choice("reject", "Reject", "discard this proposal", Kind::Deny),
            choice("leave", "Leave pending", "decide later", Kind::Skip),
        ],
        default: Some("leave".into()),
    }
}

/// One setup question in the shared question form. A refused reply reopens
/// the question with its typed text kept and the reason shown.
pub(super) fn question(
    host: Host,
    question: &super::Question,
    check: &mut super::Check<'_>,
) -> Result<Step<super::Reply>, Error> {
    let signals = host.signals;
    let mut session = host.session;
    let mut questions = setup_form(question)?;
    loop {
        session = signals.check(session)?;
        let width = session.area().width;
        session.set_height(questions.height(width, presentation()).max(8))?;
        let area = session.area();
        session.draw(|frame| {
            questions.render_with(frame, area, presentation(), 0);
            questions.render_overlay(frame, area, presentation());
        })?;
        let Some(event) = session.poll(Duration::from_millis(100))? else {
            questions.tick(Duration::from_millis(100));
            continue;
        };
        if interrupted(&event) {
            session.finish(&format!("{} → cancelled", question.title))?;
            return Err(Error::Cancelled);
        }
        questions.update(&event);
        for intent in questions.take_intents() {
            match intent {
                form::Intent::Submit(response) => {
                    let reply = reply(&response);
                    match check(&reply) {
                        Ok(()) => {
                            questions.acknowledge(&response.ticket, form::Result::Accepted);
                            let summary = summary(question, &reply);
                            session.finish(&format!("{} → {summary}", question.title))?;
                            return Ok(Step::Done(reply));
                        }
                        Err(reason) => {
                            let refused = form::Result::Refused {
                                question: "answer".into(),
                                reason,
                            };
                            questions.acknowledge(&response.ticket, refused);
                        }
                    }
                }
                // The form closed one layer; the flow decides whether that is a
                // step back or, on its first step, cancellation.
                form::Intent::Cancelled { .. } => {
                    session.finish(&format!("{} → back", question.title))?;
                    return Ok(Step::Back);
                }
                form::Intent::Copy { .. } => {}
            }
        }
    }
}

use canonic_tui::question_form::{self as form, AnswerValue, Kind as Shape, QuestionForm, Request};

fn setup_form(question: &super::Question) -> Result<QuestionForm, Error> {
    let shape = match (question.multiple, question.choices.is_empty()) {
        (true, _) => Shape::Multiple,
        (false, true) => Shape::Text,
        (false, false) => Shape::Single,
    };
    let mut asked = form::Question::new("answer", &question.title, &question.prompt, shape);
    asked.description.clone_from(&question.context);
    // Leaving a value unset is its own listed answer, so single answers are required.
    asked.required = shape != Shape::Multiple;
    asked.choices = question
        .choices
        .iter()
        .map(|(id, label, detail)| {
            let mut choice = Choice::new(id, label);
            choice.detail.clone_from(detail);
            choice
        })
        .collect();
    asked.default = question.default.clone().filter(|_| shape == Shape::Single);
    if let Some((label, placeholder)) = &question.typed {
        asked.placeholder.clone_from(placeholder);
        if shape != Shape::Text {
            asked.other = true;
            asked.other_label = Some(label.clone());
        }
    }
    QuestionForm::new(Request {
        id: "setup".into(),
        revision: 1,
        title: question.title.clone(),
        questions: vec![asked],
        partial: false,
        back: question.back,
    })
    .map_err(|_| Error::Invalid("setup question exceeds limits"))
}

/// A typed row's text, or the chosen IDs; a skipped optional question chose none.
fn reply(response: &form::Response) -> super::Reply {
    match response.answers.first().map(|a| &a.value) {
        Some(
            AnswerValue::Text(text)
            | AnswerValue::Choices {
                other: Some(text), ..
            },
        ) => super::Reply::Typed(text.trim().into()),
        Some(AnswerValue::Choices { ids, other: None }) => super::Reply::Choices(ids.clone()),
        None => super::Reply::Choices(vec![]),
    }
}

fn summary(question: &super::Question, reply: &super::Reply) -> String {
    match reply {
        super::Reply::Typed(text) => text.clone(),
        super::Reply::Choices(ids) if ids.is_empty() => "none".into(),
        super::Reply::Choices(ids) => ids
            .iter()
            .map(|id| {
                question
                    .choices
                    .iter()
                    .find(|(key, _, _)| key == id)
                    .map_or(id.as_str(), |(_, label, _)| label)
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

pub(super) fn input(host: Host, title: &str) -> Result<String, Error> {
    let signals = host.signals;
    let mut session = host.session;
    let mut editor = Editor::new(65_536);
    editor.set_reject_overflow(true);
    loop {
        session = signals.check(session)?;
        let area = session.area();
        session.draw(|frame| {
            editor.render_with(
                frame,
                area,
                EditorView {
                    title,
                    footer: "Enter continue · Ctrl-J newline · Esc cancel",
                    presentation: presentation(),
                    ..EditorView::default()
                },
                true,
            );
        })?;
        let Some(event) = session.poll(Duration::from_millis(100))? else {
            continue;
        };
        if cancelled(&event) {
            session.finish("Input → cancelled")?;
            return Err(Error::Cancelled);
        }
        if entered(&event) && session.confirmation_available() {
            let text = editor.text().trim().to_owned();
            session.finish("Input → received")?;
            return Ok(text);
        }
        editor.update(&event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canonic_tui::{
        event::KeyEvent,
        ratatui::{Terminal, backend::TestBackend},
    };
    fn card(stale: bool) -> Card {
        Card {
            title: "pack add".into(),
            summary: "Install acme/git 1.0.0".into(),
            reason: "Initialize the repository.".into(),
            resources: vec![("asked after `git init`".into(), "codex".into())],
            details: "Change policy\n  + acme/git 1.0.0\n".into(),
            stale: stale.then(|| "Made against an older policy.".into()),
        }
    }
    fn chosen(review: &mut PermissionReview, keys: &[KeyCode]) -> Option<String> {
        for code in keys {
            review.update(&Event::Key(KeyEvent::new(*code, KeyModifiers::NONE)));
        }
        review
            .take_intents()
            .into_iter()
            .find_map(|intent| match intent {
                permission_review::Intent::Respond(response) => Some(response.choice),
                permission_review::Intent::Dismissed { .. } => None,
            })
    }
    #[test]
    fn proposals_are_chosen_without_ids_and_accepting_takes_a_second_check() {
        let mut review = PermissionReview::new(request(&card(false))).unwrap();
        assert_eq!(review.selected().map(|c| c.id.as_str()), Some("leave"));
        assert_eq!(
            chosen(&mut review, &[KeyCode::Char('2'), KeyCode::Enter]),
            Some("reject".into())
        );
        let mut review = PermissionReview::new(request(&card(false))).unwrap();
        // Enter on Accept opens the check on Back; only a deliberate second choice accepts.
        assert_eq!(
            chosen(&mut review, &[KeyCode::Char('1'), KeyCode::Enter]),
            None
        );
        assert!(review.confirming());
        assert_eq!(
            chosen(&mut review, &[KeyCode::Char('2'), KeyCode::Enter]),
            Some("accept".into())
        );
        // A stale proposal cannot be accepted.
        let mut review = PermissionReview::new(request(&card(true))).unwrap();
        assert_eq!(
            chosen(&mut review, &[KeyCode::Char('1'), KeyCode::Enter]),
            None
        );
        assert!(!review.confirming());
    }
    #[test]
    fn the_review_shows_the_change_reason_and_request_in_monochrome() {
        let mut terminal = Terminal::new(TestBackend::new(80, 14)).unwrap();
        let mut review = PermissionReview::new(request(&card(false))).unwrap();
        terminal
            .draw(|frame| {
                review.render_with(
                    frame,
                    frame.area(),
                    Presentation {
                        colors: ColorMode::NoColor,
                        ..Presentation::default()
                    },
                    1,
                );
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(canonic_tui::ratatui::buffer::Cell::symbol)
            .collect::<String>();
        for expected in [
            "Install acme/git 1.0.0",
            "Initialize the repository.",
            "git init",
            "Accept",
            "Reject",
            "Leave pending",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
    }
    #[test]
    fn the_accept_check_is_drawn_over_the_card() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut review = PermissionReview::new(request(&card(false))).unwrap();
        assert_eq!(
            chosen(&mut review, &[KeyCode::Char('1'), KeyCode::Enter]),
            None
        );
        terminal
            .draw(|frame| {
                let p = Presentation {
                    colors: ColorMode::NoColor,
                    ..Presentation::default()
                };
                review.render_with(frame, frame.area(), p, 0);
                review.render_overlay(frame, frame.area(), p);
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(canonic_tui::ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(text.contains("Back"), "{text}");
        assert!(text.contains("[ Install acme/git 1.0.0 ]"), "{text}");
        assert!(text.contains("active policy right away"), "{text}");
    }
    #[test]
    fn multiple_agents_are_staged_and_empty_confirmation_skips() {
        let mut picker = ChoicePicker::new(
            vec![
                Choice::new("claude", "Claude Code"),
                Choice::new("codex", "Codex"),
            ],
            None,
            SelectionMode::Multiple,
            Filter::Off,
        )
        .unwrap();
        let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
        assert_eq!(
            collected(&mut picker, &key(KeyCode::Enter), true),
            Some(vec![])
        );
        assert_eq!(collected(&mut picker, &key(KeyCode::Char('1')), true), None);
        assert_eq!(collected(&mut picker, &key(KeyCode::Char('2')), true), None);
        assert_eq!(collected(&mut picker, &key(KeyCode::Enter), false), None);
        assert_eq!(
            collected(&mut picker, &key(KeyCode::Enter), true),
            Some(vec!["claude".into(), "codex".into()])
        );
    }
    #[test]
    fn digits_highlight_and_only_visible_deliberate_enter_confirms() {
        let mut picker = ChoicePicker::new(
            vec![
                Choice::new("cancel", "Cancel"),
                Choice::new("apply", "Apply change"),
            ],
            Some("cancel"),
            SelectionMode::Single,
            Filter::Off,
        )
        .unwrap();
        let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
        assert_eq!(accepted(&mut picker, &key(KeyCode::Char('2')), true), None);
        assert_eq!(picker.selected(), Some("apply"));
        assert_eq!(accepted(&mut picker, &key(KeyCode::Enter), false), None);
        let mut repeat = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        repeat.kind = KeyEventKind::Repeat;
        assert_eq!(accepted(&mut picker, &Event::Key(repeat), true), None);
        assert_eq!(
            accepted(&mut picker, &key(KeyCode::Enter), true),
            Some("apply".into())
        );
    }
    #[test]
    fn narrow_monochrome_review_keeps_safe_choice_and_explicit_apply_label() {
        let mut terminal = Terminal::new(TestBackend::new(32, 10)).unwrap();
        let mut picker = ChoicePicker::new(
            vec![
                Choice::new("cancel", "Cancel"),
                Choice::new("apply", "Apply change"),
            ],
            Some("cancel"),
            SelectionMode::Single,
            Filter::Off,
        )
        .unwrap();
        terminal
            .draw(|frame| {
                picker.render(
                    frame,
                    frame.area(),
                    PickerView {
                        title: "Review",
                        context: "Project policy",
                        presentation: Presentation {
                            colors: ColorMode::NoColor,
                            ..Presentation::default()
                        },
                        ..PickerView::default()
                    },
                    true,
                );
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(canonic_tui::ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(text.contains("Cancel"));
        assert!(text.contains("Apply change"));
        assert_eq!(picker.selected(), Some("cancel"));
    }
}
