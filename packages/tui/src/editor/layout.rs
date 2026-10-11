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
    pub atom: bool,
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
fn display(
    grapheme: &str,
    column: usize,
    symbols: Symbols,
    atoms: &std::collections::BTreeMap<char, super::InlineAtom>,
) -> String {
    if let Some(atom) = grapheme.chars().next().and_then(|c| atoms.get(&c)) {
        return atom.label.clone();
    }
    if grapheme == "\t" {
        return " ".repeat(TAB_STOP - column % TAB_STOP);
    }
    grapheme
        .chars()
        .map(|c| {
            if !c.is_control() {
                c.to_string()
            } else if symbols == Symbols::Ascii && c == '\u{1b}' {
                "^[".into()
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
// Unicode word boundaries can split a grapheme (for example a prepend character
// followed by whitespace). Merge those segments before deriving cursor rows.
fn segments(text: &str) -> Vec<(usize, &str)> {
    let ends: Vec<_> = text
        .grapheme_indices(true)
        .map(|(i, g)| i + g.len())
        .collect();
    let mut start = 0;
    let mut segments = Vec::new();
    for (index, word) in text.split_word_bound_indices() {
        let end = index + word.len();
        if ends.binary_search(&end).is_ok() {
            segments.push((start, &text[start..end]));
            start = end;
        }
    }
    segments
}
pub(super) fn rows(
    text: &str,
    width: u16,
    symbols: Symbols,
    atoms: &std::collections::BTreeMap<char, super::InlineAtom>,
) -> Vec<Row> {
    let width = usize::from(width).max(1);
    let mut rows = Vec::new();
    let mut row = Row {
        start: 0,
        end: 0,
        glyphs: Vec::new(),
    };
    let mut used = 0;
    for (segment_start, segment) in segments(text) {
        let segment_cells: usize = segment
            .graphemes(true)
            .map(|g| display(g, 0, symbols, atoms).width())
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
            let mut displayed = display(grapheme, used, symbols, atoms);
            let mut cells = displayed.width();
            if used + cells > width && !row.glyphs.is_empty() {
                push(&mut rows, &mut row, start);
                used = 0;
                displayed = display(grapheme, 0, symbols, atoms);
                cells = displayed.width();
            }
            if cells > width {
                if grapheme == "\t" {
                    displayed = " ".repeat(width);
                    cells = width;
                } else if grapheme
                    .chars()
                    .next()
                    .is_some_and(|c| atoms.contains_key(&c))
                {
                    displayed = displayed.chars().take(width).collect();
                    cells = displayed.width();
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
                atom: grapheme
                    .chars()
                    .next()
                    .is_some_and(|c| atoms.contains_key(&c)),
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
