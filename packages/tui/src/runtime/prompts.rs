//! Explicit inline prompt hosts; failures never switch to ad-hoc terminal input.
use super::{Capabilities, InlineOptions, InlineSession};
use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    input::{Action, LineInput},
    picker::{Choice, ChoicePicker, Filter, Intent, PickerView, SelectionMode},
    theme::Presentation,
};
use std::{io, time::Duration};
mod suspend;

fn open(height: u16) -> io::Result<InlineSession> {
    let term = std::env::var("TERM").unwrap_or_default();
    if term.is_empty() || term == "dumb" {
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
    let cleanup = session.map_or(Ok(()), |session| session.finish(""));
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
        let area = open.area();
        open.draw(|frame| field.render(frame, area, text, title, hidden, presentation()))?;
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
        ChoiceSelection::Cancelled => Ok(Selection::Cancelled),
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
        ChoiceSelection::Back | ChoiceSelection::Cancelled => None,
    })
}

enum ChoiceSelection {
    Selected(String),
    Back,
    Cancelled,
}

fn select_choices(
    title: &str,
    context: &str,
    choices: Vec<Choice>,
    default: Option<&str>,
) -> io::Result<ChoiceSelection> {
    let mut picker = ChoicePicker::new(choices, default, SelectionMode::Single, Filter::Auto)
        .map_err(io::Error::other)?;
    if !picker.highlight(default) {
        picker.highlight(None);
    }
    let width = super::terminal_size().map_or(80, |(width, _)| width);
    let height = picker
        .height(
            width,
            PickerView {
                context,
                max_height: 12,
                ..PickerView::default()
            },
        )
        .saturating_add(1)
        .clamp(if context.is_empty() { 5 } else { 8 }, 12);
    let mut session = Some(open(height)?);
    let result = select_loop(&mut session, &mut picker, title, context);
    finish(session, result)
}
fn select_loop(
    session: &mut Option<InlineSession>,
    picker: &mut ChoicePicker,
    title: &str,
    context: &str,
) -> io::Result<ChoiceSelection> {
    loop {
        let open = session
            .as_mut()
            .ok_or_else(|| io::Error::other("Terminal is closed"))?;
        let area = open.area();
        open.draw(|frame| {
            if area.height >= 5 && area.width >= 12 {
                picker.render(
                    frame,
                    area,
                    PickerView {
                        title,
                        context: if area.height >= 8 { context } else { "" },
                        presentation: presentation(),
                        max_height: 12,
                        ..PickerView::default()
                    },
                    true,
                );
            } else {
                frame.render_widget(
                    crate::ratatui::widgets::Paragraph::new("esc cancels; enlarge terminal"),
                    area,
                );
            }
        })?;
        let Some(event) = open.poll(Duration::from_millis(100))? else {
            continue;
        };
        // Ctrl-Z is query undo while filtering.
        if suspend::requested(&event, !picker.filtering()) {
            suspend::suspend(session)?;
            continue;
        }
        let session = session
            .as_mut()
            .ok_or_else(|| io::Error::other("Terminal is closed"))?;
        if matches!(&event, Event::Key(key) if key.kind != KeyEventKind::Release && key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'd')))
        {
            return Ok(ChoiceSelection::Cancelled);
        }
        // Number keys highlight; only a later deliberate Enter can submit.
        if let Event::Key(key) = &event
            && !picker.filtering()
            && key.kind == KeyEventKind::Press
            && key.modifiers.is_empty()
            && let KeyCode::Char(c @ '1'..='9') = key.code
        {
            let id = picker
                .choices()
                .get((c as usize) - ('1' as usize))
                .map(|choice| choice.id.clone());
            if let Some(id) = id {
                picker.highlight(Some(&id));
            }
            continue;
        }
        match picker.update(&event) {
            Intent::Selected(id)
                if area.height >= 5 && area.width >= 12 && session.confirmation_available() =>
            {
                return Ok(ChoiceSelection::Selected(id));
            }
            Intent::Cancelled => {
                return Ok(ChoiceSelection::Back);
            }
            _ => {}
        }
    }
}
