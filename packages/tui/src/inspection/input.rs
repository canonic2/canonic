//! Read-only navigation, isolated fields, literal search and host effect requests.

use super::{Field, Inspector, Kind, Operation, Origin, State};
use crate::{
    editor,
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn update(v: &mut Inspector, event: &Event) -> bool {
    if !v.active || !v.focused {
        return false;
    }
    let Event::Key(k) = event else {
        if let Event::Paste(text) = event {
            if v.field.is_some() && !text.contains(['\n', '\r']) {
                v.editor.update(event);
                v.field_changed();
            }
            return true;
        }
        return false;
    };
    if k.kind == KeyEventKind::Release {
        return true;
    }
    if v.field.is_some() {
        return field(v, event);
    }
    if k.modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return false;
    }
    let deliberate = k.kind == KeyEventKind::Press && k.modifiers == KeyModifiers::NONE;
    if k.code == KeyCode::Esc && deliberate {
        if v.anchor.take().is_some() {
            v.notice.clear();
        } else if !v.query.is_empty() {
            v.query.clear();
            v.search();
        } else {
            v.close();
        }
        return true;
    }
    if k.code == KeyCode::Tab && deliberate && v.item.origin == Origin::Tree {
        v.close();
        return true;
    }
    if matches!(v.state, State::Loading(_)) {
        return true;
    }
    if let State::Failed(_) = v.state {
        if k.code == KeyCode::Char('r') && deliberate {
            v.request(Operation::Load);
        }
        return true;
    }
    if k.code == KeyCode::Char('e') && deliberate && v.item.editor {
        v.request(Operation::Editor { line: v.line() });
        return true;
    }
    if let State::Unavailable { preview, .. } = &v.state {
        if k.code == KeyCode::Enter
            && deliberate
            && let Some(content) = preview.clone()
            && let Ok(loaded) = super::model::parse(&content)
        {
            v.loaded = loaded;
            v.state = State::Ready(content);
            v.partial = true;
            v.notice = "partial content · supplied preview only".into();
        }
        return true;
    }
    navigate(v, k.code, deliberate)
}

fn navigate(v: &mut Inspector, code: KeyCode, deliberate: bool) -> bool {
    match code {
        KeyCode::Up => v.move_by(-1),
        KeyCode::Down => v.move_by(1),
        KeyCode::Home => v.jump(0),
        KeyCode::End => v.jump(v.rows().len().saturating_sub(1)),
        KeyCode::PageDown => v.page(false),
        KeyCode::PageUp => v.page(true),
        KeyCode::Left => v.horizontal = v.horizontal.saturating_sub(8),
        KeyCode::Right => v.horizontal = (v.horizontal + 8).min(262_144),
        KeyCode::Char(']' | '[') => v.hunk(code == KeyCode::Char('[')),
        KeyCode::Char('n' | 'N') => v.next_match(code == KeyCode::Char('N')),
        KeyCode::Char('/' | ':') if deliberate => {
            v.field = Some(if code == KeyCode::Char('/') {
                Field::Find
            } else {
                Field::Line
            });
            v.editor.set_text(if v.field == Some(Field::Find) {
                &v.query
            } else {
                ""
            });
        }
        KeyCode::Char('w') if deliberate => {
            v.wrap = !v.wrap;
            v.horizontal = 0;
            v.top_part = 0;
        }
        KeyCode::Char('v') if deliberate => {
            if !v.rows().is_empty() {
                v.anchor = Some(v.cursor);
            }
        }
        KeyCode::Char('y' | 'c') if deliberate => v.copy(code == KeyCode::Char('y')),
        KeyCode::Enter if deliberate => {
            if let Some((n, _)) = v.fold(v.cursor) {
                v.expanded.insert(n);
            }
        }
        KeyCode::Tab if deliberate => v.toggle_file(),
        _ => return false,
    }
    true
}

