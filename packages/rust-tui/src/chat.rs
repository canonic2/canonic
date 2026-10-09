//! Owns a bounded conversation view and basic composer; hosts own agent execution.

use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    text::{clean, wrapped},
    theme::Theme,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Block, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

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

/// Reusable full-screen conversation composition with a basic append-only editor.
pub struct Chat {
    messages: Vec<Message>,
    input: String,
    status: String,
    scroll: u16,
    follow: bool,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
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
        &self.input
    }

    /// Restore a host-rejected draft or provide a fixture, within the input limit.
    pub fn set_input(&mut self, text: &str) {
        self.input = clean(text, INPUT_LIMIT, true);
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
            Event::Paste(text) => self.input.push_str(&clean(
                text,
                INPUT_LIMIT.saturating_sub(self.input.len()),
                true,
            )),
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Action::Cancel;
                }
                KeyCode::Enter
                    if key
                        .modifiers
                        .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
                {
                    if self.input.len() < INPUT_LIMIT {
                        self.input.push('\n');
                    }
                }
                KeyCode::Enter if !self.input.trim().is_empty() => {
                    return Action::Submit(std::mem::take(&mut self.input));
                }
                KeyCode::Backspace => {
                    if let Some((index, _)) = self.input.grapheme_indices(true).next_back() {
                        self.input.truncate(index);
                    }
                }
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && !character.is_control() =>
                {
                    if self.input.len() + character.len_utf8() <= INPUT_LIMIT {
                        self.input.push(character);
                    }
                }
                KeyCode::PageUp => {
                    self.follow = false;
                    self.scroll = self.scroll.saturating_sub(5);
                }
                KeyCode::PageDown => {
                    self.follow = false;
                    self.scroll = self.scroll.saturating_add(5);
                }
                KeyCode::End => self.follow = true,
                _ => {}
            },
            _ => {}
        }
        Action::None
    }

    /// Draw the composition. Transcript position follows output until manually scrolled.
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Theme) {
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
        let block = Block::bordered().title("Message");
        let inner = block.inner(composer);
        let mut rows = wrapped(&self.input, inner.width);
        if rows
            .last()
            .is_some_and(|line| line.width() >= usize::from(inner.width).max(1))
        {
            rows.push(String::new());
        }
        let offset = rows.len().saturating_sub(usize::from(inner.height));
        let cursor_x = rows.last().map_or(0, |line| line.width());
        let cursor_y = rows.len().saturating_sub(offset + 1);
        frame.render_widget(
            Paragraph::new(
                rows.into_iter()
                    .skip(offset)
                    .map(Line::raw)
                    .collect::<Vec<_>>(),
            )
            .block(block)
            .style(theme.base()),
            composer,
        );
        if inner.width > 0 && inner.height > 0 {
            frame.set_cursor_position((
                inner.x + u16::try_from(cursor_x).unwrap_or(0).min(inner.width - 1),
                inner.y + u16::try_from(cursor_y).unwrap_or(0).min(inner.height - 1),
            ));
        }
        frame.render_widget(
            Paragraph::new(
                "Enter send · Alt-Enter newline · Ctrl-X interrupt · PgUp/PgDn scroll · End follow",
            )
            .style(theme.muted()),
            hints,
        );
    }
}
