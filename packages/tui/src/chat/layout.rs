//! Caches wrapped blocks and logical source anchors independently of color.

use super::{
    content::{Block, Ink, Kind, Run},
    model::Anchor,
};
use std::fmt::Write;
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Display projection budgets, in addition to source and part limits.
pub(super) const ROW_LIMIT: usize = 32_768;
pub(super) const BYTE_LIMIT: usize = 8 * 1024 * 1024;
pub(super) const PART_ROWS: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BlockId {
    pub message: String,
    pub part: String,
    pub start: usize,
}
#[derive(Clone)]
pub(super) struct Row {
    pub anchor: Anchor,
    pub block: Option<BlockId>,
    pub prefix: String,
    pub runs: Vec<Run>,
    pub surface: bool,
    pub header: bool,
    pub searchable: bool,
    pub map: Option<Vec<usize>>,
}
impl Row {
    pub(super) fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.prefix.capacity()
            + self.anchor.message.capacity()
            + self.anchor.part.capacity()
            + self
                .block
                .as_ref()
                .map_or(0, |id| id.message.capacity() + id.part.capacity())
            + self.runs.capacity() * std::mem::size_of::<Run>()
            + self.runs.iter().map(|r| r.text.capacity()).sum::<usize>()
            + self
                .map
                .as_ref()
                .map_or(0, |map| map.capacity() * std::mem::size_of::<usize>())
    }
    pub(super) fn plain(anchor: Anchor, text: String, ink: Ink) -> Self {
        Self {
            anchor,
            block: None,
            prefix: String::new(),
            runs: vec![Run {
                text,
                ink,
                generated: true,
            }],
            surface: false,
            header: false,
            searchable: false,
            map: None,
        }
    }
}
#[derive(Clone)]
pub(super) struct Located {
    pub rows: Arc<Vec<Row>>,
    pub index: usize,
}
impl Located {
    pub(super) fn row(&self) -> &Row {
        &self.rows[self.index]
    }
}

pub(super) struct Cached {
    pub revision: u64,
    pub blocks: Vec<Block>,
    pub width: u16,
    pub expanded: bool,
    pub rows: Arc<Vec<Row>>,
    pub bytes: usize,
    pub memory_limited: bool,
    pub budget: usize,
}

pub(super) fn layout(
    blocks: &[Block],
    message: &str,
    part: &str,
    width: u16,
    expanded: bool,
    budget: usize,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for block in blocks {
        let id = BlockId {
            message: message.into(),
            part: part.into(),
            start: block.start,
        };
        let mut rendered = block_rows(block, &id, width, expanded);
        if !rows.is_empty() {
            rendered.insert(0, decoration(&id, String::new(), muted()));
        }
        let left = budget
            .saturating_sub(rows.len())
            .min(PART_ROWS.saturating_sub(rows.len()));
        if rendered.len() > left {
            let missing = rendered.len().saturating_sub(left.saturating_sub(1));
            rendered.truncate(left.saturating_sub(1));
            if left > 0 {
                rendered.push(Row::plain(
                    anchor(&id, block.start),
                    format!("  ⋯ {missing} display rows not shown · layout limit"),
                    muted(),
                ));
            }
            rows.extend(rendered);
            break;
        }
        rows.append(&mut rendered);
    }
    rows
}
fn muted() -> Ink {
    Ink {
        muted: true,
        ..Ink::default()
    }
}
fn anchor(id: &BlockId, offset: usize) -> Anchor {
    Anchor {
        message: id.message.clone(),
        part: id.part.clone(),
        offset,
    }
}
fn decoration(id: &BlockId, text: String, ink: Ink) -> Row {
    let mut row = Row::plain(anchor(id, id.start), text, ink);
    row.block = Some(id.clone());
    row
}

