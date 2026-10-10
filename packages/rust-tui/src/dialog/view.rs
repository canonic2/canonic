//! Owns modal geometry and shared rule chrome; caller bodies own scrolling and actions.

use super::{Size, Stack};
use crate::{theme::Presentation, ui::Rule};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Clear},
};

pub(super) fn outer(area: Rect, size: Size) -> Rect {
    let width = area.width.min(size.width.max(1));
    let height = area.height.min(size.height.max(1));
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

impl<B, T: Clone + Eq> Stack<B, T> {
    /// Active outer geometry for pointer routing or host composition.
    #[must_use]
    pub fn area(&self, viewport: Rect) -> Option<Rect> {
        self.layers.last().map(|l| outer(viewport, l.size))
    }
    /// Draw layers from bottom to top. Only the active body receives a focus
    /// target; hosts must render their background with focus disabled while open.
    /// At fewer than three rows the body gets the whole region without chrome.
    pub fn render(
        &mut self,
        frame: &mut Frame<'_>,
        viewport: Rect,
        p: Presentation,
        mut body: impl FnMut(&mut B, &mut Frame<'_>, Rect, Option<&T>),
    ) {
        let count = self.layers.len();
        for (i, layer) in self.layers.iter_mut().enumerate() {
            let active = i + 1 == count;
            let area = outer(viewport, layer.size);
            if area.is_empty() {
                continue;
            }
            frame.render_widget(Clear, area);
            frame.render_widget(Block::default().style(p.base()), area);
            let inner = if area.height >= 3 {
                Rect::new(area.x, area.y + 1, area.width, area.height - 2)
            } else {
                area
            };
            body(
                &mut layer.body,
                frame,
                inner,
                if active { self.focus.active() } else { None },
            );
            if area.height >= 3 {
                let style = if active { p.focus() } else { p.muted() };
                frame.render_widget(
                    Rule {
                        left: Line::from(Span::styled(layer.title.as_str(), style)),
                        right: Line::default(),
                        glyph: p.symbols.rule(),
                        style,
                    },
                    Rect::new(area.x, area.y, area.width, 1),
                );
                let hint = if layer.close.escape {
                    "esc close"
                } else {
                    "host closes"
                };
                frame.render_widget(
                    Rule {
                        left: Line::default(),
                        right: Line::from(Span::styled(hint, p.muted())),
                        glyph: p.symbols.rule(),
                        style,
                    },
                    Rect::new(area.x, area.bottom() - 1, area.width, 1),
                );
            }
        }
    }
}
