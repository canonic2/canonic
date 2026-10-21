//! Isolates form input, staged navigation, cancellation and deliberate submission.

use super::{Intent, Kind, QuestionForm, Response, State, Ticket, model};
use crate::{
    dialog::{ClosePolicy, Dialog, DialogView, Dispatch, Size},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    picker::{Choice, ChoicePicker, Filter, Intent as Pick, SelectionMode},
};
use std::time::Duration;

impl QuestionForm {
    /// Consume input before the background Composer; paste never confirms.
    pub fn update(&mut self, event: &Event) {
        if self.closed() || self.state == State::Submitting {
            return;
        }
        if matches!(event, Event::Key(k) if k.kind == KeyEventKind::Release) {
            return;
        }
        if self.cancelling() {
            self.cancel_event(event);
            return;
        }
        if self.state == State::Expired {
            if let Event::Key(k) = event
                && k.kind == KeyEventKind::Press
                && k.modifiers.is_empty()
            {
                match k.code {
                    KeyCode::Char('y') => self.copy_answers(),
                    KeyCode::Esc => self.close(),
                    _ => {}
                }
            }
            return;
        }
        if !self.editing
            && !self.retained.is_empty()
            && matches!(event, Event::Key(k) if k.code == KeyCode::Char('y') && k.modifiers.is_empty() && k.kind == KeyEventKind::Press)
        {
            self.copy_answers();
            return;
        }
        if self.navigation(event) {
            return;
        }
        if self.step == self.stages.len() {
            self.review_event(event);
            return;
        }
        if self.editing {
            self.field_event(event);
        } else {
            self.choice_event(event);
        }
    }
    fn navigation(&mut self, event: &Event) -> bool {
        let Event::Key(k) = event else {
            return false;
        };
        let previous = k.code == KeyCode::BackTab
            || (k.code == KeyCode::Tab && k.modifiers == KeyModifiers::SHIFT)
            || (!self.editing && k.code == KeyCode::Left && k.modifiers.is_empty());
        let next = (k.code == KeyCode::Tab && k.modifiers.is_empty())
            || (!self.editing && k.code == KeyCode::Right && k.modifiers.is_empty());
        if !previous && !next {
            return false;
        }
        if k.kind == KeyEventKind::Press {
            self.go(if previous {
                self.step.saturating_sub(1)
            } else {
                (self.step + 1).min(self.last_step())
            });
            self.from_review = false;
        }
        true
    }
    fn last_step(&self) -> usize {
        if self.stages.len() == 1 {
            0
        } else {
            self.stages.len()
        }
    }
    /// Escape goes back one step with answers kept; on the first step it cancels.
    fn back(&mut self) {
        if self.step == 0 {
            self.ask_cancel();
        } else {
            self.go(self.step - 1);
            self.from_review = false;
        }
    }
    pub(super) fn go(&mut self, step: usize) {
        if let Some(s) = self.stages.get_mut(self.step) {
            s.visited = true;
        }
        self.step = step.min(self.last_step());
        self.editing = self
            .request
            .questions
            .get(self.step)
            .is_some_and(|q| q.kind == Kind::Text);
        self.offset = 0;
        self.review_row = None;
        self.notice.clear();
    }
    fn review_event(&mut self, event: &Event) {
        let Event::Key(k) = event else {
            return;
        };
        if k.modifiers.is_empty() && k.kind == KeyEventKind::Press {
            match k.code {
                KeyCode::Enter => {
                    if let Some(row) = self.review_row {
                        self.go(row);
                        self.from_review = true;
                    } else {
                        self.submit(false);
                    }
                }
                KeyCode::Esc => self.back(),
                KeyCode::Char(c @ '1'..='9') => {
                    let step = c as usize - '1' as usize;
                    if step < self.stages.len() {
                        self.go(step);
                        self.from_review = true;
                    }
                }
                _ => {}
            }
        }
        // Review scrolling also reaches questions beyond the numbered shortcuts.
        if k.modifiers.is_empty() {
            match k.code {
                KeyCode::Up => {
                    self.review_row = Some(self.review_row.unwrap_or(1).saturating_sub(1));
                }
                KeyCode::Down => {
                    self.review_row = Some(
                        self.review_row
                            .map_or(0, |r| (r + 1).min(self.stages.len() - 1)),
                    );
                }
                KeyCode::Home => self.review_row = Some(0),
                KeyCode::End => self.review_row = Some(self.stages.len() - 1),
                _ => {}
            }
        }
    }
    fn field_event(&mut self, event: &Event) {
        if let Event::Key(k) = event
            && k.modifiers.is_empty()
        {
            match k.code {
                KeyCode::Esc if k.kind == KeyEventKind::Press => {
                    self.editing = false;
                    return;
                }
                KeyCode::Enter => {
                    if k.kind == KeyEventKind::Press {
                        self.advance();
                    }
                    return;
                }
                _ => {}
            }
        }
        let s = &mut self.stages[self.step];
        let before = s.field.text().to_owned();
        s.field.update(event);
        if s.field.text() != before {
            self.notice.clear();
            s.other = !s.field.text().trim().is_empty();
            if s.other && self.request.questions[self.step].kind == Kind::Single {
                s.marks.clear();
            }
        }
        if s.field.rejected() {
            self.notice = s
                .field
                .notice()
                .unwrap_or("insertion rejected · 65536 byte limit")
                .into();
        }
    }
    fn choice_event(&mut self, event: &Event) {
        if let Event::Paste(_) = event {
            if self.other_row() {
                self.editing = true;
                self.field_event(event);
            }
            return;
        }
        let Event::Key(k) = event else {
            return;
        };
        if k.modifiers == KeyModifiers::CONTROL
            && k.code == KeyCode::Char('z')
            && k.kind == KeyEventKind::Press
        {
            let s = &mut self.stages[self.step];
            s.field.update(event);
            s.other = !s.field.text().trim().is_empty();
            if s.other && self.request.questions[self.step].kind == Kind::Single {
                s.marks.clear();
            }
            self.notice.clear();
            return;
        }
        if !k.modifiers.is_empty() {
            return;
        }
        match k.code {
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown => self.move_choice(k.code),
            KeyCode::Esc if k.kind == KeyEventKind::Press => self.back(),
            KeyCode::Enter if k.kind == KeyEventKind::Press => {
                if self.other_row() || self.request.questions[self.step].kind == Kind::Text {
                    self.editing = true;
                } else if self.request.questions[self.step].kind == Kind::Single {
                    self.choose();
                } else {
                    self.advance();
                }
            }
            KeyCode::Char(' ')
                if k.kind == KeyEventKind::Press
                    && self.request.questions[self.step].kind == Kind::Multiple =>
            {
                self.choose();
            }
            KeyCode::Char(c @ '1'..='9')
                if k.kind == KeyEventKind::Press
                    && self.request.questions[self.step].kind == Kind::Single =>
            {
                let row = c as usize - '1' as usize;
                if row < self.choice_count() {
                    self.stages[self.step].cursor = row;
                    self.choose();
                }
            }
            KeyCode::Delete if k.kind == KeyEventKind::Press && self.other_row() => {
                let s = &mut self.stages[self.step];
                s.field.update(&Event::Key(KeyEvent::new(
                    KeyCode::Char('g'),
                    KeyModifiers::CONTROL,
                )));
                s.field.insert("");
                s.other = false;
                self.notice = "answer cleared · ctrl+z brings it back".into();
            }
            KeyCode::Char(_) if self.other_row() => {
                self.editing = true;
                self.field_event(event);
            }
            _ => {}
        }
    }
    fn choice_count(&self) -> usize {
        let q = &self.request.questions[self.step];
        q.choices.len() + usize::from(q.other)
    }
    fn other_row(&self) -> bool {
        let q = &self.request.questions[self.step];
        q.other && self.stages[self.step].cursor == q.choices.len()
    }
    fn move_choice(&mut self, code: KeyCode) {
        let last = self.choice_count().saturating_sub(1);
        let s = &mut self.stages[self.step];
        s.cursor = match code {
            KeyCode::Up => s.cursor.saturating_sub(1),
            KeyCode::Down => (s.cursor + 1).min(last),
            KeyCode::Home => 0,
            KeyCode::End => last,
            KeyCode::PageUp => s.cursor.saturating_sub(self.viewport),
            _ => s.cursor.saturating_add(self.viewport).min(last),
        };
    }
    fn choose(&mut self) {
        if self.other_row() {
            self.editing = true;
            return;
        }
        let q = &self.request.questions[self.step];
        let s = &mut self.stages[self.step];
        let Some(c) = q.choices.get(s.cursor) else {
            return;
        };
        if let Some(reason) = &c.disabled {
            self.notice = crate::text::clean(reason, 4096, false);
            return;
        }
        self.notice.clear();
        if q.kind == Kind::Multiple {
            if !s.marks.remove(&s.cursor) {
                if q.maximum
                    .is_some_and(|m| s.marks.len() + usize::from(s.other) >= m)
                {
                    self.notice = "limit reached · unmark one first".into();
                    return;
                }
                s.marks.insert(s.cursor);
            }
        } else {
            s.marks.clear();
            s.marks.insert(s.cursor);
            s.other = false;
            if !s.field.text().is_empty() {
                s.field.update(&Event::Key(KeyEvent::new(
                    KeyCode::Char('g'),
                    KeyModifiers::CONTROL,
                )));
                s.field.insert("");
            }
            self.advance();
        }
    }
    fn advance(&mut self) {
        let q = &self.request.questions[self.step];
        if let Some(reason) = model::validation(q, &self.stages[self.step]) {
            self.notice = reason;
            return;
        }
        if self.stages.len() == 1 {
            self.submit(false);
        } else {
            self.go(if self.from_review {
                self.stages.len()
            } else {
                self.step + 1
            });
            self.from_review = false;
        }
    }
    fn submit(&mut self, partial: bool) {
        for (i, (q, s)) in self.request.questions.iter().zip(&self.stages).enumerate() {
            if partial && model::answer(q, s).is_none() {
                continue;
            }
            if let Some(reason) = model::validation(q, s) {
                self.go(i);
                self.notice = reason;
                self.from_review = self.stages.len() > 1;
                return;
            }
        }
        let response = Response {
            id: self.request.id.clone(),
            revision: self.request.revision,
            ticket: Ticket::new(),
            answers: self.answers(),
            partial,
        };
        self.pending = Some(response.clone());
        self.intents.push(Intent::Submit(response));
        self.state = State::Submitting;
        self.elapsed = Duration::ZERO;
        self.notice.clear();
        self.cancel.clear();
    }
    fn close(&mut self) {
        self.state = State::Cancelled;
        self.cancel.clear();
        self.copy = None;
        self.intents.push(Intent::Cancelled {
            id: self.request.id.clone(),
            revision: self.request.revision,
        });
    }
    fn ask_cancel(&mut self) {
        // Going back to the host's previous step drops nothing it has accepted.
        if self.answers().is_empty() || self.request.back {
            self.close();
            return;
        }
        let mut choices = vec![Choice::new("keep", "Keep answering")];
        if self.request.partial {
            choices.push(Choice::new("partial", "Send answers so far"));
        }
        choices.push(Choice::new("cancel", "Cancel and send nothing"));
        let body = ChoicePicker::new(choices, Some("keep"), SelectionMode::Single, Filter::Off)
            .expect("distinct action IDs");
        let _ = self.cancel.push(Dialog {
            id: "cancel".into(),
            title: "cancel questions?".into(),
            targets: vec![1],
            body,
            size: Size::default(),
            close: ClosePolicy::default(),
            view: DialogView {
                message: if self.request.partial {
                    "The host accepts partial answers.".into()
                } else {
                    format!("Your {} answers will be dropped.", self.answers().len())
                },
                footer: "enter choose · esc keep answering".into(),
                compact_footer: "enter · esc keep".into(),
                ..DialogView::default()
            },
        });
    }
    fn cancel_event(&mut self, event: &Event) {
        let mut chosen = Pick::None;
        let dispatched = self.cancel.dispatch(event, self.area, |body, _, e| {
            chosen = body.update(e);
            !matches!(chosen, Pick::Cancelled)
        });
        if matches!(dispatched, Dispatch::Dismissed(_)) {
            return;
        }
        if let Pick::Selected(id) = chosen {
            self.cancel.clear();
            match id.as_str() {
                "partial" => self.submit(true),
                "cancel" => self.close(),
                _ => {}
            }
        }
    }
    fn copy_answers(&mut self) {
        if self.copy.is_some() {
            return;
        }
        let text = if self.state == State::Expired {
            self.answer_text()
        } else {
            self.retained.clone()
        };
        let ticket = Ticket::new();
        self.copy = Some(ticket.clone());
        self.intents.push(Intent::Copy {
            id: self.request.id.clone(),
            revision: self.request.revision,
            ticket,
            text,
        });
        self.notice = "copying answers · waiting for host".into();
    }
}
