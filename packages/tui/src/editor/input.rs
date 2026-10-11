//! Maps terminal events to editor actions; submission, modal routing and focus stay in hosts.

use super::{Editor, Outcome, history::EditKind};
use crate::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

impl Editor {
    pub(super) fn input(&mut self, event: &Event) -> Outcome {
        match event {
            Event::Paste(text) => self.insert(text),
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if !kill_key(key.code, key.modifiers) {
                    self.kill_group = None;
                }
                if !(key.code == KeyCode::Char('y') && key.modifiers == KeyModifiers::ALT) {
                    self.yanked = None;
                }
                self.before_key();
                self.key(key.code, key.modifiers)
            }
            _ => Outcome::Unhandled,
        }
    }

    fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Outcome {
        let extend = modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let word = modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match code {
            KeyCode::Esc if self.draft.anchor.is_some() => {
                self.draft.anchor = None;
                self.history.break_group();
            }
            KeyCode::Char('z' | 'Z' | '-') if ctrl => return self.history(extend),
            KeyCode::Char('z' | 'Z') if modifiers == KeyModifiers::ALT => {
                return self.history(true);
            }
            KeyCode::Char('y') if modifiers == KeyModifiers::ALT => return self.yank(true),
            KeyCode::Char('y') if ctrl => return self.yank(false),
            KeyCode::Char('d') if modifiers == KeyModifiers::ALT => {
                let cursor = self.cursor();
                return self.kill(self.selection().unwrap_or(cursor..self.word_boundary(true)));
            }
            KeyCode::Backspace if word => {
                return self.kill(self.selection().unwrap_or(self.word_part()..self.cursor()));
            }
            KeyCode::Char('a') if ctrl && extend => {
                self.move_to(0, false);
                self.move_to(self.text().len(), true);
                self.goal = None;
            }
            KeyCode::Char('g') if ctrl => {
                self.move_to(0, false);
                self.move_to(self.text().len(), true);
                self.goal = None;
            }
            KeyCode::Char('a' | 'e') if ctrl => {
                let start = self.visual_edge(false);
                self.move_to(self.visual_edge(code == KeyCode::Char('e')), extend);
                self.draft.upstream = code == KeyCode::Char('e') && start < self.cursor();
                self.goal = None;
            }
            KeyCode::Char('w') if ctrl => {
                return self.kill(
                    self.selection()
                        .unwrap_or(self.space_boundary()..self.cursor()),
                );
            }
            KeyCode::Char('u' | 'k') if ctrl => {
                return self.kill_line(code == KeyCode::Char('k'));
            }
            KeyCode::Char('j') if ctrl => return self.replace("\n", EditKind::Newline),
            KeyCode::Enter
                if !ctrl && modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
            {
                return self.replace("\n", EditKind::Newline);
            }
            KeyCode::Left | KeyCode::Right => self.horizontal(code == KeyCode::Right, word, extend),
            KeyCode::Up | KeyCode::Down => self.vertical(code == KeyCode::Down, extend, 1),
            KeyCode::PageUp | KeyCode::PageDown => {
                self.vertical(code == KeyCode::PageDown, extend, self.height.max(1));
            }
            KeyCode::Home | KeyCode::End => {
                let start = self.visual_edge(false);
                let target = if ctrl {
                    if code == KeyCode::Home {
                        0
                    } else {
                        self.text().len()
                    }
                } else {
                    self.visual_edge(code == KeyCode::End)
                };
                self.move_to(target, extend);
                self.draft.upstream = code == KeyCode::End && !ctrl && start < target;
                self.goal = None;
            }
            KeyCode::Backspace | KeyCode::Delete => {
                let chip = !word
                    && self.text()[self.cursor()..]
                        .chars()
                        .next()
                        .is_some_and(|c| self.draft.atoms.contains_key(&c));
                return self.delete(code == KeyCode::Delete || chip, word);
            }
            KeyCode::Char(c) if !word && !c.is_control() => {
                return self.replace(&c.to_string(), EditKind::Typing);
            }
            _ => return Outcome::Unhandled,
        }
        Outcome::Handled
    }
}

fn kill_key(code: KeyCode, modifiers: KeyModifiers) -> bool {
    (modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('u' | 'k' | 'w')))
        || (modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
            && code == KeyCode::Backspace)
        || (modifiers == KeyModifiers::ALT && code == KeyCode::Char('d'))
}
