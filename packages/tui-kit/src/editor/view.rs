//! Renders editor state using shared tokens/rules and explicit host labels.

use super::{Editor, layout};
use crate::{
    theme::{
        Presentation,
        tokens::spacing::{CURSOR_CONTEXT, EDITOR_CHROME},
    },
    ui::Rule,
};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_segmentation::UnicodeSegmentation;

/// Host-owned presentation props; none changes draft or editing semantics.
#[derive(Clone, Copy)]
pub struct EditorView<'a> {
    /// Title on the top rule.
    pub title: &'a str,
    /// Dim hint when the draft is empty; never stored in the draft.
    pub placeholder: &'a str,
    /// Footer metadata. Empty uses the logical line/grapheme column.
    pub footer: &'a str,
    /// Resolved capability and palette inputs.
    pub presentation: Presentation,
    /// Draw title/footer rules; disable for a host-labeled inline input row.
    pub chrome: bool,
    /// Emphasize plain @path tokens without changing text or payloads.
    pub highlight_file_references: bool,
}

impl Default for EditorView<'_> {
    fn default() -> Self {
        Self {
            title: "draft",
            placeholder: "",
            footer: "",
            presentation: Presentation::default(),
            chrome: true,
            highlight_file_references: false,
        }
    }
}

fn label(text: &str) -> String {
    // Host strings are display data too, and cannot emit raw terminal controls.
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

impl Editor {
    /// Draw a host-supplied suggestion after the actual text, without changing it.
    pub(crate) fn render_ghost(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        view: EditorView<'_>,
        text: &str,
    ) {
        if self.cursor() != self.text().len() || self.selection().is_some() || text.is_empty() {
            return;
        }
        let inner = Rect::new(
            area.x,
            area.y
                .saturating_add(u16::from(view.chrome && area.height > 0)),
            area.width,
            area.height
                .saturating_sub(if view.chrome { EDITOR_CHROME } else { 0 }),
        );
        if inner.is_empty() {
            return;
        }
        let rows = layout::rows(self.text(), self.width, self.symbols, &self.draft.atoms);
        let (y, x) = layout::position(&rows, self.cursor(), self.width, self.draft.upstream);
        let x = u16::try_from(x).unwrap_or(u16::MAX);
        let y = u16::try_from(y.saturating_sub(self.offset)).unwrap_or(u16::MAX);
        if x < inner.width && y < inner.height {
            frame.render_widget(
                Paragraph::new(label(text)).style(view.presentation.faded()),
                Rect::new(inner.x + x, inner.y + y, inner.width - x, 1),
            );
        }
    }
    pub(super) fn view(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        view: EditorView<'_>,
        focused: bool,
    ) {
        let p = view.presentation;
        let chrome = if view.chrome { EDITOR_CHROME } else { 0 };
        let inner = Rect::new(
            area.x,
            area.y
                .saturating_add(u16::from(view.chrome && area.height > 0)),
            area.width,
            area.height.saturating_sub(chrome),
        );
        self.width = inner.width.max(1);
        self.height = usize::from(inner.height);
        self.symbols = p.symbols;
        let rows = layout::rows(self.text(), self.width, self.symbols, &self.draft.atoms);
        let (y, x) = layout::position(&rows, self.cursor(), self.width, self.draft.upstream);
        if focused && self.height > 0 {
            let context = if self.height >= 4 {
                CURSOR_CONTEXT.min(1)
            } else {
                0
            };
            if y < self.offset + context {
                self.offset = y.saturating_sub(context);
            }
            if y >= self.offset + self.height - context {
                self.offset = y + 1 + context - self.height;
            }
        }
        self.offset = self
            .offset
            .min(rows.len().saturating_sub(self.height.max(1)));
        let selection = self.selection();
        let references = file_references(self.text(), view.highlight_file_references);
        let lines: Vec<_> = rows
            .iter()
            .skip(self.offset)
            .take(self.height)
            .map(|row| {
                Line::from(
                    row.glyphs
                        .iter()
                        .map(|g| {
                            let selected = selection
                                .as_ref()
                                .or(self.flash.as_ref())
                                .is_some_and(|r| g.start < r.end && g.end > r.start);
                            Span::styled(
                                g.display.clone(),
                                if selected {
                                    p.selection()
                                } else if g.atom && focused && g.start == self.cursor() {
                                    p.selection().patch(p.heading())
                                } else if g.atom {
                                    if p.tokens().surface.is_some() {
                                        p.surface().patch(p.muted())
                                    } else {
                                        p.muted()
                                    }
                                } else if references.iter().any(|range| range.contains(&g.start)) {
                                    p.tokens().info.map_or(p.base(), |color| p.base().fg(color))
                                } else if g.control {
                                    p.muted()
                                } else {
                                    p.base()
                                },
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines).style(p.base()), inner);
        if self.text().is_empty() && !view.placeholder.is_empty() {
            frame.render_widget(
                Paragraph::new(label(view.placeholder)).style(p.muted()),
                inner,
            );
        }
        if view.chrome {
            self.rules(frame, area, view, focused, rows.len());
        }
        if focused && !inner.is_empty() {
            frame.set_cursor_position((
                inner.x + u16::try_from(x).unwrap_or(0).min(inner.width - 1),
                inner.y
                    + u16::try_from(y.saturating_sub(self.offset))
                        .unwrap_or(0)
                        .min(inner.height - 1),
            ));
        }
    }

    fn rules(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        view: EditorView<'_>,
        focused: bool,
        rows: usize,
    ) {
        if area.is_empty() {
            return;
        }
        let p = view.presentation;
        let rule_style = if focused { p.accent() } else { p.muted() };
        let title_style = if focused { p.focus() } else { p.muted() };
        let above = if self.height == 0 {
            format!("{} lines", self.text().split('\n').count())
        } else if self.offset > 0 {
            format!("{} {} more", p.symbols.more(true), self.offset)
        } else {
            String::new()
        };
        frame.render_widget(
            Rule {
                left: Line::from(Span::styled(label(view.title), title_style)),
                right: Line::from(Span::styled(above, p.muted())),
                glyph: p.symbols.rule(),
                style: rule_style,
            },
            Rect::new(area.x, area.y, area.width, 1),
        );
        if area.height < 2 {
            return;
        }
        let below = rows.saturating_sub(self.offset + self.height);
        let mut metadata = Vec::new();
        if below > 0 {
            metadata.push(format!("{} {below} more", p.symbols.more(false)));
        }
        let bytes = self.draft.expanded_size();
        let near_limit = self.limit > 0 && bytes >= self.limit - self.limit / 10;
        if focused {
            metadata.push(self.footer(view));
        } else if self.rejected || near_limit {
            metadata.push(format!("{bytes} / {} bytes", self.limit));
        } else if let Some(selection) = self.selection() {
            let selected = &self.text()[selection];
            metadata.push(format!(
                "{} selected{}{} lines",
                selected.graphemes(true).count(),
                p.symbols.separator(),
                selected.split('\n').count()
            ));
        }
        let footer_style = if self.rejected || bytes >= self.limit {
            p.error()
        } else if near_limit {
            let style = p.warning();
            if focused {
                style
            } else {
                style.remove_modifier(ratatui::style::Modifier::BOLD)
            }
        } else {
            p.muted()
        };
        frame.render_widget(
            Rule {
                left: Line::default(),
                right: Line::from(Span::styled(
                    metadata.join(p.symbols.separator()),
                    footer_style,
                )),
                glyph: p.symbols.rule(),
                style: rule_style,
            },
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }

    fn footer(&self, view: EditorView<'_>) -> String {
        if let Some(notice) = self.notice() {
            return label(notice).replace(" · ", view.presentation.symbols.separator());
        }
        if self.rejected {
            return format!(
                "byte limit reached{}{} / {} bytes",
                view.presentation.symbols.separator(),
                self.draft.expanded_size(),
                self.limit
            );
        }
        if self.limit > 0 && self.draft.expanded_size() >= self.limit - self.limit / 10 {
            return format!("{} / {} bytes", self.draft.expanded_size(), self.limit);
        }
        if !view.footer.is_empty() {
            return label(view.footer);
        }
        if self.text().is_empty() {
            return String::new();
        }
        let prefix = &self.text()[..self.cursor()];
        let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
        let column = prefix
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .graphemes(true)
            .count()
            + 1;
        let position = format!("ln {line}:{column}");
        self.selection().map_or(position.clone(), |r| {
            let selected = &self.text()[r];
            let lines = selected.split('\n').count();
            let stats = format!(
                "{} selected{}{} {}",
                selected.graphemes(true).count(),
                view.presentation.symbols.separator(),
                lines,
                if lines == 1 { "line" } else { "lines" }
            );
            let full = format!("{stats}{}{position}", view.presentation.symbols.separator());
            if unicode_width::UnicodeWidthStr::width(full.as_str()) <= usize::from(self.width) {
                full
            } else {
                stats
            }
        })
    }
}

fn file_references(text: &str, enabled: bool) -> Vec<std::ops::Range<usize>> {
    if !enabled {
        return Vec::new();
    }
    text.match_indices('@')
        .filter_map(|(start, _)| {
            if start > 0
                && !text[..start]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace)
            {
                return None;
            }
            let tail = &text[start + 1..];
            let end = start + 1 + tail.find(char::is_whitespace).unwrap_or(tail.len());
            (end > start + 1).then_some(start..end)
        })
        .collect()
}
