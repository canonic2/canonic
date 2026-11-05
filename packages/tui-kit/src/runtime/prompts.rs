//! Explicit inline prompt hosts; failures never switch to ad-hoc terminal input.
use super::{Capabilities, InlineOptions, InlineSession};
use crate::{
    input::{Action, LineInput},
    picker::Choice,
    theme::Presentation,
};
use std::{io, time::Duration};
mod guided;
pub mod setup;
mod suspend;
pub use guided::{Check, Checkpoint, Host, Outcome, PickerPrompt, Question, Reply};

fn open(height: u16) -> io::Result<InlineSession> {
    let term = std::env::var("TERM").unwrap_or_default();
    // Windows consoles expose cursor operations through the native API and
    // normally have no TERM. Lease acquisition verifies the actual console.
    if (term.is_empty() && !cfg!(windows)) || term == "dumb" {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "interactive TUI requires a cursor-capable TERM",
        ));
    }
    let session = InlineSession::open(InlineOptions {
        height,
        capabilities: Capabilities {
            cursor: true,
            alternate: false,
        },
    })?;
    if height >= 5 && session.area().height < 5 {
        return finish(
            Some(session),
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "window too small for choices; enlarge the terminal and retry",
            )),
        );
    }
    Ok(session)
}
fn presentation() -> Presentation {
    super::profile::detect()
}
/// Result of a single-line prompt. Cancellation never implies submission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// A deliberate Enter submitted the caller's buffer.
    Submitted,
    /// Escape requested a previous step.
    Back,
    /// Ctrl-C or Ctrl-D cancelled the flow.
    Cancelled,
}
/// Edit caller-owned text without history or settled secret output. The buffer
/// is reserved to `limit` bytes before editing, so edits never reallocate it and
/// leave no stray copies; zeroizing it afterwards is the caller's job.
/// `tick` runs between bounded polls and can print settled output through the
/// supplied session. It must not acquire another terminal or write around its region.
/// # Errors
/// Invalid defaults and terminal/tick failures are returned after restoration.
/// A refused insertion leaves the buffer unchanged, is shown in the field title
/// and editing continues.
pub fn read(
    label: &str,
    text: &mut String,
    hidden: bool,
    limit: usize,
    tick: &mut dyn FnMut(&mut InlineSession) -> io::Result<()>,
) -> io::Result<Answer> {
    if !LineInput::valid(text, limit) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Input must be a bounded single line",
        ));
    }
    // Reserve before any secret is typed: later inserts stay within capacity.
    text.reserve_exact(limit - text.len());
    let mut field = LineInput::new(text, limit);
    let mut session = Some(open(3)?);
    let result = read_loop(&mut session, &mut field, label, text, hidden, tick);
    finish(session, result)
}
fn finish<T>(session: Option<InlineSession>, result: io::Result<T>) -> io::Result<T> {
    finish_record(session, result, "")
}
fn finish_record<T>(
    session: Option<InlineSession>,
    result: io::Result<T>,
    record: &str,
) -> io::Result<T> {
    let cleanup = session.map_or(Ok(()), |session| session.finish(record));
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(io::Error::other(format!(
            "{error}; terminal restoration: {cleanup}"
        ))),
    }
}
fn read_loop(
    session: &mut Option<InlineSession>,
    field: &mut LineInput,
    label: &str,
    text: &mut String,
    hidden: bool,
    tick: &mut dyn FnMut(&mut InlineSession) -> io::Result<()>,
) -> io::Result<Answer> {
    let refused = format!("{label} · refused: one line within the limit");
    let mut title = label;
    loop {
        let open = session
            .as_mut()
            .ok_or_else(|| io::Error::other("Terminal is closed"))?;
        tick(open)?;
        open.draw(|frame| field.render(frame, frame.area(), text, title, hidden, presentation()))?;
        let Some(event) = open.poll(Duration::from_millis(100))? else {
            continue;
        };
        // The line field has no undo, so plain Ctrl-Z suspends.
        if suspend::requested(&event, true) {
            suspend::suspend(session)?;
            continue;
        }
        let session = session
            .as_mut()
            .ok_or_else(|| io::Error::other("Terminal is closed"))?;
        let answer = match field.update(text, &event) {
            Action::None => {
                title = label;
                continue;
            }
            // Refuse only this insertion; the buffer is unchanged and editing continues.
            Action::Invalid => {
                title = &refused;
                continue;
            }
            Action::Submit if session.confirmation_available() => Answer::Submitted,
            Action::Submit => continue,
            Action::Back => Answer::Back,
            Action::Cancel => Answer::Cancelled,
        };
        return Ok(answer);
    }
}
/// Choose an original index; cancellation returns `None` and never selects a default.
/// # Errors
/// Returns terminal acquisition, rendering, input and restoration errors.
pub fn choose(
    title: &str,
    choices: Vec<String>,
    default: Option<usize>,
) -> io::Result<Option<usize>> {
    Ok(match select(title, choices, default)? {
        Selection::Selected(index) => Some(index),
        Selection::Back | Selection::Cancelled => None,
    })
}
/// Selection intent, retaining the distinction between Escape and interruption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    /// Original index confirmed with Enter.
    Selected(usize),
    /// Escape requested a previous step.
    Back,
    /// Ctrl-C or Ctrl-D cancelled the flow.
    Cancelled,
}
/// Choose while retaining guided-flow cancellation semantics.
/// # Errors
/// Returns terminal acquisition, rendering, input and restoration errors.
pub fn select(title: &str, choices: Vec<String>, default: Option<usize>) -> io::Result<Selection> {
    let choices = choices
        .into_iter()
        .enumerate()
        .map(|(index, label)| Choice::new(index.to_string(), label))
        .collect();
    match select_choices(
        title,
        "",
        choices,
        default.map(|index| index.to_string()).as_deref(),
    )? {
        ChoiceSelection::Selected(id) => {
            Ok(Selection::Selected(id.parse().map_err(io::Error::other)?))
        }
        ChoiceSelection::Back => Ok(Selection::Back),
        ChoiceSelection::Confirmed | ChoiceSelection::Cancelled => Ok(Selection::Cancelled),
    }
}