fn block_rows(block: &Block, id: &BlockId, width: u16, expanded: bool) -> Vec<Row> {
    let width = usize::from(width).max(1);
    match &block.kind {
        Kind::Code {
            language,
            file,
            line,
        } => {
            let mut label = if language.is_empty() {
                "code".into()
            } else {
                language.clone()
            };
            if !file.is_empty() {
                let _ = write!(label, " · {}", short(file, width));
            }
            if let Some(line) = line {
                let _ = write!(label, " · {line}");
            }
            let mut rows = vec![decoration(id, quiet_rule(&label, width), muted())];
            rows.extend(wrap(
                &block.runs,
                id,
                width.saturating_sub(2).max(1),
                "  ",
                true,
                true,
            ));
            rows
        }
        Kind::Table { headers, rows } => table_rows(headers, rows, id, width),
        Kind::Rule => vec![decoration(
            id,
            format!("  {}", "─".repeat(width.saturating_sub(2).min(20))),
            muted(),
        )],
        Kind::Quote => wrap(
            &block.runs,
            id,
            width.saturating_sub(4).clamp(1, 100),
            "  │ ",
            false,
            false,
        ),
        Kind::Text => wrap(
            &block.runs,
            id,
            width.saturating_sub(2).clamp(1, 100),
            "  ",
            false,
            false,
        ),
        Kind::Fold { summary, reasoning } => {
            let mut rows = vec![decoration(
                id,
                format!("  {} {summary}", if expanded { "▾" } else { "▸" }),
                muted(),
            )];
            if expanded {
                let runs = if *reasoning {
                    block.runs.clone()
                } else {
                    let text = block.semantic();
                    vec![Run {
                        generated: false,
                        text: text.split_once('\n').map_or("", |(_, out)| out).into(),
                        ink: muted(),
                    }]
                };
                let mut body = wrap(
                    &runs,
                    id,
                    width.saturating_sub(4).clamp(1, 100),
                    "    ",
                    false,
                    false,
                );
                if !reasoning {
                    let offset = block.semantic().find('\n').map_or(0, |n| n + 1);
                    for row in &mut body {
                        row.anchor.offset += offset;
                        if let Some(map) = &mut row.map {
                            for source in map {
                                *source += offset;
                            }
                        }
                    }
                }
                rows.extend(body);
            }
            rows
        }
        Kind::Diff { file, summary } => {
            diff_rows(&block.semantic(), file, summary, id, width, expanded)
        }
    }
}
fn quiet_rule(label: &str, width: usize) -> String {
    let label = format!("  {label} ");
    let remaining = width.saturating_sub(label.width());
    format!("{label}{}", "─".repeat(remaining))
}
fn short(text: &str, width: usize) -> String {
    let text = if width < 50 {
        text.rsplit('/').next().unwrap_or(text)
    } else {
        text
    };
    let limit = width.saturating_sub(16).max(1);
    if text.width() <= limit {
        return text.into();
    }
    let mut result = String::new();
    for grapheme in text.graphemes(true) {
        if result.width() + grapheme.width() + 1 > limit {
            break;
        }
        result.push_str(grapheme);
    }
    result.push('…');
    result
}

struct Wrapper<'a> {
    id: &'a BlockId,
    width: usize,
    prefix: &'a str,
    code: bool,
    surface: bool,
    rows: Vec<Row>,
    runs: Vec<Run>,
    cells: usize,
    offset: usize,
    start: usize,
    continuation: bool,
    map: Option<Vec<usize>>,
}
impl Wrapper<'_> {
    fn push(&mut self) {
        self.rows.push(Row {
            anchor: anchor(self.id, self.start),
            block: Some(self.id.clone()),
            prefix: if self.code && self.continuation {
                "↩ ".into()
            } else {
                self.prefix.into()
            },
            runs: std::mem::take(&mut self.runs),
            surface: self.surface,
            header: false,
            searchable: true,
            map: self.map.as_mut().map(std::mem::take),
        });
        self.cells = 0;
        self.start = self.offset;
    }
    fn token(&mut self, token: &str, ink: Ink, generated: bool) {
        if self.rows.len() >= PART_ROWS - 1 {
            return;
        }
        if token == "\n" {
            self.offset += usize::from(!generated);
            self.push();
            self.start = self.offset;
            self.continuation = false;
            return;
        }
        let token_width = token.width();
        if !self.code
            && token_width <= self.width
            && self.cells > 0
            && self.cells + token_width > self.width
        {
            self.push();
            self.continuation = true;
        }
        for grapheme in token.graphemes(true) {
            if self.rows.len() >= PART_ROWS - 1 {
                return;
            }
            let expanded = if grapheme == "\t" {
                " ".repeat(4 - self.cells % 4)
            } else {
                grapheme.into()
            };
            let cells = expanded.width();
            if self.cells > 0 && self.cells + cells > self.width {
                self.push();
                self.continuation = true;
            }
            if !self.code && self.cells == 0 && grapheme == " " {
                self.offset += usize::from(!generated);
                self.start = self.offset;
                continue;
            }
            let text = if cells > self.width { "�" } else { &expanded };
            if let Some(map) = &mut self.map {
                map.extend(std::iter::repeat_n(self.offset, text.len()));
            }
            if let Some(last) = self
                .runs
                .last_mut()
                .filter(|run| run.ink == ink && run.generated == generated)
            {
                last.text.push_str(text);
            } else {
                self.runs.push(Run {
                    generated,
                    text: text.into(),
                    ink,
                });
            }
            self.cells += cells.min(self.width);
            self.offset += if generated { 0 } else { grapheme.len() };
        }
    }
}
fn wrap(
    runs: &[Run],
    id: &BlockId,
    width: usize,
    prefix: &str,
    code: bool,
    surface: bool,
) -> Vec<Row> {
    let mut wrapper = Wrapper {
        id,
        width,
        prefix,
        code,
        surface,
        rows: vec![],
        runs: vec![],
        cells: 0,
        offset: id.start,
        start: id.start,
        continuation: false,
        map: runs
            .iter()
            .any(|run| run.text.contains('\t'))
            .then(Vec::new),
    };
    for run in runs {
        for token in run.text.split_word_bounds() {
            wrapper.token(token, run.ink, run.generated);
        }
    }
    let length: usize = runs
        .iter()
        .filter(|run| !run.generated)
        .map(|run| run.text.len())
        .sum();
    if wrapper.offset < id.start + length {
        wrapper.push();
        wrapper.rows.push(decoration(
            id,
            format!(
                "  ⋯ {} source bytes not drawn · layout limit",
                id.start + length - wrapper.offset
            ),
            muted(),
        ));
        return wrapper.rows;
    }
    if !wrapper.runs.is_empty() || wrapper.rows.is_empty() {
        wrapper.push();
    }
    wrapper.rows
}

