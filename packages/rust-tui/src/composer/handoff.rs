//! Owns external-editor tickets, cancellation and explicit conflicting draft choices.

use super::{
    Composer, Identity, Version,
    lifecycle::{Intent, StoredDraft},
};
use crate::{
    dialog::{ClosePolicy, Dialog, DialogView, Size, Stack},
    editor::Editor,
    event::{Event, KeyCode},
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect, widgets::Paragraph};

/// Host editor request; hosts choose command, temp location and terminal suspension.
#[derive(Clone, Debug)]
pub struct EditorRequest {
    pub ticket: Identity,
    pub version: Version,
    pub text: String,
    /// Original attachment metadata for retaining image placeholders on return.
    pub document: crate::editor::Document,
    pub label: String,
}
/// An editor completion never implicitly replaces a conflicting newer draft.
#[derive(Clone, Debug)]
pub enum EditorResult {
    Text(String),
    Failed(String),
    Cancelled,
}
pub(super) struct Conflict {
    pub(super) returned: StoredDraft,
    selected: bool,
}

impl Composer {
    /// Emit one editor launch intent and lock ordinary input.
    pub fn begin_editor(&mut self, label: &str) -> Option<EditorRequest> {
        if self.external.is_some() || self.conflict.is_some() {
            return None;
        }
        self.restore_draft();
        let request = EditorRequest {
            ticket: Identity::new(),
            version: self.version.clone(),
            text: self.editor.expanded_text(),
            document: self.editor.document(),
            label: crate::text::clean(label, 64, false),
        };
        if !self.lock(&request.label) {
            return None;
        }
        self.external = Some(request.clone());
        self.intents.push(Intent::Editor(request.clone()));
        Some(request)
    }
    /// Resolve only the active editor ticket; empty/failing results keep the draft.
    pub fn finish_editor(&mut self, ticket: &Identity, result: EditorResult) -> bool {
        let Some(request) = &self.external else {
            return false;
        };
        if &request.ticket != ticket {
            return false;
        }
        let version = request.version.clone();
        let document = request.document.clone();
        self.external = None;
        self.unlock();
        match result {
            EditorResult::Cancelled => self.editor.notify("Editor cancelled · draft kept"),
            EditorResult::Failed(reason) => self.editor.notify(&format!(
                "Editor failed · {} · draft kept",
                crate::text::clean(&reason, 256, false)
            )),
            EditorResult::Text(text) => {
                let mut editor = Editor::new(self.limit);
                if text.is_empty() || editor.set_text(&text) {
                    self.editor
                        .notify("Empty or oversized editor result · draft kept");
                    return true;
                }
                Self::restore_editor_images(&mut editor, &document);
                let returned = StoredDraft {
                    editor,
                    mode: self.input_mode,
                    at: self.now,
                    age_seconds: 0,
                };
                if version == self.version {
                    self.install_draft(returned);
                    self.advance();
                    self.editor.notify("Editor text restored");
                } else {
                    self.conflict = Some(Conflict {
                        returned,
                        selected: true,
                    });
                }
            }
        }
        true
    }
    pub(super) fn cancel_editor(&mut self) {
        self.external = None;
        self.unlock();
        self.intents.push(Intent::Stop);
        self.editor
            .notify("Stopped waiting for editor · draft kept");
    }
    /// Pick the editor text (`true`) or the current box (`false`); stash the other.
    pub fn resolve_editor_conflict(&mut self, use_editor: bool) -> bool {
        let Some(conflict) = self.conflict.take() else {
            return false;
        };
        if use_editor {
            let current = self.take_visible_draft();
            self.push_stash(current);
            self.install_draft(conflict.returned);
        } else {
            self.push_stash(conflict.returned);
        }
        self.advance();
        self.editor
            .notify("Conflict resolved · other version stashed");
        true
    }
    pub(super) fn conflict_input(&mut self, event: &Event) {
        let Event::Key(key) = event else {
            return;
        };
        if key.kind != crate::event::KeyEventKind::Press || !key.modifiers.is_empty() {
            return;
        }
        match key.code {
            KeyCode::Char('1') => {
                self.resolve_editor_conflict(true);
            }
            KeyCode::Char('2') | KeyCode::Esc => {
                self.resolve_editor_conflict(false);
            }
            KeyCode::Up | KeyCode::Down | KeyCode::Tab => {
                if let Some(conflict) = &mut self.conflict {
                    conflict.selected = !conflict.selected;
                }
            }
            KeyCode::Enter => {
                let selected = self.conflict.as_ref().is_some_and(|c| c.selected);
                self.resolve_editor_conflict(selected);
            }
            _ => {}
        }
    }
    pub fn render_overlay(&self, frame: &mut Frame<'_>, area: Rect, presentation: Presentation) {
        let Some(conflict) = &self.conflict else {
            return;
        };
        let Ok(mut stack) = Stack::new(vec![0_usize]) else {
            return;
        };
        if stack.push(Dialog {
            id: "composer-editor-conflict".into(), title: "draft changed meanwhile".into(),
            body: (), targets: vec![0], size: Size::default(), close: ClosePolicy::default(),
            view: DialogView { message: "The box changed while the editor was open. Which version goes in the box?".into(),
                footer: "1 editor · 2 box · enter chooses · esc keeps box · other version is stashed".into(),
                compact_footer: "1–2 choose · esc keeps box".into(), ..DialogView::default() },
        }).is_err() { return; }
        stack.render_with(
            frame,
            area,
            presentation,
            |(), _, _| 2,
            |(), frame, area, _, _| {
                let lines = vec![
                    ratatui::text::Line::styled(
                        format!(
                            "1 the editor's · {} lines",
                            conflict.returned.editor.text().lines().count()
                        ),
                        if conflict.selected {
                            presentation.selection()
                        } else {
                            presentation.base()
                        },
                    ),
                    ratatui::text::Line::styled(
                        format!(
                            "2 the message box's · {} lines",
                            self.editor.text().lines().count()
                        ),
                        if conflict.selected {
                            presentation.base()
                        } else {
                            presentation.selection()
                        },
                    ),
                ];
                frame.render_widget(Paragraph::new(lines), area);
                None
            },
        );
    }
}
