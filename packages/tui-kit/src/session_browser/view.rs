//! Dedicated responsive session rows, full confirmation wording and scoped fields.

use super::{DeleteCheck, Kind, Session, SessionBrowser, Status, Tone};
use crate::{
    editor::EditorView,
    picker::PickerView,
    text::{clean, elide, wrapped},
    theme::{Presentation, Symbols},
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
use unicode_width::UnicodeWidthStr;

fn count(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}
fn glyph(p: Presentation, unicode: &'static str, ascii: &'static str) -> &'static str {
    if p.symbols == Symbols::Ascii {
        ascii
    } else {
        unicode
    }
}
fn rule(frame: &mut Frame<'_>, area: Rect, left: &str, right: &str, p: Presentation) {
    frame.render_widget(
        Rule {
            left: Line::styled(
                elide(
                    left,
                    area.width.saturating_sub(count(right.width() + 1)),
                    p.symbols,
                ),
                p.accent(),
            ),
            right: Line::styled(
                elide(right, area.width.saturating_sub(10), p.symbols),
                p.muted(),
            ),
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
fn cell(text: &str, width: u16, style: Style, p: Presentation) -> Span<'static> {
    let text = elide(
        &clean(text, 65536, false),
        width.saturating_sub(1),
        p.symbols,
    );
    let pad = usize::from(width).saturating_sub(text.width());
    Span::styled(format!("{text}{}", " ".repeat(pad)), style)
}
impl SessionBrowser {
    /// Effective footer, showing only supplied enabled row actions.
    #[must_use]
    pub fn hint(&self, width: u16) -> String {
        if width < 25 {
            return "enter · esc".into();
        }
        let sep = " · ";
        if self.confirming() {
            return "enter keep/delete · esc keep · pgdn text".into();
        }
        if let Some(rename) = &self.rename {
            let limit = self.row(&rename.id).map_or(0, |r| r.rename_limit);
            let n = rename.editor.text().chars().count();
            if rename.editor.text().trim().is_empty() || n > limit {
                return format!("esc cancel · {n} of {limit} · title required/over limit");
            }
            return format!(
                "enter save · esc cancel · {} of {limit}",
                rename.editor.text().chars().count()
            );
        }
        if self.menu.is_some() {
            return "enter choose · esc close menu".into();
        }
        if self.search_row {
            return if self.search.is_some() {
                "↑↓ move · esc stop searching".into()
            } else if self.searching {
                "enter search all · esc leave search".into()
            } else {
                "enter search all · esc clear".into()
            };
        }
        if self.searching {
            return if self.search.is_some() || self.load.as_ref().is_some_and(|r| r.search) {
                "enter open · ↑↓ move · esc stop searching".into()
            } else {
                "enter open · ↑↓ move · esc leave search".into()
            };
        }
        let mut keys = vec![
            if self.selected.as_deref() == Some(&self.origin) {
                "enter back to it"
            } else {
                "enter open"
            }
            .to_owned(),
        ];
        if let Some(row) = self.selected().and_then(|id| self.row(id)) {
            if self.pending.contains_key(&row.id) {
                keys = vec!["↑↓ move".into()];
            } else {
                let open = row
                    .actions
                    .iter()
                    .any(|a| a.kind == Kind::Open && a.disabled.is_none());
                if !open {
                    keys.clear();
                }
                if width >= 50 {
                    keys.push("/ search".into());
                }
                if !row.actions.is_empty() {
                    keys.push("a actions".into());
                }
                if width >= 100 {
                    keys.extend(
                        row.actions
                            .iter()
                            .filter(|a| a.kind != Kind::Open && a.disabled.is_none())
                            .map(|a| format!("{} {}", a.kind.key(), clean(&a.label, 128, false))),
                    );
                }
            }
        } else if self.snapshot.rows.is_empty() {
            keys.clear();
        }
        if self.load_failure.is_some() || matches!(self.snapshot.status, Status::Failed(_)) {
            keys.push("r retry".into());
        }
        if self.snapshot.status == Status::Loading && self.snapshot.rows.is_empty() {
            keys.push("/ search".into());
        }
        let joined = keys.join(sep);
        let escape = if self.search.is_some() || self.load.as_ref().is_some_and(|r| r.search) {
            "esc stop searching"
        } else if !self.query.text().is_empty() {
            "esc clear"
        } else if width < 50 || joined.width() + 18 > usize::from(width) {
            "esc"
        } else {
            "esc back to chat"
        };
        let body = elide(
            &joined,
            width.saturating_sub(count(escape.width() + 3)),
            Symbols::Unicode,
        );
        if body.is_empty() {
            escape.into()
        } else {
            format!("{body}{sep}{escape}")
        }
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Three responsive row anatomies share exact title and metadata geometry"
    )]
    fn row_lines(&self, row: &Session, width: u16, p: Presentation) -> Vec<Line<'static>> {
        let selected = self.selected.as_deref() == Some(&row.id) && !self.search_row;
        let pending = self.pending.get(&row.id);
        let style = if pending.is_some() {
            p.disabled()
        } else if selected {
            p.selection()
        } else {
            p.base()
        };
        let state_style = match row.tone {
            Tone::Normal => p.base(),
            Tone::Working => p.accent(),
            Tone::Attention => p.warning(),
            Tone::Failed => p.error(),
            Tone::Done => p.tokens().success.map_or(p.base(), |c| p.base().fg(c)),
        };
        let title = pending
            .filter(|r| r.request.kind == Kind::Rename)
            .map_or(row.title.as_str(), |r| r.request.value.as_str());
        let marker = if row.id == self.origin {
            glyph(p, "●", "*")
        } else if row.parent.is_some() {
            glyph(p, "└", "`-")
        } else {
            " "
        };
        let prefix = if selected && p.symbols == Symbols::Ascii {
            format!(">{marker}")
        } else {
            format!(" {marker}")
        };
        let state = pending.map_or_else(
            || row.state.clone(),
            |r| {
                format!(
                    "{} {}{}",
                    glyph(p, "···", "..."),
                    r.request.kind.pending(),
                    if r.elapsed.as_secs() >= 3 {
                        format!(" · {}s", r.elapsed.as_secs())
                    } else {
                        String::new()
                    }
                )
            },
        );
        let age_width = 6.min(width / 4);
        let title_width = if width < 50 {
            width.saturating_sub(age_width + 4)
        } else if width < 100 {
            width.saturating_sub(32).max(8)
        } else {
            28.min(width / 3)
        };
        let mut first = vec![
            Span::styled(
                format!(
                    "{prefix}{}",
                    " ".repeat(3_usize.saturating_sub(prefix.width()))
                ),
                style,
            ),
            cell(
                title,
                title_width,
                style.add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
                p,
            ),
        ];
        if width < 50 {
            first.push(cell(&row.age, age_width, p.muted(), p));
            let second = format!(
                "   {} {}{}",
                row.project,
                row.reference,
                if state.is_empty() {
                    String::new()
                } else {
                    format!(" · {state}")
                }
            );
            return vec![
                Line::from(first).style(style),
                Line::styled(
                    elide(&clean(&second, 65536, false), width, p.symbols),
                    if selected { style } else { state_style },
                ),
            ];
        }
        let project_width = if width >= 100 { 20 } else { 15 };
        first.push(cell(
            &format!("{} {}", row.project, row.reference),
            project_width,
            p.info(),
            p,
        ));
        if width >= 100 {
            first.push(cell(
                &row.activity,
                width.saturating_sub(3 + title_width + project_width + age_width + 15),
                p.muted(),
                p,
            ));
        }
        first.push(cell(
            &state,
            if width >= 100 {
                15
            } else {
                width.saturating_sub(3 + title_width + project_width + age_width)
            },
            if pending.is_some() {
                p.disabled()
            } else {
                state_style
            },
            p,
        ));
        first.push(cell(&row.age, age_width, p.muted(), p));
        vec![Line::from(first).style(style)]
    }
    /// Render the whole dedicated screen, including its local delete Dialog.
    #[expect(
        clippy::too_many_lines,
        reason = "One dedicated screen coordinates header, query, preview, footer and modal layering"
    )]
    pub fn render_with(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if area.is_empty() {
            return;
        }
        frame.render_widget(Block::default().style(p.base()), area);
        let footer_height = 2.min(area.height);
        let mut top = area.y.saturating_add(1);
        if (!self.query.text().is_empty() || self.searching)
            && top < area.bottom().saturating_sub(footer_height)
        {
            frame.render_widget(
                Paragraph::new("search ").style(p.muted()),
                Rect::new(area.x, top, area.width.min(7), 1),
            );
            self.query.render_with(
                frame,
                Rect::new(
                    area.x.saturating_add(7),
                    top,
                    area.width.saturating_sub(7),
                    1,
                ),
                EditorView {
                    presentation: p,
                    chrome: false,
                    ..Default::default()
                },
                self.searching,
            );
            top = top.saturating_add(1);
        }
        let status = if self.search.is_some() {
            Some((
                format!(
                    "{} searching all sessions · {}s",
                    glyph(p, "···", "..."),
                    self.elapsed.as_secs()
                ),
                p.muted(),
            ))
        } else if !self.search_error.is_empty() {
            Some((
                format!(
                    "{} search failed · {}",
                    glyph(p, "✕", "x"),
                    self.search_error
                ),
                p.error(),
            ))
        } else if self.load.is_some() || self.snapshot.status == Status::Loading {
            Some((
                format!(
                    "{} loading sessions · {}s",
                    glyph(p, "···", "..."),
                    self.elapsed.as_secs()
                ),
                p.muted(),
            ))
        } else if let Some(reason) = &self.load_failure {
            Some((
                format!("{} couldn't load sessions · {}", glyph(p, "✕", "x"), reason),
                p.error(),
            ))
        } else if let Status::Failed(reason) = &self.snapshot.status {
            Some((
                format!(
                    "{} couldn't load sessions · {}",
                    glyph(p, "✕", "x"),
                    clean(reason, 4096, false)
                ),
                p.error(),
            ))
        } else {
            None
        };
        if let Some((text, style)) = status
            && top < area.bottom().saturating_sub(footer_height)
        {
            frame.render_widget(
                Paragraph::new(elide(&text, area.width, p.symbols)).style(style),
                Rect::new(area.x, top, area.width, 1),
            );
            top = top.saturating_add(1);
        }
        let preview_height = if area.width >= 100 && area.height >= 20 && self.menu.is_none() {
            5
        } else {
            0
        };
        let notice_height = u16::from(!self.notice.is_empty() && area.height >= 8);
        let bottom = area
            .bottom()
            .saturating_sub(footer_height + preview_height + notice_height);
        let body = Rect::new(area.x, top, area.width, bottom.saturating_sub(top));
        let (above, below) = self.render_rows(frame, body, p);
        let mut summary = if self.search.is_some() {
            format!("searching all · {}s", self.elapsed.as_secs())
        } else if self.remote.is_some() {
            format!("{} results · all sessions", self.rows().len())
        } else if !self.query.text().is_empty() {
            format!(
                "{} of {} · loaded",
                self.visible().len(),
                self.snapshot.rows.len()
            )
        } else {
            format!(
                "{} sessions · {}",
                self.rows().len(),
                clean(&self.snapshot.scope, 4096, false)
            )
        };
        if area.width < 25 {
            summary = if self.query.text().is_empty() {
                self.rows().len().to_string()
            } else {
                format!("{}/{}", self.visible().len(), self.rows().len())
            };
        }
        if above > 0 && area.width >= 25 {
            let _ = write!(summary, " · {} {above} more", glyph(p, "↑", "^"));
        }
        rule(
            frame,
            Rect::new(area.x, area.y, area.width, 1),
            "sessions",
            &summary,
            p,
        );
        if preview_height > 0 {
            let row = self.selected().and_then(|id| self.row(id));
            rule(
                frame,
                Rect::new(area.x, bottom, area.width, 1),
                "preview",
                row.map_or("", |r| r.project.as_str()),
                p,
            );
            if let Some(row) = row {
                let lines = wrapped(&clean(&row.preview, 65536, true), area.width)
                    .into_iter()
                    .take(4)
                    .map(|s| Line::styled(s, p.base()))
                    .collect::<Vec<_>>();
                frame.render_widget(
                    Paragraph::new(lines),
                    Rect::new(area.x, bottom.saturating_add(1), area.width, 4),
                );
            }
        }
        if notice_height > 0 {
            frame.render_widget(
                Paragraph::new(elide(
                    &clean(&self.notice, 4096, false),
                    area.width,
                    p.symbols,
                ))
                .style(p.muted()),
                Rect::new(
                    area.x,
                    area.bottom().saturating_sub(footer_height + 1),
                    area.width,
                    1,
                ),
            );
        }
        if footer_height >= 2 {
            rule(
                frame,
                Rect::new(area.x, area.bottom() - 2, area.width, 1),
                "",
                &if below > 0 {
                    format!(
                        "{} {below} more{}",
                        glyph(p, "↓", "v"),
                        if self.snapshot.more.is_some() {
                            " · loading as you go"
                        } else {
                            ""
                        }
                    )
                } else {
                    String::new()
                },
                p,
            );
        }
        if footer_height > 0 {
            frame.render_widget(
                Paragraph::new(elide(
                    &if p.symbols == Symbols::Ascii {
                        self.hint(area.width)
                            .replace("↑↓", "up/down")
                            .replace(" · ", " | ")
                            .replace("⋯", "...")
                    } else {
                        self.hint(area.width)
                    },
                    area.width,
                    p.symbols,
                ))
                .style(p.muted()),
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
        self.delete.render_with(
            frame,
            area,
            p,
            |b, width, _| count(wrapped(&b.text, width.saturating_sub(2)).len() + 3).min(16),
            |b, frame, area, _, _| {
                b.render(frame, area, p);
                None
            },
        );
    }
    #[expect(
        clippy::too_many_lines,
        reason = "One measured row projection aligns stable IDs, inline editors and menus in a shared viewport"
    )]
    fn render_rows(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        p: Presentation,
    ) -> (usize, usize) {
        let indices = self.visible();
        let mut lines = vec![];
        let mut positions = vec![];
        let mut group = String::new();
        let mut menu_at = None;
        let mut rename_at = None;
        for i in &indices {
            let row = &self.rows()[*i];
            if row.group != group {
                group.clone_from(&row.group);
                if !group.is_empty() {
                    lines.push(Line::styled(clean(&group, 4096, false), p.muted()));
                }
            }
            positions.push((row.id.clone(), lines.len()));
            let at = lines.len();
            lines.extend(self.row_lines(row, area.width, p));
            if self.rename.as_ref().is_some_and(|r| r.id == row.id) {
                rename_at = Some(at);
            }
            if self.remote.is_none()
                && !self.query.text().is_empty()
                && !row
                    .title
                    .to_lowercase()
                    .contains(&self.query.text().to_lowercase())
            {
                lines.push(Line::styled(
                    format!("   '{}' in messages", clean(self.query.text(), 4096, false)),
                    p.muted(),
                ));
            }
            if let Some(f) = self.failures.get(&row.id) {
                lines.push(Line::styled(
                    format!(
                        "{} not {} · {}",
                        glyph(p, "✕", "x"),
                        f.kind.pending(),
                        elide(&f.reason, area.width.saturating_sub(20), p.symbols)
                    ),
                    p.error().add_modifier(Modifier::BOLD),
                ));
            }
            if self.menu.as_ref().is_some_and(|m| m.id == row.id) {
                let height = self
                    .menu
                    .as_ref()
                    .map_or(0, |m| {
                        m.picker.height(
                            area.width,
                            PickerView {
                                max_height: 9,
                                ..Default::default()
                            },
                        )
                    })
                    .min(area.height.saturating_sub(1));
                menu_at = Some((lines.len(), height));
                lines.extend((0..height).map(|_| Line::default()));
            }
        }
        let search_position = lines.len();
        if self.search_available() {
            lines.push(Line::styled(
                format!(
                    "{} search all sessions for '{}'",
                    glyph(p, "↵", "enter"),
                    clean(self.query.text(), 4096, false)
                ),
                if self.search_row {
                    p.selection()
                } else {
                    p.info()
                },
            ));
        }
        if indices.is_empty()
            && (!self.query.text().is_empty() || self.snapshot.status == Status::Ready)
        {
            let text = if self.query.text().is_empty() {
                self.snapshot.empty.clone()
            } else {
                format!(
                    "No loaded session matches '{}'.",
                    clean(self.query.text(), 4096, false)
                )
            };
            lines.insert(
                0,
                Line::styled(
                    elide(&clean(&text, 4096, false), area.width, p.symbols),
                    p.muted(),
                ),
            );
        }
        self.viewport = (usize::from(area.height) / if area.width < 50 { 2 } else { 1 }).max(1);
        let focus = if self.search_row {
            search_position + usize::from(indices.is_empty())
        } else {
            positions
                .iter()
                .find(|(id, _)| Some(id.as_str()) == self.selected())
                .map_or(0, |(_, n)| *n)
        };
        if focus < self.offset {
            self.offset = focus;
        }
        let end = menu_at.map_or(focus, |(start, h)| start + usize::from(h).saturating_sub(1));
        if end >= self.offset + usize::from(area.height) {
            self.offset = end
                .saturating_add(1)
                .saturating_sub(usize::from(area.height));
        }
        self.offset = self
            .offset
            .min(lines.len().saturating_sub(usize::from(area.height)));
        frame.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .skip(self.offset)
                    .take(usize::from(area.height))
                    .collect::<Vec<_>>(),
            ),
            area,
        );
        if let Some(at) = rename_at
            .filter(|at| *at >= self.offset && *at < self.offset + usize::from(area.height))
            && let Some(rename) = &mut self.rename
        {
            rename.editor.render_with(
                frame,
                Rect::new(
                    area.x.saturating_add(3),
                    area.y + count(at - self.offset),
                    area.width.saturating_sub(3),
                    1,
                ),
                EditorView {
                    presentation: p,
                    chrome: false,
                    ..Default::default()
                },
                true,
            );
        }
        if let Some((at, h)) = menu_at.filter(|(at, _)| *at >= self.offset) {
            let y = area.y.saturating_add(count(at - self.offset));
            if y < area.bottom()
                && let Some(menu) = &mut self.menu
            {
                menu.picker.render(
                    frame,
                    Rect::new(
                        area.x.saturating_add(3),
                        y,
                        area.width.saturating_sub(3),
                        h.min(area.bottom() - y),
                    ),
                    PickerView {
                        title: if matches!(menu.kind, super::MenuKind::Move) {
                            "move to"
                        } else {
                            "actions"
                        },
                        presentation: p,
                        max_height: h,
                        ..Default::default()
                    },
                    true,
                );
            }
        }
        let above = positions.iter().filter(|(_, n)| *n < self.offset).count();
        let below = positions
            .iter()
            .filter(|(_, n)| *n >= self.offset + usize::from(area.height))
            .count();
        (above, below)
    }
}
impl DeleteCheck {
    fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if area.is_empty() {
            return;
        }
        let rows = wrapped(&clean(&self.text, 65536, true), area.width);
        self.total = rows.len();
        self.viewport = usize::from(area.height.saturating_sub(3)).max(1);
        self.offset = self.offset.min(rows.len().saturating_sub(self.viewport));
        let mut lines = rows
            .into_iter()
            .skip(self.offset)
            .take(self.viewport)
            .map(|s| Line::styled(s, p.surface()))
            .collect::<Vec<_>>();
        lines.push(Line::styled(
            "1 Keep it",
            if self.delete { p.base() } else { p.selection() },
        ));
        lines.push(Line::styled(
            "2 Delete session",
            if self.delete {
                p.selection()
            } else {
                p.error()
            },
        ));
        let hint = if self.confirmation.armed("delete") {
            "enter again within 3 s to delete".into()
        } else if self.offset + self.viewport < self.total {
            format!(
                "{} more · pgdn · esc keep",
                self.total - self.offset - self.viewport
            )
        } else {
            "enter · esc keep".into()
        };
        lines.push(Line::styled(hint, p.warning()));
        frame.render_widget(Paragraph::new(lines), area);
    }
}