/// Choose a stable identity with separate labels, details and current-value marks.
/// No implicit first choice is highlighted when `default` is absent or invalid.
/// # Errors
/// Returns invalid choices and terminal acquisition, input or restoration errors.
pub fn choose_choices(
    title: &str,
    context: &str,
    choices: Vec<Choice>,
    default: Option<&str>,
) -> io::Result<Option<String>> {
    Ok(match select_choices(title, context, choices, default)? {
        ChoiceSelection::Selected(id) => Some(id),
        ChoiceSelection::Confirmed | ChoiceSelection::Back | ChoiceSelection::Cancelled => None,
    })
}

/// Mark several stable identities, starting from `marked`. Space marks the
/// highlighted row, typing filters and Enter confirms the marked set. With
/// `minimum` 0 an empty set confirms and, while nothing is marked, the footer
/// reads `enter <empty>`, such as `enter continue` or `enter skip`; below a
/// nonzero minimum Enter stays in the list and a notice row names Space.
/// Escape and interruption return `None` and confirm nothing.
/// # Errors
/// Returns invalid choices, a `minimum` above the enabled choices
/// ([`io::ErrorKind::InvalidInput`], before acquiring the terminal) and terminal
/// acquisition, input or restoration errors.
pub fn choose_many(
    title: &str,
    context: &str,
    choices: Vec<Choice>,
    marked: &[&str],
    minimum: usize,
    empty: &str,
) -> io::Result<Option<Vec<String>>> {
    let mut prompt = PickerPrompt::multiple(title, context, choices, marked, minimum, empty)?;
    Ok(match Host::open(16)?.pick(&mut prompt)? {
        Outcome::Submitted(ids) => Some(ids),
        Outcome::Back | Outcome::Cancelled => None,
    })
}

enum ChoiceSelection {
    Selected(String),
    Confirmed,
    Back,
    Cancelled,
}

fn select_choices(
    title: &str,
    context: &str,
    choices: Vec<Choice>,
    default: Option<&str>,
) -> io::Result<ChoiceSelection> {
    let mut prompt = PickerPrompt::single(title, context, choices, default)?;
    Ok(match Host::open(12)?.pick(&mut prompt)? {
        Outcome::Submitted(ids) => ids
            .into_iter()
            .next()
            .map_or(ChoiceSelection::Confirmed, ChoiceSelection::Selected),
        Outcome::Back => ChoiceSelection::Back,
        Outcome::Cancelled => ChoiceSelection::Cancelled,
    })
}
