//! Owns shared presentation primitives, independent of editor state and hosts.

use ratatui::{buffer::Buffer, layout::Rect, style::Style, text::Line, widgets::Widget};

/// Copy-safe horizontal rule with optional heading and metadata.
pub struct Rule<'a> {
    /// Heading.
    pub left: Line<'a>,
    /// Metadata; takes priority at narrow widths.
    pub right: Line<'a>,
    /// One-cell glyph from symbol tokens.
    pub glyph: char,
    /// Rule style.
    pub style: Style,
}

impl Widget for Rule<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let right = u16::try_from(self.right.width())
            .unwrap_or(u16::MAX)
            .min(area.width);
        let left_budget = area
            .width
            .saturating_sub(right.saturating_add(u16::from(right > 0)));
        let left = u16::try_from(self.left.width())
            .unwrap_or(u16::MAX)
            .min(left_budget);
        let symbol = self.glyph.to_string();
        for x in area.x..area.right() {
            buffer[(x, area.y)]
                .set_symbol(&symbol)
                .set_style(self.style);
        }
        if left > 0 {
            self.left.render(Rect::new(area.x, area.y, left, 1), buffer);
            if left < left_budget {
                buffer[(area.x + left, area.y)].set_symbol(" ");
            }
        }
        if right > 0 {
            let x = area.right() - right;
            if x > area.x {
                buffer[(x - 1, area.y)].set_symbol(" ");
            }
            self.right.render(Rect::new(x, area.y, right, 1), buffer);
        }
    }
}
