//! Composes public editors and focus scopes with resettable local fixtures.

use crate::{
    editor::{Editor, EditorView, Outcome},
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    focus::Focus,
    input::{Action as InputAction, LineInput},
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
    line: LineInput,
    line_text: String,
}

fn binding_fixture(editor: &mut Editor, scenario: usize) -> &'static str {
    let key = |code, modifiers| Event::Key(crate::event::KeyEvent::new(code, modifiers));
    match scenario {
        12 => {
            editor.set_text("First kill");
            editor.update(&key(KeyCode::Char('u'), KeyModifiers::CONTROL));
            editor.insert("Second kill");
            editor.update(&key(KeyCode::Char('u'), KeyModifiers::CONTROL));
            editor.update(&key(KeyCode::Char('y'), KeyModifiers::CONTROL));
            "kill-ring · alt+y cycles"
        }
        13 => {
            editor.set_text("getHTTPHeader42 snake_case.path");
            "word-parts · alt+backspace / alt+d / ctrl+w"
        }
        14 => {
            editor.set_text("First line\nSecond line");
            "line-kills · repeat ctrl+u / ctrl+k to join"
        }
        _ => {
            *editor = Editor::new(3);
            editor.set_text("ab");
            editor.update(&key(KeyCode::Char('界'), KeyModifiers::NONE));
            "typed-refused"
        }
    }
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
            line: LineInput::new("", 8192),
            line_text: String::new(),
        };
        demo.reset();
        demo
    }

    fn reset_line(&mut self) {
        self.line_text = match self.scenario {
            25 => "Acme".into(),
            27 | 28 => "A long default with Unicode 界 and a horizontally scrolling caret".into(),
            29 => "fixture-token-never-rendered".into(),
            30 => "12345678".into(),
            _ => String::new(),
        };
        self.line = LineInput::new(&self.line_text, if self.scenario == 30 { 8 } else { 8192 });
    }

    fn reset(&mut self) {
        self.reset_line();
        self.editors = std::array::from_fn(|_| Editor::default());
        self.focus =
            Focus::new(vec![0, 1]).unwrap_or_else(|| unreachable!("fixture IDs are unique"));
        let first = &mut self.editors[0];
        first.set_text("Edit this draft in the middle.\nEnter submits; Ctrl-J inserts a newline.");
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
            12..=15 => binding_fixture(first, self.scenario),
            16..=24 => edge_fixture(first, self.scenario),
            _ => "focused",
        };
        self.editors[1].set_text("Independent second draft");
        if matches!(self.scenario, 16 | 17) {
            self.focus.advance(false);
        }
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
        if matches!(event, Event::Key(key) if key.kind != KeyEventKind::Release && key.code == KeyCode::F(2))
        {
            self.scenario = (self.scenario + 1) % 32;
            self.reset();
            return true;
        }
        if self.scenario >= 25 {
            self.note = match self.line.update(&mut self.line_text, event) {
                InputAction::Submit => "Submitted · local simulation",
                InputAction::Back => "Back · local simulation",
                InputAction::Cancel => "Cancelled · local simulation",
                InputAction::Invalid => "Input refused · bounded single line required",
                InputAction::None => "Enter submits · Esc back · Ctrl-C cancels · F2 next fixture",
            }
            .into();
            return true;
        }
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
                KeyCode::Enter if key.modifiers.is_empty() => {
                    if key.kind == KeyEventKind::Press
                        && let Some(index) = self.focus.active().copied()
                    {
                        self.note = format!(
                            "Submitted {} bytes · local simulation",
                            self.editors[index].expanded_text().len()
                        );
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
        if self.scenario >= 25 {
            let [field, note] =
                Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);
            let field = if self.scenario == 28 {
                Rect::new(field.x, field.y, field.width.min(22), field.height)
            } else {
                field
            };
            self.line.render(
                frame,
                field,
                &self.line_text,
                "Single-line prompt",
                self.scenario == 29,
                theme,
            );
            frame.render_widget(
                Paragraph::new(if self.scenario == 31 {
                    "Cancelled · no action executed"
                } else {
                    &self.note
                })
                .style(theme.muted()),
                note,
            );
            return;
        }
        let [first, second, hints] = Layout::vertical([
            Constraint::Percentage(60),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .areas(area);
        let first = if matches!(self.scenario, 18..=20) {
            Rect::new(
                first.x,
                first.y,
                first.width,
                first
                    .height
                    .min(u16::try_from(21 - self.scenario).unwrap_or(1)),
            )
        } else {
            first
        };
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

fn edge_fixture(editor: &mut Editor, scenario: usize) -> &'static str {
    match scenario {
        16 => {
            editor.set_text(&"Blurred draft retains its scroll position.\n".repeat(24));
            // Seed an actual rendered viewport before transferring focus, using
            // an in-memory terminal rather than acquiring the user's terminal.
            let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 8))
                .unwrap_or_else(|_| unreachable!("in-memory fixture backend"));
            terminal
                .draw(|frame| editor.render_with(frame, frame.area(), EditorView::default(), true))
                .unwrap_or_else(|_| unreachable!("in-memory fixture render"));
            "blurred-scrolled"
        }
        17 => {
            *editor = Editor::new(12);
            editor.set_text("Full draft!!");
            editor.insert("x");
            "blurred-limit"
        }
        18..=20 => {
            editor.set_text("First row\nSecond row\nThird row\nFourth row");
            "tiny · draft preserved"
        }
        21 => {
            *editor = Editor::new(3);
            editor.set_text("ab");
            editor.insert_atom(crate::editor::InlineAtom {
                kind: crate::editor::AtomKind::Paste,
                label: "[paste]".into(),
                content: "界".into(),
                reference: None,
            });
            "paste-refused-whole"
        }
        22 => {
            editor.set_text("");
            for _ in 0..=256 {
                editor.insert_atom(crate::editor::InlineAtom {
                    kind: crate::editor::AtomKind::Paste,
                    label: "[chip]".into(),
                    content: "fixture".into(),
                    reference: None,
                });
            }
            "chip-cap · 256 retained"
        }
        23 => {
            editor.set_text("Controls stay display data: \u{85} \u{1b} \u{7f}");
            "control-characters"
        }
        _ => {
            *editor = Editor::new(7);
            editor.insert_typed("抹茶ラテ");
            "typed-cut · IME commit"
        }
    }
}
