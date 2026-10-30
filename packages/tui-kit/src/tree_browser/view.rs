//! Responsive tree projection, semantic status and full selected-row inspection.

use super::{Cursor, Kind, Node, Operation, Placement, Row, State, TreeBrowser};
use crate::{
    editor::EditorView,
    picker::PickerView,
    text::{clean, elide, wrapped},
    theme::{ColorMode, Presentation, Symbols},
    ui::Rule,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use std::fmt::Write;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
fn cells(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}
fn symbol(p: Presentation, unicode: &'static str, ascii: &'static str) -> &'static str {
    if p.symbols == Symbols::Ascii {
        ascii
    } else {
        unicode
    }
}
fn middle(text: &str, width: u16, p: Presentation) -> String {
    let text = clean(text, 65536, false);
    let limit = usize::from(width);
    if text.width() <= limit {
        return text;
    }
    let cut = symbol(p, "…", "...");
    if limit <= cut.width() {
        return elide(&text, width, p.symbols);
    }
    let remaining = limit - cut.width();
    let extension = text
        .rsplit_once('.')
        .map_or(0, |(_, suffix)| suffix.width() + 1);
    let right = (remaining / 2).max(if extension <= remaining { extension } else { 0 });
    let left = remaining - right;
    let mut prefix = String::new();
    for g in text.graphemes(true) {
        if prefix.width() + g.width() > left {
            break;
        }
        prefix.push_str(g);
    }
    let mut suffix = String::new();
    for g in text.graphemes(true).rev() {
        if suffix.width() + g.width() > right {
            break;
        }
        suffix.insert_str(0, g);
    }
    format!("{prefix}{cut}{suffix}")
}
fn mark(state: State, p: Presentation) -> &'static str {
    match state {
        State::Plain => "",
        State::Modified => "M",
        State::Added => "A",
        State::Deleted => "D",
        State::Renamed => "R",
        State::Untracked => "?",
        State::Ignored | State::Interrupted => "!",
        State::Working => symbol(p, "···", "..."),
        State::Done => symbol(p, "✓", "ok"),
        State::Failed => symbol(p, "✕", "x"),
        State::Queued => symbol(p, "○", "o"),
        State::Cancelled => "-",
    }
}
fn guides(depth: usize, p: Presentation) -> String {
    let mut prefix = if depth >= 5 {
        format!("{} ", symbol(p, "⋯", "..."))
    } else {
        String::new()
    };
    prefix.push_str(&symbol(p, "│ ", "| ").repeat(depth % 5));
    prefix
}
fn state_style(state: State, p: Presentation) -> Style {
    match state {
        State::Added | State::Done => p
            .tokens()
            .success
            .map_or_else(|| p.base(), |color| p.base().fg(color)),
        State::Modified | State::Renamed => p.warning(),
        State::Deleted | State::Failed | State::Interrupted => p.error(),
        State::Working => p.accent(),
        _ => p.muted(),
    }
}
fn rule(frame: &mut Frame<'_>, area: Rect, left: &str, right: &str, p: Presentation) {
    let right = if area.width < 45 {
        String::new()
    } else {
        elide(
            &clean(right, 65536, false),
            area.width.saturating_sub(12),
            p.symbols,
        )
    };
    frame.render_widget(
        Rule {
            left: Line::styled(
                elide(
                    &clean(left, 4096, false),
                    area.width
                        .saturating_sub(cells(right.width() + usize::from(!right.is_empty()))),
                    p.symbols,
                ),
                p.accent(),
            ),
            right: Line::styled(right, p.muted()),
            glyph: if p.symbols == Symbols::Ascii {
                '-'
            } else {
                '─'
            },
            style: p.accent(),
        },
        area,
    );
}
impl TreeBrowser {
    /// Effective shortcuts for the active scope; disabled actions are omitted.
    #[must_use]
    pub fn hint(&self) -> String {
        if !self.focused {
            return "main view focused".into();
        }
        if self.menu.is_some() {
            return "enter choose · esc close menu".into();
        }
        if self.filtering {
            return "type filter · ↑↓ move · esc clear".into();
        }
        let mut hints = vec![];
        match &self.cursor {
            Some(Cursor::Children(id)) if !self.pending.contains_key(id) => {
                hints.push("enter load/retry".to_owned());
            }
            Some(Cursor::Node(id)) => {
                if let Some(n) = self.node(id) {
                    if !self.pending.contains_key(&Some(id.clone())) {
                        if let Some(a) = n
                            .actions
                            .iter()
                            .find(|a| n.primary.as_ref() == Some(&a.id) && a.disabled.is_none())
                        {
                            hints.push(format!("enter {}", a.label));
                        }
                        if !n.actions.is_empty() {
                            hints.push("a actions".into());
                        }
                    }
                    if n.has_children {
                        hints.push("←→ fold".into());
                    } else if n.parent.is_some() {
                        hints.push("← parent".into());
                    }
                }
            }
            _ => {}
        }
        hints.push("/ filter".into());
        if self.placement == Placement::Panel {
            hints.push("tab main view".into());
        }
        hints.push("esc back".into());
        hints.join(" · ")
    }
    fn details(&self) -> String {
        match &self.cursor {
            Some(Cursor::Node(id)) => self.node(id).map_or_else(String::new, |n| {
                let mut text = format!("{}\n{}", n.name, n.detail);
                if let Some(s) = self.failures.get(&Some(id.clone())) {
                    let _ = write!(text, "\nfailed: {s}");
                }
                for a in &n.actions {
                    if let Some(s) = &a.disabled {
                        let _ = write!(text, "\n{} unavailable: {s}", a.label);
                    }
                }
                clean(&text, 8 * 1024 * 1024, true)
            }),
            Some(Cursor::Children(id)) => self
                .load_failures
                .get(id)
                .cloned()
                .or_else(|| {
                    let state = id
                        .as_deref()
                        .and_then(|id| self.node(id))
                        .map_or(&self.snapshot.roots, |n| &n.children);
                    if let super::Children::Failed(s) = state {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map_or_else(String::new, |s| clean(&s, 65536, true)),
            None => String::new(),
        }
    }
    fn node_status(&self, n: &Node, width: u16, p: Presentation) -> (String, Style) {
        if let Some(pending) = self.pending.get(&Some(n.id.clone()))
            && let Operation::Action(action) = &pending.request.operation
            && let Some(a) = n.actions.iter().find(|a| a.id == *action)
        {
            return (
                format!(
                    "{} {} {} {}s",
                    symbol(p, "···", "..."),
                    a.pending,
                    symbol(p, "·", "|"),
                    pending.elapsed.as_secs()
                ),
                p.muted(),
            );
        }
        if let Some(reason) = n
            .primary
            .as_ref()
            .and_then(|id| n.actions.iter().find(|a| a.id == *id))
            .and_then(|a| a.disabled.as_ref())
        {
            return (reason.clone(), p.disabled());
        }
        let status = if n.kind == Kind::Task {
            n.detail.clone()
        } else if width < 34 {
            mark(n.state, p).into()
        } else {
            format!("{} {}", mark(n.state, p), n.detail).trim().into()
        };
        (status, state_style(n.state, p))
    }
    fn row_line(&self, row: &Row, width: u16, p: Presentation) -> Line<'static> {
        let selected = row.key.is_some() && row.key == self.cursor;
        let style = if selected && self.focused {
            p.surface()
        } else if selected {
            p.muted().add_modifier(Modifier::UNDERLINED)
        } else {
            p.base()
        };
        let mut prefix = guides(row.depth, p);
        let mut name = if p.symbols == Symbols::Ascii {
            row.text.replace('·', "|")
        } else {
            row.text.clone()
        };
        let mut status = String::new();
        let mut status_style = p.muted();
        let mut name_style = p.base();
        if let Some(Cursor::Node(id)) = &row.key {
            if let Some(n) = self.node(id) {
                prefix.push_str(if n.has_children {
                    if self.folded.contains(id) || !self.query.text().is_empty() {
                        symbol(p, "▾ ", "v ")
                    } else {
                        symbol(p, "▸ ", "> ")
                    }
                } else {
                    "  "
                });
                if n.kind == Kind::Task {
                    prefix.push_str(mark(n.state, p));
                    prefix.push(' ');
                }
                name.clone_from(&n.name);
                if n.kind == Kind::Directory && !name.ends_with('/') {
                    name.push('/');
                }
                (status, status_style) = self.node_status(n, width, p);
                if self.opened.as_ref() == Some(id) {
                    name_style = if p.colors == ColorMode::NoColor {
                        p.base().add_modifier(Modifier::UNDERLINED)
                    } else {
                        p.accent()
                    };
                }
                if n.primary
                    .as_ref()
                    .and_then(|id| n.actions.iter().find(|a| a.id == *id))
                    .is_some_and(|a| a.disabled.is_some())
                {
                    name_style = p.disabled();
                }
                if !self.query.text().is_empty()
                    && n.name
                        .to_lowercase()
                        .contains(&self.query.text().to_lowercase())
                {
                    name_style = name_style.add_modifier(Modifier::UNDERLINED);
                }
            }
        } else if !row.text.is_empty() {
            prefix.push_str(symbol(p, "· ", ". "));
        }
        if selected && !self.focused {
            prefix = format!(">{prefix}");
        }
        let available = width.saturating_sub(cells(prefix.width()));
        let status_width = if available >= 18 {
            cells(status.width()).min(available / 2)
        } else if status.is_empty() {
            0
        } else {
            cells(status.width()).min(available / 3)
        };
        let name_width = available.saturating_sub(status_width + u16::from(status_width > 0));
        let name = middle(&name, name_width, p);
        let gap = usize::from(name_width).saturating_sub(name.width());
        let status = elide(&clean(&status, 65536, false), status_width, p.symbols);
        Line::from(vec![
            Span::styled(prefix, p.muted()),
            Span::styled(name, name_style),
            Span::raw(" ".repeat(gap + usize::from(status_width > 0))),
            Span::styled(status, status_style),
        ])
        .style(style)
    }
    /// Render the view in a caller-owned frame without discovering or opening data.
    #[expect(
        clippy::too_many_lines,
        reason = "One geometry pass allocates tree rows, selected details and the scoped menu in the caller's rectangle"
    )]
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if area.is_empty() {
            return;
        }
        frame.render_widget(Block::default().style(p.base()), area);
        if area.height < 3 {
            frame.render_widget(
                Paragraph::new(elide(
                    &clean(&self.snapshot.title, 4096, false),
                    area.width,
                    p.symbols,
                ))
                .style(p.accent()),
                area,
            );
            return;
        }
        rule(
            frame,
            Rect::new(area.x, area.y, area.width, 1),
            &self.snapshot.title,
            &self.snapshot.summary,
            p,
        );
        let mut y = area.y + 1;
        let mut height = area.height - 2;
        if self.filtering && height > 1 {
            self.query.render_with(
                frame,
                Rect::new(area.x, y, area.width, 1),
                EditorView {
                    chrome: false,
                    presentation: p,
                    placeholder: "filter",
                    ..EditorView::default()
                },
                self.focused,
            );
            y += 1;
            height -= 1;
        }
        let details = self.details();
        let inspect_height = if height >= 8 && !details.is_empty() {
            3
        } else {
            0
        };
        let feedback = if self.skipped > 0 {
            format!("! skipped {} invalid nodes", self.skipped)
        } else {
            self.notice.clone()
        };
        let feedback_height = u16::from(!feedback.is_empty() && height > 2);
        let list_height = height
            .saturating_sub(inspect_height + feedback_height)
            .max(1);
        self.viewport = usize::from(list_height);
        let rows = self.rows();
        let selected = self
            .cursor
            .as_ref()
            .and_then(|c| rows.iter().position(|r| r.key.as_ref() == Some(c)))
            .unwrap_or(0);
        let context = usize::from(list_height > 2);
        if selected < self.offset + context {
            self.offset = selected.saturating_sub(context);
        } else if selected + context >= self.offset + self.viewport {
            self.offset = (selected + context + 1).saturating_sub(self.viewport);
        }
        self.offset = self.offset.min(rows.len().saturating_sub(self.viewport));
        if rows.is_empty() {
            frame.render_widget(
                Paragraph::new("no matches").style(p.muted()),
                Rect::new(area.x, y, area.width, list_height),
            );
        }
        for (i, row) in rows
            .iter()
            .skip(self.offset)
            .take(self.viewport)
            .enumerate()
        {
            frame.render_widget(
                Paragraph::new(self.row_line(row, area.width, p)),
                Rect::new(area.x, y + cells(i), area.width, 1),
            );
        }
        y += list_height;
        if feedback_height > 0 {
            frame.render_widget(
                Paragraph::new(elide(&clean(&feedback, 4096, false), area.width, p.symbols))
                    .style(p.warning()),
                Rect::new(area.x, y, area.width, 1),
            );
            y += 1;
        }
        if inspect_height > 0 {
            let lines = wrapped(&details, area.width);
            self.details_offset = self
                .details_offset
                .min(lines.len().saturating_sub(usize::from(inspect_height)));
            let text = lines
                .into_iter()
                .skip(self.details_offset)
                .take(usize::from(inspect_height))
                .collect::<Vec<_>>()
                .join("\n");
            frame.render_widget(
                Paragraph::new(text).style(p.muted()),
                Rect::new(area.x, y, area.width, inspect_height),
            );
        }
        let hint = if area.width < 35 {
            if self.focused {
                "/ filter · esc back".into()
            } else {
                "main view focused".into()
            }
        } else {
            self.hint()
        };
        let hint = if p.symbols == Symbols::Ascii {
            hint.replace("↑↓", "up/down")
                .replace("←→", "left/right")
                .replace('←', "left")
                .replace('·', "|")
        } else {
            hint
        };
        let below = rows.len().saturating_sub(self.offset + self.viewport);
        let counts = if self.offset == 0 && below == 0 {
            String::new()
        } else {
            format!(
                "{}{} / {}{}",
                symbol(p, "↑", "up "),
                self.offset,
                symbol(p, "↓", "down "),
                below
            )
        };
        rule(
            frame,
            Rect::new(area.x, area.y + area.height - 1, area.width, 1),
            &hint,
            &counts,
            p,
        );
        if let Some(menu) = &mut self.menu {
            let width = area.width.min(60);
            let height = area.height.min(10);
            let rect = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height - height) / 2,
                width,
                height,
            );
            menu.picker.render(
                frame,
                rect,
                PickerView {
                    presentation: p,
                    ..PickerView::default()
                },
                true,
            );
        }
    }
}
