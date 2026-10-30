//! Projects bounded source previews into semantic rows and caller-owned frames.

use super::{
    Expansion, ToolCallCard, ToolGroupCard,
    model::{Call, Output, State},
};
use crate::theme::{ColorMode, Presentation, Symbols};
use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
};
use std::{fmt::Write, time::Duration};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Plain,
    Dim,
    Info,
    Link,
    Command,
    Success,
    Error,
    Warning,
    Accent,
    Delete,
    Add,
}
pub(crate) struct Piece {
    pub text: String,
    pub role: Role,
    pub generated: bool,
}
pub(crate) struct Row {
    pub call: Option<String>,
    pub offset: usize,
    pub prefix: String,
    pub pieces: Vec<Piece>,
    pub surface: bool,
    pub head: bool,
}
fn glyph(symbols: Symbols, unicode: &'static str, ascii: &'static str) -> &'static str {
    if symbols == Symbols::Ascii {
        ascii
    } else {
        unicode
    }
}
fn piece(text: String, role: Role) -> Piece {
    Piece {
        text,
        role,
        generated: true,
    }
}
fn duration(elapsed: Duration) -> String {
    if elapsed.is_zero() {
        String::new()
    } else if elapsed < Duration::from_secs(1) {
        format!("{:.1}s", elapsed.as_secs_f64())
    } else {
        format!("{}s", elapsed.as_secs())
    }
}
fn shortened(text: &str, width: usize, marker: &str, middle: bool) -> String {
    if text.width() <= width {
        return text.into();
    }
    if width < marker.width() {
        return ".".repeat(width);
    }
    let keep = width - marker.width();
    let mut left = String::new();
    let mut right = String::new();
    let left_budget = if middle { keep / 3 } else { keep };
    for g in text.graphemes(true) {
        if left.width() + g.width() > left_budget {
            break;
        }
        left.push_str(g);
    }
    if middle {
        for g in text.graphemes(true).rev() {
            if right.width() + g.width() > keep - left.width() {
                break;
            }
            right.insert_str(0, g);
        }
    }
    format!("{left}{marker}{right}")
}
impl ToolCallCard {
    pub(crate) fn rows_search(
        &self,
        width: u16,
        symbols: Symbols,
        offset: Option<usize>,
    ) -> Vec<Row> {
        let mut rows = self.rows(width, symbols, true);
        let Some(offset) = offset else {
            return rows;
        };
        let indent = if width < 50 { 5 } else { 9 };
        let mut source = self
            .call
            .copy_text()
            .len()
            .saturating_sub(self.call.output.text().len());
        let mut old = 0u32;
        let mut new = 0u32;
        for (index, raw) in self.call.output.text().split_inclusive('\n').enumerate() {
            if source <= offset && offset < source + raw.len() {
                let (number, sign, body, role) = output_line(
                    &self.call.output,
                    raw.trim_end_matches('\n'),
                    index,
                    &mut old,
                    &mut new,
                    self.call.state == State::Failed,
                );
                rows.truncate(1);
                rows.push(self.marker(indent, symbols, "showing loaded search match".into()));
                rows.extend(self.body_line(
                    &(source, number, sign, body, role),
                    usize::from(width),
                    indent,
                    symbols,
                    false,
                ));
                rows.push(self.marker(indent, symbols, "remaining output in details".into()));
                break;
            }
            output_line(
                &self.call.output,
                raw.trim_end_matches('\n'),
                index,
                &mut old,
                &mut new,
                false,
            );
            source += raw.len();
        }
        rows
    }
    /// Render a standalone card; input/focus and clipping remain caller-owned.
    pub fn render_with(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: Presentation,
        selected: bool,
    ) {
        let rows = self.rows(
            area.width
                .saturating_sub(u16::from(selected && theme.colors != ColorMode::NoColor)),
            theme.symbols,
            false,
        );
        render(frame, area, theme, &rows, selected);
    }
    /// Number of preview rows at the supplied width/alphabet.
    #[must_use]
    pub fn height(&self, width: u16, symbols: Symbols) -> usize {
        self.rows(width, symbols, false).len()
    }
    pub(crate) fn rows(&self, width: u16, symbols: Symbols, force: bool) -> Vec<Row> {
        let width = usize::from(width);
        let narrow = width < 50;
        let verb_width = if narrow { 4 } else { 8 };
        let indent = (verb_width + 1).min(width);
        let marker = glyph(symbols, "⋯", "...");
        let verb = if !self.call.known() {
            "tool"
        } else if narrow && self.call.verb == "search" {
            "find"
        } else {
            &self.call.verb
        };
        let (target, target_role) = target(&self.call, narrow);
        let meta = if matches!(self.call.state, State::Queued | State::Proposed) {
            if self.call.state == State::Queued {
                "queued"
            } else {
                "proposed"
            }
            .into()
        } else if self.call.state.terminal() && !narrow {
            duration(self.call.elapsed)
        } else {
            String::new()
        };
        let room = width.saturating_sub(indent + meta.width() + usize::from(!meta.is_empty()));
        let target = shortened(&target, room, marker, !narrow && target_role == Role::Info);
        let dim = matches!(self.call.state, State::Queued | State::Proposed)
            || self.call.retried.is_some();
        let verb_role = if dim || matches!(self.call.verb.as_str(), "read" | "search" | "fetch") {
            Role::Dim
        } else {
            Role::Plain
        };
        let mut pieces = vec![
            piece(format!("{verb:<verb_width$} "), verb_role),
            piece(target, if dim { Role::Dim } else { target_role }),
        ];
        let used = pieces.iter().map(|p| p.text.width()).sum::<usize>();
        if !meta.is_empty() {
            pieces.push(piece(
                format!(
                    "{}{meta}",
                    " ".repeat(width.saturating_sub(used + meta.width()))
                ),
                Role::Dim,
            ));
        }
        let mut rows = vec![Row {
            call: Some(self.call.id.clone()),
            offset: 0,
            prefix: String::new(),
            pieces,
            surface: false,
            head: true,
        }];
        let result = self.result(width.saturating_sub(indent), narrow, symbols);
        let result = Row {
            call: Some(self.call.id.clone()),
            offset: 0,
            prefix: " ".repeat(indent),
            pieces: vec![piece(result.0, result.1)],
            surface: false,
            head: false,
        };
        let failed = self.call.state == State::Failed && self.call.retried.is_none();
        let body = self.body(width, indent, symbols, force);
        if failed {
            rows.push(result);
            rows.extend(body);
        } else {
            rows.extend(body);
            rows.push(result);
        }
        if self.call.omitted_bytes > 0 {
            rows.push(self.marker(
                indent,
                symbols,
                format!("{} bytes not retained", self.call.omitted_bytes),
            ));
        }
        if !self.call.artifacts.is_empty() {
            rows.push(self.artifact_row(indent, symbols));
        }
        rows
    }
    fn artifact_row(&self, indent: usize, symbols: Symbols) -> Row {
        let mut pieces = vec![piece("artifacts ".into(), Role::Dim)];
        for (index, artifact) in self.call.artifacts.iter().enumerate() {
            if index > 0 {
                pieces.push(piece(glyph(symbols, " · ", " | ").into(), Role::Dim));
            }
            pieces.push(piece(
                format!("{} {}", artifact.label, glyph(symbols, "↗", "->")),
                if artifact.open_available {
                    Role::Link
                } else {
                    Role::Dim
                },
            ));
        }
        Row {
            call: Some(self.call.id.clone()),
            offset: 0,
            prefix: " ".repeat(indent),
            pieces,
            surface: false,
            head: false,
        }
    }
    fn result(&self, width: usize, narrow: bool, symbols: Symbols) -> (String, Role) {
        let arrow = glyph(symbols, "→", "->");
        let ok = glyph(symbols, "✓", "ok");
        let fail = glyph(symbols, "✕", "x");
        let dot = glyph(symbols, "·", "|");
        let mut text = match &self.call.state {
            State::Proposed => format!(
                "{} not started {dot} proposed by host",
                glyph(symbols, "○", "o")
            ),
            State::Queued => String::new(),
            State::Cancelling => format!("{} cancelling…", self.working(symbols)),
            State::Interrupted(reason) => format!("! no result {dot} {reason}"),
            State::Running => format!(
                "{} running{}{}",
                self.working(symbols),
                if self.call.elapsed >= Duration::from_secs(1) {
                    format!(" {dot} {}", duration(self.call.elapsed))
                } else {
                    String::new()
                },
                if self.call.output.text().is_empty() {
                    String::new()
                } else {
                    format!(" {dot} {} lines", self.call.total_lines)
                }
            ),
            State::Waiting(reason) => format!("! waiting for {reason}"),
            State::Completed => format!(
                "{arrow} {ok} {}",
                if self.call.summary.is_empty() {
                    "completed"
                } else {
                    &self.call.summary
                }
            ),
            State::Failed => format!(
                "{arrow} {fail} {}",
                if self.call.summary.is_empty() {
                    "failed"
                } else {
                    &self.call.summary
                }
            ),
            State::Cancelled(reason) => {
                format!("{} cancelled {dot} {reason}", glyph(symbols, "–", "-"))
            }
        };
        if self.call.state == State::Running {
            text = if self.call.result_unknown {
                format!(
                    "{} last seen running at {} {dot} result unknown",
                    glyph(symbols, "···", "..."),
                    duration(self.call.elapsed)
                )
            } else {
                self.progress_label(text, symbols)
            };
        }
        if symbols == Symbols::Ascii {
            text = text.replace('·', "|").replace('…', "...");
        }
        if let Output::Unavailable(reason) = &self.call.output {
            let _ = write!(text, " {dot} {reason} {dot} not shown");
        }
        if self.call.retried.is_some() {
            let _ = write!(text, " {dot} retried below");
        }
        let meta = if narrow && self.call.state.terminal() && !self.call.elapsed.is_zero() {
            format!(" {dot} {}", duration(self.call.elapsed))
        } else {
            String::new()
        };
        text = shortened(
            &text,
            width.saturating_sub(meta.width()),
            glyph(symbols, "⋯", "..."),
            false,
        );
        text.push_str(&meta);
        let role = if self.call.retried.is_some() {
            Role::Dim
        } else {
            match self.call.state {
                State::Proposed | State::Queued | State::Cancelled(_) => Role::Dim,
                State::Running | State::Cancelling => Role::Accent,
                State::Waiting(_) | State::Interrupted(_) => Role::Warning,
                State::Completed => Role::Success,
                State::Failed => Role::Error,
            }
        };
        (text, role)
    }
    fn progress_label(&self, mut text: String, symbols: Symbols) -> String {
        let dot = glyph(symbols, "·", "|");
        if let Some(progress) = &self.call.progress {
            text = if let Some(total) = progress.total {
                let filled = usize::try_from(
                    u128::from(progress.completed.min(total)) * 24 / u128::from(total),
                )
                .unwrap_or(24);
                let bar = if symbols == Symbols::Ascii {
                    format!("[{}{}]", "#".repeat(filled), "-".repeat(24 - filled))
                } else {
                    format!("{}{}", "━".repeat(filled), "─".repeat(24 - filled))
                };
                format!(
                    "{bar} {} of {total} {} {dot} {}",
                    progress.completed,
                    progress.unit,
                    duration(self.call.elapsed)
                )
            } else {
                format!(
                    "{text} {dot} {} {} so far",
                    progress.completed, progress.unit
                )
            };
        }
        if let Some(child) = &self.call.child_task {
            let _ = write!(text, " {dot} {} calls", child.total_calls);
        }
        text
    }
    fn working(&self, symbols: Symbols) -> &'static str {
        let phase = if self.reduced_motion {
            0
        } else {
            (self.elapsed.as_millis() / 140) % 3
        };
        match phase {
            1 => glyph(symbols, "·  ", ".  "),
            2 => glyph(symbols, "·· ", ".. "),
            _ => glyph(symbols, "···", "..."),
        }
    }
    fn body(&self, width: usize, indent: usize, symbols: Symbols, force: bool) -> Vec<Row> {
        if self.call.retried.is_some() {
            return vec![];
        }
        if let Some(child) = &self.call.child_task {
            return self.child_body(child, width, indent, symbols, force);
        }
        self.output_body(width, indent, symbols, force)
    }
    fn child_body(
        &self,
        child: &super::ChildTask,
        width: usize,
        indent: usize,
        symbols: Symbols,
        force: bool,
    ) -> Vec<Row> {
        let show = if force || self.expansion == Expansion::Expanded {
            10
        } else {
            2
        };
        child
            .latest_calls
            .iter()
            .rev()
            .take(show)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .flat_map(|text| {
                self.body_line(
                    &(0, None, "", text, Role::Dim),
                    width,
                    indent,
                    symbols,
                    false,
                )
            })
            .collect()
    }
    fn output_body(&self, width: usize, indent: usize, symbols: Symbols, force: bool) -> Vec<Row> {
        let text = self.displayed.text();
        if text.is_empty() {
            return vec![];
        }
        let expanded = force
            || self.expansion == Expansion::Expanded
            || (self.expansion == Expansion::Automatic
                && matches!(self.call.state, State::Waiting(_)));
        let failure = self.expansion == Expansion::Automatic && self.call.state == State::Failed;
        if !expanded
            && !failure
            && (self.expansion == Expansion::Collapsed
                && matches!(self.call.state, State::Failed | State::Waiting(_))
                || matches!(self.displayed, Output::Code { .. } | Output::Diff(_)))
        {
            return vec![];
        }
        let mut lines = Vec::new();
        let mut offset = self
            .call
            .copy_text()
            .len()
            .saturating_sub(self.call.output.text().len());
        let mut old = 0u32;
        let mut new = 0u32;
        for (index, source) in text.split_inclusive('\n').enumerate() {
            let raw = source.trim_end_matches('\n');
            let (number, sign, body, role) =
                output_line(&self.displayed, raw, index, &mut old, &mut new, failure);
            lines.push((offset, number, sign, body, role));
            offset += source.len();
        }
        let count = lines.len();
        let preview = if expanded { 20 } else { 3 };
        let initial = if expanded {
            10
        } else if failure {
            3
        } else {
            0
        };
        let mut rows = Vec::new();
        let hidden = count.saturating_sub(preview);
        if hidden > 0 && initial == 0 {
            rows.push(self.marker(indent, symbols, format!("{hidden} earlier lines")));
        }
        for (i, line) in lines.iter().enumerate() {
            let visible = count <= preview
                || i < initial
                || (!failure && i >= count.saturating_sub(if expanded { 10 } else { 3 }));
            if !visible {
                if i == initial && initial > 0 {
                    rows.push(self.marker(
                        indent,
                        symbols,
                        format!(
                            "{hidden} more lines{}",
                            if self.call.capabilities.details {
                                " · o shows all"
                            } else {
                                ""
                            }
                        ),
                    ));
                }
                continue;
            }
            rows.extend(self.body_line(
                line,
                width,
                indent,
                symbols,
                !expanded && !failure && self.call.state == State::Running,
            ));
            if rows.len() >= 255 {
                rows.truncate(255);
                rows.push(self.marker(
                    indent,
                    symbols,
                    "wrapped rows not shown · preview limit".into(),
                ));
                break;
            }
        }
        // A streaming text tail reserves its body and count rows once output begins.
        if self.call.state == State::Running && !expanded && !failure {
            if hidden == 0 {
                rows.insert(0, self.marker(indent, symbols, "0 earlier lines".into()));
            }
            while rows.len() < 4 {
                rows.insert(1, self.marker(indent, symbols, String::new()));
            }
        }
        rows
    }
    fn marker(&self, indent: usize, symbols: Symbols, text: String) -> Row {
        Row {
            call: Some(self.call.id.clone()),
            offset: 0,
            prefix: " ".repeat(indent),
            pieces: vec![piece(
                if text.is_empty() {
                    text
                } else {
                    format!("{} {text}", glyph(symbols, "⋯", "..."))
                },
                Role::Dim,
            )],
            surface: false,
            head: false,
        }
    }
    fn body_line(
        &self,
        line: &(usize, Option<u32>, &str, &str, Role),
        width: usize,
        indent: usize,
        symbols: Symbols,
        clip: bool,
    ) -> Vec<Row> {
        let (offset, number, sign, text, role) = *line;
        let separator = glyph(symbols, "│", "|");
        let prefix = format!("{}{separator} ", " ".repeat(indent.saturating_sub(2)));
        let number = if width >= 50 {
            number.map_or_else(String::new, |n| format!("{n:>4} "))
        } else {
            String::new()
        };
        let sign = if symbols == Symbols::Ascii && sign == "− " {
            "- "
        } else {
            sign
        };
        let body_width = width
            .saturating_sub(prefix.width() + number.width() + sign.width().max(2))
            .max(1);
        let text = text.replace('\t', "    ");
        let chunks = if clip {
            vec![shortened(
                &text,
                body_width,
                glyph(symbols, "⋯", "..."),
                false,
            )]
        } else {
            wrap(&text, body_width, 256)
        };
        let mut consumed = 0;
        chunks
            .into_iter()
            .enumerate()
            .map(|(index, text)| {
                let initial = index == 0;
                let mut pieces = Vec::new();
                if initial && !number.is_empty() {
                    pieces.push(piece(number.clone(), Role::Dim));
                }
                if !sign.is_empty() || !initial {
                    pieces.push(piece(
                        if initial {
                            sign.into()
                        } else {
                            format!("{} ", glyph(symbols, "↩", "\\"))
                        },
                        role,
                    ));
                }
                let row = Row {
                    call: Some(self.call.id.clone()),
                    offset: offset + consumed,
                    prefix: prefix.clone(),
                    pieces: {
                        for (i, part) in text.split('␛').enumerate() {
                            if i > 0 {
                                pieces.push(Piece {
                                    text: "␛".into(),
                                    role: Role::Dim,
                                    generated: false,
                                });
                            }
                            pieces.push(Piece {
                                text: part.into(),
                                role,
                                generated: false,
                            });
                        }
                        pieces
                    },
                    surface: matches!(self.displayed, Output::Code { .. } | Output::Diff(_)),
                    head: false,
                };
                consumed += text.len();
                row
            })
            .collect()
    }
}
fn target(call: &Call, narrow: bool) -> (String, Role) {
    if !call.known() {
        return (
            format!("{} · {} {}", call.source, call.name, call.arguments),
            Role::Plain,
        );
    }
    match call.verb.as_str() {
        "read" | "edit" | "write" | "fetch" | "index" => (
            if narrow {
                call.target
                    .rsplit('/')
                    .next()
                    .unwrap_or(&call.target)
                    .into()
            } else {
                call.target.clone()
            },
            Role::Info,
        ),
        "run" => (call.target.clone(), Role::Command),
        "search" => (
            format!("\"{}\"", call.target.trim_matches('"')),
            Role::Plain,
        ),
        _ => (call.target.clone(), Role::Plain),
    }
}
pub(crate) fn output_line<'a>(
    output: &Output,
    raw: &'a str,
    index: usize,
    old: &mut u32,
    new: &mut u32,
    failure: bool,
) -> (Option<u32>, &'static str, &'a str, Role) {
    match output {
        Output::Code { first_line, .. } => (
            Some(first_line.saturating_add(u32::try_from(index).unwrap_or(u32::MAX))),
            "",
            raw,
            Role::Dim,
        ),
        Output::Diff(_) => {
            if raw.starts_with("@@") {
                let mut words = raw.split_whitespace();
                words.next();
                let a = words
                    .next()
                    .and_then(|s| s.strip_prefix('-'))
                    .and_then(|s| s.split(',').next())
                    .and_then(|s| s.parse().ok());
                let b = words
                    .next()
                    .and_then(|s| s.strip_prefix('+'))
                    .and_then(|s| s.split(',').next())
                    .and_then(|s| s.parse().ok());
                if let (Some(a), Some(b)) = (a, b) {
                    *old = a;
                    *new = b;
                }
                return (None, "", raw, Role::Dim);
            }
            if raw.starts_with("---") || raw.starts_with("+++") || (*new == 0 && *old == 0) {
                return (None, "", raw, Role::Dim);
            }
            if let Some(text) = raw.strip_prefix('-') {
                let n = *old;
                *old = old.saturating_add(1);
                (Some(n), "− ", text, Role::Delete)
            } else if let Some(text) = raw.strip_prefix('+') {
                let n = *new;
                *new = new.saturating_add(1);
                (Some(n), "+ ", text, Role::Add)
            } else {
                let n = *new;
                *old = old.saturating_add(1);
                *new = new.saturating_add(1);
                (
                    Some(n),
                    "  ",
                    raw.strip_prefix(' ').unwrap_or(raw),
                    Role::Dim,
                )
            }
        }
        _ => (None, "", raw, if failure { Role::Error } else { Role::Dim }),
    }
}
fn wrap(text: &str, width: usize, limit: usize) -> Vec<String> {
    let mut rows = vec![String::new()];
    for g in text.graphemes(true) {
        let index = rows.len() - 1;
        if rows[index].width() + g.width() > width && !rows[index].is_empty() {
            if rows.len() >= limit {
                rows.push("... line continues · preview limit".into());
                break;
            }
            rows.push(String::new());
        }
        let index = rows.len() - 1;
        rows[index].push_str(if g.width() > width { "�" } else { g });
    }
    rows
}
impl ToolGroupCard {
    pub(crate) fn rows_search(
        &self,
        width: u16,
        symbols: Symbols,
        call: &str,
        offset: usize,
    ) -> Vec<Row> {
        let mut rows = self.rows(width, symbols, true);
        if let Some(card) = self.card(call) {
            let extra = card.rows_search(width, symbols, Some(offset));
            let index = rows
                .iter()
                .position(|row| row.call.as_deref() == Some(call))
                .unwrap_or(rows.len());
            rows.retain(|row| row.call.as_deref() != Some(call));
            rows.splice(index..index, extra);
        }
        rows
    }
    /// Render the group using the same public member views as standalone cards.
    pub fn render_with(&self, frame: &mut Frame<'_>, area: Rect, theme: Presentation) {
        render(
            frame,
            area,
            theme,
            &self.rows(area.width, theme.symbols, false),
            false,
        );
    }
    pub(crate) fn rows(&self, width: u16, symbols: Symbols, force: bool) -> Vec<Row> {
        let done = self.cards.iter().all(|c| c.call.state.terminal());
        let successes = self
            .cards
            .iter()
            .filter(|c| c.call.state == State::Completed)
            .count();
        let failures = self
            .cards
            .iter()
            .filter(|c| c.call.state == State::Failed)
            .count();
        let cancelled = self
            .cards
            .iter()
            .filter(|c| matches!(c.call.state, State::Cancelled(_)))
            .count();
        let interrupted = self
            .cards
            .iter()
            .filter(|c| matches!(c.call.state, State::Interrupted(_)))
            .count();
        let expanded = self.expanded || force;
        let mut label = format!("{} at once", self.cards.len());
        if done {
            let _ = write!(
                label,
                " · {successes} {} · {failures} {} · {cancelled} cancelled · {}",
                glyph(symbols, "✓", "ok"),
                glyph(symbols, "✕", "x"),
                duration(
                    self.cards
                        .iter()
                        .map(|c| c.call.elapsed)
                        .max()
                        .unwrap_or_default()
                )
            );
            if interrupted > 0 {
                let _ = write!(label, " · {interrupted} interrupted");
            }
        }
        let mut rows = vec![Row {
            call: None,
            offset: 0,
            prefix: String::new(),
            pieces: vec![piece(label, Role::Dim)],
            surface: false,
            head: true,
        }];
        let mut hidden = 0;
        for (index, card) in self.cards.iter().enumerate() {
            let problem = matches!(
                card.call.state,
                State::Failed | State::Cancelled(_) | State::Interrupted(_)
            );
            let visible = expanded
                || if done {
                    problem
                } else {
                    self.cards.len() <= 6 || index < 5 || problem
                };
            if visible {
                rows.extend(card.rows(width, symbols, force));
            } else {
                hidden += 1;
            }
        }
        if hidden > 0 && successes != self.cards.len() {
            rows.push(Row {
                call: None,
                offset: 0,
                prefix: String::new(),
                pieces: vec![piece(
                    format!("+{hidden} more · enter on group shows them"),
                    Role::Dim,
                )],
                surface: false,
                head: false,
            });
        }
        rows
    }
}
fn render(frame: &mut Frame<'_>, area: Rect, theme: Presentation, rows: &[Row], selected: bool) {
    let lines: Vec<_> = rows
        .iter()
        .map(|row| {
            let mut spans = vec![Span::styled(row.prefix.clone(), theme.muted())];
            spans.extend(row.pieces.iter().map(|p| {
                let mut style = match p.role {
                    Role::Plain => theme.base(),
                    Role::Dim => theme.muted(),
                    Role::Link => theme
                        .tokens()
                        .info
                        .map_or(theme.base(), |c| theme.base().fg(c))
                        .add_modifier(Modifier::UNDERLINED),
                    Role::Info => theme
                        .tokens()
                        .info
                        .map_or(theme.base(), |c| theme.base().fg(c)),
                    Role::Command => theme.base().add_modifier(Modifier::BOLD),
                    Role::Success | Role::Add => theme
                        .tokens()
                        .success
                        .map_or(theme.base(), |c| theme.base().fg(c)),
                    Role::Error => theme.error().add_modifier(Modifier::BOLD),
                    Role::Delete => theme.error(),
                    Role::Warning => theme.warning(),
                    Role::Accent => theme.accent(),
                };
                if row.surface && theme.tokens().surface.is_some() {
                    style = theme.surface().patch(style);
                }
                if selected && matches!(theme.colors, ColorMode::TrueColor | ColorMode::Ansi256) {
                    style = style.bg(theme.tokens().selection.expect("color profile selection"));
                }
                if selected && row.head && theme.colors == ColorMode::NoColor {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                Span::styled(
                    if theme.symbols == Symbols::Ascii {
                        p.text.replace('␛', "^[")
                    } else {
                        p.text.clone()
                    },
                    style,
                )
            }));
            if selected && theme.colors != ColorMode::NoColor {
                spans.insert(
                    0,
                    Span::styled(glyph(theme.symbols, "▌", "|"), theme.accent()),
                );
            }
            let line = Line::from(spans);
            if selected && matches!(theme.colors, ColorMode::TrueColor | ColorMode::Ansi256) {
                line.style(theme.selection())
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).style(theme.base()), area);
}
