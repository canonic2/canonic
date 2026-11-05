//! Shield descriptors and native signal checks over the shared prompt host.
use super::{Card, Step};
use crate::review::Error;
use canonic_tui_kit::{
    permission_review::{self, Kind, Resource},
    picker::Choice,
    runtime::{
        InlineSession,
        prompts::{self, Outcome, PickerPrompt},
    },
    tool_call_card::Output,
};
mod signals;
pub(super) type Host = prompts::Host<'static>;

pub(super) fn mcp_action(host: Host) -> Result<prompts::setup::Action, Error> {
    let mut prompt = prompts::setup::action_prompt()?;
    let ids = step(host.pick(&mut prompt)?)?.or_cancel()?;
    match ids.first().map(String::as_str) {
        Some("install") => Ok(prompts::setup::Action::Install),
        Some("remove") => Ok(prompts::setup::Action::Remove),
        _ => Err(Error::Invalid("setup action missing")),
    }
}
pub(super) fn agents(
    host: Host,
    choices: Vec<Choice>,
    marked: &[&str],
    remove: bool,
    back: bool,
) -> Result<Step<Vec<String>>, Error> {
    let mut prompt = prompts::setup::agent_prompt(choices, marked, remove)?.back(back);
    first_step(step(host.pick(&mut prompt)?)?, back)
}
/// The picker reports Escape as Back even when it names it as cancel; without
/// an earlier step to return to, that is a cancellation.
fn first_step<T>(step: Step<T>, back: bool) -> Result<Step<T>, Error> {
    match step {
        Step::Back if !back => Err(Error::Cancelled),
        step => Ok(step),
    }
}
pub(super) fn confirm_mcp(host: Host) -> Result<(), Error> {
    let mut prompt = prompts::setup::confirmation_prompt(
        "The review above lists the hooks, MCP entries, permissions and files to change. Cancel keeps your configuration.",
    )?;
    let ids = step(host.pick(&mut prompt)?)?.or_cancel()?;
    if ids.first().is_some_and(|id| id == "apply") {
        Ok(())
    } else {
        Err(Error::Cancelled)
    }
}

pub(super) fn open(height: u16) -> Result<Host, Error> {
    let signals = signals::Signals::new()?;
    let host = match Host::open(height) {
        Ok(host) => host,
        Err(_) if signals.cancelled() => return Err(Error::Cancelled),
        Err(error) => return Err(error.into()),
    };
    Ok(host.with_checkpoint(move |session| {
        signals.check(session).map_err(|error| match error {
            Error::Cancelled => {
                std::io::Error::new(std::io::ErrorKind::Interrupted, "operator cancelled")
            }
            Error::Io(error) => error,
            other => std::io::Error::other(other),
        })
    }))
}
fn choices(rows: &[(&str, &str, &str)]) -> Vec<Choice> {
    rows.iter()
        .map(|(id, label, detail)| {
            let mut choice = Choice::new(*id, *label);
            choice.detail = (*detail).into();
            choice
        })
        .collect()
}
fn step<T>(outcome: Outcome<T>) -> Result<Step<T>, Error> {
    match outcome {
        Outcome::Submitted(value) => Ok(Step::Done(value)),
        Outcome::Back => Ok(Step::Back),
        Outcome::Cancelled => Err(Error::Cancelled),
    }
}
pub(super) fn choose(
    host: Host,
    title: &str,
    context: &str,
    rows: &[(&str, &str, &str)],
    default: &str,
) -> Result<String, Error> {
    choose_step(host, title, context, rows, default, "cancel")?.or_cancel()
}
pub(super) fn choose_step(
    host: Host,
    title: &str,
    context: &str,
    rows: &[(&str, &str, &str)],
    default: &str,
    escape: &str,
) -> Result<Step<String>, Error> {
    let mut prompt =
        PickerPrompt::single(title, context, choices(rows), Some(default))?.back(escape == "back");
    Ok(match step(host.pick(&mut prompt)?)? {
        Step::Done(ids) => Step::Done(
            ids.into_iter()
                .next()
                .ok_or(Error::Invalid("prompt choice missing"))?,
        ),
        Step::Back => Step::Back,
    })
}

pub(super) fn choose_many_step(
    host: Host,
    title: &str,
    context: &str,
    rows: &[(&str, &str, &str)],
    marked: &[&str],
    empty: &str,
    escape: &str,
) -> Result<Step<Vec<String>>, Error> {
    let mut prompt = PickerPrompt::multiple(title, context, choices(rows), marked, 0, empty)?
        .back(escape == "back");
    step(host.pick(&mut prompt)?)
}
pub(super) fn question(
    host: Host,
    question: &super::Question,
    check: &mut super::Check<'_>,
) -> Result<Step<super::Reply>, Error> {
    step(host.question(question, check)?)
}
pub(super) fn input(host: Host, title: &str) -> Result<String, Error> {
    Ok(step(host.edit(title, 65_536)?)?
        .or_cancel()?
        .trim()
        .to_owned())
}
pub(super) fn review(host: Host, card: &Card, waiting: usize) -> Result<String, Error> {
    Ok(match step(host.review(request(card), waiting)?)? {
        Step::Done(choice) => choice,
        Step::Back => "leave".into(),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use canonic_tui_kit::{
        event::{Event, KeyCode, KeyEvent, KeyModifiers},
        permission_review::PermissionReview,
        ratatui::{Terminal, backend::TestBackend},
        theme::{ColorMode, Presentation},
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
    fn escape_on_the_agent_page_goes_back_only_when_an_earlier_page_exists() {
        // Guided setup came from the action page: Escape returns to it.
        assert_eq!(
            first_step(Step::<Vec<String>>::Back, true).unwrap(),
            Step::Back
        );
        // `--remove` and its dry run start on the agent page: Escape cancels.
        assert!(matches!(
            first_step(Step::<Vec<String>>::Back, false),
            Err(Error::Cancelled)
        ));
        let marked = Step::Done(vec!["claude".to_owned()]);
        assert_eq!(first_step(marked.clone(), false).unwrap(), marked);
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
            .map(canonic_tui_kit::ratatui::buffer::Cell::symbol)
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
            .map(canonic_tui_kit::ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(text.contains("Back"), "{text}");
        assert!(text.contains("[ Install acme/git 1.0.0 ]"), "{text}");
        assert!(text.contains("active policy right away"), "{text}");
    }
}
