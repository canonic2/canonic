//! Bounded review chrome, complete safe inspection and exact-scope modal rendering.

use super::{PermissionReview, ScopeCheck, State};
use crate::{
    text::{clean, elide, wrapped, wrapped_prose},
    theme::{ColorMode, Presentation, Symbols},
    tool_call_card::{
        Output,
        view::{Role, output_line},
    },
    ui::Rule,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use std::fmt::Write;

pub(super) struct InspectionRow {
    pub text: String,
    pub role: Role,
    pub source: usize,
}
fn size(n: usize) -> u16 {
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
                    area.width.saturating_sub(size(
                        right.chars().count() + usize::from(!right.is_empty()),
                    )),
                    p.symbols,
                ),
                p.accent(),
            ),
            right: Line::styled(right.to_owned(), p.muted()),
            glyph: glyph(p, "─", "-").chars().next().unwrap_or('-'),
            style: p.accent(),
        },
        area,
    );
}
impl PermissionReview {
    /// Effective keys; the Enter label names the current action.
    #[must_use]
    pub fn hint(&self) -> String {
        if self.state == State::Sending {
            return if self.elapsed.as_secs() >= 3 {
                "sent · esc can't take it back".into()
            } else {
                "sending · keys paused".into()
            };
        }
        if self.confirming() {
            return self
                .scope
                .ticket()
                .and_then(|ticket| self.scope.body(ticket))
                .map_or_else(String::new, |body| {
                    format!(
                        "enter {} · esc back · pgdn scope",
                        if body.accept {
                            body.label.as_str()
                        } else {
                            "Back"
                        }
                    )
                });
        }
        if self.searching {
            return "enter next match · esc end search".into();
        }
        if self.details && self.cursor == Some(0) {
            return "tab hide details · ↑↓ scroll · 1–9 choose · esc hide".into();
        }
        if self.locked() {
            return "tab details · esc close".into();
        }
        if self.state == State::Failed {
            return "r retry · 1–9 choose another · tab details · esc close".into();
        }
        self.selected().map_or_else(
            || "choose a response · ↑↓ 1–9 choose · tab details · esc close".into(),
            |c| {
                format!(
                    "enter {}{} · ↑↓ 1–9 choose · tab details · esc close",
                    clean(&c.label, 4096, false),
                    if c.wider { "…" } else { "" }
                )
            },
        )
    }
    /// Compact hints retain confirmation/dismissal before secondary navigation.
    #[must_use]
    pub fn hint_for(&self, width: u16) -> String {
        if width >= 50 {
            return self.hint();
        }
        if self.state == State::Sending {
            return "sending · keys paused".into();
        }
        if self.confirming() {
            return self
                .scope
                .ticket()
                .and_then(|ticket| self.scope.body(ticket))
                .map_or_else(String::new, |body| {
                    format!(
                        "enter {} · esc back",
                        if body.accept { "confirm" } else { "Back" }
                    )
                });
        }
        if self.searching {
            return "enter next · esc search".into();
        }
        if self.details {
            return "tab hide · pgdn · esc".into();
        }
        if self.state == State::Failed {
            return "r retry · esc close".into();
        }
        if self.locked() {
            return "esc close · tab details".into();
        }
        self.selected().map_or_else(
            || "choose · tab details · esc".into(),
            |c| format!("enter {}{} · esc", c.label, if c.wider { "…" } else { "" }),
        )
    }
    /// Preferred panel height, clamped by the host while retaining the dock.
    #[must_use]
    pub fn height(&self, width: u16, _p: Presentation) -> u16 {
        size(
            5 + self.request.resources.len().min(4)
                + usize::from(self.request.resources.len() > 4)
                + self.request.choices.len() * if width < 50 && !self.details { 2 } else { 1 }
                + usize::from(self.details) * 6
                + usize::from(!self.notice.is_empty()),
        )
    }
    pub(super) fn source_lines(&self) -> Vec<String> {
        self.request
            .resources
            .iter()
            .map(|r| format!("{} {}", r.name, r.detail))
            .chain(
                match &self.request.details {
                    Output::Unavailable(reason) => reason.as_str(),
                    other => other.text(),
                }
                .split('\n')
                .map(str::to_owned),
            )
            .collect()
    }
    pub(super) fn inspection_rows(&self, width: u16, p: Presentation) -> Vec<InspectionRow> {
        let mut rows = vec![];
        let text = match &self.request.details {
            Output::Unavailable(reason) => reason.as_str(),
            other => other.text(),
        };
        let mut old = 0;
        let mut new = 0;
        for (index, raw) in text.split('\n').enumerate() {
            let (number, sign, body, role) =
                output_line(&self.request.details, raw, index, &mut old, &mut new, false);
            let number = if width >= 50 {
                number.map_or_else(String::new, |n| format!("{n:>4} "))
            } else {
                String::new()
            };
            let sign = if p.symbols == Symbols::Ascii {
                sign.replace('−', "-")
            } else {
                sign.into()
            };
            let prefix = format!("{} {number}{sign}", glyph(p, "│", "|"));
            for (part, line) in wrapped(
                &clean(body, 262_144, false),
                width.saturating_sub(size(prefix.chars().count() + 2)),
            )
            .into_iter()
            .enumerate()
            {
                let continuation = if part > 0 { glyph(p, "↩ ", "> ") } else { "" };
                rows.push(InspectionRow {
                    text: format!("{prefix}{continuation}{line}"),
                    role,
                    source: self.request.resources.len() + index,
                });
            }
        }
        for (source, resource) in self.request.resources.iter().enumerate() {
            for text in wrapped(
                &clean(
                    &format!("{} {}", resource.name, resource.detail),
                    65_536,
                    false,
                ),
                width.saturating_sub(2),
            ) {
                rows.push(InspectionRow {
                    text: format!("{} {text}", glyph(p, "│", "|")),
                    role: Role::Info,
                    source,
                });
            }
        }
        rows
    }
    /// Render the above-dock review panel; owns neither event loop nor terminal.
    pub fn render_with(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        p: Presentation,
        waiting: usize,
    ) {
        let resized = self.area.width != area.width;
        self.area = area;
        if area.is_empty() {
            return;
        }
        if resized && self.searching {
            self.refresh_matches();
        }
        frame.render_widget(Block::default().style(p.base()), area);
        let mut meta = match self.state {
            State::Sending => format!("sent · {}s", self.elapsed.as_secs()),
            State::Changed => format!("changed · revision {}", self.request.revision),
            State::Failed => "not applied".into(),
            State::Refused => "refused".into(),
            State::Expired => "expired".into(),
            State::Withdrawn => "withdrawn".into(),
            _ => clean(&self.request.run, 4096, false),
        };
        if waiting > 0 {
            let _ = write!(meta, " · +{waiting} waiting");
        }
        rule(
            frame,
            Rect::new(area.x, area.y, area.width, 1),
            &clean(&self.request.title, 4096, false),
            &meta,
            p,
        );
        if area.height < 2 {
            return;
        }
        let body = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(2),
        );
        self.render_body(frame, body, p);
        let footer = if self.state == State::Sending {
            self.sending_label()
        } else if self.locked() {
            self.outcome().into()
        } else if self
            .confirmation
            .armed(self.selected().map_or("", |c| c.id.as_str()))
        {
            "armed · enter again within 3 s".into()
        } else if self.selected().is_some() {
            "nothing allowed yet".into()
        } else {
            "nothing chosen".into()
        };
        rule(
            frame,
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
            "",
            &elide(&footer, area.width, p.symbols),
            p,
        );
    }
    fn sending_label(&self) -> String {
        self.pending.as_ref().map_or_else(String::new, |r| {
            if self.elapsed.as_secs() >= 3 {
                format!(
                    "waiting for host to acknowledge · {}s",
                    self.elapsed.as_secs()
                )
            } else {
                format!(
                    "sending {} · retry {}",
                    self.selected().map_or("response", |c| c.label.as_str()),
                    r.retry
                )
            }
        })
    }
    fn render_body(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if area.is_empty() {
            return;
        }
        let narrow = area.width < 50;
        let per = if narrow && !self.details { 2 } else { 1 };
        let choice_rows =
            (size(self.request.choices.len()) * per).min(area.height.saturating_sub(1).max(1));
        let available = area.height.saturating_sub(choice_rows);
        let notice_rows = usize::from(!self.notice.is_empty() && available >= 2);
        let summary_rows = available
            .saturating_sub(1 + size(notice_rows))
            .min(if self.details { 1 } else { 7 });
        let mut y = area.y;
        if summary_rows > 0 {
            self.render_summary(frame, Rect::new(area.x, y, area.width, summary_rows), p);
            y += summary_rows;
        }
        let head = if y < area.bottom().saturating_sub(choice_rows) {
            let head = Some(Rect::new(area.x, y, area.width, 1));
            y += 1;
            head
        } else {
            None
        };
        let detail_rows = area
            .bottom()
            .saturating_sub(y + choice_rows + size(notice_rows));
        if self.details && detail_rows > 0 {
            self.render_details(frame, Rect::new(area.x, y, area.width, detail_rows), p);
            y += detail_rows;
        }
        if let Some(head) = head {
            self.render_details_head(frame, head, p);
        }
        self.render_choices(frame, Rect::new(area.x, y, area.width, choice_rows), p, per);
        if notice_rows > 0 {
            frame.render_widget(
                Paragraph::new(elide(
                    &format!("! {}", clean(&self.notice, 4096, false)),
                    area.width,
                    p.symbols,
                ))
                .style(if matches!(self.state, State::Failed | State::Refused) {
                    p.error()
                } else {
                    p.warning()
                }),
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
    }
    fn render_summary(&self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let mut lines = vec![Line::styled(
            elide(
                &clean(&self.request.summary, 65_536, false),
                area.width,
                p.symbols,
            ),
            p.base().add_modifier(Modifier::BOLD),
        )];
        if !self.details {
            if !self.request.reason.is_empty() {
                lines.push(Line::styled(
                    elide(
                        &clean(&self.request.reason, 65_536, false),
                        area.width,
                        p.symbols,
                    ),
                    p.muted(),
                ));
            }
            for r in self.request.resources.iter().take(4) {
                lines.push(Line::styled(
                    elide(
                        &format!(
                            "  {}  {}",
                            clean(&r.name, 4096, false),
                            clean(&r.detail, 4096, false)
                        ),
                        area.width,
                        p.symbols,
                    ),
                    p.info(),
                ));
            }
            if self.request.resources.len() > 4 {
                lines.push(Line::styled(
                    format!(
                        "{} {} more resources · tab lists all",
                        glyph(p, "⋯", "..."),
                        self.request.resources.len() - 4
                    ),
                    p.muted(),
                ));
            }
        }
        if lines.len() > usize::from(area.height) {
            lines.truncate(usize::from(area.height));
            if let Some(last) = lines.last_mut()
                && area.height > 1
            {
                *last = Line::styled(
                    format!("{} resources/reason in details", glyph(p, "⋯", "...")),
                    p.muted(),
                );
            }
        }
        frame.render_widget(Paragraph::new(lines), area);
    }
    fn render_details_head(&self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let more = self
            .detail_total
            .saturating_sub(self.offset + self.viewport);
        let meta = if self.searching {
            format!(
                "find {:?} · {} of {}",
                self.query.text(),
                usize::from(!self.matches.is_empty()) + self.match_index,
                self.matches.len()
            )
        } else if self.details {
            format!("{more} more lines · pgdn")
        } else {
            clean(&self.request.details_label, 4096, false)
        };
        let mark = if self.details {
            glyph(p, "▾", "v")
        } else {
            glyph(p, "▸", ">")
        };
        frame.render_widget(
            Paragraph::new(elide(
                &format!(" {mark} details  {meta}"),
                area.width,
                p.symbols,
            ))
            .style(if self.cursor == Some(0) {
                p.selection()
            } else {
                p.muted()
            }),
            area,
        );
    }
    fn render_details(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        let rows = self.inspection_rows(area.width, p);
        self.detail_total = rows.len();
        self.viewport = usize::from(area.height).max(1);
        self.offset = self.offset.min(rows.len().saturating_sub(self.viewport));
        let lines = rows
            .iter()
            .skip(self.offset)
            .take(self.viewport)
            .enumerate()
            .map(|(i, row)| {
                let style = if self.searching
                    && self.matches.get(self.match_index) == Some(&(self.offset + i))
                {
                    p.selection()
                } else {
                    match row.role {
                        Role::Add => p
                            .tokens()
                            .success
                            .map_or(p.base(), |color| p.base().fg(color))
                            .add_modifier(Modifier::BOLD),
                        Role::Delete => p.error(),
                        Role::Info => p.info(),
                        _ => p.muted(),
                    }
                };
                Line::styled(row.text.clone(), style)
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), area);
    }
    fn render_choices(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation, per: u16) {
        if area.is_empty() {
            return;
        }
        let viewport = usize::from(area.height / per).max(1);
        if let Some(index) = self.cursor.and_then(|r| r.checked_sub(1)) {
            if index < self.choice_offset {
                self.choice_offset = index;
            } else if index >= self.choice_offset + viewport {
                self.choice_offset = index + 1 - viewport;
            }
        }
        self.choice_offset = self
            .choice_offset
            .min(self.request.choices.len().saturating_sub(viewport));
        let mut lines = vec![];
        for (i, c) in self
            .request
            .choices
            .iter()
            .enumerate()
            .skip(self.choice_offset)
            .take(viewport)
        {
            let selected = self.cursor == Some(i + 1);
            let style = if selected {
                p.selection()
            } else if self.locked() || c.disabled.is_some() || self.state == State::Sending {
                p.disabled()
            } else if c.destructive {
                p.error()
            } else {
                p.base()
            };
            let marker = if selected && p.symbols == Symbols::Ascii {
                ">"
            } else {
                " "
            };
            let detail = c.disabled.as_ref().unwrap_or(&c.detail);
            let label = clean(&c.label, 4096, false);
            let right = if area.width >= 50 {
                format!("  {}", clean(detail, 4096, false))
            } else {
                String::new()
            };
            lines.push(Line::styled(
                elide(
                    &format!("{marker}{} {label}{right}", i + 1),
                    area.width,
                    p.symbols,
                ),
                style,
            ));
            if per == 2 {
                lines.push(Line::styled(
                    format!(
                        "    {}",
                        elide(
                            &clean(detail, 4096, false),
                            area.width.saturating_sub(4),
                            p.symbols
                        )
                    ),
                    if selected { style } else { p.muted() },
                ));
            }
        }
        let omitted = self
            .request
            .choices
            .len()
            .saturating_sub(self.choice_offset + viewport);
        if omitted > 0
            && let Some(last) = lines.last_mut()
        {
            last.spans
                .push(Span::styled(format!(" · +{omitted} choices"), p.muted()));
        }
        frame.render_widget(Paragraph::new(lines).style(p.base()), area);
    }
    /// Render exact-scope confirmation after the host's background and fixed dock.
    pub fn render_overlay(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        self.scope.render_with(
            frame,
            area,
            p,
            |body, width, _| size(wrapped(&body.scope, width.saturating_sub(2)).len() + 3).min(14),
            |body, frame, area, _, _| {
                body.render(frame, area, p);
                None
            },
        );
    }
}
impl ScopeCheck {
    fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if area.is_empty() {
            return;
        }
        let scope = if p.colors == ColorMode::NoColor {
            format!("[ {} ]", self.scope)
        } else {
            self.scope.clone()
        };
        // Words stay whole; a token longer than the row still splits.
        let rows = wrapped_prose(&scope, area.width);
        self.total = rows.len();
        self.viewport = usize::from(area.height.saturating_sub(3)).max(1);
        self.offset = self.offset.min(rows.len().saturating_sub(self.viewport));
        let more = rows.len().saturating_sub(self.offset + self.viewport);
        let mut lines = vec![Line::styled(
            format!(
                "{}{}",
                if self.prompt.is_empty() {
                    "Exact host scope".to_owned()
                } else {
                    clean(&self.prompt, 4096, false)
                },
                if more > 0 {
                    format!(" · {more} more · pgdn")
                } else {
                    String::new()
                }
            ),
            p.muted(),
        )];
        lines.extend(
            rows.into_iter()
                .skip(self.offset)
                .take(self.viewport)
                // Without a surface color, a reversed block reads as a selection.
                .map(|line| {
                    Line::styled(
                        line,
                        if p.tokens().surface.is_some() {
                            p.surface()
                        } else {
                            p.base().add_modifier(Modifier::BOLD)
                        },
                    )
                }),
        );
        lines.push(Line::styled(
            " 1 Back",
            if self.accept { p.base() } else { p.selection() },
        ));
        let label = if self.confirmation.armed("scope") {
            "enter again within 3 s"
        } else {
            &self.label
        };
        lines.push(Line::styled(
            format!(" 2 {}", clean(label, 4096, false)),
            if self.accept { p.selection() } else { p.base() },
        ));
        frame.render_widget(Paragraph::new(lines), area);
    }
}
