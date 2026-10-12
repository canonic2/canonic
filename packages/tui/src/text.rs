//! Normalizes display text and bounds UTF-8 input without terminal control sequences.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) fn elide(text: &str, width: u16, symbols: crate::theme::Symbols) -> String {
    let width = usize::from(width);
    if text.width() <= width {
        return text.to_owned();
    }
    let marker = if symbols == crate::theme::Symbols::Ascii {
        "..."
    } else {
        "⋯"
    };
    if width < marker.width() {
        return String::new();
    }
    let budget = width.saturating_sub(marker.width());
    let mut output = String::new();
    let mut used = 0;
    for g in text.graphemes(true) {
        if used + g.width() > budget {
            break;
        }
        output.push_str(g);
        used += g.width();
    }
    output.push_str(marker);
    output
}

pub(crate) fn clean(text: &str, limit: usize, multiline: bool) -> String {
    let mut output = String::new();
    for character in text
        .chars()
        .filter(|character| !character.is_control() || (multiline && *character == '\n'))
    {
        if output.len() + character.len_utf8() > limit {
            break;
        }
        output.push(character);
    }
    output
}

pub(crate) fn wrapped(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width).max(1);
    let mut rows = Vec::new();
    for line in text.split('\n') {
        let mut row = String::new();
        let mut used = 0;
        for grapheme in line.graphemes(true) {
            let cells = grapheme.width();
            if used + cells > width && !row.is_empty() {
                rows.push(std::mem::take(&mut row));
                used = 0;
            }
            if cells > width {
                row.push('\u{fffd}');
                used += 1;
            } else {
                row.push_str(grapheme);
                used += cells;
            }
        }
        rows.push(row);
    }
    rows
}
