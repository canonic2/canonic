//! Composes public editors and focus scopes with resettable local fixtures.

use crate::{
    editor::{Editor, EditorView, Outcome},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    focus::Focus,
    theme::Presentation,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::{Clear, Paragraph},
};

pub(super) struct EditorDemo {
    editors: [Editor; 3],
    focus: Focus<usize>,
    scenario: usize,
    note: String,
}

impl EditorDemo {
    pub(super) fn new() -> Self {
        // Fixed distinct fixture IDs; this constructor cannot fail.
        let focus =
            Focus::new(vec![0, 1]).unwrap_or_else(|| unreachable!("fixture IDs are unique"));
        let mut demo = Self {
            editors: std::array::from_fn(|_| Editor::default()),
            focus,
            scenario: 0,
            note: String::new(),
        };
        demo.reset();
        demo
    }

    fn reset(&mut self) {
        self.editors = std::array::from_fn(|_| Editor::default());
        self.focus =
            Focus::new(vec![0, 1]).unwrap_or_else(|| unreachable!("fixture IDs are unique"));
        let first = &mut self.editors[0];
        first.set_text("Edit this draft in the middle.\nEnter or Ctrl-J inserts a newline.");
        let name = match self.scenario {
            1 => {
                self.focus.advance(false);
                "blurred"
            }
            2 => {
                first.set_text("");
                "empty"
            }
            3 => {
                first.set_text("Prose wraps at word boundaries while a long URL breaks at the edge.\nhttps://example.com/a-long-unbroken-path-with-more-than-one-terminal-row");
                "wrap"
            }
            4 => {
                first.set_text(
                    "Release notes\nSelection keeps its byte identity across wrapped rows.",
                );
                first.update(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Up,
                    KeyModifiers::SHIFT,
                )));
                "selection"
            }
            5 => {
                first.set_text(
                    "Cafe\u{301} · 界 · 👩‍💻\nWide characters and combining accents remain whole.",
                );
                "unicode"
            }
            6 => {
                first.set_text(
                    &"Release notes: keep the cursor visible and leave context.\n".repeat(35),
                );
                first.update(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Home,
                    KeyModifiers::CONTROL,
                )));
                for _ in 0..12 {
                    first.update(&Event::Key(crate::event::KeyEvent::new(
                        KeyCode::Down,
                        KeyModifiers::NONE,
                    )));
                }
                "scrolled"
            }
            7 => {
                first.set_text("Seen in the worker:\n");
                first.insert(&"Example diagnostic output\n".repeat(38));
                "pasted"
            }
            8 => {
                first.set_text("Seen in the worker:\n");
                first.insert("One paste, one undo step.");
                first.update(&Event::Key(crate::event::KeyEvent::new(
                    KeyCode::Char('z'),
                    KeyModifiers::CONTROL,
                )));
                "undo"
            }
            9 => {
                *first = Editor::new(120);
                first.set_text(&"Near limit text ".repeat(8)[..108]);
                "near-limit"
            }
            10 => {
                *first = Editor::new(120);
                first.set_text(&"Full draft ".repeat(12));
                first.insert("x");
                "limit"
            }
            11 => {
                *first = Editor::new(120);
                first.set_text("Paste at limit: ");
                first.insert(&"界".repeat(80));
                "paste-cut"
            }
            _ => "focused",
        };
        self.editors[1].set_text("Independent second draft");
        self.note = format!("#editor:{name} · Tab focus · F3 modal · F2 next fixture · F4 colors");
    }

    pub(super) fn tick(&mut self, elapsed: std::time::Duration) {
        for editor in &mut self.editors {
            editor.tick(elapsed);
        }
    }

    pub(super) fn input(&self, index: usize) -> Option<&str> {
        self.editors.get(index).map(Editor::text)
    }

    pub(super) fn update(&mut self, event: &Event) -> bool {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Release {
                return false;
            }
            match key.code {
                KeyCode::Esc if self.focus.is_modal() => {
                    if let Some(index) = self.focus.active().copied()
                        && self.editors[index].update(event) != Outcome::Unhandled
                    {
                        return true;
                    }
                    self.focus.pop();
                    return true;
                }
                KeyCode::Esc => {
                    return self.focus.active().copied().is_some_and(|index| {
                        self.editors[index].update(event) != Outcome::Unhandled
                    });
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    self.focus.advance(
                        key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT),
                    );
                    return true;
                }
                KeyCode::F(3) if !self.focus.is_modal() => {
                    self.focus.push(vec![2]);
                    return true;
                }
                KeyCode::F(2) => {
                    self.scenario = (self.scenario + 1) % 12;
                    self.reset();
                    return true;
                }
                KeyCode::Enter if key.modifiers.is_empty() => {
                    if let Some(index) = self.focus.active().copied() {
                        self.editors[index].update(&Event::Key(crate::event::KeyEvent::new(
                            KeyCode::Char('j'),
                            KeyModifiers::CONTROL,
                        )));
                    }
                    return true;
                }
                _ => {}
            }
        }
        if let Some(index) = self.focus.active().copied() {
            let result = self.editors[index].update(event);
            if result == Outcome::Rejected {
                self.note = "Byte limit reached · no complete grapheme fits".into();
            }
            return result != Outcome::Unhandled;
        }
        false
    }

    pub(super) fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        let [first, second, hints] = Layout::vertical([
            Constraint::Percentage(60),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .areas(area);
        self.editors[0].render_with(
            frame,
            first,
            EditorView {
                title: if self.scenario >= 9 {
                    "commit message"
                } else {
                    "draft"
                },
                placeholder: "Write release notes",
                presentation: theme,
                ..EditorView::default()
            },
            self.focus.active() == Some(&0),
        );
        self.editors[1].render_with(
            frame,
            second,
            EditorView {
                title: "second draft",
                presentation: theme,
                ..EditorView::default()
            },
            self.focus.active() == Some(&1),
        );
        frame.render_widget(
            Paragraph::new(self.note.as_str()).style(theme.muted()),
            hints,
        );
        if self.focus.is_modal() {
            let modal = Rect::new(
                area.x + area.width / 8,
                area.y + area.height / 4,
                area.width - area.width / 4,
                area.height / 2,
            );
            frame.render_widget(Clear, modal);
            self.editors[2].render_with(
                frame,
                modal,
                EditorView {
                    title: "modal draft",
                    placeholder: "Type here",
                    presentation: theme,
                    ..EditorView::default()
                },
                true,
            );
        }
    }
}
