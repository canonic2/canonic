//! Owns Unicode draft edits, transactions and viewport state; hosts own time and focus.

mod history;
mod input;
mod layout;
mod view;

use crate::{
    event::Event,
    theme::{Symbols, Theme},
};
use history::{EditKind, History, Snapshot};
use ratatui::{Frame, layout::Rect};
use std::{ops::Range, time::Duration};
use unicode_segmentation::UnicodeSegmentation;
pub use view::EditorView;

/// Default draft byte budget.
pub const DEFAULT_LIMIT: usize = 4096;
const NOTICE_TIME: Duration = Duration::from_secs(3);

/// Input outcome; no outcome submits or executes content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// No editor binding.
    Unhandled,
    /// Consumed without editing.
    Handled,
    /// Edited, possibly cutting input at the byte limit; inspect the notice.
    Changed,
    /// No complete grapheme fits; draft and selection remain intact.
    Rejected,
}

#[derive(Clone, Default)]
struct Draft {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
    upstream: bool,
}

struct Notice {
    text: String,
    remaining: Duration,
}

/// An instance-local multiline editor with a bounded, grouped undo history.
pub struct Editor {
    draft: Draft,
    history: History,
    limit: usize,
    width: u16,
    height: usize,
    offset: usize,
    goal: Option<usize>,
    symbols: Symbols,
    rejected: bool,
    notice: Option<Notice>,
    flash: Option<Range<usize>>,
    now: Duration,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT)
    }
}

