//! Explicit typing prefixes, paste chips and submit-time invisible-character cleanup.

use super::{BoxMode, Composer, InputMode};
use crate::{
    editor::{AtomKind, InlineAtom, Outcome},
    event::{Event, KeyCode, KeyEventKind},
};

fn hidden(c: char) -> bool {
    matches!(c, '\u{200b}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}')
}

impl Composer {
    /// Insert a host clipboard image reference; no clipboard access or image decoding.
    pub fn insert_image(&mut self, reference: &str) -> bool {
        self.insert_image_with_size(reference, None)
    }
    /// Insert an opaque host image with optional pixel dimensions for its label.
    pub fn insert_image_with_size(
        &mut self,
        reference: &str,
        dimensions: Option<(u32, u32)>,
    ) -> bool {
        if !matches!(
            self.mode,
            BoxMode::Draft | BoxMode::History { .. } | BoxMode::Queued { .. }
        ) {
            return false;
        }
        let number = self
            .editor
            .document()
            .atoms
            .values()
            .filter(|a| a.kind == AtomKind::Image)
            .count()
            + 1;
        let changed = self.editor.insert_atom(InlineAtom {
            kind: AtomKind::Image,
            label: dimensions.map_or_else(
                || format!("[image {number}]"),
                |(width, height)| format!("[image {number} · {width}×{height}]"),
            ),
            content: format!("[Image {number}]"),
            reference: Some(reference.into()),
        }) == Outcome::Changed;
        if changed {
            self.adopt_recall();
            self.advance();
        }
        changed
    }
    pub(super) fn payload_input(&mut self, event: &Event) -> bool {
        if matches!(self.mode, BoxMode::Locked { .. }) {
            return false;
        }
        if let Event::Paste(text) = event {
            if text.lines().count() > 3 || text.chars().count() > 800 {
                let lines = text.split('\n').count();
                let number = self
                    .editor
                    .document()
                    .atoms
                    .values()
                    .filter(|a| a.kind == AtomKind::Paste)
                    .count()
                    + 1;
                if self.editor.insert_atom(InlineAtom {
                    kind: AtomKind::Paste,
                    label: format!("[paste {number} · {lines} lines]"),
                    content: text.replace("\r\n", "\n").replace('\r', "\n"),
                    reference: None,
                }) == Outcome::Changed
                {
                    self.adopt_recall();
                    self.advance();
                }
                return true;
            }
            return false;
        }
        let Event::Key(key) = event else {
            return false;
        };
        if key.kind != KeyEventKind::Press || !key.modifiers.is_empty() {
            return false;
        }
        if key.code == KeyCode::Char('!')
            && self.editor.text().is_empty()
            && self.input_mode == InputMode::Message
        {
            self.input_mode = InputMode::Shell;
            self.advance();
            return true;
        }
        if key.code == KeyCode::Backspace
            && self.editor.text().is_empty()
            && self.input_mode == InputMode::Shell
        {
            self.input_mode = InputMode::Message;
            self.advance();
            return true;
        }
        if self.input_mode == InputMode::Message && matches!(key.code, KeyCode::Char('/' | '@')) {
            let cursor = self
                .editor
                .selection()
                .map_or(self.editor.cursor(), |range| range.start);
            let boundary =
                cursor == 0 || self.editor.text()[..cursor].ends_with(char::is_whitespace);
            if (key.code == KeyCode::Char('/') && cursor == 0)
                || (key.code == KeyCode::Char('@') && boundary)
            {
                if self.editor.update(event) != Outcome::Changed {
                    return true;
                }
                self.adopt_recall();
                self.advance();
                self.open_completion(if key.code == KeyCode::Char('/') {
                    super::CompletionKind::Command
                } else {
                    super::CompletionKind::File
                });
                return true;
            }
        }
        if self.input_mode == InputMode::Shell && matches!(key.code, KeyCode::Char('/' | '~')) {
            let before = &self.editor.text()[..self.editor.cursor()];
            let word = before
                .split_whitespace()
                .next_back()
                .filter(|_| !before.ends_with(char::is_whitespace))
                .unwrap_or("");
            if word.contains(['/', '~']) || key.code == KeyCode::Char('~') && !word.is_empty() {
                return false;
            }
            if self.editor.update(event) == Outcome::Changed {
                self.advance();
                self.open_completion(super::CompletionKind::File);
            }
            return true;
        }
        false
    }
    pub(super) fn clean_before_send(&mut self) -> bool {
        let mut document = self.editor.document();
        let mut count = document.text.chars().filter(|c| hidden(*c)).count();
        document.cursor -= document.text[..document.cursor]
            .chars()
            .filter(|c| hidden(*c))
            .map(char::len_utf8)
            .sum::<usize>();
        document.text.retain(|c| !hidden(c));
        for atom in document.atoms.values_mut() {
            count += atom.content.chars().filter(|c| hidden(*c)).count();
            atom.content.retain(|c| !hidden(c));
        }
        if count == 0 {
            return false;
        }
        self.editor.replace_document(&document);
        self.adopt_recall();
        self.advance();
        self.editor.notify(&format!(
            "Removed {count} hidden characters · enter again sends"
        ));
        true
    }
}

impl Composer {
    pub(super) fn restore_editor_images(
        editor: &mut crate::editor::Editor,
        original: &crate::editor::Document,
    ) {
        let mut document = editor.document();
        for atom in original
            .atoms
            .values()
            .filter(|atom| matches!(atom.kind, AtomKind::Image | AtomKind::File))
        {
            if !document.text.contains(&atom.content) {
                continue;
            }
            let Some(marker) =
                ('\u{e000}'..='\u{f8ff}').find(|marker| !document.text.contains(*marker))
            else {
                continue;
            };
            document.text = document.text.replace(&atom.content, &marker.to_string());
            document.atoms.insert(marker, atom.clone());
        }
        document.cursor = document.text.len();
        let _ = editor.restore_document(&document);
    }
}
