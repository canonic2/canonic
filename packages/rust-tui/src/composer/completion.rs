//! Host-owned command/file candidates, selected above the fixed composer dock.

use super::{Composer, Identity, Version};
use crate::{
    event::{Event, KeyCode, KeyEventKind},
    picker::{Choice, ChoicePicker, Filter, Intent, PickerView, SelectionMode},
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect};

/// Prefix-triggered completion interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionKind {
    Command,
    File,
}
/// Identity of one completion opening and query revision; late results are ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionRequest {
    pub ticket: Identity,
    pub version: Version,
    pub kind: CompletionKind,
    pub query: String,
}
/// A host candidate. Command IDs are returned unchanged; file values are plain text.
#[derive(Clone, Debug)]
pub struct CompletionItem {
    pub id: String,
    pub value: String,
    pub detail: String,
    pub kind: CompletionKind,
    pub disabled: Option<String>,
    pub touched: bool,
}
pub(super) struct Completion {
    pub ticket: Identity,
    pub kind: CompletionKind,
    pub start: usize,
    pub picker: ChoicePicker,
}

impl Composer {
    /// Replace host candidates; IDs must be unique within each completion kind.
    pub fn set_completions(&mut self, items: Vec<CompletionItem>) -> bool {
        if self.completion.is_some() {
            return false;
        }
        self.replace_completion_items(items)
    }
    fn replace_completion_items(&mut self, items: Vec<CompletionItem>) -> bool {
        if items.len() > 5000
            || items.iter().enumerate().any(|(i, item)| {
                item.id.is_empty()
                    || item.value.is_empty()
                    || item.value.chars().any(char::is_control)
                    || items[..i]
                        .iter()
                        .any(|other| other.kind == item.kind && other.id == item.id)
            })
        {
            return false;
        }
        let mut items = items;
        items.sort_by_key(|item| !item.touched);
        self.completion_items = items;
        true
    }
    pub(super) fn open_completion(&mut self, kind: CompletionKind) {
        let choices = self
            .completion_items
            .iter()
            .filter(|item| item.kind == kind)
            .map(|item| {
                let mut choice = Choice::new(&item.id, &item.value);
                choice.detail.clone_from(&item.detail);
                choice.disabled.clone_from(&item.disabled);
                choice
            })
            .collect();
        if let Ok(picker) = ChoicePicker::new(choices, None, SelectionMode::Single, Filter::On) {
            self.completion = Some(Completion {
                ticket: Identity::new(),
                kind,
                start: self.editor.cursor().saturating_sub(1),
                picker,
            });
        }
    }
    pub(super) fn completion_input(&mut self, event: &Event) {
        if matches!(event, Event::Paste(_)) {
            self.completion = None;
            self.update(event);
            return;
        }
        let Some(completion) = &mut self.completion else {
            return;
        };
        if matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press && (key.modifiers.is_empty() && key.code == KeyCode::Esc || key.modifiers == crate::event::KeyModifiers::CONTROL && key.code == KeyCode::Char('c')))
        {
            self.completion = None;
            return;
        }
        if matches!(event, Event::Key(key) if key.kind != KeyEventKind::Release && matches!(key.code, KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End))
        {
            self.completion = None;
            self.update(event);
            return;
        }
        if matches!(event, Event::Key(key) if key.kind != KeyEventKind::Release && (key.code == KeyCode::Delete || key.modifiers == crate::event::KeyModifiers::CONTROL && matches!(key.code, KeyCode::Char('z' | 'y' | '-' | '.' | 'w' | 'u' | 'k' | 'g' | 'a' | 'e' | 'd' | 'x' | 'v'))))
        {
            self.completion = None;
            self.update(event);
            return;
        }
        let tab = matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press && key.modifiers.is_empty() && key.code == KeyCode::Tab);
        let changed = matches!(event, Event::Key(key) if key.kind != KeyEventKind::Release && (matches!(key.code, KeyCode::Char(_)) && key.modifiers.is_empty() || key.code == KeyCode::Backspace));
        if changed {
            if self.editor.update(event) != crate::editor::Outcome::Changed {
                return;
            }
            if self.editor.cursor() <= completion.start {
                self.completion = None;
                self.advance();
                return;
            }
            completion.ticket = Identity::new();
        }
        let result = if tab {
            completion
                .picker
                .update(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Enter,
                    crate::event::KeyModifiers::NONE,
                )))
        } else {
            completion.picker.update(event)
        };
        let (kind, start) = (completion.kind, completion.start);
        if changed {
            self.advance();
        }
        if let Intent::Selected(id) = result {
            let item = self
                .completion_items
                .iter()
                .find(|item| item.kind == kind && item.id == id)
                .cloned();
            self.completion = None;
            if let Some(item) = item {
                if kind == CompletionKind::Command && !tab {
                    self.editor
                        .replace_before_cursor(self.editor.cursor() - start, "");
                    self.advance();
                    if item.id == "stash" {
                        self.open_stash_list();
                    } else {
                        self.intents.push(super::Intent::Command(item.id));
                    }
                } else {
                    let prefix = if kind == CompletionKind::File {
                        '@'
                    } else {
                        '/'
                    };
                    let result = self.editor.replace_before_cursor(
                        self.editor.cursor() - start,
                        &format!("{prefix}{} ", item.value),
                    );
                    if result == crate::editor::Outcome::Changed {
                        self.advance();
                    }
                }
            }
        } else if matches!(result, Intent::Cancelled) {
            self.completion = None;
        }
    }
    /// Read the current opening/query identity for an asynchronous host search.
    #[must_use]
    pub fn completion_request(&self) -> Option<CompletionRequest> {
        self.completion
            .as_ref()
            .map(|completion| CompletionRequest {
                ticket: completion.ticket.clone(),
                version: self.version.clone(),
                kind: completion.kind,
                query: completion.picker.query().into(),
            })
    }
    /// Mark a current host search loading or failed without allowing stale updates.
    pub fn set_completion_status(
        &mut self,
        request: &CompletionRequest,
        status: crate::picker::Status,
    ) -> bool {
        if self.completion_request().as_ref() != Some(request) {
            return false;
        }
        if let Some(completion) = &mut self.completion {
            completion.picker.set_status(status);
        }
        true
    }
    /// Apply only current results; errors remain visible without mutating the draft.
    pub fn complete(
        &mut self,
        request: &CompletionRequest,
        result: Result<Vec<CompletionItem>, &str>,
    ) -> bool {
        if self.completion_request().as_ref() != Some(request) {
            return false;
        }
        match result {
            Ok(items) => {
                if !self.replace_completion_items(items) {
                    return false;
                }
                let choices = self
                    .completion_items
                    .iter()
                    .filter(|item| item.kind == request.kind)
                    .map(|item| {
                        let mut choice = Choice::new(&item.id, &item.value);
                        choice.detail.clone_from(&item.detail);
                        choice.disabled.clone_from(&item.disabled);
                        choice
                    })
                    .collect();
                if let Some(completion) = &mut self.completion {
                    if completion.picker.replace_choices(choices).is_err() {
                        return false;
                    }
                    completion.picker.set_status(crate::picker::Status::Ready);
                }
            }
            Err(reason) => {
                if let Some(completion) = &mut self.completion {
                    completion
                        .picker
                        .set_status(crate::picker::Status::Error(reason.into()));
                }
            }
        }
        true
    }
    pub(super) fn render_completion(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        presentation: Presentation,
    ) {
        if let Some(completion) = &mut self.completion {
            let view = PickerView {
                title: if completion.kind == CompletionKind::File {
                    "files"
                } else {
                    "commands"
                },
                context: "type filters · tab completes · enter selects · esc closes",
                presentation,
                max_height: area.height.min(9),
                ..PickerView::default()
            };
            let height = completion.picker.height(area.width, view).min(area.height);
            let area = Rect {
                y: area.bottom().saturating_sub(height),
                height,
                ..area
            };
            completion.picker.render(frame, area, view, true);
        }
    }
}
