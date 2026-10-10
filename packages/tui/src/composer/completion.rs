//! Editor token routing for the reusable completion menu; hosts own discovery.
use super::{Composer, Identity, Version};
use crate::{
    completion::{Action, Batch, Candidate, Menu},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    theme::Presentation,
};
use ratatui::{Frame, layout::Rect};

/// Prefix-triggered completion interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionKind {
    Command,
    File,
}
/// Identity of an opening and editor revision; late results are ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionRequest {
    pub ticket: Identity,
    pub version: Version,
    pub kind: CompletionKind,
    pub query: String,
}
/// A host candidate; file values are plain editable text.
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
    pub end: usize,
    pub prefix: &'static str,
    pub menu: Menu,
    host_finished: bool,
    cached_source: bool,
}

fn candidates(items: &[CompletionItem], kind: CompletionKind) -> Vec<Candidate> {
    items
        .iter()
        .filter(|item| item.kind == kind)
        .map(|item| {
            let name = if kind == CompletionKind::Command {
                format!("/{}", item.value)
            } else {
                item.value.clone()
            };
            let mut c = Candidate::new(&item.id, &name, &item.value);
            c.description.clone_from(&item.detail);
            c.disabled.clone_from(&item.disabled);
            c.recency = u64::from(item.touched);
            if item.touched {
                c.tag = if kind == CompletionKind::File {
                    "edited"
                } else {
                    "recent"
                }
                .into();
            }
            c
        })
        .collect()
}

