//! Derives word-wrapped rows and safe display glyphs without changing stored text.

use crate::theme::{Symbols, tokens::spacing::TAB_STOP};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) struct Glyph {
    pub start: usize,
    pub end: usize,
    pub display: String,
    pub cells: usize,
    pub control: bool,
}
pub(super) struct Row {
    pub start: usize,
    pub end: usize,
    pub glyphs: Vec<Glyph>,
}
impl Row {
    pub(super) fn at_column(&self, goal: usize) -> usize {
        let mut used = 0;
        for glyph in &self.glyphs {
            if used + glyph.cells > goal {
                return glyph.start;
            }
            used += glyph.cells;
        }
        self.end
    }
}
fn display(grapheme: &str, column: usize, symbols: Symbols) -> String {
    if grapheme == "\t" {
        return " ".repeat(TAB_STOP - column % TAB_STOP);
    }
    grapheme
        .chars()
        .map(|c| {
            if !c.is_control() {
                c.to_string()
            } else if symbols == Symbols::Unicode && (c <= '\u{1f}' || c == '\u{7f}') {
                char::from_u32(if c == '\u{7f}' {
                    0x2421
                } else {
                    0x2400 + u32::from(c)
                })
                .unwrap_or('�')
                .to_string()
            } else {
                format!("<{:02X}>", u32::from(c))
            }
        })
        .collect()
}
fn push(rows: &mut Vec<Row>, row: &mut Row, start: usize) {
    rows.push(std::mem::replace(
        row,
        Row {
            start,
            end: start,
            glyphs: Vec::new(),
        },
    ));
}
pub(super) fn rows(text: &str, width: u16, symbols: Symbols) -> Vec<Row> {
    let width = usize::from(width).max(1);
    let mut rows = Vec::new();
    let mut row = Row {
        start: 0,
        end: 0,
        glyphs: Vec::new(),
    };
    let mut used = 0;
    for (segment_start, segment) in text.split_word_bound_indices() {
        let segment_cells: usize = segment
            .graphemes(true)
            .map(|g| display(g, 0, symbols).width())
            .sum();
        if !segment.chars().all(char::is_whitespace)
            && segment_cells <= width
            && used > 0
            && used + segment_cells > width
        {
            push(&mut rows, &mut row, segment_start);
            used = 0;
        }
        for (relative, grapheme) in segment.grapheme_indices(true) {
            let start = segment_start + relative;
            let end = start + grapheme.len();
            if grapheme == "\n" {
                push(&mut rows, &mut row, end);
                used = 0;
                continue;
            }
            let mut displayed = display(grapheme, used, symbols);
            let mut cells = displayed.width();
            if used + cells > width && !row.glyphs.is_empty() {
                push(&mut rows, &mut row, start);
                used = 0;
                displayed = display(grapheme, 0, symbols);
                cells = displayed.width();
            }
            if cells > width {
                if grapheme == "\t" {
                    displayed = " ".repeat(width);
                    cells = width;
                } else {
                    displayed = if symbols == Symbols::Ascii {
                        "?"
                    } else {
                        "�"
                    }
                    .into();
                    cells = 1;
                }
            }
            row.glyphs.push(Glyph {
                start,
                end,
                display: displayed,
                cells,
                control: grapheme.chars().any(char::is_control),
            });
            row.end = end;
            used += cells;
        }
    }
    let end = row.end;
    rows.push(row);
    if used >= width {
        rows.push(Row {
            start: end,
            end,
            glyphs: Vec::new(),
        });
    }
    rows
}
pub(super) fn position(rows: &[Row], cursor: usize, width: u16, upstream: bool) -> (usize, usize) {
    let y = rows
        .iter()
        .rposition(|row| row.start < cursor || (!upstream && row.start == cursor))
        .unwrap_or(0);
    let x = rows[y]
        .glyphs
        .iter()
        .take_while(|g| g.end <= cursor)
        .map(|g| g.cells)
        .sum();
    if !upstream && x >= usize::from(width).max(1) && y + 1 < rows.len() {
        (y + 1, 0)
    } else {
        (y, x)
    }
}
pub(super) fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
