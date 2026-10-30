//! Copy-safe settled CLI documents: measured tables, wrapping and inert text.
use crate::text::wrapped_prose;
use std::fmt::Write as _;
use unicode_width::UnicodeWidthStr;

/// Strip whole terminal escape sequences and expose invisible direction controls.
/// Newlines remain prose; other controls never reach the terminal.
#[must_use]
pub fn sanitize(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    let mut escape = false;
                    for c in chars.by_ref() {
                        if c == '\x07' || c == '\u{009c}' || escape && c == '\\' {
                            break;
                        }
                        escape = c == '\x1b';
                    }
                }
                Some(c) if (' '..='/').contains(&c) => {
                    for c in chars.by_ref() {
                        if ('0'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\u{009b}' => {
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            '\u{009d}' | '\u{0090}' => {
                let mut escape = false;
                for c in chars.by_ref() {
                    if c == '\x07' || c == '\u{009c}' || escape && c == '\\' {
                        break;
                    }
                    escape = c == '\x1b';
                }
            }
            '\u{061c}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}' => {
                let _ = write!(result, "<U+{:04X}>", u32::from(c));
            }
            '\n' => result.push(c),
            c if c.is_control() => {
                if c == '\t' {
                    result.push(' ');
                }
            }
            c => result.push(c),
        }
    }
    result
}

