//! Owns a bounded conversation view and basic composer; hosts own agent execution.

use crate::{
    composer::{Composer, Identity, State, Submission},
    editor::EditorView,
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    text::{clean, wrapped},
    theme::{Presentation, Theme},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

const INPUT_LIMIT: usize = 65_536;
const MESSAGE_LIMIT: usize = 65536;
const HISTORY_LIMIT: usize = 100;

/// Interaction returned to the host application.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// No product operation was requested.
    None,
    /// Submit this snapshot; the host must acknowledge its ticket through the composer.
    Submit(Submission),
    /// Request cancellation of the active operation.
    Cancel,
}

struct Message {
    role: String,
    text: String,
    timestamp: Option<String>,
    delivery: Option<(Identity, Option<String>)>,
    shell: Option<(Identity, bool)>,
}

impl Message {
    fn visible_text(&self) -> String {
        use std::fmt::Write;
        if self.shell.as_ref().is_none_or(|(_, expanded)| *expanded) {
            return self.text.clone();
        }
        let count = self.text.lines().count();
        let mut text = self.text.lines().take(20).collect::<Vec<_>>().join("\n");
        if count > 20 {
            let _ = write!(text, "\n+{} more lines", count - 20);
        }
        text
    }
}

/// Reusable conversation composition with a bounded multiline editor.
pub struct Chat {
    messages: Vec<Message>,
    input: Composer,
    status: String,
    scroll: u16,
    follow: bool,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            input: Composer::new(INPUT_LIMIT),
            status: String::new(),
            scroll: 0,
            follow: true,
        }
    }
}

impl Chat {
    /// Create an empty conversation view with automatic transcript following.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Composer contents, before submission.
    #[must_use]
    pub fn input(&self) -> &str {
        self.input.editor().text()
    }

    /// Explicitly replace the draft within its lifetime, preserving any pending snapshot.
    pub fn set_input(&mut self, text: &str) {
        self.input.set_text(text);
    }

    /// Draft lifecycle owner; hosts acknowledge returned submission tickets here.
    pub fn composer_mut(&mut self) -> &mut Composer {
        &mut self.input
    }

    /// Inspect the independent draft lifecycle without mutating it.
    #[must_use]
    pub fn composer(&self) -> &Composer {
        &self.input
    }

