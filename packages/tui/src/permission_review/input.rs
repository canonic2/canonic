//! Isolated inspection, deliberate choice confirmation and exact-scope dialogs.

use super::{Intent, PermissionReview, Response, ScopeCheck, State, Ticket};
use crate::{
    dialog::{Caution, ClosePolicy, Decision, Dialog, DialogView, Dispatch, Size},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
};
use std::time::Duration;

impl PermissionReview {
    /// Consume every active review event before any background focus scope.
    pub fn update(&mut self, event: &Event) {
        if self.closed || self.state == State::Sending {
            return;
        }
        if matches!(event, Event::Paste(_)) {
            self.notice = "paste ignored · a review takes no text".into();
            self.disarm();
            if let Some(ticket) = self.scope.ticket().cloned()
                && let Some(body) = self.scope.body_mut(&ticket)
            {
                body.confirmation.disarm();
            }
            return;
        }
        let Event::Key(key) = event else {
            return;
        };
        if key.kind == KeyEventKind::Release {
            return;
        }
        if self.confirming() {
            self.scope_event(event);
            return;
        }
        if self.searching {
            self.search_event(event);
            return;
        }
        if key.code == KeyCode::Char('f')
            && key.modifiers == KeyModifiers::CONTROL
            && key.kind == KeyEventKind::Press
        {
            self.disarm();
            self.details = true;
            self.cursor = Some(0);
            self.searching = true;
            self.refresh_matches();
            return;
        }
        if !key.modifiers.is_empty() {
            return;
        }
        match key.code {
            KeyCode::Esc if key.kind == KeyEventKind::Press => self.escape(),
            KeyCode::Tab if key.kind == KeyEventKind::Press => self.toggle_details(),
            KeyCode::PageUp | KeyCode::PageDown if self.details => self.scroll(key.code),
            KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End => {
                self.move_cursor(key.code);
            }
            KeyCode::Char(c @ '1'..='9') if key.kind == KeyEventKind::Press && !self.locked() => {
                let row = usize::from(c as u8 - b'0');
                if row <= self.request.choices.len() {
                    self.select(row);
                }
            }
            KeyCode::Enter if key.kind == KeyEventKind::Press => {
                if self.cursor == Some(0) {
                    self.toggle_details();
                } else if !self.locked() {
                    self.confirm(event);
                }
            }
            KeyCode::Char('r')
                if key.kind == KeyEventKind::Press && self.state == State::Failed =>
            {
                self.retry();
            }
            _ => {}
        }
    }
    fn disarm(&mut self) {
        self.confirmation.disarm();
        if self.notice.starts_with("press enter again") {
            self.notice.clear();
        }
    }
    fn escape(&mut self) {
        self.disarm();
        if self.details {
            self.toggle_details();
            return;
        }
        self.closed = true;
        if !self.locked() {
            self.state = State::Dismissed;
        }
        self.intents.push(Intent::Dismissed {
            id: self.request.id.clone(),
            revision: self.request.revision,
        });
    }
    fn toggle_details(&mut self) {
        self.disarm();
        self.details = !self.details;
        if self.details {
            self.previous = self.cursor;
            self.cursor = Some(0);
        } else {
            self.searching = false;
            self.cursor = self.previous.take();
        }
    }
    fn select(&mut self, row: usize) {
        self.disarm();
        self.cursor = Some(row);
        if let Some(failed) = &self.failed
            && self.request.choices[row - 1].id != failed.choice
        {
            self.failed = None;
            self.state = State::Open;
            self.notice.clear();
        }
        if let Some(reason) = &self.request.choices[row - 1].disabled {
            self.notice = reason.clone();
        }
    }
    fn move_cursor(&mut self, code: KeyCode) {
        self.disarm();
        if self.details && self.cursor == Some(0) && matches!(code, KeyCode::Up | KeyCode::Down) {
            self.offset = if code == KeyCode::Up {
                self.offset.saturating_sub(1)
            } else {
                (self.offset + 1).min(self.detail_total.saturating_sub(self.viewport))
            };
            return;
        }
        if self.locked() {
            self.cursor = Some(0);
            return;
        }
        let last = self.request.choices.len();
        let row = match code {
            KeyCode::Home => 0,
            KeyCode::End => last,
            KeyCode::Up => self.cursor.unwrap_or(last + 1).saturating_sub(1),
            _ => self.cursor.map_or(0, |r| (r + 1).min(last)),
        };
        if row == 0 {
            self.cursor = Some(0);
        } else {
            self.select(row);
        }
    }
    fn scroll(&mut self, code: KeyCode) {
        self.disarm();
        self.offset = if code == KeyCode::PageUp {
            self.offset.saturating_sub(self.viewport)
        } else {
            self.offset
                .saturating_add(self.viewport)
                .min(self.detail_total.saturating_sub(self.viewport))
        };
    }
    fn confirm(&mut self, event: &Event) {
        let Some(choice) = self.selected() else {
            return;
        };
        if let Some(reason) = &choice.disabled {
            self.notice = reason.clone();
            return;
        }
        if self.failed.as_ref().is_some_and(|r| r.choice == choice.id) {
            self.notice = "not applied · press r to retry".into();
            return;
        }
        if choice.wider {
            self.open_scope();
            return;
        }
        let id = choice.id.clone();
        let destructive = choice.destructive;
        let caution = if destructive {
            Caution::Irreversible
        } else {
            Caution::Normal
        };
        match self.confirmation.update(event, &id, &caution, "") {
            Decision::Confirm => self.send(0),
            Decision::Armed => self.notice = "press enter again within 3 s to confirm".into(),
            _ => {}
        }
    }
    fn send(&mut self, retry: u32) {
        let Some(choice) = self.selected() else {
            return;
        };
        let response = Response {
            id: self.request.id.clone(),
            revision: self.request.revision,
            choice: choice.id.clone(),
            scope: choice.scope.clone(),
            ticket: Ticket::new(),
            retry,
        };
        self.start(response);
    }
    fn start(&mut self, response: Response) {
        self.cursor = self
            .request
            .choices
            .iter()
            .position(|c| c.id == response.choice)
            .map(|i| i + 1);
        self.state = State::Sending;
        self.elapsed = Duration::ZERO;
        self.notice.clear();
        self.failed = None;
        self.searching = false;
        self.details = false;
        self.scope.clear();
        self.disarm();
        self.pending = Some(response.clone());
        self.intents.push(Intent::Respond(response));
    }
    fn retry(&mut self) {
        if let Some(mut response) = self.failed.take() {
            response.ticket = Ticket::new();
            response.retry = response.retry.saturating_add(1);
            self.start(response);
        }
    }
    fn open_scope(&mut self) {
        let Some(c) = self.selected() else {
            return;
        };
        let body = ScopeCheck {
            scope: c.scope.clone(),
            label: c.label.clone(),
            accept: false,
            offset: 0,
            viewport: 1,
            total: 0,
            destructive: c.destructive,
            confirmation: crate::dialog::Confirmation::default(),
        };
        let title = format!("{}?", c.label);
        self.disarm();
        let _ = self.scope.push(Dialog {
            id: "scope".into(),
            title,
            targets: vec![1],
            body,
            size: Size::default(),
            close: ClosePolicy::default(),
            view: DialogView {
                footer: "1–2 choose · enter confirm · esc back".into(),
                compact_footer: "enter · esc back · pgdn scope".into(),
                ..DialogView::default()
            },
        });
    }
    fn scope_event(&mut self, event: &Event) {
        let mut decision = Decision::None;
        let mut back = false;
        let dispatched = self.scope.dispatch(event, self.area, |body, _, event| {
            let Event::Key(k) = event else {
                return true;
            };
            if k.code == KeyCode::Esc && k.modifiers.is_empty() && k.kind == KeyEventKind::Press {
                return false;
            }
            if !k.modifiers.is_empty() {
                return true;
            }
            match k.code {
                KeyCode::Up | KeyCode::Down => {
                    body.accept = k.code == KeyCode::Down;
                    body.confirmation.disarm();
                }
                KeyCode::Char('1' | '2') if k.kind == KeyEventKind::Press => {
                    body.accept = k.code == KeyCode::Char('2');
                    body.confirmation.disarm();
                }
                KeyCode::PageUp | KeyCode::PageDown => {
                    body.offset = if k.code == KeyCode::PageUp {
                        body.offset.saturating_sub(body.viewport)
                    } else {
                        (body.offset + body.viewport).min(body.total.saturating_sub(body.viewport))
                    };
                    body.confirmation.disarm();
                }
                KeyCode::Enter if k.kind == KeyEventKind::Press => {
                    if body.accept {
                        decision = body.confirmation.update(
                            event,
                            "scope",
                            &if body.destructive {
                                Caution::Irreversible
                            } else {
                                Caution::Normal
                            },
                            "",
                        );
                    } else {
                        back = true;
                    }
                }
                _ => {}
            }
            true
        });
        if back || matches!(dispatched, Dispatch::Dismissed(_)) {
            self.scope.clear();
            self.notice = "scope not confirmed".into();
        } else if decision == Decision::Confirm {
            self.send(0);
        } else if decision == Decision::Armed {
            self.notice = "press enter again within 3 s to confirm".into();
        } else if self.notice.starts_with("press enter again")
            && self
                .scope
                .ticket()
                .and_then(|t| self.scope.body(t))
                .is_none_or(|b| !b.confirmation.armed("scope"))
        {
            self.notice.clear();
        }
    }
    fn search_event(&mut self, event: &Event) {
        if let Event::Key(k) = event
            && k.modifiers.is_empty()
        {
            if k.code == KeyCode::Enter {
                if k.kind == KeyEventKind::Press && !self.matches.is_empty() {
                    self.match_index = (self.match_index + 1) % self.matches.len();
                    self.offset = self.matches[self.match_index];
                }
                return;
            }
            if k.code == KeyCode::Esc {
                if k.kind == KeyEventKind::Press {
                    self.searching = false;
                    self.notice = "search closed · esc hide details".into();
                }
                return;
            }
        }
        let before = self.query.text().to_owned();
        self.query.update(event);
        if self.query.text() != before {
            self.refresh_matches();
        }
    }
    pub(super) fn refresh_matches(&mut self) {
        let query = self.query.text().to_lowercase();
        let sources = self.source_lines();
        let matching: std::collections::HashSet<_> = sources
            .iter()
            .enumerate()
            .filter(|(_, line)| !query.is_empty() && line.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect();
        let mut seen = std::collections::HashSet::new();
        self.matches = self
            .inspection_rows(
                self.area.width.max(1),
                crate::theme::Presentation::default(),
            )
            .iter()
            .enumerate()
            .filter(|(_, r)| matching.contains(&r.source) && seen.insert(r.source))
            .map(|(i, _)| i)
            .collect();
        self.match_index = 0;
        if let Some(offset) = self.matches.first() {
            self.offset = *offset;
        }
    }
}