fn table_rows(headers: &[String], data: &[Vec<String>], id: &BlockId, width: usize) -> Vec<Row> {
    let count = headers
        .len()
        .max(data.iter().map(Vec::len).max().unwrap_or(0));
    let widths: Vec<_> = (0..count)
        .map(|column| {
            headers.get(column).map_or(0, |s| s.width()).max(
                data.iter()
                    .filter_map(|r| r.get(column))
                    .map(|s| s.width())
                    .max()
                    .unwrap_or(0),
            )
        })
        .collect();
    if width < 50
        || widths.iter().sum::<usize>() + count.saturating_sub(1) * 2 + 2 > width
        || headers
            .iter()
            .chain(data.iter().flatten())
            .any(|s| s.contains(['\n', '\t']))
    {
        return record_rows(headers, data, id, width, count);
    }
    let numeric: Vec<_> = (0..count)
        .map(|i| {
            data.iter()
                .filter_map(|r| r.get(i))
                .any(|s| s.trim().parse::<f64>().is_ok())
        })
        .collect();
    let row_text = |values: &[String], heading: bool, offset: usize| {
        let mut text = String::new();
        let mut map = Vec::new();
        let mut source = offset;
        for (i, value) in values.iter().enumerate() {
            let padding = " ".repeat(widths[i].saturating_sub(value.width()));
            if i > 0 {
                text.push_str("  ");
                map.extend([usize::MAX; 2]);
                source += " · ".len();
            }
            if !heading && numeric[i] {
                text.push_str(&padding);
                map.extend(std::iter::repeat_n(usize::MAX, padding.len()));
            }
            text.push_str(value);
            map.extend((0..value.len()).map(|byte| source + byte));
            source += value.len();
            if heading || !numeric[i] {
                text.push_str(&padding);
                map.extend(std::iter::repeat_n(usize::MAX, padding.len()));
            }
        }
        let mut row = decoration(
            id,
            text,
            Ink {
                bold: heading,
                ..Ink::default()
            },
        );
        row.prefix = "  ".into();
        row.searchable = true;
        row.runs[0].generated = false;
        row.map = Some(map);
        row.anchor.offset = offset;
        row
    };
    let mut rows = vec![
        row_text(headers, true, id.start),
        decoration(
            id,
            format!(
                "  {}",
                widths
                    .iter()
                    .map(|n| "─".repeat(*n))
                    .collect::<Vec<_>>()
                    .join("  ")
            ),
            muted(),
        ),
    ];
    let mut offset = id.start + headers.join(" · ").len() + 1;
    for values in data {
        let row = row_text(values, false, offset);
        offset += values.join(" · ").len() + 1;
        rows.push(row);
    }
    rows
}

