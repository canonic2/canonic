//! Owns a bounded conversation view and basic composer; hosts own agent execution.

use crate::{
    editor::{Editor, EditorView},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    text::{clean, wrapped},
    theme::{Presentation, Theme},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Block, Paragraph},
};

const INPUT_LIMIT: usize = 4096;
const MESSAGE_LIMIT: usize = 65536;
const HISTORY_LIMIT: usize = 100;

/// Interaction returned to the host application.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// No product operation was requested.
    None,
    /// Submit this text to the product's backend.
    Submit(String),
    /// Request cancellation of the active operation.
    Cancel,
}

struct Message {
    role: String,
    text: String,
}

/// Reusable conversation composition with a bounded multiline editor.
pub struct Chat {
    messages: Vec<Message>,
    input: Editor,
    status: String,
    scroll: u16,
    follow: bool,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            input: Editor::new(INPUT_LIMIT),
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
        self.input.text()
    }

    /// Restore a host-rejected draft or provide a fixture, within the input limit.
    pub fn set_input(&mut self, text: &str) {
        self.input.set_text(text);
    }

    /// Dismiss only editor selection state, without cancelling or exiting chat.
    /// Returns whether there was selection state to dismiss.
    pub fn dismiss_selection(&mut self) -> bool {
        self.input.update(&Event::Key(crate::event::KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        ))) != crate::editor::Outcome::Unhandled
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
        });
    }

    /// Append a streamed text fragment to the last message, within its limit.
    pub fn append(&mut self, text: &str) {
        if let Some(message) = self.messages.last_mut() {
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
                KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Action::Cancel;
                }
                KeyCode::Enter
                    if key.modifiers.is_empty()
                        && key.kind == KeyEventKind::Press
                        && !self.input.text().trim().is_empty() =>
                {
                    let text = self.input.text().to_owned();
                    self.input.set_text("");
                    return Action::Submit(text);
                }
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
                    self.input.update(event);
                }
            },
            _ => {
                self.input.update(event);
            }
        }
        Action::None
    }

    /// Draw the composition. Transcript position follows output until manually scrolled.
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Theme) {
        self.render_with(frame, area, theme.presentation());
    }

    /// Render with explicit terminal capabilities and design tokens.
    pub fn render_with(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        let [status, transcript, composer, hints] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .areas(area);
        frame.render_widget(
            Paragraph::new(format!("Chat · {}", self.status)).style(theme.focus()),
            status,
        );
        let block = Block::bordered().title("Conversation");
        let inner = block.inner(transcript);
        let mut lines = Vec::new();
        for message in &self.messages {
            lines.extend(
                wrapped(&message.role, inner.width)
                    .into_iter()
                    .map(|line| Line::styled(line, theme.focus())),
            );
            lines.extend(
                wrapped(&message.text, inner.width)
                    .into_iter()
                    .map(Line::raw),
            );
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
        self.input.render_with(
            frame,
            composer,
            EditorView {
                title: "message",
                placeholder: "Write a message",
                presentation: theme,
                ..EditorView::default()
            },
            true,
        );
        frame.render_widget(
            Paragraph::new(
                "Enter send · Ctrl-J newline · Ctrl-X interrupt · Alt-PgUp/PgDn scroll · Alt-End follow",
            )
            .style(theme.muted()),
            hints,
        );
    }

    /// Advance editor messages and undo timing from the host's clock.
    pub fn tick(&mut self, elapsed: std::time::Duration) {
        self.input.tick(elapsed);
    }
}