impl Editor {
    /// Create an empty editor with an explicit byte budget, including zero.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            draft: Draft::default(),
            history: History::default(),
            limit,
            width: 80,
            height: 1,
            offset: 0,
            goal: None,
            symbols: Symbols::Unicode,
            rejected: false,
            notice: None,
            flash: None,
            now: Duration::ZERO,
        }
    }
    /// Current text, stored as UTF-8 with LF line endings.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.draft.text
    }
    /// Caret byte offset on a grapheme boundary.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.draft.cursor
    }
    /// Nonempty selected byte range.
    #[must_use]
    pub fn selection(&self) -> Option<Range<usize>> {
        self.draft
            .anchor
            .filter(|a| *a != self.cursor())
            .map(|a| a.min(self.cursor())..a.max(self.cursor()))
    }
    /// Whether input has reached or crossed the byte limit since the last edit making room.
    #[must_use]
    pub fn rejected(&self) -> bool {
        self.rejected
    }
    /// Current transient edit message; the persistent limit status is separate.
    #[must_use]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_ref().map(|n| n.text.as_str())
    }
    /// Replace from the host, truncate at a whole grapheme and reset editor state.
    /// Returns true when text was truncated.
    pub fn set_text(&mut self, text: &str) -> bool {
        let normalized = layout::normalize(text);
        let end = Self::fitting_end(&normalized, self.limit);
        self.draft = Draft {
            text: normalized[..end].into(),
            cursor: end,
            anchor: None,
            upstream: false,
        };
        self.history = History::default();
        self.offset = 0;
        self.goal = None;
        self.notice = None;
        self.flash = None;
        self.rejected = end < normalized.len();
        self.rejected
    }
    fn fitting_end(text: &str, budget: usize) -> usize {
        text.grapheme_indices(true)
            .map(|(i, g)| i + g.len())
            .take_while(|end| *end <= budget)
            .last()
            .unwrap_or(0)
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            draft: self.draft.clone(),
            offset: self.offset,
        }
    }
    fn message(&mut self, text: String) {
        self.notice = Some(Notice {
            text,
            remaining: NOTICE_TIME,
        });
    }
    /// Advance the injected clock. Does not acquire timers or terminal resources.
    pub fn tick(&mut self, elapsed: Duration) {
        self.now = self.now.saturating_add(elapsed);
        if let Some(notice) = &mut self.notice {
            notice.remaining = notice.remaining.saturating_sub(elapsed);
            if notice.remaining.is_zero() {
                self.notice = None;
                self.flash = None;
            }
        }
    }
    /// Insert a complete payload as one undo transaction. Never submits.
    /// Oversized input retains the prefix ending at the last whole grapheme that fits.
    pub fn insert(&mut self, text: &str) -> Outcome {
        self.before_key();
        self.replace(text, EditKind::Paste)
    }
    fn replace(&mut self, text: &str, kind: EditKind) -> Outcome {
        let text = layout::normalize(text);
        let range = self.selection().unwrap_or(self.cursor()..self.cursor());
        let budget = self.limit.saturating_sub(self.text().len() - range.len());
        let end = Self::fitting_end(&text, budget);
        let dropped = text.len() - end;
        if end == 0 && !text.is_empty() {
            self.rejected = true;
            self.message(format!(
                "byte limit reached · {} / {} bytes",
                self.text().len(),
                self.limit
            ));
            return Outcome::Rejected;
        }
        if range.is_empty() && text.is_empty() {
            return Outcome::Handled;
        }
        let old_len = self.text().len();
        let snapshot = self.snapshot();
        self.history
            .record(snapshot, kind, self.now, !range.is_empty());
        self.draft.text.replace_range(range.clone(), &text[..end]);
        self.draft.cursor = self.boundary_after(range.start + end);
        self.draft.upstream = false;
        self.draft.anchor = None;
        self.goal = None;
        if self.text().len() < old_len {
            self.rejected = false;
        }
        self.rejected |= dropped > 0 || self.text().len() == self.limit;
        if kind == EditKind::Paste {
            self.flash = Some(range.start..self.cursor());
            self.message(if dropped > 0 {
                format!("paste cut at limit · {dropped} bytes left out")
            } else {
                format!(
                    "pasted {} lines · ctrl+z undoes it",
                    text[..end].split('\n').count()
                )
            });
        }
        if text[..end].ends_with(char::is_whitespace) {
            self.history.break_group();
        }
        Outcome::Changed
    }
    fn before_key(&mut self) {
        self.notice = None;
        self.flash = None;
    }
    fn boundary_after(&self, position: usize) -> usize {
        self.text()
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= position)
            .unwrap_or(self.text().len())
    }
    fn previous(&self) -> usize {
        self.text()
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .take_while(|i| *i < self.cursor())
            .last()
            .unwrap_or(0)
    }
    fn next(&self) -> usize {
        self.text()
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i > self.cursor())
            .unwrap_or(self.text().len())
    }
    fn move_to(&mut self, position: usize, extend: bool) {
        self.history.break_group();
        if extend {
            self.draft.anchor.get_or_insert(self.cursor());
        } else {
            self.draft.anchor = None;
        }
        self.draft.cursor = position;
        self.draft.upstream = false;
    }
    fn horizontal(&mut self, right: bool, word: bool, extend: bool) {
        let target = if !extend && !word && self.selection().is_some() {
            self.selection()
                .map_or(self.cursor(), |r| if right { r.end } else { r.start })
        } else if word {
            self.word_boundary(right)
        } else if right {
            self.next()
        } else {
            self.previous()
        };
        self.move_to(target, extend);
        self.goal = None;
    }
    fn word_boundary(&self, right: bool) -> usize {
        let words: Vec<_> = self.text().unicode_word_indices().collect();
        if right {
            words
                .iter()
                .map(|(i, w)| i + w.len())
                .find(|i| *i > self.cursor())
                .map_or(self.text().len(), |i| self.boundary_after(i))
        } else {
            words
                .iter()
                .map(|(i, _)| *i)
                .take_while(|i| *i < self.cursor())
                .last()
                .map_or(0, |i| self.boundary_after(i))
        }
    }
    fn visual_edge(&self, end: bool) -> usize {
        let rows = layout::rows(self.text(), self.width, self.symbols);
        let (row, _) = layout::position(&rows, self.cursor(), self.width, self.draft.upstream);
        if end { rows[row].end } else { rows[row].start }
    }
    fn vertical(&mut self, down: bool, extend: bool, amount: usize) {
        let rows = layout::rows(self.text(), self.width, self.symbols);
        let (row, column) = layout::position(&rows, self.cursor(), self.width, self.draft.upstream);
        let goal = *self.goal.get_or_insert(column);
        let target = if down {
            row.saturating_add(amount).min(rows.len() - 1)
        } else {
            row.saturating_sub(amount)
        };
        self.move_to(rows[target].at_column(goal), extend);
        self.draft.upstream =
            self.cursor() == rows[target].end && rows[target].start < self.cursor();
    }
    fn delete_range(&mut self, range: Range<usize>, kind: EditKind) -> Outcome {
        if range.is_empty() {
            return Outcome::Handled;
        }
        let snapshot = self.snapshot();
        self.history
            .record(snapshot, kind, self.now, self.selection().is_some());
        self.draft.text.replace_range(range.clone(), "");
        self.draft.cursor = self.boundary_after(range.start);
        self.draft.upstream = false;
        self.draft.anchor = None;
        self.goal = None;
        self.rejected = false;
        Outcome::Changed
    }
    fn delete(&mut self, forward: bool, word: bool) -> Outcome {
        if let Some(range) = self.selection() {
            return self.delete_range(range, EditKind::Delete);
        }
        let cursor = self.cursor();
        let end = if word {
            self.word_boundary(forward)
        } else if forward {
            self.next()
        } else {
            self.previous()
        };
        self.delete_range(
            cursor.min(end)..cursor.max(end),
            if forward {
                EditKind::Delete
            } else {
                EditKind::Backspace
            },
        )
    }
    fn history(&mut self, redo: bool) -> Outcome {
        let snapshot = self.snapshot();
        let Some((snapshot, kind)) = self.history.restore(snapshot, redo) else {
            return Outcome::Handled;
        };
        self.draft = snapshot.draft;
        self.offset = snapshot.offset;
        self.goal = None;
        self.rejected = false;
        self.message(format!(
            "{} {} · {}",
            if redo { "redid" } else { "undid" },
            kind.label(),
            if redo { "ctrl+z undo" } else { "ctrl+y redo" }
        ));
        Outcome::Changed
    }
    /// Handle editor bindings; plain Enter remains a host action.
    pub fn update(&mut self, event: &Event) -> Outcome {
        self.input(event)
    }
    /// Render with default host labels and terminal-native styles.
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Theme, focused: bool) {
        self.render_with(
            frame,
            area,
            EditorView {
                presentation: theme.presentation(),
                ..EditorView::default()
            },
            focused,
        );
    }
    /// Render with explicit host labels and semantic design-system inputs.
    pub fn render_with(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        view: EditorView<'_>,
        focused: bool,
    ) {
        self.view(frame, area, view, focused);
    }
}
