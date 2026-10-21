//! Owns compact row geometry, viewport hints and composition of shared primitives.

use super::{ChoicePicker, SelectionMode, Status};
use crate::{
    editor::EditorView,
    picker::model::Match,
    text::{clean, wrapped},
    theme::{
        Presentation,
        tokens::spacing::{CURSOR_CONTEXT, INDENT, PICKER_DETAIL, PICKER_GAP, PICKER_HEIGHT},
    },
    ui::Rule,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Host-owned labels and maximum height; no property executes or discovers data.
#[derive(Clone, Copy)]
pub struct PickerView<'a> {
    /// Heading on the top rule.
    pub title: &'a str,
    /// Optional right-side source label.
    pub source: &'a str,
    /// Optional explanation, limited to three wrapped rows.
    pub context: &'a str,
    /// Maximum total height. Default nine; a smaller caller area clamps it.
    pub max_height: u16,
    /// Semantic palette and decoration alphabet.
    pub presentation: Presentation,
    /// What Escape does here, shown as `esc <escape>`: `cancel` by default, or
    /// `back` when the host's flow returns to its previous step.
    pub escape: &'a str,
}
impl Default for PickerView<'_> {
    fn default() -> Self {
        Self {
            title: "choose",
            source: "",
            context: "",
            max_height: PICKER_HEIGHT,
            presentation: Presentation::default(),
            escape: "cancel",
        }
    }
}

pub(super) enum Row {
    Group(String),
    Choice(Match),
}

