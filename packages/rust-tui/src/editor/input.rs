//! Maps terminal events to editor actions; submission, modal routing and focus stay in hosts.

use super::{Editor, Outcome, history::EditKind};
use crate::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

impl Editor {
    pub(super) fn input(&mut self, event: &Event) -> Outcome {
        match event {
            Event::Paste(text) => self.insert(text),
            Event::Key(key) if key.kind != KeyEventKind::Release => {
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
            KeyCode::Char('z' | '-') if ctrl => return self.history(extend),
            KeyCode::Char('y' | '.') if ctrl => return self.history(true),
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
            KeyCode::Char('w') if ctrl => return self.delete(false, true),
            KeyCode::Char('u' | 'k') if ctrl => {
                let cursor = self.cursor();
                let end = if code == KeyCode::Char('k') {
                    self.text()[cursor..]
                        .find('\n')
                        .map_or(self.text().len(), |i| cursor + i)
                } else {
                    self.text()[..cursor].rfind('\n').map_or(0, |i| i + 1)
                };
                return self.delete_range(
                    self.selection().unwrap_or(cursor.min(end)..cursor.max(end)),
                    EditKind::Delete,
                );
            }
            KeyCode::Char('j') if ctrl => return self.replace("\n", EditKind::Newline),
            KeyCode::Enter if word || extend => return self.replace("\n", EditKind::Newline),
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
                return self.delete(code == KeyCode::Delete, word);
            }
            KeyCode::Char(c) if !word && !c.is_control() => {
                return self.replace(&c.to_string(), EditKind::Typing);
            }
            _ => return Outcome::Unhandled,
        }
        Outcome::Handled
    }
}
