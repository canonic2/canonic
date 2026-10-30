//! Stable section/notice navigation, explicit folding and action intent routing.

use super::{Action, Mode, State, StatusPanels, Target};
use crate::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
pub(super) fn update(p: &mut StatusPanels, event: &Event) -> bool {
    let Event::Key(key) = event else {
        return p.mode != Mode::Closed;
    };
    if key.kind == KeyEventKind::Release {
        return p.mode != Mode::Closed;
    }
    if key.code == KeyCode::Char('g') && key.modifiers == KeyModifiers::CONTROL {
        if key.kind == KeyEventKind::Press
            && let Some(n) = p.current_notice()
            && let Some(action) = n.action.clone()
        {
            p.issue(Target::Notice(n.id.clone()), action);
        }
        return p.current_notice().is_some();
    }
    if p.mode == Mode::Closed {
        return false;
    }
    if key.modifiers == KeyModifiers::ALT && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
    {
        p.details_offset = if key.code == KeyCode::PageUp {
            p.details_offset.saturating_sub(4)
        } else {
            p.details_offset.saturating_add(4)
        };
        return true;
    }
    if key.modifiers != KeyModifiers::NONE {
        return false;
    }
    let press = key.kind == KeyEventKind::Press;
    match key.code {
        KeyCode::Esc if press => p.close(),
        KeyCode::Up => p.navigate(-1),
        KeyCode::Down => p.navigate(1),
        KeyCode::Home => p.navigate(isize::MIN),
        KeyCode::End => p.navigate(isize::MAX),
        KeyCode::PageUp => p.navigate(-isize::try_from(p.viewport.max(1)).unwrap_or(isize::MAX)),
        KeyCode::PageDown => p.navigate(isize::try_from(p.viewport.max(1)).unwrap_or(isize::MAX)),
        KeyCode::Left | KeyCode::Right if p.mode == Mode::Status => {
            p.details_offset = 0;
            if let Some(Target::Section { id, .. }) = p.cursor.clone() {
                if key.code == KeyCode::Left {
                    p.folded.insert(id.clone());
                    p.cursor = Some(Target::Section { id, row: None });
                } else {
                    p.folded.remove(&id);
                }
            }
        }
        KeyCode::Enter if press => {
            if let Some(target) = p.cursor()
                && let Some(action) = p.offered(&target, false)
            {
                p.issue(target, action);
            }
        }
        KeyCode::Char('r') if press && p.mode == Mode::Status => {
            if let Some(Target::Section { id, .. }) = p.cursor.clone() {
                let target = Target::Section { id, row: None };
                if let Some(action) = p.offered(&target, true) {
                    p.issue(target, action);
                }
            }
        }
        KeyCode::Char('d') if press && p.mode == Mode::Notices => {
            if let Some(id) = p.notice_cursor.clone() {
                p.dismiss(&id);
            }
        }
        _ => return false,
    }
    true
}
impl StatusPanels {
    pub(super) fn targets(&self) -> Vec<Target> {
        if self.mode == Mode::Notices {
            return self
                .notices()
                .into_iter()
                .map(|n| Target::Notice(n.id.clone()))
                .collect();
        }
        let mut targets = vec![];
        for s in &self.snapshot.sections {
            targets.push(Target::Section {
                id: s.id.clone(),
                row: None,
            });
            if !self.folded.contains(&s.id) && s.state == State::Ready {
                targets.extend(s.rows.iter().map(|r| Target::Section {
                    id: s.id.clone(),
                    row: Some(r.id.clone()),
                }));
            }
        }
        targets
    }
    fn navigate(&mut self, delta: isize) {
        let targets = self.targets();
        let current = self.cursor();
        let index = targets
            .iter()
            .position(|t| Some(t) == current.as_ref())
            .unwrap_or(0);
        if let Some(target) = targets.get(
            index
                .saturating_add_signed(delta)
                .min(targets.len().saturating_sub(1)),
        ) {
            if let Target::Notice(id) = target {
                self.notice_cursor = Some(id.clone());
            } else {
                self.cursor = Some(target.clone());
            }
        }
        self.details_offset = 0;
    }
    pub(super) fn offered(&self, target: &Target, retry: bool) -> Option<Action> {
        match target {
            Target::Section { id, row } => {
                let s = self.snapshot.sections.iter().find(|s| &s.id == id)?;
                if retry {
                    return (s.state == State::Error).then(|| s.retry.clone()).flatten();
                }
                if let Some(id) = row {
                    s.rows.iter().find(|r| &r.id == id)?.action.clone()
                } else if s.state == State::Ready {
                    s.action.clone()
                } else {
                    None
                }
            }
            Target::Notice(id) => self
                .notices()
                .into_iter()
                .find(|n| &n.id == id)?
                .action
                .clone(),
        }
    }
}
