//! Owns bounded host-supplied sent history and editor-first recall navigation.

use super::{BoxMode, Composer, InputMode};
use crate::{
    editor::{Document, Editor},
    event::{Event, KeyCode, KeyEventKind},
    theme::Symbols,
};

/// Repository history supplied by the host, including earlier-session provenance.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub document: Document,
    pub mode: InputMode,
    /// None means this session; examples of host labels: yesterday, 3 weeks ago.
    pub origin: Option<String>,
}

impl Composer {
    /// Load full entries. This-session entries precede earlier sessions, stably.
    /// Paste payloads and shell/message modes remain attached to their entries.
    pub fn set_history_entries(&mut self, mut entries: Vec<HistoryEntry>) -> bool {
        if self.history_browsing || entries.len() > 500 {
            return false;
        }
        let mut editor = Editor::new(self.limit);
        if entries
            .iter()
            .any(|entry| !editor.restore_document(&entry.document))
        {
            return false;
        }
        entries.sort_by_key(|entry| entry.origin.is_some());
        self.history.clear();
        self.shell_history.clear();
        self.history_documents.clear();
        self.history_origins.clear();
        for entry in entries {
            editor.restore_document(&entry.document);
            let text = editor.expanded_text();
            let history = if entry.mode == InputMode::Shell {
                &mut self.shell_history
            } else {
                &mut self.history
            };
            if history.contains(&text) {
                continue;
            }
            history.push(text.clone());
            if let Some(origin) = entry.origin {
                self.history_origins
                    .insert(text.clone(), crate::text::clean(&origin, 64, false));
            }
            self.history_documents.insert(text, entry.document);
        }
        true
    }
    /// Load repository history, newest first, without storage I/O.
    /// At most 500 entries must fit the editor exactly. Invalid input or replacement
    /// during active browsing changes nothing. Hosts retain session metadata/pastes.
    pub fn set_history(&mut self, entries: Vec<String>) -> bool {
        if self.history_browsing || entries.len() > 500 {
            return false;
        }
        let mut editor = Editor::new(self.limit);
        if entries
            .iter()
            .any(|text| editor.set_text(text) || editor.text() != text)
        {
            return false;
        }
        self.history = entries;
        self.history_documents
            .retain(|text, _| self.history.contains(text) || self.shell_history.contains(text));
        self.history_origins
            .retain(|text, _| self.history.contains(text) || self.shell_history.contains(text));
        true
    }

    /// Sent history for host persistence, newest first.
    #[must_use]
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Recall a one-based position from loaded history, preserving the session draft.
    /// Subsequent plain Up/Down browse older/newer; Down past newest restores the draft.
    pub fn browse_history(&mut self, position: usize) -> bool {
        let Some(text) = position
            .checked_sub(1)
            .and_then(|i| self.active_history().get(i))
            .cloned()
        else {
            return false;
        };
        if self.mode == BoxMode::Draft {
            if !self.recall(
                BoxMode::History {
                    position,
                    total: self.active_history().len(),
                },
                &text,
            ) {
                return false;
            }
        } else if self.history_browsing && matches!(self.mode, BoxMode::History { .. }) {
            self.editor.set_text(&text);
            self.mode = BoxMode::History {
                position,
                total: self.active_history().len(),
            };
        } else {
            return false;
        }
        if let Some(document) = self.history_documents.get(&text) {
            self.editor.restore_document(document);
        }
        self.editor.restore_cursor(0);
        self.history_browsing = true;
        true
    }

    pub(super) fn history_input(&mut self, event: &Event) -> bool {
        let Event::Key(key) = event else {
            return false;
        };
        if key.kind == KeyEventKind::Release || !key.modifiers.is_empty() {
            return false;
        }
        match (&self.mode, key.code) {
            (BoxMode::History { position, .. }, KeyCode::Up | KeyCode::Down)
                if self.history_browsing =>
            {
                let position = *position;
                if key.code == KeyCode::Down && position == 1 {
                    self.restore_draft();
                } else {
                    let next = if key.code == KeyCode::Up {
                        position.saturating_add(1)
                    } else {
                        position - 1
                    };
                    if !self.browse_history(next) {
                        self.editor.notify("That's the oldest message");
                    }
                }
                true
            }
            (BoxMode::Draft, KeyCode::Up) if self.editor.cursor() == 0 => {
                if !self.edit_queued(self.queue.len()) && !self.browse_history(1) {
                    self.editor.notify("No sent messages yet");
                }
                true
            }
            (BoxMode::Draft, KeyCode::Up) if self.editor.on_first_visual_row() => {
                self.editor.restore_cursor(0);
                true
            }
            (BoxMode::Queued { position, .. }, KeyCode::Up | KeyCode::Down)
                if self.editing_queue.is_some() =>
            {
                let position = *position;
                self.restore_draft();
                if key.code == KeyCode::Down {
                    if position < self.queue.len() {
                        self.edit_queued(position + 1);
                    }
                } else if position > 1 {
                    self.edit_queued(position - 1);
                } else {
                    self.browse_history(1);
                }
                true
            }
            _ => false,
        }
    }

    pub(super) fn draft_rule_hint(&self, symbols: Symbols) -> String {
        if let Some(atom) = self.editor.text()[self.editor.cursor()..]
            .chars()
            .next()
            .and_then(|c| self.editor.document().atoms.get(&c).cloned())
        {
            return format!("{} · {} bytes", atom.label, atom.content.len());
        }
        let mut parts = Vec::new();
        if let Some((arm, _)) = self.arm {
            parts.push(
                match arm {
                    super::lifecycle::Arm::Clear => "esc again clears",
                    super::lifecycle::Arm::Rewind => "esc again edits last message",
                    super::lifecycle::Arm::Quit('d') => "ctrl+d again quits",
                    super::lifecycle::Arm::Quit(_) => "ctrl+c again quits",
                }
                .into(),
            );
        } else if self.editor.cursor() == 0
            && (!self.active_history().is_empty() || !self.queue.is_empty())
        {
            parts.push(format!("{} history", symbols.more(true)));
        } else if self.returned.is_some() {
            parts.push("returned".into());
        }
        if !self.stash.is_empty() {
            parts.push(format!("{} stashed", self.stash.len()));
        }
        parts.join(symbols.separator())
    }

    pub(super) fn active_history(&self) -> &[String] {
        if self.input_mode == InputMode::Shell {
            &self.shell_history
        } else {
            &self.history
        }
    }
    pub(super) fn remember_sent(&mut self, text: String) {
        self.remember_in_mode(text, self.input_mode);
    }
    pub(super) fn remember_in_mode(&mut self, text: String, mode: InputMode) {
        let history = if mode == InputMode::Shell {
            &mut self.shell_history
        } else {
            &mut self.history
        };
        history.retain(|entry| entry != &text);
        history.insert(0, text);
        history.truncate(500);
        self.history_origins.remove(&history[0]);
        self.history_origins
            .retain(|key, _| self.history.contains(key) || self.shell_history.contains(key));
        self.history_documents
            .retain(|key, _| self.history.contains(key) || self.shell_history.contains(key));
        if self.history_browsing {
            if let Some(index) = self
                .active_history()
                .iter()
                .position(|entry| entry == &self.editor.expanded_text())
            {
                self.mode = BoxMode::History {
                    position: index + 1,
                    total: self.active_history().len(),
                };
            } else {
                self.restore_draft();
            }
        }
    }
}
