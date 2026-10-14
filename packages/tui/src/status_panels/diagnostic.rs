//! Settled command feedback using the notice severity palette, without terminal ownership.
use super::Severity;
use crate::{
    text::{wrapped, wrapped_prose},
    theme::{ColorMode, Presentation},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::Paragraph,
};
use std::io::{self, Write};

/// Host-authored feedback. Context and suggestions describe facts; they never run actions.
/// Each field is limited to 64 KiB of safe display text, with an explicit truncation
/// marker. Up to sixteen context fields and eight suggestions are displayed.
pub struct Diagnostic<'a> {
    pub severity: Severity,
    pub title: &'a str,
    pub message: &'a str,
    pub context: &'a [(&'a str, &'a str)],
    pub help: &'a [&'a str],
}
impl Diagnostic<'_> {
    /// Wrapped, sanitized rows, independent of terminal input or environment.
    #[must_use]
    pub fn lines(&self, width: u16, p: Presentation) -> Vec<Line<'static>> {
        let (name, style) = match self.severity {
            Severity::Error => ("Error", p.error()),
            Severity::Warning => ("Warning", p.warning()),
            Severity::Success => (
                "Success",
                p.tokens()
                    .success
                    .map_or(p.heading(), |color| p.heading().fg(color)),
            ),
            Severity::Info => ("Info", p.info()),
        };
        let mut rows = wrapped_prose(&format!("{name}: {}", safe(self.title, false)), width)
            .into_iter()
            .map(|s| Line::styled(s, style.add_modifier(Modifier::BOLD)))
            .collect::<Vec<_>>();
        rows.push(Line::default());
        rows.extend(
            wrapped_prose(&safe(self.message, true), width)
                .into_iter()
                .map(Line::raw),
        );
        if !self.context.is_empty() {
            rows.push(Line::default());
            for (key, value) in self.context.iter().take(16) {
                rows.extend(
                    wrapped(
                        &format!("{}: {}", safe(key, false), safe(value, false)),
                        width,
                    )
                    .into_iter()
                    .map(|s| Line::styled(s, p.muted())),
                );
            }
            if self.context.len() > 16 {
                rows.extend(
                    wrapped_prose("[additional context omitted]", width)
                        .into_iter()
                        .map(Line::raw),
                );
            }
        }
        if !self.help.is_empty() {
            rows.push(Line::default());
            rows.extend(
                wrapped_prose("Next", width)
                    .into_iter()
                    .map(|s| Line::styled(s, p.focus())),
            );
            for item in self.help.iter().take(8) {
                rows.extend(
                    wrapped_prose(&format!("- {}", safe(item, true)), width)
                        .into_iter()
                        .map(Line::raw),
                );
            }
            if self.help.len() > 8 {
                rows.extend(
                    wrapped_prose("[additional suggestions omitted]", width)
                        .into_iter()
                        .map(Line::raw),
                );
            }
        }
        rows
    }
    /// Render into caller-owned rows. Hosts own clipping or scrolling through `lines`.
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, p: Presentation) {
        frame.render_widget(Paragraph::new(self.lines(area.width, p)), area);
    }
    /// Write permanent normal-screen output. This acquires no terminal lease, changes
    /// no modes or cursor positioning, reads no environment and never waits for acknowledgement.
    /// `NoColor` writes literal text only; other profiles use semantic styles.
    /// # Errors
    /// Propagates writer failures; the host retains the original operation error.
    pub fn write(&self, writer: &mut impl Write, width: u16, p: Presentation) -> io::Result<()> {
        let result = write_rows(writer, self.lines(width, p), p.colors != ColorMode::NoColor);
        if result.is_err() && p.colors != ColorMode::NoColor {
            let _ = writer.write_all(b"\x1b[0m");
        }
        result
    }
}
fn write_rows(writer: &mut impl Write, rows: Vec<Line<'static>>, styled: bool) -> io::Result<()> {
    for line in rows {
        for span in line.spans {
            if styled {
                write!(writer, "{}{}\x1b[0m", ansi(span.style), span.content)?;
            } else {
                write!(writer, "{}", span.content)?;
            }
        }
        writeln!(writer)?;
    }
    Ok(())
}
// Encode explicit presentation only; backend command dispatch could query or
// modify a Windows console, which a caller-owned writer must never do.
fn ansi(style: Style) -> String {
    let mut codes = vec!["0".to_owned()];
    if style.add_modifier.contains(Modifier::BOLD) {
        codes.push("1".into());
    }
    if style.add_modifier.contains(Modifier::DIM) {
        codes.push("2".into());
    }
    if let Some(color) = style.fg {
        let code = match color {
            Color::Reset => "39".into(),
            Color::Black => "30".into(),
            Color::Red => "31".into(),
            Color::Green => "32".into(),
            Color::Yellow => "33".into(),
            Color::Blue => "34".into(),
            Color::Magenta => "35".into(),
            Color::Cyan => "36".into(),
            Color::Gray => "37".into(),
            Color::DarkGray => "90".into(),
            Color::LightRed => "91".into(),
            Color::LightGreen => "92".into(),
            Color::LightYellow => "93".into(),
            Color::LightBlue => "94".into(),
            Color::LightMagenta => "95".into(),
            Color::LightCyan => "96".into(),
            Color::White => "97".into(),
            Color::Indexed(n) => format!("38;5;{n}"),
            Color::Rgb(r, g, b) => format!("38;2;{r};{g};{b}"),
        };
        codes.push(code);
    }
    format!("\x1b[{}m", codes.join(";"))
}
fn safe(text: &str, multiline: bool) -> String {
    let mut output = String::new();
    for c in text.chars() {
        let escaped = if (c.is_control() && !(multiline && c == '\n'))
            || matches!(c,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
        {
            c.escape_default().to_string()
        } else {
            c.to_string()
        };
        if output.len() + escaped.len() > 65_536 {
            output.push_str(" [truncated]");
            break;
        }
        output.push_str(&escaped);
    }
    output
}