/// One settled output block. Hosts supply facts and decide when to print them.
#[derive(Clone, Debug)]
pub enum Block {
    /// A section title.
    Heading(String),
    /// Wrapped prose.
    Text(String),
    /// Labeled values, stacked when narrow.
    Fields(Vec<(String, String)>),
    /// Column headers and rows; narrow terminals show labeled records.
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

/// A terminal-independent document. Rendering performs no I/O or environment reads.
#[derive(Default, Debug)]
pub struct Document(pub Vec<Block>);

impl Document {
    /// Render every supplied fact within the host's cell width, without escape bytes.
    #[must_use]
    pub fn render(&self, width: u16) -> String {
        self.render_with(
            width,
            crate::theme::Presentation {
                colors: crate::theme::ColorMode::NoColor,
                ..crate::theme::Presentation::default()
            },
        )
    }
    /// Render with semantic heading accents for an explicitly interactive output.
    /// The caller must use plain `render` for redirected or machine output.
    #[must_use]
    pub fn render_with(&self, width: u16, presentation: crate::theme::Presentation) -> String {
        let width = width.max(1);
        let mut output = String::new();
        for block in &self.0 {
            match block {
                Block::Heading(text) => {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    let color = match presentation.tokens().accent {
                        Some(ratatui::style::Color::Rgb(r, g, b)) => {
                            Some(crossterm::style::Color::Rgb { r, g, b })
                        }
                        Some(ratatui::style::Color::Indexed(index)) => {
                            Some(crossterm::style::Color::AnsiValue(index))
                        }
                        Some(ratatui::style::Color::Cyan) => Some(crossterm::style::Color::Cyan),
                        _ => None,
                    };
                    if let Some(color) = color {
                        let _ = write!(
                            output,
                            "{}{}",
                            crossterm::style::SetForegroundColor(color),
                            crossterm::style::SetAttribute(crossterm::style::Attribute::Bold)
                        );
                    }
                    prose(&mut output, text, width, "");
                    if color.is_some() {
                        let _ = write!(
                            output,
                            "{}{}",
                            crossterm::style::SetForegroundColor(crossterm::style::Color::Reset),
                            crossterm::style::SetAttribute(
                                crossterm::style::Attribute::NormalIntensity
                            )
                        );
                    }
                }
                Block::Text(text) => prose(&mut output, text, width, "  "),
                Block::Fields(fields) => fields_text(&mut output, fields, width),
                Block::Table { headers, rows } => table(&mut output, headers, rows, width),
            }
        }
        output
    }
}
fn prose(output: &mut String, text: &str, width: u16, indent: &str) {
    let indent = if usize::from(width) > indent.len() {
        indent
    } else {
        ""
    };
    for row in wrapped_prose(
        &sanitize(text),
        width
            .saturating_sub(u16::try_from(indent.len()).unwrap_or(width))
            .max(1),
    ) {
        output.push_str(indent);
        output.push_str(&row);
        output.push('\n');
    }
}
fn fields_text(output: &mut String, fields: &[(String, String)], width: u16) {
    let label_width = fields
        .iter()
        .map(|(label, _)| sanitize(label).width())
        .max()
        .unwrap_or(0)
        .min(24);
    for (label, value) in fields {
        let label = sanitize(label).replace('\n', " ");
        if usize::from(width) >= label_width + 24 && label.width() <= label_width {
            let prefix = format!(
                "  {label}{}  ",
                " ".repeat(label_width.saturating_sub(label.width()))
            );
            for (i, row) in wrapped_prose(
                &sanitize(value),
                width
                    .saturating_sub(u16::try_from(prefix.width()).unwrap_or(width))
                    .max(1),
            )
            .iter()
            .enumerate()
            {
                output.push_str(if i == 0 { &prefix } else { "" });
                if i > 0 {
                    output.push_str(&" ".repeat(prefix.width()));
                }
                output.push_str(row);
                output.push('\n');
            }
        } else {
            prose(output, &format!("{label}:"), width, "  ");
            prose(output, value, width, "    ");
        }
    }
}
fn table(output: &mut String, headers: &[String], rows: &[Vec<String>], width: u16) {
    let mut headers: Vec<_> = headers
        .iter()
        .map(|text| sanitize(text).replace('\n', " "))
        .collect();
    // Cells beyond the headers are still facts: label them rather than drop them.
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    for index in headers.len()..columns {
        headers.push(format!("Column {}", index + 1));
    }
    let rows: Vec<Vec<_>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|text| sanitize(text).replace('\n', " "))
                .collect()
        })
        .collect();
    let sizes: Vec<_> = headers
        .iter()
        .enumerate()
        .map(|(i, text)| {
            rows.iter()
                .filter_map(|row| row.get(i))
                .map(|v| v.width())
                .max()
                .unwrap_or(0)
                .max(text.width())
        })
        .collect();
    let total = sizes.iter().sum::<usize>() + sizes.len().saturating_sub(1) * 2 + 2;
    if total > usize::from(width) {
        for (index, row) in rows.iter().enumerate() {
            if index > 0 {
                output.push('\n');
            }
            let fields = headers
                .iter()
                .enumerate()
                .map(|(i, label)| (label.clone(), row.get(i).cloned().unwrap_or_default()))
                .collect::<Vec<_>>();
            fields_text(output, &fields, width);
        }
    } else {
        for row in std::iter::once(&headers).chain(rows.iter()) {
            output.push_str("  ");
            for (i, size) in sizes.iter().enumerate() {
                let value = row.get(i).map_or("", String::as_str);
                output.push_str(value);
                if i + 1 < sizes.len() {
                    output.push_str(&" ".repeat(size.saturating_sub(value.width()) + 2));
                }
            }
            output.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn documents_keep_fields_and_wide_text_when_narrow() {
        let doc = Document(vec![Block::Table {
            headers: vec!["Name".into(), "ID".into()],
            rows: vec![vec!["東京".into(), "simulator:ACME-123456".into()]],
        }]);
        let wide = doc.render(100);
        assert!(wide.contains("東京  simulator:ACME-123456"));
        let narrow = doc.render(16);
        assert!(narrow.contains("Name:") && narrow.contains("東京") && narrow.contains("ID:"));
        assert!(narrow.lines().all(|line| line.width() <= 16));
    }
    #[test]
    fn cells_beyond_the_headers_are_labeled_instead_of_dropped() {
        let doc = Document(vec![Block::Table {
            headers: vec!["Name".into()],
            rows: vec![vec!["acme".into(), "extra-fact".into()]],
        }]);
        // 16 cells stack the record; the wide layout needs 18.
        for width in [100, 16] {
            let text = doc.render(width);
            assert!(text.contains("extra-fact"), "{text}");
            assert!(text.contains("Column 2"), "{text}");
        }
    }
    #[test]
    fn terminal_sequences_are_removed_whole_and_invisibles_are_visible() {
        assert_eq!(
            sanitize("ok\x1b[31m red\x1b[0m\x1b]52;c;secret\x07 end\x1bPdiscard\x1b\\"),
            "ok red end"
        );
        assert_eq!(sanitize("a\u{009b}2Jb\u{202e}c"), "ab<U+202E>c");
        assert_eq!(
            sanitize("a\u{061c}b\u{2028}c\u{2029}d\u{2060}e\u{2064}f"),
            "a<U+061C>b<U+2028>c<U+2029>d<U+2060>e<U+2064>f"
        );
    }
}