impl ChoicePicker {
    /// Render list rows without picker title, footer or query field, for a host
    /// that supplies its own chrome. Matching, paging and selection stay owned here.
    pub fn render_list(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        p: Presentation,
        focused: bool,
    ) {
        let rows = self.rows();
        self.viewport = usize::from(area.height);
        self.scroll(&rows, focused);
        self.body(frame, area, &rows, p, focused);
    }
    /// Measure preferred height for host placement, preserving opening height while
    /// filtering. Rendering still clamps this to the supplied area.
    #[must_use]
    pub fn height(&self, width: u16, view: PickerView<'_>) -> u16 {
        let context_rows = if view.context.is_empty() {
            0
        } else {
            wrapped(&clean(view.context, 4096, true), width)
                .len()
                .min(3)
        };
        self.opened_height
            .unwrap_or_else(|| self.natural_height(context_rows))
            .min(view.max_height)
    }
    fn natural_height(&self, context_rows: usize) -> u16 {
        let (source_rows, _) = self
            .choices
            .iter()
            .fold((0_usize, ""), |(count, previous), c| {
                (
                    count + 1 + usize::from(!c.group.is_empty() && c.group != previous),
                    c.group.as_str(),
                )
            });
        u16::try_from(2 + context_rows + usize::from(self.filtering()) + source_rows.max(1))
            .unwrap_or(u16::MAX)
    }
    pub(super) fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut group = "";
        for m in &self.matches {
            let name = self.choices[m.index].group.as_str();
            if !name.is_empty() && name != group {
                rows.push(Row::Group(name.to_owned()));
            }
            group = name;
            rows.push(Row::Choice(m.clone()));
        }
        rows
    }
    pub(super) fn view(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        view: PickerView<'_>,
        focused: bool,
    ) -> Rect {
        let p = view.presentation;
        let context: Vec<_> = if view.context.is_empty() {
            Vec::new()
        } else {
            wrapped(&clean(view.context, 4096, true), area.width)
                .into_iter()
                .take(3)
                .collect()
        };
        let rows = self.rows();
        let natural = self.natural_height(context.len());
        let desired = *self.opened_height.get_or_insert(natural);
        let used = Rect::new(
            area.x,
            area.y,
            area.width,
            area.height.min(desired).min(view.max_height),
        );
        if used.is_empty() {
            self.viewport = 0;
            return used;
        }
        frame.render_widget(Block::default().style(p.base()), used);
        let footer_y = used.bottom() - 1;
        let mut y = used.y + 1;
        for line in context {
            if y >= footer_y {
                break;
            }
            frame.render_widget(
                Paragraph::new(line).style(p.muted()),
                Rect::new(used.x, y, used.width, 1),
            );
            y += 1;
        }
        if self.filtering() && y < footer_y {
            let prefix = used.width.min(7);
            frame.render_widget(
                Paragraph::new("filter ").style(p.muted()),
                Rect::new(used.x, y, prefix, 1),
            );
            self.query.render_with(
                frame,
                Rect::new(used.x + prefix, y, used.width - prefix, 1),
                EditorView {
                    placeholder: "type to filter",
                    presentation: p,
                    chrome: false,
                    ..EditorView::default()
                },
                focused && self.status == Status::Ready,
            );
            y += 1;
        }
        self.viewport = usize::from(footer_y.saturating_sub(y));
        self.scroll(&rows, focused);
        self.body(
            frame,
            Rect::new(
                used.x,
                y.min(footer_y),
                used.width,
                footer_y.saturating_sub(y),
            ),
            &rows,
            p,
            focused,
        );
        self.chrome(frame, used, view, focused, rows.len());
        used
    }
    fn scroll(&mut self, rows: &[Row], focused: bool) {
        if focused && self.viewport > 0
            && let Some(index) = rows.iter().position(|row| matches!(row, Row::Choice(m) if self.selected.as_ref() == Some(&self.choices[m.index].id))) {
                let context = CURSOR_CONTEXT.min(self.viewport.saturating_sub(1) / 2);
                if index < self.offset + context { self.offset = index.saturating_sub(context); }
                if index >= self.offset + self.viewport - context { self.offset = index + 1 + context - self.viewport; }
        }
        self.offset = self
            .offset
            .min(rows.len().saturating_sub(self.viewport.max(1)));
    }
    fn body(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        rows: &[Row],
        p: Presentation,
        focused: bool,
    ) {
        if area.is_empty() {
            return;
        }
        let message = match &self.status {
            Status::Loading(text) => Some(format!("working: {}", clean(text, 4096, false))),
            Status::Error(text) => Some(format!("error: {}", clean(text, 4096, false))),
            Status::Ready if self.choices.is_empty() => Some("No choices available".to_owned()),
            Status::Ready if self.matches.is_empty() => {
                Some(format!("No choices match \"{}\"", self.query()))
            }
            Status::Ready => None,
        };
        if let Some(message) = message {
            frame.render_widget(Paragraph::new(message).style(p.muted()), area);
            return;
        }
        let label_width = self
            .matches
            .iter()
            .map(|m| self.choices[m.index].label.width())
            .max()
            .unwrap_or(0);
        let prefix = if self.mode == SelectionMode::Multiple {
            5
        } else if self.filtering() {
            usize::from(INDENT)
        } else {
            5
        };
        let details = usize::from(area.width.saturating_sub(1))
            >= prefix + label_width + PICKER_GAP + PICKER_DETAIL;
        for (i, row) in rows
            .iter()
            .skip(self.offset)
            .take(self.viewport)
            .enumerate()
        {
            let rect = Rect::new(
                area.x,
                area.y + u16::try_from(i).unwrap_or(0),
                area.width,
                1,
            );
            match row {
                Row::Group(group) => {
                    frame.render_widget(Paragraph::new(group.as_str()).style(p.muted()), rect);
                }
                Row::Choice(m) => {
                    self.choice_row(frame, rect, m, (prefix, label_width, details), p, focused);
                }
            }
        }
    }
    fn choice_row(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        m: &Match,
        columns: (usize, usize, bool),
        p: Presentation,
        focused: bool,
    ) {
        let choice = &self.choices[m.index];
        let active = self.selected.as_ref() == Some(&choice.id) && focused;
        if active {
            frame.render_widget(
                Block::default().style(p.selection()),
                Rect::new(
                    area.x.saturating_add(1),
                    area.y,
                    area.width.saturating_sub(2),
                    1,
                ),
            );
        }
        let (prefix, label_width, details) = columns;
        let key = self.row_mark(m, p);
        let key_style = if active {
            p.base()
                .fg(Color::Reset)
                .add_modifier(Modifier::REVERSED | Modifier::BOLD)
                .remove_modifier(Modifier::DIM)
        } else if choice.current {
            p.accent()
        } else {
            p.muted()
        };
        let label_style = self.label_style(m, p, active);
        let budget = usize::from(area.width.saturating_sub(1)).saturating_sub(prefix);
        let label_text = availability_text(
            &choice.label,
            choice.disabled.is_some() && !details && self.mode == SelectionMode::Single,
            p,
        );
        let label = clip(
            &label_text,
            if details { label_width } else { budget },
            p.symbols.ellipsis_text(),
        );
        let mut spans = vec![
            Span::styled(
                if active && p.symbols == crate::theme::Symbols::Ascii {
                    ">"
                } else {
                    " "
                },
                p.base()
                    .fg(Color::Reset)
                    .bg(Color::Reset)
                    .remove_modifier(Modifier::REVERSED | Modifier::DIM),
            ),
            Span::styled(key, key_style),
        ];
        if self.mode == SelectionMode::Single && !self.filtering() {
            spans.push(Span::raw(" "));
        }
        spans.extend(highlight(
            &label,
            &m.label,
            label_style,
            choice.disabled.is_none(),
        ));
        if details {
            spans.push(Span::raw(
                " ".repeat(label_width.saturating_sub(label.width()) + PICKER_GAP),
            ));
            let detail = availability_text(
                choice.disabled.as_deref().unwrap_or(&choice.detail),
                choice.disabled.is_some() && self.mode == SelectionMode::Single,
                p,
            );
            spans.extend(highlight(
                &clip(
                    &detail,
                    budget.saturating_sub(label_width + PICKER_GAP),
                    p.symbols.ellipsis_text(),
                ),
                &m.detail,
                p.muted(),
                choice.disabled.is_none(),
            ));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(area.x, area.y, area.width.saturating_sub(1), 1),
        );
    }
    fn label_style(&self, m: &Match, p: Presentation, active: bool) -> Style {
        let choice = &self.choices[m.index];
        let at_limit = !self.marked.contains(&choice.id)
            && self.mode == SelectionMode::Multiple
            && self
                .maximum
                .is_some_and(|maximum| self.marked.len() >= maximum);
        let style = if choice.disabled.is_some() || at_limit {
            p.disabled()
        } else if choice.danger {
            p.error()
        } else {
            p.base().fg(Color::Reset).remove_modifier(Modifier::DIM)
        };
        if active {
            style
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::DIM)
        } else {
            style
        }
    }
    fn row_mark(&self, m: &Match, p: Presentation) -> String {
        let choice = &self.choices[m.index];
        let current = if choice.current { '*' } else { ' ' };
        if choice.disabled.is_some() && self.mode == SelectionMode::Multiple {
            return format!("[-]{current}");
        }
        if self.mode == SelectionMode::Multiple {
            let marked = self.marked.contains(&choice.id);
            let boxed = matches!(
                p.colors,
                crate::theme::ColorMode::Ansi16 | crate::theme::ColorMode::NoColor
            ) || p.symbols == crate::theme::Symbols::Ascii;
            return if boxed {
                format!("[{}]{current}", if marked { 'x' } else { ' ' })
            } else {
                format!("{}  {current}", if marked { '◉' } else { '○' })
            };
        }
        if self.filtering() {
            current.to_string()
        } else if m.index < 9 {
            format!(" {}{current}", m.index + 1)
        } else {
            format!(" {current} ")
        }
    }
    fn chrome(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        view: PickerView<'_>,
        focused: bool,
        rows: usize,
    ) {
        let p = view.presentation;
        let style = if focused { p.accent() } else { p.muted() };
        let above = if self.offset > 0 && self.status == Status::Ready {
            format!("{} {} more", p.symbols.more(true), self.offset)
        } else {
            self.source_metadata(view.source)
        };
        frame.render_widget(
            Rule {
                left: Line::from(Span::styled(
                    clean(view.title, 128, false),
                    if focused { p.focus() } else { p.muted() },
                )),
                right: Line::from(Span::styled(above, p.muted())),
                glyph: p.symbols.rule(),
                style,
            },
            Rect::new(area.x, area.y, area.width, 1),
        );
        if area.height < 2 {
            return;
        }
        let mut hints = Vec::new();
        // Escape clears a query before it closes this layer.
        let escape = if self.query().is_empty() {
            format!("esc {}", clean(view.escape, 32, false))
        } else {
            "esc clears".to_owned()
        };
        let below = rows.saturating_sub(self.offset + self.viewport);
        if below > 0 && self.status == Status::Ready {
            hints.push(format!("{} {below} more", p.symbols.more(false)));
        }
        if focused {
            if self.status == Status::Ready {
                if self.mode == SelectionMode::Multiple {
                    hints.push(format!("{} marked", self.marked.len()));
                    hints.push("space / alt-a mark".to_owned());
                } else if self.filtering() {
                    hints.push(format!("{} of {}", self.matches.len(), self.choices.len()));
                } else {
                    let keys = self.key_hints(p);
                    if !keys.is_empty() {
                        hints.push(keys);
                    }
                }
                if self.mode == SelectionMode::Multiple || self.selected.is_some() {
                    hints.push("enter".to_owned());
                }
                hints.push(escape.clone());
                if self.inspect && self.selected.is_some() && area.width >= 50 {
                    hints.push("F3 inspect".to_owned());
                }
            } else {
                hints.push(format!("esc {}", clean(view.escape, 32, false)));
            }
        }
        if area.width < 50 && focused {
            hints = vec![escape];
        }
        frame.render_widget(
            Rule {
                left: Line::default(),
                right: Line::from(Span::styled(hints.join(p.symbols.separator()), p.muted())),
                glyph: p.symbols.rule(),
                style,
            },
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }
    fn source_metadata(&self, source: &str) -> String {
        if self.mode != SelectionMode::Multiple {
            return clean(source, 128, false);
        }
        let count = self.marked.len();
        let hidden = self
            .choices
            .iter()
            .enumerate()
            .filter(|(index, choice)| {
                self.marked.contains(&choice.id) && !self.matches.iter().any(|m| m.index == *index)
            })
            .count();
        let label = self.maximum.map_or_else(
            || format!("{count} marked"),
            |max| format!("{count} of {max} marked"),
        );
        if hidden > 0 {
            return format!("{label} · {hidden} hidden");
        }
        label
    }
    fn key_hints(&self, p: Presentation) -> String {
        let keys: Vec<_> = self
            .choices
            .iter()
            .take(9)
            .enumerate()
            .filter(|(_, c)| c.disabled.is_none())
            .map(|(i, _)| i + 1)
            .collect();
        if keys.len() > 1 && keys.windows(2).all(|pair| pair[1] == pair[0] + 1) {
            let separator = if p.symbols == crate::theme::Symbols::Ascii {
                "-"
            } else {
                "–"
            };
            format!("{}{separator}{}", keys[0], keys[keys.len() - 1])
        } else {
            keys.iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
}

fn availability_text(text: &str, disabled: bool, p: Presentation) -> String {
    if disabled
        && (p.colors == crate::theme::ColorMode::NoColor
            || p.symbols == crate::theme::Symbols::Ascii)
    {
        format!("[-] {text}")
    } else {
        text.to_owned()
    }
}

fn clip(text: &str, width: usize, marker: &str) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut output = String::new();
    let mut cells = 0;
    for g in text.graphemes(true) {
        if cells + g.width() > width.saturating_sub(marker.width()) {
            break;
        }
        output.push_str(g);
        cells += g.width();
    }
    output.extend(marker.chars().take(width));
    output
}
fn highlight(
    text: &str,
    ranges: &[Range<usize>],
    style: Style,
    enabled: bool,
) -> Vec<Span<'static>> {
    text.grapheme_indices(true)
        .map(|(i, g)| {
            let matched = enabled && ranges.iter().any(|r| r.contains(&i));
            Span::styled(
                g.to_owned(),
                if matched {
                    style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
                } else {
                    style
                },
            )
        })
        .collect()
}
