//! Focused navigation, field isolation and deliberate row/confirmation requests.

use super::{
    DeleteCheck, Discovery, Intent, Kind, Menu, MenuKind, Pending, Rename, Request, SessionBrowser,
    Status, Ticket,
};
use crate::{
    dialog::{Caution, Confirmation, Decision, Dialog, Size},
    editor::Editor,
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    picker::{Choice, ChoicePicker, Filter, Intent as Pick, SelectionMode},
};
use std::time::Duration;

impl SessionBrowser {
    /// Consume one event in this dedicated scope; unhandled events never reach Chat.
    pub fn update(&mut self, event: &Event) {
        if self.closed || matches!(event,Event::Key(k) if k.kind==KeyEventKind::Release) {
            return;
        }
        if self.confirming() {
            self.delete_event(event);
            return;
        }
        if self.rename.is_some() {
            self.rename_event(event);
            return;
        }
        if self.menu.is_some() {
            self.menu_event(event);
            return;
        }
        if self.searching {
            self.search_event(event);
            return;
        }
        let Event::Key(key) = event else {
            return;
        };
        if key.kind == KeyEventKind::Repeat && key.modifiers.is_empty() {
            self.navigation(key.code);
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        if key.code == KeyCode::Esc && key.modifiers.is_empty() {
            if self.search.is_some() || self.load.as_ref().is_some_and(|r| r.search) {
                self.search = None;
                if self.load.as_ref().is_some_and(|r| r.search) {
                    self.load = None;
                }
                self.notice = "search stopped".into();
            } else if !self.query.text().is_empty() {
                self.query.set_text("");
                self.refilter();
            } else {
                self.leave(self.origin.clone(), false);
            }
            return;
        }
        if !key.modifiers.is_empty() {
            return;
        }
        if self.navigation(key.code) {
            return;
        }
        match key.code {
            KeyCode::Char('/')
                if !self.rows().is_empty() || self.snapshot.status != Status::Ready =>
            {
                self.searching = true;
            }
            KeyCode::Enter if self.search_row => self.discover(true, false),
            KeyCode::Enter => self.act(Kind::Open),
            KeyCode::Char('a') => self.open_menu(false),
            KeyCode::Char('r')
                if self.load_failure.is_some()
                    || matches!(self.snapshot.status, Status::Failed(_)) =>
            {
                self.discover(false, true);
            }
            KeyCode::Char('r') => self.act(Kind::Rename),
            KeyCode::Char('f') => self.act(Kind::Fork),
            KeyCode::Char('m') => self.act(Kind::Move),
            KeyCode::Char('p') => self.act(Kind::Pin),
            KeyCode::Char('d') => self.act(Kind::Delete),
            _ => {}
        }
    }
    fn navigation(&mut self, code: KeyCode) -> bool {
        let ids = self
            .visible_ids()
            .into_iter()
            .map(|id| id.map(str::to_owned))
            .collect::<Vec<_>>();
        let selected = if self.search_row {
            None
        } else {
            self.selected.clone()
        };
        let old = ids.iter().position(|id| *id == selected).unwrap_or(0);
        let last = ids.len().saturating_sub(1);
        let next = match code {
            KeyCode::Up => old.saturating_sub(1),
            KeyCode::Down => (old + 1).min(last),
            KeyCode::PageUp => old.saturating_sub(self.viewport.max(1)),
            KeyCode::PageDown => (old + self.viewport.max(1)).min(last),
            KeyCode::Home => 0,
            KeyCode::End => last,
            _ => return false,
        };
        if let Some(id) = ids.get(next) {
            self.selected.clone_from(id);
            self.search_row = id.is_none();
        }
        if next == last
            && matches!(code, KeyCode::Down | KeyCode::PageDown | KeyCode::End)
            && (self.query.text().is_empty() || self.remote.is_some())
            && self.snapshot().more.is_some()
            && self.load_failure.is_none()
            && !matches!(self.snapshot.status, Status::Failed(_))
        {
            self.discover(false, false);
        }
        true
    }
    fn refilter(&mut self) {
        self.search = None;
        self.search_error.clear();
        if self.load.as_ref().is_some_and(|r| r.search) {
            self.load = None;
            self.load_cursor.clear();
            self.load_failure = None;
        }
        self.end_remote();
        self.offset = 0;
        self.reconcile(false);
        self.repair(&[]);
        if self.selected.is_none() {
            self.search_row = self.search_available();
        }
    }
    fn search_event(&mut self, event: &Event) {
        if let Event::Key(k) = event
            && k.modifiers.is_empty()
            && self.navigation(k.code)
        {
            return;
        }
        if let Event::Key(k) = event
            && k.kind == KeyEventKind::Press
            && k.modifiers.is_empty()
        {
            if k.code == KeyCode::Esc {
                if self.search.is_some() || self.load.as_ref().is_some_and(|r| r.search) {
                    self.search = None;
                    if self.load.as_ref().is_some_and(|r| r.search) {
                        self.load = None;
                    }
                    self.notice = "search stopped".into();
                } else {
                    self.searching = false;
                }
                return;
            }
            if k.code == KeyCode::Enter {
                if self.search_row {
                    self.discover(true, false);
                } else {
                    self.act(Kind::Open);
                }
                return;
            }
        }
        if matches!(event,Event::Key(k) if k.code==KeyCode::Enter) {
            return;
        }
        let before = self.query.text().to_owned();
        if matches!(event,Event::Paste(s) if s.contains(['\r','\n'])) {
            self.notice = "single-line search · paste refused".into();
            return;
        }
        if matches!(event,Event::Key(k) if k.code==KeyCode::Char('j') && k.modifiers.contains(KeyModifiers::CONTROL))
        {
            return;
        }
        self.query.update(event);
        if let Some(reason) = self.query.notice() {
            self.notice = reason.into();
        }
        if self.query.text() != before {
            self.refilter();
        }
    }
    pub(super) fn begin_rename(&mut self, id: &str) {
        let Some(row) = self.row(id) else {
            return;
        };
        let text = self
            .failures
            .get(id)
            .filter(|f| f.kind == Kind::Rename)
            .map_or(&row.title, |f| &f.value)
            .clone();
        if text.len() > 4096 {
            self.notice = "title exceeds the 4 KiB editor resource limit".into();
            return;
        }
        let mut editor = Editor::new(4096);
        editor.set_reject_overflow(true);
        editor.set_text(&text);
        if !self.failures.contains_key(id) {
            editor.update(&Event::Key(KeyEvent::new(
                KeyCode::Home,
                KeyModifiers::NONE,
            )));
            editor.update(&Event::Key(KeyEvent::new(
                KeyCode::End,
                KeyModifiers::SHIFT,
            )));
        }
        self.rename = Some(Rename {
            id: id.into(),
            revision: row.revision,
            editor,
        });
    }
    fn rename_event(&mut self, event: &Event) {
        if matches!(event,Event::Key(k) if k.code==KeyCode::Esc && k.kind==KeyEventKind::Press && k.modifiers.is_empty())
        {
            self.rename = None;
            return;
        }
        if matches!(event,Event::Paste(s) if s.contains(['\r','\n'])) {
            self.notice = "single-line title · paste refused".into();
            return;
        }
        if matches!(event,Event::Key(k) if k.code==KeyCode::Char('j') && k.modifiers.contains(KeyModifiers::CONTROL))
        {
            return;
        }
        if matches!(event,Event::Key(k) if k.code==KeyCode::Enter) {
            if !matches!(event,Event::Key(k) if k.kind==KeyEventKind::Press && k.modifiers.is_empty())
            {
                return;
            }
            let Some(rename) = &self.rename else {
                return;
            };
            let valid = self.row(&rename.id).is_some_and(|r| {
                !rename.editor.text().trim().is_empty()
                    && rename.editor.text().chars().count() <= r.rename_limit
            });
            if !valid {
                self.notice = "title required and within the character limit".into();
                return;
            }
            if let Some(rename) = self.rename.take() {
                self.send(&rename.id, Kind::Rename, rename.editor.text().into());
            }
            return;
        }
        if let Some(rename) = &mut self.rename {
            rename.editor.update(event);
            if let Some(reason) = rename.editor.notice() {
                self.notice = reason.into();
            }
        }
    }
    fn act(&mut self, kind: Kind) {
        let Some(id) = self.selected().map(str::to_owned) else {
            return;
        };
        if self.pending.contains_key(&id) {
            return;
        }
        let Some(row) = self.row(&id) else {
            return;
        };
        let Some(offered) = row.actions.iter().find(|a| a.kind == kind) else {
            return;
        };
        if let Some(reason) = &offered.disabled {
            self.notice = crate::text::clean(reason, 4096, true);
            return;
        }
        if kind == Kind::Open && id == self.origin {
            self.leave(self.origin.clone(), false);
            return;
        }
        match kind {
            Kind::Rename => self.begin_rename(&id),
            Kind::Move => self.open_menu(true),
            Kind::Delete => {
                let body = DeleteCheck {
                    id,
                    revision: row.revision,
                    text: offered.confirmation.clone(),
                    delete: false,
                    confirmation: Confirmation::default(),
                    offset: 0,
                    viewport: 1,
                    total: 0,
                };
                let _ = self.delete.push(Dialog {
                    id: "delete-session".into(),
                    title: "delete session".into(),
                    targets: vec![0],
                    body,
                    size: Size {
                        width: 72,
                        height: 12,
                    },
                    close: crate::dialog::ClosePolicy::default(),
                    view: crate::dialog::DialogView::default(),
                });
            }
            Kind::Pin => self.send(&id, kind, (!row.pinned).to_string()),
            _ => self.send(&id, kind, String::new()),
        }
    }
    fn send(&mut self, id: &str, kind: Kind, value: String) {
        if self.pending.len() >= 256 {
            self.notice = "256 requests are already pending · wait for the host".into();
            return;
        }
        let Some(row) = self.row(id) else {
            return;
        };
        if self.pending.contains_key(id)
            || (kind == Kind::Open && self.pending.values().any(|p| p.request.kind == Kind::Open))
        {
            return;
        }
        let request = Request {
            ticket: Ticket::new(),
            id: id.into(),
            revision: row.revision,
            kind,
            value,
        };
        self.failures.remove(id);
        self.notice.clear();
        self.pending.insert(
            id.into(),
            Pending {
                request: request.clone(),
                elapsed: Duration::ZERO,
            },
        );
        self.intents.push(Intent::Action(request));
    }
    fn open_menu(&mut self, moving: bool) {
        let Some(row) = self.selected().and_then(|id| self.row(id)) else {
            return;
        };
        if self.pending.contains_key(&row.id) {
            return;
        }
        let (choices, kind) = if moving {
            (row.destinations.clone(), MenuKind::Move)
        } else {
            let choices = row
                .actions
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let mut c = Choice::new(i.to_string(), format!("{} {}", a.kind.key(), a.label));
                    c.disabled.clone_from(&a.disabled);
                    c.danger = a.kind == Kind::Delete;
                    c
                })
                .collect();
            (
                choices,
                MenuKind::Actions(row.actions.iter().map(|a| a.kind).collect()),
            )
        };
        if let Ok(picker) = ChoicePicker::new(choices, None, SelectionMode::Single, Filter::Off) {
            self.menu = Some(Menu {
                id: row.id.clone(),
                revision: row.revision,
                picker,
                kind,
            });
        }
    }
    fn menu_event(&mut self, event: &Event) {
        if matches!(event, Event::Paste(_)) {
            self.notice = "paste ignored in actions".into();
            return;
        }
        let Some(mut menu) = self.menu.take() else {
            return;
        };
        let intent = menu.picker.update(event);
        match intent {
            Pick::Cancelled => {}
            Pick::Selected(id) => match menu.kind {
                MenuKind::Move => self.send(&menu.id, Kind::Move, id),
                MenuKind::Actions(kinds) => {
                    if let Some(kind) = id.parse::<usize>().ok().and_then(|i| kinds.get(i)) {
                        self.act(*kind);
                    }
                }
            },
            _ => self.menu = Some(menu),
        }
    }
    fn discover(&mut self, search: bool, retry: bool) {
        if (search && (!self.search_available() || self.search.is_some()))
            || (!search && self.load.is_some())
        {
            return;
        }
        let cursor = if search {
            String::new()
        } else if retry {
            self.load_cursor.clone()
        } else {
            self.snapshot().more.clone().unwrap_or_default()
        };
        let r = Discovery {
            ticket: Ticket::new(),
            revision: self.material_revision(),
            query: if search || self.remote.is_some() {
                self.query.text().into()
            } else {
                String::new()
            },
            cursor,
            search: search || self.remote.is_some(),
        };
        self.elapsed = Duration::ZERO;
        if search {
            self.search_error.clear();
            self.search = Some(r.clone());
        } else {
            self.load_cursor.clone_from(&r.cursor);
            self.load_failure = None;
            self.load = Some(r.clone());
        }
        self.intents.push(Intent::Discover(r));
    }
    fn delete_event(&mut self, event: &Event) {
        let Some(ticket) = self.delete.ticket().cloned() else {
            return;
        };
        let mut send = None;
        let mut close = false;
        if let Some(b) = self.delete.body_mut(&ticket) {
            if matches!(event, Event::Paste(_)) {
                b.confirmation.disarm();
                return;
            }
            let Event::Key(k) = event else {
                return;
            };
            if k.kind != KeyEventKind::Press || !k.modifiers.is_empty() {
                return;
            }
            match k.code {
                KeyCode::Esc => close = true,
                KeyCode::Char('1') | KeyCode::Up | KeyCode::Home => {
                    b.delete = false;
                    b.confirmation.disarm();
                }
                KeyCode::Char('2') | KeyCode::Down | KeyCode::End => {
                    b.delete = true;
                    b.confirmation.disarm();
                }
                KeyCode::PageDown => {
                    b.offset = (b.offset + b.viewport).min(b.total.saturating_sub(b.viewport));
                    b.confirmation.disarm();
                }
                KeyCode::PageUp => {
                    b.offset = b.offset.saturating_sub(b.viewport);
                    b.confirmation.disarm();
                }
                KeyCode::Enter if !b.delete => close = true,
                KeyCode::Enter
                    if b.confirmation
                        .update(event, "delete", &Caution::Irreversible, "")
                        == Decision::Confirm =>
                {
                    send = Some(b.id.clone());
                    close = true;
                }
                _ => {}
            }
        }
        if close {
            self.delete.clear();
        }
        if let Some(id) = send {
            self.send(&id, Kind::Delete, String::new());
        }
    }
}
