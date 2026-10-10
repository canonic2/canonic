//! Fixed-height completion rules and rows; the host keeps its editor cursor.
use super::{Candidate, Menu, matching::matched};
use crate::{
    text::wrapped,
    theme::{Presentation, Symbols},
    ui::Rule,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

impl Menu {
    /// Opening height, including two rules; filtering does not change it.
    #[must_use]
    pub fn height(&self, width: u16) -> u16 {
        if self.open {
            self.body_rows.min(if width <= 40 { 5 } else { 8 }) + 2
        } else {
            0
        }
    }
    /// Draw above a host-owned box, clamped to any available rectangle.
    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        if !self.open || area.is_empty() {
            return;
        }
        let height = self.height(area.width).min(area.height);
        let area = Rect {
            y: area.bottom() - height,
            height,
            ..area
        };
        frame.render_widget(Block::default().style(p.base()), area);
        self.page_rows = usize::from(height.saturating_sub(2)).max(1);
        self.render_rule(frame, Rect { height: 1, ..area }, p, false, 0);
        if height < 2 {
            return;
        }
        let shown = self.render_rows(
            frame,
            Rect::new(area.x, area.y + 1, area.width, height - 2),
            p,
        );
        self.render_rule(
            frame,
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
            p,
            true,
            shown,
        );
    }
    fn render_rule(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        p: Presentation,
        footer: bool,
        shown: usize,
    ) {
        let total = self
            .sources
            .iter()
            .filter_map(|s| s.batch.total)
            .fold(0, usize::saturating_add);
        let scanned = self
            .sources
            .iter()
            .map(|s| s.batch.scanned)
            .fold(0, usize::saturating_add);
        let count = if self.loading() {
            format!("{} so far", self.visible.len())
        } else if total > self.visible.len() {
            format!("{} of {total}", self.visible.len())
        } else {
            self.visible.len().to_string()
        };
        let more = self.visible.len().saturating_sub(self.offset + shown);
        let progress = if self.loading() {
            format!(
                "{} searching {scanned} of {total}",
                if p.symbols == Symbols::Ascii {
                    "..."
                } else {
                    "···"
                }
            )
        } else if more > 0 {
            format!(
                "{} {more} more",
                if p.symbols == Symbols::Ascii {
                    "v"
                } else {
                    "↓"
                }
            )
        } else {
            String::new()
        };
        frame.render_widget(
            Rule {
                left: Line::styled(
                    if footer {
                        String::new()
                    } else {
                        self.title.clone()
                    },
                    if footer { p.muted() } else { p.heading() },
                ),
                right: Line::styled(if footer { progress } else { count }, p.muted()),
                glyph: if p.symbols == Symbols::Ascii {
                    '-'
                } else {
                    '─'
                },
                style: p.message_line(),
            },
            area,
        );
    }
    fn render_rows(&mut self, frame: &mut Frame<'_>, area: Rect, p: Presentation) -> usize {
        if area.is_empty() {
            return 0;
        }
        let warning = self
            .failures()
            .into_iter()
            .map(|(source, error)| format!("{source}: {error}"))
            .collect::<Vec<_>>()
            .join(" · ");
        if self.visible.is_empty() {
            self.render_empty(frame, area, p, &warning);
            return 0;
        }
        let at = self
            .visible
            .iter()
            .position(|i| Some(&self.rows[*i].id) == self.selected.as_ref())
            .unwrap_or(0);
        let budget = area.height.saturating_sub(u16::from(!warning.is_empty()));
        if budget == 0 {
            frame.render_widget(Paragraph::new(warning).style(p.warning()), area);
            return 0;
        }
        self.page_rows = usize::from(budget);
        let column = u16::try_from(
            self.visible
                .iter()
                .map(|i| self.rows[*i].name.width())
                .max()
                .unwrap_or(0)
                + 4,
        )
        .unwrap_or(u16::MAX);
        let selected_lines = self
            .selected()
            .map_or(1, |row| {
                description(row, area.width, p, column, true)
                    .len()
                    .clamp(1, 3)
            })
            .min(usize::from(budget));
        if at < self.offset {
            self.offset = at;
        }
        if at + selected_lines > self.offset + usize::from(budget) {
            self.offset = at + selected_lines - usize::from(budget);
        }
        let mut y = area.y;
        let mut shown = 0;
        for i in self.visible.iter().skip(self.offset) {
            if y >= area.y + budget {
                break;
            }
            let row = &self.rows[*i];
            y += draw_candidate(
                frame,
                Rect::new(area.x, y, area.width, area.y + budget - y),
                p,
                row,
                Some(&row.id) == self.selected.as_ref(),
                &self.request.query,
                column,
            );
            shown += 1;
        }
        if !warning.is_empty() {
            frame.render_widget(
                Paragraph::new(format!("! {warning}")).style(p.warning()),
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
        shown
    }
    fn render_empty(&self, frame: &mut Frame<'_>, area: Rect, p: Presentation, warning: &str) {
        let text = if !warning.is_empty() && self.all_failed() {
            format!(
                "{} {warning}\ntab retries",
                if p.symbols == Symbols::Ascii {
                    "x"
                } else {
                    "✕"
                }
            )
        } else if self.loading() {
            format!(
                "{} listing {}",
                if p.symbols == Symbols::Ascii {
                    "..."
                } else {
                    "···"
                },
                self.title
            )
        } else if let Some(guess) = &self.guess {
            format!(
                "No matches for {:?}\nclosest: {} · tab takes it",
                self.request.query, guess.name
            )
        } else {
            format!("No matches for {:?}", self.request.query)
        };
        frame.render_widget(
            Paragraph::new(text).style(if self.all_failed() {
                p.error()
            } else {
                p.muted()
            }),
            area,
        );
        if !warning.is_empty() && !self.all_failed() {
            frame.render_widget(
                Paragraph::new(format!("! {warning}")).style(p.warning()),
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
    }
}

fn description(
    row: &Candidate,
    width: u16,
    p: Presentation,
    column: u16,
    selected: bool,
) -> Vec<String> {
    let available = width.saturating_sub(column);
    if width < 30 || available < 8 {
        return vec![String::new()];
    }
    let tag = row.disabled.as_deref().unwrap_or(&row.tag);
    let tag_width = u16::try_from(tag.width() + 2).unwrap_or(u16::MAX);
    let desc_width = if width >= 60 && available > tag_width.saturating_add(12) {
        available - tag_width
    } else {
        available
    };
    if selected && width > 40 {
        word_wrapped(&row.description, desc_width)
    } else {
        vec![shorten(
            row.description.lines().next().unwrap_or(""),
            usize::from(desc_width),
            p,
        )]
    }
}
fn word_wrapped(text: &str, width: u16) -> Vec<String> {
    let mut rows = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.width() + 1 + word.width() > usize::from(width) {
                rows.push(std::mem::take(&mut line));
            }
            if word.width() > usize::from(width) {
                let mut chunks = wrapped(word, width);
                line = chunks.pop().unwrap_or_default();
                rows.extend(chunks);
            } else {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
        }
        rows.push(line);
    }
    rows
}
fn shorten(text: &str, width: usize, p: Presentation) -> String {
    if text.width() <= width {
        return text.into();
    }
    let suffix = if p.symbols == Symbols::Ascii {
        "..."
    } else {
        "⋯"
    };
    let mut clipped = String::new();
    for g in text.graphemes(true) {
        if clipped.width() + g.width() + suffix.width() > width {
            break;
        }
        clipped.push_str(g);
    }
    clipped.push_str(suffix);
    clipped
}
fn draw_candidate(
    frame: &mut Frame<'_>,
    area: Rect,
    p: Presentation,
    row: &Candidate,
    selected: bool,
    query: &str,
    column: u16,
) -> u16 {
    let lines = description(row, area.width, p, column, selected);
    let height = u16::try_from(lines.len().min(if selected { 3 } else { 1 }))
        .unwrap_or(1)
        .min(area.height);
    let band = if selected { p.selection() } else { p.base() };
    frame.render_widget(Block::default().style(band), Rect { height, ..area });
    let style = if row.disabled.is_some() {
        p.muted()
    } else if selected {
        p.accent().add_modifier(Modifier::BOLD)
    } else {
        p.accent()
    };
    let positions = matched(&row.name, query).map_or_else(Vec::new, |(_, positions)| positions);
    // Keep whole graphemes together even when highlighting case-folded matches.
    let mut char_index = 0;
    let name = Line::from(
        row.name
            .graphemes(true)
            .map(|g| {
                let highlight =
                    (char_index..char_index + g.chars().count()).any(|i| positions.contains(&i));
                char_index += g.chars().count();
                Span::styled(
                    g.to_owned(),
                    if highlight {
                        style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                    } else {
                        style
                    },
                )
            })
            .collect::<Vec<_>>(),
    );
    let indent = area.width.min(2);
    frame.render_widget(
        Paragraph::new(name),
        Rect::new(area.x + indent, area.y, area.width - indent, 1),
    );
    if area.width >= 30 && area.width.saturating_sub(column) >= 8 {
        let tag = row.disabled.as_deref().unwrap_or(&row.tag);
        let tag_width = u16::try_from(tag.width() + 2).unwrap_or(u16::MAX);
        let available = area.width - column;
        let show_tag = area.width >= 60 && available > tag_width.saturating_add(12);
        let desc_width = if show_tag {
            available - tag_width
        } else {
            available
        };
        for (n, text) in lines.iter().take(usize::from(height)).enumerate() {
            let text = if n + 1 == usize::from(height) && lines.len() > usize::from(height) {
                format!(
                    "{text}{}",
                    if p.symbols == Symbols::Ascii {
                        "..."
                    } else {
                        "⋯"
                    }
                )
            } else {
                text.clone()
            };
            frame.render_widget(
                Paragraph::new(shorten(&text, usize::from(desc_width), p)).style(p.muted()),
                Rect::new(
                    area.x + column,
                    area.y + u16::try_from(n).unwrap_or(0),
                    desc_width,
                    1,
                ),
            );
        }
        if show_tag {
            frame.render_widget(
                Paragraph::new(tag).style(p.muted()),
                Rect::new(area.right() - tag_width + 2, area.y, tag_width - 2, 1),
            );
        }
    }
    height
}