    /// Dismiss only editor selection state, without cancelling or exiting chat.
    /// Returns whether there was selection state to dismiss.
    pub fn dismiss_selection(&mut self) -> bool {
        let selected = self.input.editor().selection().is_some();
        self.input.update(&Event::Key(crate::event::KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        )));
        selected
    }

    /// Number of retained messages (at most 100).
    #[must_use]
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Update the host-provided activity/connection label.
    pub fn set_status(&mut self, status: &str) {
        self.status = clean(status, 256, false);
    }

    /// Add a message, retaining the newest 100 and at most 64 KiB per message.
    pub fn push(&mut self, role: &str, text: &str) {
        if self.messages.len() == HISTORY_LIMIT {
            self.messages.remove(0);
        }
        self.messages.push(Message {
            role: clean(role, 128, false),
            text: clean(text, MESSAGE_LIMIT, true),
            timestamp: None,
            delivery: None,
            shell: None,
        });
    }

    /// Add a normal host turn with a dim display timestamp.
    pub fn push_turn(&mut self, role: &str, text: &str, timestamp: &str) {
        self.push(role, text);
        if let Some(message) = self.messages.last_mut() {
            message.timestamp = Some(clean(timestamp, 32, false));
        }
    }

    /// Project a fixed submitted turn with a host-supplied display timestamp.
    /// Execution and transport time remain host-owned.
    pub fn push_submission(&mut self, submission: &Submission, timestamp: &str) {
        let shell = submission.mode == crate::composer::InputMode::Shell;
        self.push(if shell { "shell" } else { "you" }, &submission.text);
        if let Some(message) = self.messages.last_mut() {
            if shell {
                message.role = clean(&format!("$ {}", submission.text), 128, false);
                message.text.clear();
                message.shell = Some((submission.ticket.clone(), false));
            }
            message.timestamp = Some(clean(timestamp, 32, false));
            message.delivery = Some((submission.ticket.clone(), None));
        }
    }

    /// Acknowledge the pending intent and update its turn, preserving the fresh draft.
    /// Returns false for a stale or foreign response without changing the transcript.
    pub fn acknowledge_submission(&mut self, ticket: &Identity, result: Result<(), &str>) -> bool {
        if !self.input.acknowledge(ticket, result) {
            return false;
        }
        for message in &mut self.messages {
            if message
                .delivery
                .as_ref()
                .is_some_and(|(id, _)| id == ticket)
            {
                message.delivery = result
                    .err()
                    .map(|reason| (ticket.clone(), Some(clean(reason, 256, false))));
                break;
            }
        }
        true
    }
    /// Replace host shell output for its exact turn; retain up to 64 KiB, collapsed at 20 lines.
    pub fn set_shell_output(&mut self, ticket: &Identity, output: &str) -> bool {
        let Some(message) = self
            .messages
            .iter_mut()
            .find(|message| message.shell.as_ref().is_some_and(|(id, _)| id == ticket))
        else {
            return false;
        };
        message.text = clean(output, MESSAGE_LIMIT, true);
        true
    }
    /// Expand/collapse an existing shell output turn without executing anything.
    pub fn toggle_shell_output(&mut self, ticket: &Identity) -> bool {
        let Some((_, expanded)) = self
            .messages
            .iter_mut()
            .filter_map(|message| message.shell.as_mut())
            .find(|(id, _)| id == ticket)
        else {
            return false;
        };
        *expanded = !*expanded;
        true
    }

    /// Append to the latest host message, skipping fixed submitted user turns.
    pub fn append(&mut self, text: &str) {
        if let Some(message) = self
            .messages
            .iter_mut()
            .rev()
            .find(|message| message.timestamp.is_none())
        {
            message.text.push_str(&clean(
                text,
                MESSAGE_LIMIT.saturating_sub(message.text.len()),
                true,
            ));
        }
    }

    /// Translate input to a product-independent action. Paste never submits.
    pub fn update(&mut self, event: &Event) -> Action {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::PageUp if key.modifiers.contains(KeyModifiers::ALT) => {
                    self.follow = false;
                    self.scroll = self.scroll.saturating_sub(5);
                }
                KeyCode::PageDown if key.modifiers.contains(KeyModifiers::ALT) => {
                    self.follow = false;
                    self.scroll = self.scroll.saturating_add(5);
                }
                KeyCode::End if key.modifiers.contains(KeyModifiers::ALT) => self.follow = true,
                _ => {
                    if let Some(submission) = self.input.update(event) {
                        return Action::Submit(submission);
                    }
                }
            },
            _ => {
                self.input.update(event);
            }
        }
        if self.input.take_stop() {
            Action::Cancel
        } else {
            Action::None
        }
    }

    /// Draw the composition. Transcript position follows output until manually scrolled.
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Theme) {
        self.render_with(frame, area, theme.presentation());
    }

    /// Render with explicit terminal capabilities and design tokens.
    pub fn render_with(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        self.render_composition(frame, area, theme, true);
    }
    /// Render an unframed conversation with a fixed composer dock and host footer.
    pub fn render_docked_with(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        self.render_composition(frame, area, theme, false);
    }
    fn render_composition(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: Presentation,
        framed: bool,
    ) {
        let [status, transcript, activity, composer, hints] = Layout::vertical([
            Constraint::Length(u16::from(framed)),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .areas(area);
        frame.render_widget(
            Paragraph::new(format!("Chat · {}", self.status)).style(theme.focus()),
            status,
        );
        let [transcript, above] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(self.input.above_height_for(area.width)),
        ])
        .areas(transcript);
        self.input.render_above(frame, above, theme);
        let block = if framed {
            Block::bordered().title("Conversation")
        } else {
            Block::default()
        };
        let inner = block.inner(transcript);
        let mut lines = Vec::new();
        for message in &self.messages {
            let role_style = if message.timestamp.is_some() {
                theme.base().add_modifier(Modifier::BOLD)
            } else {
                theme.focus()
            };
            let mut header = vec![Span::styled(message.role.clone(), role_style)];
            if let Some(time) = &message.timestamp {
                header.push(Span::styled(
                    format!("{}{}", theme.symbols.separator(), time),
                    theme.muted().add_modifier(Modifier::DIM),
                ));
            }
            if let Some((_, reason)) = &message.delivery {
                header.push(Span::styled(
                    format!(
                        "{}{}",
                        theme.symbols.separator(),
                        reason.as_ref().map_or_else(
                            || "sending".into(),
                            |reason| format!("not sent · {reason}")
                        )
                    ),
                    if reason.is_none() {
                        theme.muted().add_modifier(Modifier::DIM)
                    } else {
                        theme.error()
                    },
                ));
            }
            lines.push(Line::from(header));
            let output = message.visible_text();
            lines.extend(wrapped(&output, inner.width).into_iter().map(Line::raw));
            lines.push(Line::raw(""));
        }
        let maximum = u16::try_from(lines.len().saturating_sub(usize::from(inner.height)))
            .unwrap_or(u16::MAX);
        self.scroll = if self.follow {
            maximum
        } else {
            self.scroll.min(maximum)
        };
        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .scroll((self.scroll, 0))
                .style(theme.base()),
            transcript,
        );
        self.render_activity(frame, activity, theme);
        self.input.render_with(
            frame,
            composer,
            EditorView {
                title: "message",
                placeholder: "Write a message",
                presentation: theme,
                footer: if framed { "" } else { &self.status },
                ..EditorView::default()
            },
            true,
        );
        frame.render_widget(
            Paragraph::new(self.input_hint(theme, hints.width)).style(theme.muted()),
            hints,
        );
        self.input.render_overlay(frame, area, theme);
    }

    fn render_activity(&self, frame: &mut Frame<'_>, activity: Rect, theme: Presentation) {
        let activity_style = if self.input.queue_paused() && !self.input.queue().is_empty() {
            theme.warning()
        } else {
            match self.input.state() {
                State::Rejected { .. } => theme.error(),
                State::Pending(_) | State::Accepted | State::Ready => theme.muted(),
            }
        };
        frame.render_widget(
            Paragraph::new(if self.input.completion_menu().is_some() {
                self.input_hint(theme, activity.width)
            } else {
                self.input
                    .status()
                    .replace(" · ", theme.symbols.separator())
            })
            .style(activity_style),
            activity,
        );
    }
    fn input_hint(&self, theme: Presentation, width: u16) -> String {
        let hint = self.input.footer_hint_for(width);
        hint.replace(" · ", theme.symbols.separator())
            .replace(
                "↑↓",
                if theme.symbols == crate::theme::Symbols::Ascii {
                    "up/down"
                } else {
                    "↑↓"
                },
            )
            .replace('↑', theme.symbols.more(true))
    }

    /// Advance editor messages and undo timing from the host's clock.
    pub fn tick(&mut self, elapsed: std::time::Duration) {
        self.input.tick(elapsed);
    }
}