fn field(v: &mut Inspector, event: &Event) -> bool {
    let Event::Key(k) = event else {
        return true;
    };
    if matches!(k.code, KeyCode::Enter | KeyCode::Esc) {
        if k.kind != KeyEventKind::Press || k.modifiers != KeyModifiers::NONE {
            return true;
        }
        if k.code == KeyCode::Enter {
            if v.field == Some(Field::Line) {
                if let Ok(n) = v.editor.text().parse::<usize>() {
                    v.goto(n.max(1));
                } else {
                    v.notice = "enter a positive line number".into();
                    return true;
                }
            } else if let Some(&(row, _)) = v.matches.first() {
                v.jump(row);
                v.current_match = Some(0);
            }
        } else if v.field == Some(Field::Find) {
            v.query.clear();
            v.search();
        }
        v.field = None;
        return true;
    }
    if matches!(v.editor.update(event), editor::Outcome::Changed) {
        v.field_changed();
    }
    true
}
impl Inspector {
    fn field_changed(&mut self) {
        if self.field == Some(Field::Find) {
            self.query = self.editor.text().into();
            self.search();
        }
    }
    pub(super) fn search(&mut self) {
        self.matches.clear();
        self.current_match = None;
        if self.notice.starts_with("find limited") {
            self.notice.clear();
        }
        if self.query.is_empty() {
            return;
        }
        let sensitive = self.query.chars().any(char::is_uppercase);
        let query = if sensitive {
            self.query.clone()
        } else {
            self.query.to_lowercase()
        };
        let mut matches = vec![];
        for (i, row) in self.rows().iter().enumerate() {
            let (text, map) = folded(&row.text, sensitive);
            for (start, _) in text.match_indices(&query) {
                let end = start + query.len();
                let first = map.get(map.partition_point(|(range, _)| range.end <= start));
                let last = map.get(
                    map.partition_point(|(range, _)| range.start < end)
                        .saturating_sub(1),
                );
                if let (Some((_, a)), Some((_, b))) = (first, last) {
                    matches.push((i, a.start..b.end));
                    if matches.len() == 65_536 {
                        self.notice = "find limited to 65,536 matches · refine query".into();
                        self.matches = matches;
                        return;
                    }
                }
            }
        }
        self.matches = matches;
    }
    pub(crate) fn goto(&mut self, n: usize) {
        let new_side = self.rows().iter().any(|r| r.new.is_some());
        let rows = self.rows();
        let index = rows
            .iter()
            .position(|r| {
                if new_side {
                    r.new.is_some_and(|line| line >= n)
                } else {
                    r.old.is_some_and(|line| line >= n)
                }
            })
            .or_else(|| rows.iter().rposition(|r| r.new.or(r.old).is_some()));
        if let Some(i) = index {
            self.jump(i);
        } else {
            self.notice = "no source line available".into();
        }
    }
    fn jump(&mut self, index: usize) {
        self.cursor = index.min(self.rows().len().saturating_sub(1));
        self.reveal(self.cursor);
        self.top = self.cursor;
        self.top_part = 0;
    }
    fn move_by(&mut self, delta: isize) {
        let projection = self.projection();
        let i = projection
            .iter()
            .position(|r| *r == self.cursor)
            .unwrap_or(0);
        let target = i
            .saturating_add_signed(delta)
            .min(projection.len().saturating_sub(1));
        if let Some(&row) = projection.get(target) {
            self.cursor = row;
        }
    }
    fn page(&mut self, back: bool) {
        let step = self.page_rows.saturating_sub(2).max(1);
        let target = if back {
            super::view::previous_page(self, step)
        } else {
            self.visible
                .get(step)
                .or_else(|| self.visible.last())
                .copied()
                .unwrap_or((self.cursor, 0))
        };
        self.cursor = target.0;
        self.top = target.0;
        self.top_part = target.1;
    }
    fn hunk(&mut self, back: bool) {
        let headers: Vec<_> = if self.whole {
            let mut seen = None;
            self.rows()
                .iter()
                .enumerate()
                .filter_map(|(i, r)| {
                    if r.hunk.is_some() && r.hunk != seen {
                        seen = r.hunk;
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            self.loaded.hunks.clone()
        };
        let target = if back {
            headers.iter().rev().find(|i| **i < self.cursor)
        } else {
            headers.iter().find(|i| **i > self.cursor)
        };
        if let Some(&i) = target {
            self.jump(i);
        } else {
            self.notice = if back { "first hunk" } else { "last hunk" }.into();
        }
    }
    fn next_match(&mut self, back: bool) {
        if self.matches.is_empty() {
            self.notice = "no match".into();
            return;
        }
        let next = self.current_match.map_or_else(
            || {
                if back {
                    self.matches
                        .iter()
                        .rposition(|(r, _)| *r <= self.cursor)
                        .unwrap_or(self.matches.len() - 1)
                } else {
                    self.matches
                        .iter()
                        .position(|(r, _)| *r >= self.cursor)
                        .unwrap_or(0)
                }
            },
            |i| {
                if back {
                    (i + self.matches.len() - 1) % self.matches.len()
                } else {
                    (i + 1) % self.matches.len()
                }
            },
        );
        self.current_match = Some(next);
        self.jump(self.matches[next].0);
    }
    fn toggle_file(&mut self) {
        if !self.loaded.is_diff {
            return;
        }
        if self.loaded.file.is_none() {
            self.notice = "whole new file not supplied by host".into();
            return;
        }
        let line = self.line().unwrap_or(1);
        self.whole = !self.whole;
        self.anchor = None;
        self.cursor = 0;
        self.top = 0;
        self.top_part = 0;
        self.goto(line);
        self.search();
    }
    fn copy(&mut self, as_patch: bool) {
        let Some(selection) = self.selection() else {
            self.notice = "v selects lines to copy".into();
            return;
        };
        let patch = as_patch && self.loaded.is_diff && !self.whole;
        let mut text = String::new();
        let mut lines = 0;
        for row in &self.rows()[selection] {
            if matches!(row.kind, Kind::Removed | Kind::Header | Kind::Meta) && !patch {
                continue;
            }
            if patch {
                text.push_str(match row.kind {
                    Kind::Added => "+",
                    Kind::Removed => "-",
                    Kind::Context => " ",
                    _ => "",
                });
            }
            text.push_str(&row.text);
            text.push('\n');
            lines += 1;
        }
        if lines == 0 {
            self.notice = "selection has no new code".into();
            return;
        }
        self.request(Operation::Copy { text, lines, patch });
    }
}
type FoldMap = Vec<(Range<usize>, Range<usize>)>;
fn folded(text: &str, sensitive: bool) -> (String, FoldMap) {
    let mut output = String::new();
    let mut map = vec![];
    for (i, g) in text.grapheme_indices(true) {
        let start = output.len();
        if sensitive {
            output.push_str(g);
        } else {
            output.push_str(&g.to_lowercase());
        }
        map.push((start..output.len(), i..i + g.len()));
    }
    (output, map)
}
