//! Owns centered panel geometry, faded backgrounds and shared modal chrome.

use super::{Layer, Size, Stack};
use crate::theme::tokens::spacing::{DIALOG_PANEL_GAP, DIALOG_SCOPE_WIDTH, DIALOG_WIDTH};
use crate::{
    text::{clean, wrapped},
    theme::{ColorMode, Presentation, Symbols},
    ui::Rule,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Caller-owned labels; keys belong to the footer, never the title.
#[derive(Clone, Debug, Default)]
pub struct DialogView {
    /// Optional source/scope, omitted at narrow widths.
    pub scope: String,
    /// Optional explanation, wrapped to at most three rows.
    pub message: String,
    /// Render the host's failure message with the semantic error role.
    pub message_error: bool,
    /// Named actions, e.g. `enter rename · esc cancel`.
    pub footer: String,
    /// Abbreviated actions at narrow widths.
    pub compact_footer: String,
}
impl DialogView {
    pub(super) fn sanitized(mut self) -> Self {
        self.scope = clean(&self.scope, 128, false);
        self.message = clean(&self.message, 4096, true);
        self.footer = clean(&self.footer, 512, false);
        self.compact_footer = clean(&self.compact_footer, 128, false);
        self
    }
}
impl<B> Layer<B> {
    pub(super) fn geometry(&self) -> Size {
        Size {
            height: self.rendered_height.unwrap_or(self.size.height),
            ..self.size
        }
    }
}
pub(super) fn outer(area: Rect, size: Size) -> Rect {
    if area.height < 3 {
        return area;
    }
    let width = area
        .width
        .saturating_sub(4)
        .max(1)
        .min(area.width)
        .min(size.width.clamp(1, DIALOG_WIDTH));
    let limit = area
        .height
        .saturating_sub(DIALOG_PANEL_GAP)
        .max(1)
        .min(area.height);
    let height = size.height.max(1).min(limit);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}
fn boxed(p: Presentation) -> bool {
    matches!(p.colors, ColorMode::Ansi16 | ColorMode::NoColor)
}
fn inset(area: Rect) -> Rect {
    let sides = u16::from(area.width >= 3);
    Rect::new(
        area.x + sides,
        area.y,
        area.width.saturating_sub(sides * 2),
        area.height,
    )
}
fn message(view: &DialogView, width: u16) -> Vec<String> {
    if view.message.is_empty() {
        Vec::new()
    } else {
        wrapped(&view.message, width.max(1))
            .into_iter()
            .take(3)
            .collect()
    }
}
impl<B, T: Clone + Eq> Stack<B, T> {
    /// Last measured active geometry, clamped to the current host stage.
    #[must_use]
    pub fn area(&self, viewport: Rect) -> Option<Rect> {
        self.layers.last().map(|l| outer(viewport, l.geometry()))
    }
    /// Compatibility rendering using the supplied preferred height.
    /// Use `render_with` for content-driven geometry.
    pub fn render(
        &mut self,
        frame: &mut Frame<'_>,
        viewport: Rect,
        p: Presentation,
        mut body: impl FnMut(&mut B, &mut Frame<'_>, Rect, Option<&T>),
    ) {
        let height = self
            .layers
            .last()
            .map_or(1, |l| l.size.height.saturating_sub(2));
        self.render_with(
            frame,
            viewport,
            p,
            |_, _, _| height,
            |b, f, a, t, _| {
                body(b, f, a, t);
                None
            },
        );
    }
    /// Measure and render only the top body. Callbacks receive its usable width
    /// and whether fallback boxing requires component chrome. The body returns
    /// an optional title scroll hint. The host supplies its visible screen.
    /// Visible rows already drawn on the frame fade while a dialog is open.
    pub fn render_with(
        &mut self,
        frame: &mut Frame<'_>,
        viewport: Rect,
        p: Presentation,
        measure: impl FnOnce(&B, u16, bool) -> u16,
        body: impl FnOnce(&mut B, &mut Frame<'_>, Rect, Option<&T>, bool) -> Option<String>,
    ) {
        let separator = if p.symbols == Symbols::Ascii {
            " > "
        } else {
            " › "
        };
        let trail = self
            .layers
            .iter()
            .take(self.len().saturating_sub(1))
            .map(|l| l.title.as_str())
            .collect::<Vec<_>>()
            .join(separator);
        let Some(layer) = self.layers.last_mut() else {
            return;
        };
        let border = boxed(p) && viewport.height >= 3;
        let width = inset(outer(viewport, layer.size)).width;
        let messages = if viewport.height < 3 {
            Vec::new()
        } else {
            message(&layer.view, width)
        };
        layer.rendered_height = Some(
            measure(&layer.body, width, border)
                .saturating_add(u16::try_from(messages.len()).unwrap_or(3))
                .saturating_add(2),
        );
        let area = outer(viewport, layer.geometry());
        if area.is_empty() {
            return;
        }
        let screen = frame.area();
        frame.buffer_mut().set_style(screen, p.faded());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(
                p.base()
                    .fg(Color::Reset)
                    .bg(p.tokens().surface.unwrap_or(Color::Reset))
                    .remove_modifier(Modifier::DIM | Modifier::REVERSED),
            ),
            area,
        );
        let columns = inset(area);
        let content = if area.height >= 3 {
            Rect::new(columns.x, area.y + 1, columns.width, area.height - 2)
        } else {
            area
        };
        let message_h = u16::try_from(messages.len())
            .unwrap_or(3)
            .min(content.height.saturating_sub(1));
        frame.render_widget(
            Paragraph::new(
                messages
                    .into_iter()
                    .take(usize::from(message_h))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .style(if layer.view.message_error {
                p.error().add_modifier(Modifier::BOLD)
            } else {
                p.base()
            }),
            Rect::new(content.x, content.y, content.width, message_h),
        );
        let body_area = Rect::new(
            content.x,
            content.y + message_h,
            content.width,
            content.height - message_h,
        );
        let hint = body(
            &mut layer.body,
            frame,
            body_area,
            self.focus.active(),
            border,
        )
        .map(|hint| clean(&hint, 128, false));
        if area.height >= 3 {
            chrome(frame, area, p, layer, &trail, hint.as_deref());
        }
    }
}
fn chrome<B>(
    frame: &mut Frame<'_>,
    area: Rect,
    p: Presentation,
    layer: &Layer<B>,
    trail: &str,
    hint: Option<&str>,
) {
    let border = boxed(p);
    if border && area.width >= 3 {
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_style(p.accent()),
            area,
        );
        if p.symbols == Symbols::Ascii {
            for y in area.y + 1..area.bottom() - 1 {
                for x in [area.x, area.right() - 1] {
                    frame.buffer_mut()[(x, y)].set_symbol("|");
                }
            }
            for (x, y) in [
                (area.x, area.y),
                (area.right() - 1, area.y),
                (area.x, area.bottom() - 1),
                (area.right() - 1, area.bottom() - 1),
            ] {
                frame.buffer_mut()[(x, y)].set_symbol("+");
            }
        }
    }
    let mut columns = inset(area);
    if border && area.width >= 5 {
        for y in [area.y, area.bottom() - 1] {
            for x in [columns.x, columns.right() - 1] {
                frame.buffer_mut()[(x, y)].set_symbol(" ");
            }
        }
        columns.x += 1;
        columns.width -= 2;
    }
    let separator = if p.symbols == Symbols::Ascii {
        " > "
    } else {
        " › "
    };
    let scope = hint.unwrap_or(if area.width >= DIALOG_SCOPE_WIDTH {
        &layer.view.scope
    } else {
        ""
    });
    let budget = usize::from(columns.width)
        .saturating_sub(scope.width() + usize::from(!scope.is_empty()))
        .saturating_sub(layer.title.width() + separator.width());
    let mut prefix = String::new();
    for grapheme in trail.graphemes(true) {
        if prefix.width() + grapheme.width() > budget {
            break;
        }
        prefix.push_str(grapheme);
    }
    let title = Line::from(vec![
        Span::styled(
            if prefix.is_empty() {
                String::new()
            } else {
                format!("{prefix}{separator}")
            },
            p.muted(),
        ),
        Span::styled(layer.title.as_str(), p.heading()),
    ]);
    frame.render_widget(
        Rule {
            left: title,
            right: Line::from(Span::styled(scope, p.muted())),
            glyph: p.symbols.rule(),
            style: p.accent(),
        },
        Rect::new(columns.x, area.y, columns.width, 1),
    );
    let footer = if area.width < DIALOG_SCOPE_WIDTH && !layer.view.compact_footer.is_empty() {
        &layer.view.compact_footer
    } else if !layer.view.footer.is_empty() {
        &layer.view.footer
    } else if layer.close.escape {
        "esc cancel"
    } else {
        "host closes"
    };
    frame.render_widget(
        Rule {
            left: Line::default(),
            right: Line::from(Span::styled(footer, p.muted())),
            glyph: p.symbols.rule(),
            style: p.accent(),
        },
        Rect::new(columns.x, area.bottom() - 1, columns.width, 1),
    );
}
