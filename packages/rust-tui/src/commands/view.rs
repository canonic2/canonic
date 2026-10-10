//! Owns terminal-cell columns, command chrome, search emphasis and help details.

use super::Commands;
use crate::{editor::EditorView, picker::PickerView, theme::Presentation, ui::Rule};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) enum Row {
    Group(String),
    Command(usize),
    Detail(String),
}

pub(super) fn matches(text: &str, query: &str) -> bool {
    let lower = text.to_lowercase();
    let mut chars = lower.chars();
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .all(|q| chars.by_ref().any(|c| c == q))
}
fn clip(text: &str, budget: usize, p: Presentation) -> String {
    if text.width() <= budget {
        return text.into();
    }
    if budget == 0 {
        return String::new();
    }
    let mut s = String::new();
    for g in text.graphemes(true) {
        if s.width() + g.width() >= budget {
            break;
        }
        s.push_str(g);
    }
    s.push(p.symbols.ellipsis());
    s
}
fn highlighted(text: &str, query: &str, style: Style) -> Vec<Span<'static>> {
    let lower = query.to_lowercase();
    let mut query = lower.chars().filter(|c| !c.is_whitespace()).peekable();
    text.graphemes(true)
        .map(|g| {
            let selected = query.peek().is_some_and(|c| g.to_lowercase().contains(*c));
            if selected {
                query.next();
            }
            Span::styled(
                g.to_owned(),
                if selected {
                    style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else {
                    style
                },
            )
        })
        .collect()
}
impl Commands {
    pub(super) fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut previous = String::new();
        for &i in &self.matches {
            let group = self.group(i);
            if self.query().is_empty() && group != previous && !group.is_empty() {
                rows.push(Row::Group(group.into()));
                previous = group.into();
            }
            rows.push(Row::Command(i));
            if self.keys
                && self.selected == Some(i)
                && let Some(info) = self.metadata.get(&self.entries[i].id)
            {
                rows.push(Row::Detail(format!(
                    "default {} · {}:{}",
                    info.defaults
                        .iter()
                        .map(super::Binding::label)
                        .collect::<Vec<_>>()
                        .join(" / "),
                    crate::text::clean(&info.origin.path, 512, false),
                    info.origin.line
                )));
                for note in info.conflicts.iter().take(2) {
                    rows.push(Row::Detail(crate::text::clean(note, 1024, false)));
                }
            }
        }
        rows
    }
    pub(super) fn draw(&mut self, frame: &mut Frame<'_>, area: Rect, view: PickerView<'_>) -> Rect {
        let rows = self.rows();
        if !self.keys {
            self.opened_height
                .get_or_insert(u16::try_from(rows.len().max(1) + 3).unwrap_or(u16::MAX));
        }
        let used = Rect::new(
            area.x,
            area.y,
            area.width,
            self.height(area.width, view).min(area.height),
        );
        if used.is_empty() {
            self.viewport = 0;
            return used;
        }
        let p = view.presentation;
        frame.render_widget(ratatui::widgets::Clear, used);
        frame.render_widget(Block::default().style(p.base()), used);
        self.viewport = usize::from(used.height.saturating_sub(3));
        if let Some(at) = rows
            .iter()
            .position(|r| matches!(r,Row::Command(i) if Some(*i)==self.selected))
        {
            if at < self.offset {
                self.offset = at;
            }
            if at >= self.offset + self.viewport.max(1) {
                self.offset = at + 1 - self.viewport.max(1);
            }
        }
        self.offset = self
            .offset
            .min(rows.len().saturating_sub(self.viewport.max(1)));
        self.chrome(frame, used, p, &rows);
        if used.height < 3 {
            return used;
        }
        let filter = Rect::new(used.x, used.y + 1, used.width, 1);
        let prefix = filter.width.min(7);
        frame.render_widget(
            Paragraph::new("filter ").style(p.muted()),
            Rect::new(filter.x, filter.y, prefix, 1),
        );
        self.query.render_with(
            frame,
            Rect::new(filter.x + prefix, filter.y, filter.width - prefix, 1),
            EditorView {
                chrome: false,
                placeholder: "type a command or a key",
                presentation: p,
                ..EditorView::default()
            },
            true,
        );
        let body = Rect::new(used.x, used.y + 2, used.width, used.height - 3);
        if rows.is_empty() && body.height > 0 {
            let text = if self.entries.is_empty() {
                "No commands available.".into()
            } else {
                format!("No command matches \"{}\".", self.query())
            };
            frame.render_widget(Paragraph::new(text).style(p.muted()), body);
        }
        for (line, row) in rows
            .iter()
            .skip(self.offset)
            .take(self.viewport)
            .enumerate()
        {
            let rect = Rect::new(
                body.x,
                body.y + u16::try_from(line).unwrap_or(0),
                body.width,
                1,
            );
            match row {
                Row::Group(s) => {
                    frame.render_widget(Paragraph::new(s.as_str()).style(p.muted()), rect);
                }
                Row::Detail(s) => {
                    frame.render_widget(Paragraph::new(format!("  {s}")).style(p.muted()), rect);
                }
                Row::Command(i) => self.command_row(frame, rect, *i, p),
            }
        }
        used
    }
    fn chrome(&self, frame: &mut Frame<'_>, area: Rect, p: Presentation, rows: &[Row]) {
        let count = if self.query().is_empty() && self.scope.is_none() {
            self.entries.len().to_string()
        } else {
            format!("{} of {}", self.matches.len(), self.entries.len())
        };
        let custom = self.metadata.values().filter(|m| m.custom).count();
        let conflicts: std::collections::HashSet<_> = self
            .metadata
            .values()
            .flat_map(|m| {
                m.conflicts
                    .iter()
                    .map(|s| s.strip_prefix("ignored · ").unwrap_or(s))
            })
            .collect();
        let count = if self.keys && self.query().is_empty() {
            format!(
                "{count}{}{}",
                if custom > 0 {
                    format!(" · {custom} custom")
                } else {
                    String::new()
                },
                if conflicts.is_empty() {
                    String::new()
                } else {
                    format!(" · {} conflict", conflicts.len())
                }
            )
        } else {
            count
        };
        frame.render_widget(
            Rule {
                left: Line::from(Span::styled(
                    if self.keys { "keys" } else { "commands" },
                    p.heading(),
                )),
                right: Line::from(Span::styled(count, p.muted())),
                glyph: p.symbols.rule(),
                style: p.accent(),
            },
            Rect::new(area.x, area.y, area.width, 1),
        );
        if area.height < 2 {
            return;
        }
        let mut hints = Vec::new();
        let below = rows
            .iter()
            .skip(self.offset + self.viewport)
            .filter(|r| matches!(r, Row::Command(_)))
            .count();
        if below > 0 {
            hints.push(format!("{} {below} more", p.symbols.more(false)));
        }
        if self.keys {
            if self
                .selected()
                .is_some_and(|id| self.metadata.contains_key(id))
            {
                hints.push("o open file".into());
            }
            hints.push("tab scope".into());
        } else if self.selected.is_some() {
            hints.push(
                if area.width < 64 {
                    "enter"
                } else {
                    "enter run"
                }
                .into(),
            );
        }
        hints.push(
            if self.query().is_empty() {
                if area.width < 64 { "esc" } else { "esc close" }
            } else {
                "esc clears"
            }
            .into(),
        );
        frame.render_widget(
            Rule {
                left: Line::default(),
                right: Line::from(Span::styled(hints.join(p.symbols.separator()), p.muted())),
                glyph: p.symbols.rule(),
                style: p.accent(),
            },
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }
    fn command_row(&self, frame: &mut Frame<'_>, area: Rect, i: usize, p: Presentation) {
        let c = &self.entries[i];
        let active = self.selected == Some(i);
        if active {
            frame.render_widget(
                Block::default().style(p.surface()),
                Rect::new(
                    area.x.saturating_add(1),
                    area.y,
                    area.width.saturating_sub(2),
                    1,
                ),
            );
        }
        let style = if c.disabled.is_some() {
            p.muted()
        } else {
            p.base().fg(Color::Reset).remove_modifier(Modifier::DIM)
        };
        let style = if active {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        };
        let key = c
            .bindings
            .first()
            .map_or(String::new(), super::Binding::label);
        let available = usize::from(area.width.saturating_sub(3));
        let key_width = self
            .matches
            .iter()
            .map(|i| {
                self.entries[*i]
                    .bindings
                    .first()
                    .map_or(0, |b| b.label().width())
            })
            .max()
            .unwrap_or(0)
            .min(available / 2);
        let label_width = self
            .matches
            .iter()
            .map(|i| self.entries[*i].label.width())
            .max()
            .unwrap_or(0)
            .min(available.saturating_sub(key_width + 2));
        let mut spans = vec![Span::styled(" ", p.base().bg(Color::Reset))];
        if self.keys {
            self.keys_row(frame, area, i, p, style);
            return;
        }
        let label = clip(&c.label, label_width, p);
        spans.push(Span::raw(" "));
        spans.extend(highlighted(&label, self.query(), style));
        let details = available.saturating_sub(label_width + key_width + 4);
        spans.push(Span::raw(
            " ".repeat(label_width.saturating_sub(label.width()) + 2),
        ));
        if details >= 12 {
            spans.extend(highlighted(
                &clip(c.disabled.as_deref().unwrap_or(&c.description), details, p),
                self.query(),
                p.muted(),
            ));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(
                area.x,
                area.y,
                area.width
                    .saturating_sub(u16::try_from(key_width + 2).unwrap_or(0)),
                1,
            ),
        );
        if key_width > 0 {
            let key = clip(&key, key_width, p);
            let w = u16::try_from(key.width()).unwrap_or(0);
            frame.render_widget(
                Paragraph::new(Line::from(highlighted(&key, self.query(), style))),
                Rect::new(area.right().saturating_sub(w + 1), area.y, w, 1),
            );
        }
    }
    fn help_key(&self, i: usize) -> String {
        let c = &self.entries[i];
        c.bindings
            .first()
            .or_else(|| self.metadata.get(&c.id).and_then(|m| m.ignored.first()))
            .map_or(String::new(), super::Binding::label)
    }
    fn keys_row(&self, frame: &mut Frame<'_>, area: Rect, i: usize, p: Presentation, style: Style) {
        let c = &self.entries[i];
        let info = self.metadata.get(&c.id);
        let conflict = info.is_some_and(|m| !m.conflicts.is_empty());
        let status = if conflict {
            if c.bindings.is_empty() {
                "ignored"
            } else {
                "wins"
            }
        } else if info.is_some_and(|m| m.custom) {
            "custom"
        } else {
            ""
        };
        let key = self.help_key(i);
        let available = usize::from(area.width.saturating_sub(3));
        let key_width = self
            .matches
            .iter()
            .map(|i| self.help_key(*i).width())
            .max()
            .unwrap_or(0)
            .min(available / 2);
        let key_style = if conflict {
            p.warning().add_modifier(if self.selected == Some(i) {
                Modifier::BOLD
            } else {
                Modifier::empty()
            })
        } else {
            style
        };
        let mut spans = vec![Span::styled(" ", p.base().bg(Color::Reset)), Span::raw(" ")];
        let left = clip(&key, key_width, p);
        spans.extend(highlighted(&left, self.query(), key_style));
        spans.push(Span::raw(
            " ".repeat(key_width.saturating_sub(left.width()) + 2),
        ));
        spans.extend(highlighted(
            &clip(
                &c.label,
                available.saturating_sub(
                    key_width + 2 + status.len() + usize::from(!status.is_empty()) * 2,
                ),
                p,
            ),
            self.query(),
            style,
        ));
        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(area.x, area.y, area.width.saturating_sub(1), 1),
        );
        if !status.is_empty() && usize::from(area.width) > status.len() + 1 {
            let width = u16::try_from(status.len()).unwrap_or(0);
            frame.render_widget(
                Paragraph::new(status).style(if conflict { p.warning() } else { p.muted() }),
                Rect::new(area.right() - width - 1, area.y, width, 1),
            );
        }
    }
}