fn diff_rows(
    patch: &str,
    file: &str,
    summary: &str,
    id: &BlockId,
    width: usize,
    expanded: bool,
) -> Vec<Row> {
    let mut rows = vec![decoration(
        id,
        format!("  edit     {} · {summary}", short(file, width)),
        muted(),
    )];
    let hunks = patch.lines().filter(|line| line.starts_with("@@")).count();
    let mut entered = 0;
    let mut old = 0u32;
    let mut new = 0u32;
    let mut offset = id.start;
    for source in patch.split_inclusive('\n') {
        let line = source.trim_end_matches('\n');
        if line.starts_with("@@") {
            if entered > 0 && !expanded {
                break;
            }
            if let Some((a, b)) = hunk_positions(line) {
                old = a;
                new = b;
                entered += 1;
                rows.push(decoration(id, format!("  {line}"), muted()));
            } else {
                rows.push(decoration(
                    id,
                    "  malformed hunk header · raw patch".into(),
                    muted(),
                ));
                rows.extend(raw_patch(patch, id, width));
                return rows;
            }
        } else if line == "\\ No newline at end of file" {
            rows.push(decoration(id, format!("  {line}"), muted()));
        } else if entered > 0 {
            let (number, sign, ink) = diff_line(line, &mut old, &mut new);
            let body = if line.starts_with(['+', '-', ' ']) {
                &line[1..]
            } else {
                line
            };
            let prefix = format!("  {number:>4} {sign} ");
            let mut rendered = wrap(
                &[Run {
                    text: body.into(),
                    ink,
                    generated: false,
                }],
                id,
                width.saturating_sub(prefix.width()).max(1),
                &prefix,
                true,
                false,
            );
            for row in &mut rendered {
                let extra = offset - id.start + usize::from(body != line);
                row.anchor.offset += extra;
                if let Some(map) = &mut row.map {
                    for source in map {
                        *source += extra;
                    }
                }
            }
            rows.extend(rendered);
        }
        offset += source.len();
    }
    if entered == 0 {
        rows.push(decoration(
            id,
            "  patch has no text hunks · raw patch".into(),
            muted(),
        ));
        rows.extend(raw_patch(patch, id, width));
    } else if hunks > entered {
        rows.push(decoration(
            id,
            format!("  +{} more hunks · o opens full patch", hunks - entered),
            muted(),
        ));
    }
    rows
}

fn raw_patch(patch: &str, id: &BlockId, width: usize) -> Vec<Row> {
    wrap(
        &[Run {
            text: patch.into(),
            ink: muted(),
            generated: false,
        }],
        id,
        width.saturating_sub(2).max(1),
        "  ",
        true,
        false,
    )
}

fn record_rows(
    headers: &[String],
    data: &[Vec<String>],
    id: &BlockId,
    width: usize,
    count: usize,
) -> Vec<Row> {
    let mut rows = vec![decoration(
        id,
        format!("  table · {count} columns, too wide · shown as records"),
        muted(),
    )];
    let mut offset = id.start + headers.join(" · ").len() + 1;
    rows.extend(wrap(
        &[Run {
            text: headers.join(" · "),
            generated: false,
            ink: Ink {
                bold: true,
                ..Ink::default()
            },
        }],
        id,
        width.saturating_sub(2).max(1),
        "  ",
        false,
        false,
    ));
    for record in data {
        let mut runs = Vec::new();
        for (i, value) in record.iter().enumerate() {
            if i > 0 {
                runs.push(Run {
                    text: " · ".into(),
                    generated: false,
                    ink: muted(),
                });
                runs.push(Run {
                    text: format!("{} ", headers.get(i).map_or("column", String::as_str)),
                    generated: true,
                    ink: muted(),
                });
            }
            runs.push(Run {
                text: value.clone(),
                generated: false,
                ink: Ink::default(),
            });
        }
        let mut lines = wrap(
            &runs,
            id,
            width.saturating_sub(2).max(1),
            "  ",
            false,
            false,
        );
        for row in &mut lines {
            row.anchor.offset += offset - id.start;
            if let Some(map) = &mut row.map {
                for source in map {
                    *source += offset - id.start;
                }
            }
        }
        offset += record.join(" · ").len() + 1;
        rows.extend(lines);
    }
    rows
}

fn hunk_positions(line: &str) -> Option<(u32, u32)> {
    let mut words = line.split_whitespace();
    if words.next()? != "@@" {
        return None;
    }
    let parse = |range: &str| {
        let mut fields = range.split(',');
        let start = fields.next()?.parse::<u32>().ok()?;
        if let Some(count) = fields.next() {
            count.parse::<u32>().ok()?;
        }
        if fields.next().is_some() {
            return None;
        }
        Some(start)
    };
    let old = parse(words.next()?.strip_prefix('-')?)?;
    let new = parse(words.next()?.strip_prefix('+')?)?;
    (words.next()? == "@@").then_some((old, new))
}

fn diff_line(line: &str, old: &mut u32, new: &mut u32) -> (u32, &'static str, Ink) {
    if line.starts_with('-') {
        let n = *old;
        *old = old.saturating_add(1);
        (
            n,
            "−",
            Ink {
                error: true,
                ..Ink::default()
            },
        )
    } else if line.starts_with('+') {
        let n = *new;
        *new = new.saturating_add(1);
        (
            n,
            "+",
            Ink {
                success: true,
                ..Ink::default()
            },
        )
    } else {
        let n = *new;
        *old = old.saturating_add(1);
        *new = new.saturating_add(1);
        (n, " ", muted())
    }
}
