//! Owns box modes and the untouched editor set aside during host-driven recall.

use super::{Composer, InputMode};
use crate::{
    editor::{Editor, EditorView},
    event::{Event, KeyCode, KeyEventKind},
    ui::Rule,
};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
};

/// The identity of the content currently displayed in the message box.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BoxMode {
    /// The session's editable draft.
    #[default]
    Draft,
    /// A host-supplied queued message; positions are one-based.
    Queued { position: usize, total: usize },
    /// A host-supplied history copy; positions are one-based.
    History { position: usize, total: usize },
    /// Host editor handoff; input is suspended, without launching a process.
    Locked { editor: String },
}

/// Host-persistable draft text and grapheme cursor; storage remains outside the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftCheckpoint {
    /// Exact logical editor text.
    pub text: String,
    /// UTF-8 cursor byte offset, on a grapheme boundary.
    pub cursor: usize,
}

impl Composer {
    /// Current box content identity, independent of submission acknowledgement.
    #[must_use]
    pub fn mode(&self) -> &BoxMode {
        &self.mode
    }

    /// Snapshot the session draft, including while a recall copy is on screen.
    #[must_use]
    pub fn checkpoint(&self) -> DraftCheckpoint {
        let editor = self
            .set_aside
            .as_ref()
            .map_or(&self.editor, |draft| &draft.editor);
        DraftCheckpoint {
            text: editor.text().into(),
            cursor: editor.cursor(),
        }
    }

    /// Restore host storage atomically. Invalid boundaries or oversized text do nothing.
    pub fn restore_checkpoint(&mut self, checkpoint: &DraftCheckpoint) -> bool {
        let mut editor = Editor::new(self.limit);
        if editor.set_text(&checkpoint.text)
            || editor.text() != checkpoint.text
            || !editor.restore_cursor(checkpoint.cursor)
        {
            return false;
        }
        self.restore_draft();
        self.editor = editor;
        self.advance();
        true
    }

    /// Display a bounded host-owned recall entry, setting the entire draft editor aside.
    /// The host retains source entries. Recall starts only from a normal draft.
    /// Use `browse_history` to navigate the loaded history rather than a single copy.
    pub fn recall(&mut self, mode: BoxMode, text: &str) -> bool {
        let (BoxMode::Queued { position, total } | BoxMode::History { position, total }) = &mode
        else {
            return false;
        };
        if *position == 0 || *position > *total || !matches!(self.mode, BoxMode::Draft) {
            return false;
        }
        let mut editor = Editor::new(self.limit);
        if editor.set_text(text) {
            return false;
        }
        self.set_aside = Some(self.take_visible_draft());
        self.editor = editor;
        self.mode = mode;
        true
    }

    /// Restore the untouched draft, cursor, selection, viewport and undo history.
    /// Returns false when there is no recall copy to dismiss.
    pub fn restore_draft(&mut self) -> bool {
        let Some(draft) = self.set_aside.take() else {
            return false;
        };
        self.install_draft(draft);
        self.mode = BoxMode::Draft;
        self.history_browsing = false;
        self.editing_queue = None;
        true
    }

    /// Suspend input for a host-owned editor. Only a normal draft can be locked.
    pub fn lock(&mut self, editor: &str) -> bool {
        if self.mode != BoxMode::Draft {
            return false;
        }
        self.mode = BoxMode::Locked {
            editor: crate::text::clean(editor, 64, false),
        };
        true
    }

    /// End a host editor suspension without replacing the draft.
    pub fn unlock(&mut self) -> bool {
        if !matches!(self.mode, BoxMode::Locked { .. }) {
            return false;
        }
        self.mode = BoxMode::Draft;
        true
    }

    /// Drafts protected when editing a history copy, newest first.
    /// At most fifty drafts are kept; inserting the fifty-first evicts the oldest.
    #[must_use]
    pub fn stashed_drafts(&self) -> Vec<DraftCheckpoint> {
        self.stash
            .iter()
            .rev()
            .map(|draft| DraftCheckpoint {
                text: draft.editor.text().into(),
                cursor: draft.editor.cursor(),
            })
            .collect()
    }

    pub(super) fn mode_input(&mut self, event: &Event) -> bool {
        if matches!(self.mode, BoxMode::Locked { .. }) {
            return true;
        }
        if self.history_input(event) {
            return true;
        }
        if let Event::Key(key) = event
            && key.kind != KeyEventKind::Release
            && key.modifiers.is_empty()
        {
            if key.code == KeyCode::Esc && self.restore_draft() {
                return true;
            }
            if key.code == KeyCode::Enter
                && self.mode != BoxMode::Draft
                && !self.history_browsing
                && self.editing_queue.is_none()
            {
                return true;
            }
        }
        matches!(self.mode, BoxMode::Queued { .. }) && self.editing_queue.is_none()
    }

