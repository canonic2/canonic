//! Routes local focus, logical navigation and acknowledged semantic host intents.

use super::{
    Action, Anchor, Chat, Intent, Match, Page, Pending, PendingKind, Search, Selection,
    content::Kind,
    layout::BlockId,
    model::{End, Mode},
};
use crate::{
    editor::Editor,
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
};
use std::{collections::BTreeSet, time::Duration};

impl Chat {
    /// Handle one event. Chat keys never edit the saved Composer draft.
    pub fn update(&mut self, event: &Event) -> Action {
        if matches!(event, Event::Key(k) if k.kind == KeyEventKind::Release) {
            return Action::None;
        }
        self.sync_reviews();
        if self.reviews.update(event) {
            self.sync_reviews();
            return Action::None;
        }
        if let Some(mut chooser) = self.artifacts.take() {
            if chooser.update(self, event) {
                self.artifacts = Some(chooser);
            }
            return Action::None;
        }
        if self.search.is_some() {
            self.search_event(event);
            return Action::None;
        }
        if self.selection.is_some() {
            self.select_event(event);
            return Action::None;
        }
        if self.run_event(event) {
            return Action::None;
        }
        if !self.input.overlay_open() && self.chat_key(event) {
            return Action::None;
        }
        let before = self.input.editor().text().to_owned();
        let submission = self.input.update(event);
        if self.input.editor().text() != before {
            self.reviews.note_typing();
        }
        if let Some(submission) = submission {
            self.resume();
            return Action::Submit(submission);
        }
        if self.input.take_stop() {
            Action::Cancel
        } else {
            Action::None
        }
    }
    /// Dismiss only local search/select or Composer selection, for nested hosts' Escape routing.
    pub fn dismiss_selection(&mut self) -> bool {
        if self.artifacts.take().is_some() {
            return true;
        }
        if self.search.is_some() {
            self.close_search(false);
            return true;
        }
        if self.selection.is_some() {
            self.close_selection();
            return true;
        }
        if self.input.editor().selection().is_none() {
            return false;
        }
        self.input
            .update(&Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        true
    }
    fn chat_key(&mut self, event: &Event) -> bool {
        let Event::Key(key) = event else {
            return false;
        };
        if self.leader.take().is_some() {
            if key.kind != KeyEventKind::Press {
                return true;
            }
            if key.modifiers.is_empty() {
                match key.code {
                    KeyCode::Char('v') => {
                        self.open_selection();
                        return true;
                    }
                    KeyCode::Char('t') => {
                        self.timestamps = !self.timestamps;
                        self.refresh();
                        return true;
                    }
                    KeyCode::Char('r') => {
                        self.retry_reply();
                        return true;
                    }
                    KeyCode::Char('o') if self.sessions_available.is_some() => {
                        self.request_sessions();
                        return true;
                    }
                    KeyCode::Char('f') if self.files_available.is_some() => {
                        self.request_files();
                        return true;
                    }
                    _ => {}
                }
            }
            self.input.update(&Event::Key(KeyEvent::new(
                KeyCode::Char('x'),
                KeyModifiers::CONTROL,
            )));
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('f') => {
                    self.open_search();
                    return true;
                }
                KeyCode::Char('x') => {
                    self.leader = Some(self.now + Duration::from_secs(3));
                    return true;
                }
                _ => {}
            }
        }
        if !key.modifiers.contains(KeyModifiers::ALT) {
            return false;
        }
        match key.code {
            KeyCode::PageUp => {
                self.move_rows(-(isize::try_from(self.height).unwrap_or(isize::MAX)));
            }
            KeyCode::PageDown => self.move_rows(isize::try_from(self.height).unwrap_or(isize::MAX)),
            KeyCode::Up => self.move_rows(-3),
            KeyCode::Down => self.move_rows(3),
            KeyCode::Home => self.oldest(),
            KeyCode::End => self.latest(),
            _ => return false,
        }
        true
    }
    fn move_rows(&mut self, amount: isize) {
        self.home = None;
        self.follow = false;
        self.scroll = self
            .scroll
            .saturating_add_signed(amount)
            .min(self.maximum());
        self.capture_anchor();
    }
    fn oldest(&mut self) {
        if self.paging_pending() {
            return;
        }
        self.follow = false;
        if self.scroll > 0 || self.home.as_ref() != self.anchor.as_ref() || self.home.is_none() {
            self.scroll = 0;
            self.capture_anchor();
            self.home = self.anchor.clone();
            return;
        }
        if self.earlier == 0 || matches!(self.page, Some(Page::Loading(_))) {
            return;
        }
        let request = self.request();
        self.newer_failure = None;
        let before = self
            .messages
            .first()
            .map_or_else(String::new, |m| m.turn.id.clone());
        self.page = Some(Page::Loading(request.clone()));
        self.intents.push(Intent::Earlier { request, before });
    }
    fn latest(&mut self) {
        if self.paging_pending() {
            return;
        }
        self.home = None;
        if self.newer > 0 {
            if self.pending.is_some() {
                return;
            }
            let request = self.request();
            self.newer_failure = None;
            self.page = None;
            self.pending = Some(Pending {
                request: request.clone(),
                label: "newest history loaded".into(),
                kind: PendingKind::Latest,
            });
            self.intents.push(Intent::Latest { request });
        } else {
            self.resume();
            self.refresh();
        }
    }
    fn paging_pending(&self) -> bool {
        matches!(self.page, Some(Page::Loading(_)))
            || self
                .pending
                .as_ref()
                .is_some_and(|p| matches!(p.kind, PendingKind::Latest))
    }
    fn retry_reply(&mut self) {
        if !self.retry {
            self.say("retry unavailable");
            return;
        }
        if self.pending.is_some() {
            return;
        }
        let Some(message) = self
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.turn.end, End::Failed(_) | End::Interrupted(_)))
            .map(|m| m.turn.id.clone())
        else {
            self.say("no failed reply");
            return;
        };
        let request = self.request();
        self.pending = Some(Pending {
            request: request.clone(),
            label: "retry requested".into(),
            kind: PendingKind::Retry,
        });
        self.intents.push(Intent::Retry { request, message });
    }
    /// Enter loaded-only search, preserving the logical viewport and Composer caret.
    pub fn open_search(&mut self) {
        if self.search.is_some() {
            return;
        }
        self.close_selection();
        self.capture_anchor();
        self.search = Some(Search {
            editor: Editor::new(1024),
            saved: self.anchor.clone(),
            saved_follow: self.follow,
            matches: vec![],
            current: 0,
            wrapped: false,
            temporary: BTreeSet::new(),
            cached: std::collections::HashMap::new(),
            query: String::new(),
            limited: false,
        });
        self.follow = false;
        self.leader = None;
    }
    /// Leave search. `stay` keeps the found location; false restores the saved anchor.
    pub fn close_search(&mut self, stay: bool) {
        let Some(search) = self.search.take() else {
            return;
        };
        if !stay {
            self.anchor = search.saved;
            self.follow = false;
        }
        self.refresh();
        if !stay && search.saved_follow && self.scroll == self.maximum() {
            self.follow = true;
        }
    }
    fn search_event(&mut self, event: &Event) {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Esc => {
                    self.close_search(false);
                    return;
                }
                KeyCode::Enter if key.modifiers.is_empty() && key.kind == KeyEventKind::Press => {
                    self.close_search(true);
                    return;
                }
                KeyCode::Enter => return,
                KeyCode::Up => {
                    self.next_match(false);
                    return;
                }
                KeyCode::Down => {
                    self.next_match(true);
                    return;
                }
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.next_match(false);
                    return;
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.next_match(true);
                    return;
                }
                _ => {}
            }
        }
        if let Some(search) = &mut self.search {
            search.editor.update(event);
        }
        self.recompute_search();
        self.show_match();
    }
    pub(super) fn recompute_search(&mut self) {
        let Some(search) = &mut self.search else {
            return;
        };
        let query = search.editor.text().to_lowercase();
        let previous = search.matches.get(search.current).cloned();
        if query != search.query {
            search.cached.clear();
            search.query.clone_from(&query);
        }
        let mut matches = Vec::new();
        let mut temporary = BTreeSet::new();
        let mut limited = false;
        search.cached.retain(|key, _| self.cache.contains_key(key));
        if !query.is_empty() {
            for message in &self.messages {
                for part in &message.turn.parts {
                    if matches.len() >= 4096 {
                        limited = true;
                        break;
                    }
                    let key = (message.turn.id.clone(), part.id.clone());
                    let Some(cache) = self.cache.get(&key) else {
                        continue;
                    };
                    if search.cached.get(&key).is_none_or(|(revision, budget, _)| {
                        *revision != part.revision || *budget != 4097 - matches.len()
                    }) {
                        let found = part_matches(cache, &key, &query, 4097 - matches.len());
                        search
                            .cached
                            .insert(key.clone(), (part.revision, 4097 - matches.len(), found));
                    }
                    if let Some((_, _, found)) = search.cached.get(&key) {
                        for found in found {
                            if matches.len() >= 4096 {
                                limited = true;
                                break;
                            }
                            if cache.blocks.iter().any(|b| {
                                b.start == found.block.start
                                    && matches!(
                                        b.kind,
                                        Kind::Fold { .. }
                                            | Kind::Diff { .. }
                                            | Kind::Tool { .. }
                                            | Kind::ToolGroup
                                    )
                            }) {
                                temporary.insert(key.clone());
                            }
                            matches.push(found.clone());
                        }
                    }
                }
            }
        }
        search.current = previous
            .as_ref()
            .and_then(|old| matches.iter().position(|m| m == old))
            .unwrap_or_else(|| search.current.min(matches.len().saturating_sub(1)));
        search.matches = matches;
        search.temporary = temporary;
        search.limited = limited;
    }
    fn next_match(&mut self, forward: bool) {
        let Some(search) = &mut self.search else {
            return;
        };
        let count = search.matches.len();
        if count == 0 {
            return;
        }
        search.wrapped = if forward {
            search.current + 1 == count
        } else {
            search.current == 0
        };
        search.current = if forward {
            (search.current + 1) % count
        } else {
            (search.current + count - 1) % count
        };
        self.show_match();
    }
    fn show_match(&mut self) {
        let found = self
            .search
            .as_ref()
            .and_then(|s| s.matches.get(s.current))
            .cloned();
        if let Some(found) = found {
            self.refresh();
            let anchor = Anchor {
                message: found.block.message,
                part: found.block.part,
                offset: found.start,
            };
            if let Some(index) = self.locate(&anchor)
                && (index < self.scroll + 3 || index >= self.scroll + self.height.saturating_sub(3))
            {
                self.scroll = index.saturating_sub(3).min(self.maximum());
                self.capture_anchor();
            }
        }
    }
    /// Enter block selection at the newest content block; no terminal mouse capture.
    pub fn open_selection(&mut self) {
        if self.mode() != Mode::Compose {
            return;
        }
        self.refresh();
        let Some(block) = self.block_ids().last().cloned() else {
            self.say("no content to select");
            return;
        };
        self.selection = Some(Selection {
            first: block.clone(),
            last: block.clone(),
            whole: false,
            was_following: self.follow,
        });
        self.follow = false;
        self.reveal_block(&block);
    }
    /// Exit selection, resuming only when it was entered while following.
    pub fn close_selection(&mut self) {
        if let Some(selection) = self.selection.take()
            && selection.was_following
        {
            self.resume();
        }
    }
    fn select_event(&mut self, event: &Event) {
        if self.tool_event(event) {
            return;
        }
        let Event::Key(key) = event else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.close_selection(),
            KeyCode::Up => self.move_block(false, key.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::Down => self.move_block(true, key.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::Char('m') if key.modifiers.is_empty() => {
                if let Some(selection) = &mut self.selection {
                    selection.whole = true;
                }
            }
            KeyCode::Char('y') if key.kind == KeyEventKind::Press && key.modifiers.is_empty() => {
                self.copy(false);
            }
            KeyCode::Char('c') if key.kind == KeyEventKind::Press && key.modifiers.is_empty() => {
                self.copy(true);
            }
            KeyCode::Char('o') if key.kind == KeyEventKind::Press && key.modifiers.is_empty() => {
                self.open_block();
            }
            KeyCode::Enter if key.kind == KeyEventKind::Press && key.modifiers.is_empty() => {
                self.toggle_block();
            }
            _ => {}
        }
    }
    pub(super) fn block_ids(&self) -> Vec<BlockId> {
        self.messages
            .iter()
            .flat_map(|m| {
                m.turn.parts.iter().flat_map(|p| {
                    self.cache
                        .get(&(m.turn.id.clone(), p.id.clone()))
                        .into_iter()
                        .flat_map(|c| {
                            c.blocks
                                .iter()
                                .filter(|b| {
                                    !matches!(b.kind, Kind::Tool { .. } | Kind::ToolGroup)
                                        || c.rows.iter().any(|r| {
                                            r.block.as_ref().is_some_and(|id| id.start == b.start)
                                        })
                                })
                                .map(|b| BlockId {
                                    message: m.turn.id.clone(),
                                    part: p.id.clone(),
                                    start: b.start,
                                })
                        })
                })
            })
            .collect()
    }
    fn selected_ids(&self) -> Vec<BlockId> {
        let Some(selection) = &self.selection else {
            return vec![];
        };
        let ids = self.block_ids();
        if selection.whole {
            return ids
                .into_iter()
                .filter(|id| id.message == selection.last.message)
                .collect();
        }
        let first = ids
            .iter()
            .position(|id| id == &selection.first)
            .unwrap_or(0);
        let last = ids
            .iter()
            .position(|id| id == &selection.last)
            .unwrap_or(first);
        ids.into_iter()
            .skip(first.min(last))
            .take(first.abs_diff(last) + 1)
            .collect()
    }
    pub(super) fn selected(&self, id: &BlockId) -> bool {
        self.selected_ids().contains(id)
    }
    fn move_block(&mut self, forward: bool, extend: bool) {
        let ids = self.block_ids();
        let Some(selection) = &mut self.selection else {
            return;
        };
        let index = ids.iter().position(|id| id == &selection.last).unwrap_or(0);
        let next = if forward {
            index.saturating_add(1).min(ids.len().saturating_sub(1))
        } else {
            index.saturating_sub(1)
        };
        if let Some(id) = ids.get(next) {
            selection.last = id.clone();
            selection.whole = false;
            if !extend {
                selection.first = id.clone();
            }
            self.reveal_block(id);
        }
    }
    fn reveal_block(&mut self, id: &BlockId) {
        if let Some(index) = self
            .rows
            .iter()
            .position(|row| row.row().block.as_ref() == Some(id))
            && (index < self.scroll || index >= self.scroll + self.height)
        {
            self.scroll = index.saturating_sub(2).min(self.maximum());
            self.capture_anchor();
        }
    }
    fn block(&self, id: &BlockId) -> Option<&super::content::Block> {
        self.cache
            .get(&(id.message.clone(), id.part.clone()))?
            .blocks
            .iter()
            .find(|b| b.start == id.start)
    }
    fn copy(&mut self, code: bool) {
        if self.pending.is_some() {
            return;
        }
        let ids = self.selected_ids();
        let payloads: Vec<_> = ids
            .iter()
            .filter_map(|id| self.block(id))
            .filter_map(|block| {
                if code {
                    block.code()
                } else {
                    Some(block.markdown.clone())
                }
            })
            .collect();
        if payloads.is_empty() {
            self.say("select a code block to copy code");
            return;
        }
        let text = payloads.join("\n\n");
        let lines = text.lines().count();
        let kind = if code { "code" } else { "Markdown" }.to_owned();
        let request = self.request();
        self.pending = Some(Pending {
            request: request.clone(),
            label: format!("✓ copied {kind} · {lines} lines"),
            kind: PendingKind::Copy,
        });
        self.intents.push(Intent::Copy {
            request,
            text,
            kind,
            lines,
        });
    }
    fn open_block(&mut self) {
        if self.pending.is_some() {
            return;
        }
        let Some(id) = self.selection.as_ref().map(|s| s.last.clone()) else {
            return;
        };
        let Some(block) = self.block(&id) else {
            return;
        };
        let target = block.open.clone();
        let inspection = matches!(
            block.kind,
            Kind::Table { .. } | Kind::Diff { .. } | Kind::Code { .. }
        );
        if let Some(target) = target.filter(|_| !inspection) {
            let request = self.request();
            self.pending = Some(Pending {
                request: request.clone(),
                label: format!("→ opened {target}"),
                kind: PendingKind::Open(target.clone()),
            });
            self.intents.push(Intent::Open { request, target });
        } else if inspection {
            let content = if let Kind::Table { headers, rows } = &block.kind {
                Some(super::Content::Table {
                    headers: headers.clone(),
                    rows: rows.clone(),
                })
            } else if let Kind::Code {
                language,
                file,
                line,
            } = &block.kind
            {
                Some(super::Content::Code {
                    text: block.semantic(),
                    language: language.clone(),
                    file: file.clone(),
                    line: *line,
                })
            } else {
                self.turn(&id.message)
                    .and_then(|t| t.parts.iter().find(|p| p.id == id.part))
                    .map(|p| p.content.clone())
            };
            if let Some(content) = content {
                let request = self.request();
                self.pending = Some(Pending {
                    request: request.clone(),
                    label: "→ opened full content".into(),
                    kind: PendingKind::Inspect,
                });
                self.intents.push(Intent::Inspect {
                    request,
                    message: id.message,
                    part: id.part,
                    content,
                });
            }
        } else {
            self.say("nothing available to open");
        }
    }
    fn toggle_block(&mut self) {
        let Some(id) = self.selection.as_ref().map(|s| s.last.clone()) else {
            return;
        };
        if let Some(ticket) = self
            .messages
            .iter()
            .find(|message| message.turn.id == id.message)
            .and_then(|message| message.shell.as_ref().map(|(ticket, _)| ticket.clone()))
        {
            self.toggle_shell_output(&ticket);
            self.reveal_block(&id);
            return;
        }
        if !self
            .block(&id)
            .is_some_and(|b| matches!(b.kind, Kind::Fold { .. }))
        {
            return;
        }
        let key = (id.message.clone(), id.part.clone());
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        self.refresh();
        self.reveal_block(&id);
    }
}

fn part_matches(
    cache: &super::layout::Cached,
    key: &(String, String),
    query: &str,
    budget: usize,
) -> Vec<Match> {
    let mut matches = Vec::new();
    for block in &cache.blocks {
        let text = block.semantic();
        let (folded, map) = lower_map(&text);
        for (start, _) in folded.match_indices(query) {
            if matches.len() >= budget {
                return matches;
            }
            let end = start + query.len();
            let start = map.get(start).copied().unwrap_or(text.len());
            let mut end = map.get(end).copied().unwrap_or(text.len());
            if end <= start {
                end = start + text[start..].chars().next().map_or(0, char::len_utf8);
            }
            matches.push(Match {
                block: BlockId {
                    message: key.0.clone(),
                    part: key.1.clone(),
                    start: block.start,
                },
                start: block.start + start,
                end: block.start + end,
            });
        }
    }
    matches
}
fn lower_map(text: &str) -> (String, Vec<usize>) {
    let mut lowered = String::new();
    let mut map = Vec::new();
    for (offset, c) in text.char_indices() {
        let lower: String = c.to_lowercase().collect();
        map.extend(std::iter::repeat_n(offset, lower.len()));
        lowered.push_str(&lower);
    }
    map.push(text.len());
    (lowered, map)
}
