//! Atomic inline payloads travel with editor transactions and restart snapshots.

use super::{Draft, Editor, Outcome, history::EditKind};
use std::collections::BTreeMap;

/// An inline chip with a visible label and a complete host payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineAtom {
    pub label: String,
    pub content: String,
    /// Opaque host attachment identity; the component never reads image bytes.
    pub reference: Option<String>,
}

/// Lossless editor content. Marker characters are internal atomic cursor positions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub text: String,
    pub cursor: usize,
    pub atoms: BTreeMap<char, InlineAtom>,
}
impl Document {
    /// Visible chip labels, suitable for sanitized queue and stash previews.
    #[must_use]
    pub fn display_text(&self) -> String {
        self.text
            .chars()
            .map(|c| {
                self.atoms
                    .get(&c)
                    .map_or_else(|| c.to_string(), |atom| atom.label.clone())
            })
            .collect()
    }
}

impl Editor {
    /// Snapshot text, cursor and inline payloads without undo or terminal state.
    #[must_use]
    pub fn document(&self) -> Document {
        Document {
            text: self.text().into(),
            cursor: self.cursor(),
            atoms: self.draft.atoms.clone(),
        }
    }
    /// Expand chips for sending or an external editor.
    #[must_use]
    pub fn expanded_text(&self) -> String {
        self.text()
            .chars()
            .map(|c| {
                self.draft
                    .atoms
                    .get(&c)
                    .map_or_else(|| c.to_string(), |a| a.content.clone())
            })
            .collect()
    }
    /// Restore a validated document atomically; invalid input leaves the editor intact.
    pub fn restore_document(&mut self, document: &Document) -> bool {
        if document.atoms.len() > 256
            || document.atoms.iter().any(|(c, a)| {
                !(('\u{e000}'..='\u{f8ff}').contains(c))
                    || !document.text.contains(*c)
                    || a.label.is_empty()
                    || a.label.len() > 256
                    || a.label.chars().any(char::is_control)
                    || a.reference
                        .as_ref()
                        .is_some_and(|reference| reference.len() > 4096)
            })
        {
            return false;
        }
        let mut candidate = Self::new(self.limit);
        if candidate.set_text(&document.text)
            || candidate.text() != document.text
            || !candidate.restore_cursor(document.cursor)
        {
            return false;
        }
        candidate.draft.atoms.clone_from(&document.atoms);
        if candidate.expanded_text().len() > self.limit {
            return false;
        }
        *self = candidate;
        true
    }
    /// Replace a validated document as one undoable transaction.
    pub fn replace_document(&mut self, document: &Document) -> Outcome {
        let mut candidate = Self::new(self.limit);
        if !candidate.restore_document(document) {
            return Outcome::Rejected;
        }
        if candidate.draft.text == self.draft.text && candidate.draft.atoms == self.draft.atoms {
            return Outcome::Handled;
        }
        self.history.record(
            self.snapshot(),
            EditKind::Paste,
            self.now,
            self.selection().is_some(),
        );
        self.draft = candidate.draft;
        self.goal = None;
        Outcome::Changed
    }
    /// Insert an entire chip as one undo transaction, or retain the original draft.
    pub fn insert_atom(&mut self, atom: InlineAtom) -> Outcome {
        let range = self.selection().unwrap_or(self.cursor()..self.cursor());
        let removed: usize = self.text()[range.clone()]
            .chars()
            .map(|c| {
                self.draft
                    .atoms
                    .get(&c)
                    .map_or(c.len_utf8(), |a| a.content.len())
            })
            .sum();
        if self.draft.atoms.len() >= 256
            || atom.label.is_empty()
            || atom.label.len() > 256
            || atom.label.chars().any(char::is_control)
            || atom
                .reference
                .as_ref()
                .is_some_and(|reference| reference.len() > 4096)
            || self.expanded_text().len() - removed + atom.content.len() > self.limit
        {
            self.notify("Paste does not fit · draft kept");
            return Outcome::Rejected;
        }
        let marker = ('\u{e000}'..='\u{f8ff}').find(|c| !self.text().contains(*c));
        let Some(marker) = marker else {
            return Outcome::Rejected;
        };
        let snapshot = self.snapshot();
        self.history
            .record(snapshot, EditKind::Paste, self.now, !range.is_empty());
        self.draft
            .text
            .replace_range(range.clone(), &marker.to_string());
        self.draft.cursor = range.start + marker.len_utf8();
        self.draft.anchor = None;
        self.draft.atoms.retain(|c, _| self.draft.text.contains(*c));
        self.draft.atoms.insert(marker, atom);
        self.goal = None;

        Outcome::Changed
    }
    /// Replace a prefix before the cursor in one transaction (completion/newline).
    pub fn replace_before_cursor(&mut self, bytes: usize, text: &str) -> Outcome {
        let Some(start) = self.cursor().checked_sub(bytes) else {
            return Outcome::Rejected;
        };
        if self.boundary_after(start) != start {
            return Outcome::Rejected;
        }
        let removed = self.text()[start..self.cursor()]
            .chars()
            .map(|c| {
                self.draft
                    .atoms
                    .get(&c)
                    .map_or(c.len_utf8(), |a| a.content.len())
            })
            .sum::<usize>();
        if self.expanded_text().len() - removed + text.len() > self.limit {
            self.notify("Replacement does not fit · draft kept");
            return Outcome::Rejected;
        }
        self.draft.anchor = Some(start);
        self.replace(text, EditKind::Replace)
    }
}

impl Draft {
    pub(super) fn expanded_size(&self) -> usize {
        self.text
            .chars()
            .map(|c| self.atoms.get(&c).map_or(c.len_utf8(), |a| a.content.len()))
            .sum()
    }
}