    pub(super) fn adopt_recall(&mut self) {
        if matches!(self.mode, BoxMode::History { .. }) {
            if let Some(draft) = self.set_aside.take()
                && !draft.editor.text().is_empty()
            {
                self.push_stash(draft);
                self.notify("Draft stashed · original draft kept");
            }
            self.mode = BoxMode::Draft;
            self.history_browsing = false;
        }
    }

    pub(super) fn render_mode(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        view: EditorView<'_>,
        focused: bool,
    ) {
        let p = view.presentation;
        let view = EditorView {
            highlight_file_references: self.input_mode == InputMode::Message,
            ..view
        };
        self.editor.render_with(
            frame,
            area,
            view,
            focused && !self.overlay_open() && !matches!(self.mode, BoxMode::Locked { .. }),
        );
        if matches!(self.mode, BoxMode::Locked { .. }) {
            frame.buffer_mut().set_style(area, p.muted());
        }
        if !view.chrome || area.is_empty() {
            return;
        }
        let (title, mut right, glyph, style) = match &self.mode {
            BoxMode::Draft => (
                if self.input_mode == InputMode::Shell {
                    "shell"
                } else {
                    "message"
                },
                if self.input_mode == InputMode::Shell {
                    format!("runs in {}", self.repository_label)
                } else {
                    self.draft_rule_hint(p.symbols)
                },
                p.symbols.rule(),
                if self.input_mode == InputMode::Shell {
                    p.warning()
                } else {
                    p.message_line()
                },
            ),
            BoxMode::Queued { position, total } => (
                "queued",
                format!("{position} of {total}"),
                p.symbols.recall_rule(),
                p.info_line(),
            ),
            BoxMode::History { position, total } => (
                "history",
                format!(
                    "{position} of {total}{}{}",
                    p.symbols.separator(),
                    self.active_history()
                        .get(position - 1)
                        .and_then(|text| self.history_origins.get(text))
                        .map_or("esc your draft", String::as_str)
                ),
                p.symbols.recall_rule(),
                p.info_line(),
            ),
            BoxMode::Locked { editor } => (
                "message",
                format!("in {editor}"),
                p.symbols.rule(),
                p.muted(),
            ),
        };
        self.scroll_hint(&mut right, area, p.symbols);
        let title_style = match self.mode {
            BoxMode::Queued { .. } | BoxMode::History { .. } => {
                p.info_line().remove_modifier(ratatui::style::Modifier::DIM)
            }
            BoxMode::Locked { .. } => p.muted(),
            BoxMode::Draft => {
                if self.input_mode == InputMode::Shell {
                    p.warning()
                } else {
                    p.accent()
                }
            }
        };
        frame.render_widget(
            Rule {
                left: Line::from(Span::styled(title, title_style)),
                right: Line::from(Span::styled(right, p.muted())),
                glyph,
                style,
            },
            Rect::new(area.x, area.y, area.width, 1),
        );
        Self::render_footer(frame, area, view);
    }
}

impl Composer {
    fn scroll_hint(&self, right: &mut String, area: Rect, symbols: crate::theme::Symbols) {
        if matches!(self.mode, BoxMode::Locked { .. }) {
            return;
        }
        let above = self.editor.first_visible_row();
        let below = self
            .editor
            .visual_rows(area.width, symbols)
            .saturating_sub(above + usize::from(area.height.saturating_sub(2)));
        if above == 0 && below == 0 {
            return;
        }
        let more = format!(
            "{}{}{}{}",
            if above > 0 {
                format!("{} {above} more", symbols.more(true))
            } else {
                String::new()
            },
            if above > 0 && below > 0 {
                symbols.separator()
            } else {
                ""
            },
            if below > 0 { symbols.more(false) } else { "" },
            if below > 0 {
                format!(" {below} more")
            } else {
                String::new()
            }
        );
        if right.is_empty() {
            *right = more;
        } else {
            right.push_str(symbols.separator());
            right.push_str(&more);
        }
    }
}

impl Composer {
    fn render_footer(frame: &mut Frame<'_>, area: Rect, view: EditorView<'_>) {
        let p = view.presentation;
        if area.height > 1 && !view.footer.is_empty() {
            frame.render_widget(
                Rule {
                    left: Line::default(),
                    right: Line::from(Span::styled(
                        crate::text::clean(view.footer, 256, false),
                        p.tokens()
                            .info
                            .map_or(p.heading(), |color| p.heading().fg(color)),
                    )),
                    glyph: p.symbols.rule(),
                    style: p.message_line(),
                },
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
    }
}
