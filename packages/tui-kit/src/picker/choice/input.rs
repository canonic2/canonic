//! Routes navigation and explicit confirmation separately from the filter editor.

use super::{ChoicePicker, Intent, SelectionMode, Status};
use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    text::clean,
};

impl ChoicePicker {
    pub(super) fn input(&mut self, event: &Event) -> Intent {
        let Event::Key(key) = event else {
            if let Event::Paste(text) = event
                && self.filtering()
                && self.status == Status::Ready
            {
                self.query.insert(&clean(text, 4096, false));
                self.refilter();
            }
            return Intent::None;
        };
        if key.kind == KeyEventKind::Release {
            return Intent::None;
        }
        self.notice = None;
        if key.code == KeyCode::Esc {
            if self.status == Status::Ready && !self.query().is_empty() {
                self.query.set_text("");
                self.refilter();
                return Intent::None;
            }
            return Intent::Cancelled;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Intent::Cancelled;
        }
        if self.status != Status::Ready {
            return Intent::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End => {
                self.navigate(key.code);
                return Intent::None;
            }
            KeyCode::Char('p' | 'n') if ctrl => {
                self.navigate(if key.code == KeyCode::Char('p') {
                    KeyCode::Up
                } else {
                    KeyCode::Down
                });
                return Intent::None;
            }
            KeyCode::Enter if key.kind == KeyEventKind::Press && key.modifiers.is_empty() => {
                return self.confirm();
            }
            KeyCode::Char(' ')
                if self.mode == SelectionMode::Multiple
                    && key.kind == KeyEventKind::Press
                    && key.modifiers.is_empty() =>
            {
                if let Some(id) = self.selected.clone() {
                    self.toggle(&id);
                }
                return Intent::None;
            }
            KeyCode::Char('a')
                if self.mode == SelectionMode::Multiple
                    && key.modifiers == KeyModifiers::ALT
                    && key.kind == KeyEventKind::Press =>
            {
                self.mark_visible();
                return Intent::None;
            }
            KeyCode::F(3) if self.inspect && key.kind == KeyEventKind::Press => {
                return self.selected.clone().map_or(Intent::None, Intent::Inspect);
            }
            KeyCode::Char(c @ '1'..='9')
                if !self.filtering()
                    && key.modifiers.is_empty()
                    && key.kind == KeyEventKind::Press =>
            {
                if let Some(choice) = self.choices.get((c as usize) - ('1' as usize))
                    && choice.disabled.is_none()
                {
                    let id = choice.id.clone();
                    self.selected = Some(id.clone());
                    if self.mode == SelectionMode::Multiple {
                        self.toggle(&id);
                    } else {
                        return Intent::Selected(id);
                    }
                }
                return Intent::None;
            }
            // Enter repeats/newline modifiers never become filter input.
            KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab => return Intent::None,
            KeyCode::Char('j') if ctrl => return Intent::None,
            KeyCode::Char(' ') if self.mode == SelectionMode::Multiple => return Intent::None,
            _ => {}
        }
        if self.filtering() {
            self.query.update(event);
            self.refilter();
        }
        Intent::None
    }

    fn toggle(&mut self, id: &str) {
        if self.marked.remove(id) {
            return;
        }
        if self
            .maximum
            .is_none_or(|maximum| self.marked.len() < maximum)
        {
            self.marked.insert(id.to_owned());
        } else {
            self.notice = Some("unmark one first".into());
        }
    }
    fn mark_visible(&mut self) {
        let ids: Vec<_> = self
            .matches
            .iter()
            .map(|m| &self.choices[m.index])
            .filter(|c| c.disabled.is_none())
            .map(|c| c.id.clone())
            .collect();
        let remove = ids.iter().all(|id| self.marked.contains(id));
        for id in ids {
            if remove {
                self.marked.remove(&id);
            } else if self
                .maximum
                .is_none_or(|maximum| self.marked.len() < maximum)
            {
                self.marked.insert(id);
            }
        }
    }
    fn confirm(&mut self) -> Intent {
        if self.choices.is_empty() {
            return Intent::None;
        }
        if self.mode == SelectionMode::Multiple {
            if self.marked.len() < self.minimum {
                self.notice = Some(format!("press space to mark at least {}", self.minimum));
                return Intent::None;
            }
            Intent::Confirmed(self.marked().into_iter().map(str::to_owned).collect())
        } else {
            self.selected.clone().map_or(Intent::None, Intent::Selected)
        }
    }
    fn navigate(&mut self, code: KeyCode) {
        let rows = self.rows();
        let enabled: Vec<_> = rows
            .iter()
            .enumerate()
            .filter_map(|(row, item)| {
                let super::view::Row::Choice(m) = item else {
                    return None;
                };
                let choice = &self.choices[m.index];
                choice
                    .disabled
                    .is_none()
                    .then_some((row, choice.id.clone()))
            })
            .collect();
        let current = enabled
            .iter()
            .position(|(_, id)| Some(id) == self.selected.as_ref());
        let index = current.unwrap_or(0);
        let next = match code {
            KeyCode::End => enabled.last(),
            KeyCode::Up if current.is_some() => enabled.get(index.saturating_sub(1)),
            KeyCode::Down if current.is_some() => {
                enabled.get((index + 1).min(enabled.len().saturating_sub(1)))
            }
            KeyCode::PageDown => {
                let target = enabled
                    .get(index)
                    .map_or(0, |(r, _)| r.saturating_add(self.viewport));
                enabled
                    .iter()
                    .find(|(r, _)| *r >= target)
                    .or_else(|| enabled.last())
            }
            KeyCode::PageUp => {
                let target = enabled
                    .get(index)
                    .map_or(0, |(r, _)| r.saturating_sub(self.viewport));
                enabled
                    .iter()
                    .rev()
                    .find(|(r, _)| *r <= target)
                    .or_else(|| enabled.first())
            }
            _ => enabled.first(),
        };
        self.selected = next.map(|(_, id)| id.clone());
    }
}