impl Composer {
    /// Replace cached host candidates while completion is closed.
    pub fn set_completions(&mut self, items: Vec<CompletionItem>) -> bool {
        if self.completion.is_some() {
            return false;
        }
        self.replace_completion_items(items)
    }
    fn replace_completion_items(&mut self, items: Vec<CompletionItem>) -> bool {
        if items.len() > 5000
            || [CompletionKind::Command, CompletionKind::File]
                .iter()
                .any(|kind| Menu::new("", "", &["host"], candidates(&items, *kind)).is_err())
        {
            return false;
        }
        self.completion_items = items;
        true
    }
    pub(super) fn open_completion(&mut self, kind: CompletionKind) {
        let prefix = if self.input_mode == super::InputMode::Shell {
            ""
        } else if kind == CompletionKind::Command {
            "/"
        } else {
            "@"
        };
        let cursor = self.editor.cursor();
        let start = if prefix.is_empty() {
            self.editor.text()[..cursor]
                .char_indices()
                .rfind(|(_, c)| c.is_whitespace())
                .map_or(0, |(i, c)| i + c.len_utf8())
        } else {
            cursor.saturating_sub(1)
        };
        let end = self.editor.text()[cursor..]
            .find(char::is_whitespace)
            .map_or(self.editor.text().len(), |i| cursor + i);
        let query = &self.editor.text()[start + prefix.len()..end];
        let rows = candidates(&self.completion_items, kind);
        if let Ok(mut menu) = Menu::new(
            if kind == CompletionKind::Command {
                "commands"
            } else {
                "files"
            },
            query,
            &["host"],
            rows.clone(),
        ) {
            menu.complete(
                &menu.request(),
                "host",
                Batch {
                    total: Some(rows.len()),
                    candidates: rows,
                    complete: true,
                    ..Batch::default()
                },
            );
            self.completion = Some(Completion {
                ticket: Identity::new(),
                kind,
                start,
                end,
                prefix,
                menu,
                host_finished: false,
                cached_source: true,
            });
        }
    }
    pub(super) fn completion_input(&mut self, event: &Event) -> Option<super::Submission> {
        let action = self.completion.as_mut()?.menu.update(event);
        match action {
            Action::Insert(candidate) => {
                self.insert_completion(&candidate);
            }
            Action::Close => self.completion = None,
            Action::PassEnter => {
                self.completion = None;
                return self.submit_with_commands(false);
            }
            Action::Retry(_) => {
                if let Some(c) = &mut self.completion {
                    c.ticket = Identity::new();
                    c.host_finished = false;
                }
                if let Some(request) = self.completion_request() {
                    self.intents.push(super::Intent::RetryCompletion(request));
                }
            }
            Action::Unhandled => self.edit_completion(event),
            Action::None => {}
        }
        None
    }
    fn insert_completion(&mut self, candidate: &Candidate) {
        let Some(c) = &self.completion else {
            return;
        };
        let (start, token_end, prefix) = (c.start, c.end, c.prefix);
        let end = token_end + usize::from(self.editor.text()[token_end..].starts_with(' '));
        let kind = c.kind;
        let old_cursor = self.editor.cursor();
        self.editor.restore_cursor(end);
        let outcome = self
            .editor
            .replace_before_cursor(end - start, &format!("{prefix}{} ", candidate.insertion));
        if outcome == crate::editor::Outcome::Changed {
            if kind == CompletionKind::Command {
                let item = CompletionItem {
                    id: candidate.id.clone(),
                    value: candidate
                        .insertion
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .into(),
                    detail: candidate.description.clone(),
                    kind,
                    disabled: None,
                    touched: true,
                };
                if let Some(existing) = self
                    .completion_items
                    .iter_mut()
                    .find(|i| i.kind == kind && i.id == item.id)
                {
                    *existing = item;
                } else {
                    if self.completion_items.len() == 5000 {
                        self.completion_items.remove(0);
                    }
                    self.completion_items.push(item);
                }
            }
            self.completion = None;
            self.adopt_recall();
            self.advance();
            if kind == CompletionKind::Command {
                self.inserted_hint = Some((
                    candidate.argument_hint.clone(),
                    candidate.argument_placeholder.clone(),
                ));
                self.inserted_command = Some((
                    candidate.id.clone(),
                    candidate
                        .insertion
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .into(),
                ));
            }
        } else {
            self.editor.restore_cursor(old_cursor);
        }
    }
    fn edit_completion(&mut self, event: &Event) {
        // Destructive/selection/undo operations close first; ordinary cursor movement
        // inside the token preserves the query and its request identity.
        if matches!(event, Event::Paste(_))
            || matches!(event,Event::Key(k) if k.kind != KeyEventKind::Release && (k.modifiers.contains(KeyModifiers::CONTROL) && !matches!(k.code,KeyCode::Char('a'|'e')|KeyCode::Left|KeyCode::Right|KeyCode::Home|KeyCode::End) || k.modifiers.contains(KeyModifiers::ALT) || k.modifiers.contains(KeyModifiers::SHIFT) && !matches!(k.code,KeyCode::Char(_))))
        {
            self.completion = None;
            self.update(event);
            return;
        }
        let old_len = self.editor.text().len();
        let old_cursor = self.editor.cursor();
        let changed = self.editor.update(event) == crate::editor::Outcome::Changed;
        let Some(c) = &mut self.completion else {
            return;
        };
        if changed {
            c.end = c
                .end
                .saturating_add(self.editor.text().len())
                .saturating_sub(old_len);
            c.ticket = Identity::new();
            c.host_finished = false;
        }
        let begin = c.start + c.prefix.len();
        let cursor = self.editor.cursor();
        if cursor < begin
            || cursor > c.end
            || c.end > self.editor.text().len()
            || !self.editor.text().is_char_boundary(begin)
            || !self.editor.text().is_char_boundary(c.end)
            || self.editor.text()[c.start..c.end]
                .chars()
                .any(char::is_whitespace)
            || !self.editor.text()[c.start..].starts_with(c.prefix)
        {
            self.completion = None;
        } else if changed {
            c.menu.set_query(&self.editor.text()[begin..c.end]);
            let rows = candidates(&self.completion_items, c.kind);
            c.cached_source = c.menu.complete(
                &c.menu.request(),
                "host",
                Batch {
                    total: Some(rows.len()),
                    candidates: rows,
                    complete: true,
                    ..Batch::default()
                },
            );
        }
        if changed {
            self.adopt_recall();
            self.advance();
        } else if old_cursor != self.editor.cursor() {
            self.save_due = Some(self.now + std::time::Duration::from_millis(500));
        }
    }
    /// Inspect the public menu without giving hosts access to the editor token range.
    #[must_use]
    pub fn completion_menu(&self) -> Option<&Menu> {
        self.completion.as_ref().map(|c| &c.menu)
    }
    #[must_use]
    pub fn completion_request(&self) -> Option<CompletionRequest> {
        self.completion.as_ref().map(|c| CompletionRequest {
            ticket: c.ticket.clone(),
            version: self.version.clone(),
            kind: c.kind,
            query: c.menu.request().query,
        })
    }
    /// Poll the debounced request once for this query.
    pub fn completion_due(&mut self) -> Option<CompletionRequest> {
        self.completion.as_mut()?.menu.take_request()?;
        self.completion_request()
    }
    /// Configure asynchronous sources for the current route, clearing cached results.
    pub fn set_completion_sources(
        &mut self,
        request: &CompletionRequest,
        sources: &[&str],
    ) -> bool {
        if self.completion_request().as_ref() != Some(request) {
            return false;
        }
        let Ok(menu) = Menu::new(
            if request.kind == CompletionKind::Command {
                "commands"
            } else {
                "files"
            },
            &request.query,
            sources,
            Vec::new(),
        ) else {
            return false;
        };
        if let Some(c) = &mut self.completion {
            c.menu = menu;
            c.host_finished = false;
            c.cached_source = false;
        }
        true
    }
    /// Apply a named streaming source batch only to the current draft query.
    pub fn complete_source(
        &mut self,
        request: &CompletionRequest,
        source: &str,
        batch: Batch,
    ) -> bool {
        if self.completion_request().as_ref() != Some(request) {
            return false;
        }
        self.completion
            .as_mut()
            .is_some_and(|c| c.menu.complete(&c.menu.request(), source, batch))
    }
    /// Retry failed sources from a host warning/details UI, retaining good results.
    pub fn retry_completion_sources(&mut self) -> Option<CompletionRequest> {
        let c = self.completion.as_mut()?;
        c.menu.retry_failed()?;
        c.ticket = Identity::new();
        c.host_finished = false;
        self.completion_request()
    }
    /// Supply an explicit closest guess, which only Tab can insert.
    pub fn set_completion_guess(&mut self, request: &CompletionRequest, guess: Candidate) -> bool {
        if self.completion_request().as_ref() != Some(request)
            || request.kind != CompletionKind::File
        {
            return false;
        }
        self.completion
            .as_mut()
            .is_some_and(|c| c.menu.set_guess(&c.menu.request(), guess))
    }
    /// Compatibility adapter for one host source's loading/error state.
    pub fn set_completion_status(
        &mut self,
        request: &CompletionRequest,
        status: crate::picker::Status,
    ) -> bool {
        if self.completion_request().as_ref() != Some(request) {
            return false;
        }
        match status {
            crate::picker::Status::Loading(_) => self.set_completion_sources(request, &["host"]),
            crate::picker::Status::Error(reason) => {
                self.set_completion_sources(request, &["host"])
                    && self.complete_source(
                        request,
                        "host",
                        Batch {
                            error: Some(reason),
                            ..Batch::default()
                        },
                    )
            }
            crate::picker::Status::Ready => true,
        }
    }
    /// Apply a complete legacy host snapshot; invalid/stale updates change nothing.
    pub fn complete(
        &mut self,
        request: &CompletionRequest,
        result: Result<Vec<CompletionItem>, &str>,
    ) -> bool {
        if self.completion_request().as_ref() != Some(request)
            || self
                .completion
                .as_ref()
                .is_some_and(|c| c.host_finished || c.menu.sources() != ["host"])
        {
            return false;
        }
        match result {
            Ok(items) => {
                let rows = candidates(&items, request.kind);
                if items.len() > 5000
                    || [CompletionKind::Command, CompletionKind::File]
                        .iter()
                        .any(|kind| {
                            Menu::new("", "", &["host"], candidates(&items, *kind)).is_err()
                        })
                {
                    return false;
                }
                if let Some(c) = &mut self.completion
                    && c.cached_source
                {
                    c.menu.begin_search();
                    c.cached_source = false;
                }
                let accepted = self.complete_source(
                    request,
                    "host",
                    Batch {
                        candidates: rows,
                        complete: true,
                        ..Batch::default()
                    },
                );
                if accepted {
                    self.completion_items = items;
                    if let Some(c) = &mut self.completion {
                        c.host_finished = true;
                    }
                }
                accepted
            }
            Err(reason) => {
                if let Some(c) = &mut self.completion
                    && c.cached_source
                {
                    c.menu.begin_search();
                    c.cached_source = false;
                }
                let accepted = self.complete_source(
                    request,
                    "host",
                    Batch {
                        error: Some(reason.into()),
                        ..Batch::default()
                    },
                );
                if accepted && let Some(c) = &mut self.completion {
                    c.host_finished = true;
                }
                accepted
            }
        }
    }
    pub(super) fn invoke_command(&mut self) -> bool {
        if self.input_mode != super::InputMode::Message
            || !matches!(self.mode, super::BoxMode::Draft)
            || self.blocked.is_some()
        {
            return false;
        }
        let Some(name) = self
            .editor
            .text()
            .strip_prefix('/')
            .and_then(|s| s.split_whitespace().next())
        else {
            return false;
        };
        let chosen = self
            .inserted_command
            .as_ref()
            .filter(|(_, selected)| selected == name)
            .map(|(id, _)| id.as_str());
        let item = self.completion_items.iter().find(|i| {
            i.kind == CompletionKind::Command
                && i.value == name
                && chosen.is_none_or(|id| i.id == id)
        });
        if chosen.is_some() && item.is_none_or(|i| i.disabled.is_some()) {
            self.editor.notify("Command unavailable · draft kept");
            return true;
        }
        let Some(item) = item.filter(|i| i.disabled.is_none()) else {
            return false;
        };
        let id = item.id.clone();
        let arguments = self
            .editor
            .expanded_text()
            .strip_prefix('/')
            .unwrap_or("")
            .split_once(char::is_whitespace)
            .map_or_else(String::new, |(_, args)| args.trim().to_owned());
        self.editor.set_text("");
        self.advance();
        if id == "stash" {
            self.open_stash_list();
        } else if arguments.is_empty() {
            self.intents.push(super::Intent::Command(id));
        } else {
            self.intents
                .push(super::Intent::CommandWithArguments { id, arguments });
        }
        true
    }
    pub(super) fn command_ready(&self) -> bool {
        self.inserted_command.is_some()
            || self.input_mode == super::InputMode::Message
                && self
                    .editor
                    .text()
                    .strip_prefix('/')
                    .and_then(|s| s.split_whitespace().next())
                    .is_some_and(|name| {
                        self.completion_items.iter().any(|i| {
                            i.kind == CompletionKind::Command
                                && i.value == name
                                && i.disabled.is_none()
                        })
                    })
    }
    pub(super) fn render_completion(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if let Some(c) = &mut self.completion {
            c.menu.render(frame, area, p);
        }
    }
}
