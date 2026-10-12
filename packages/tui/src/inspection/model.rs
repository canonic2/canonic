//! Bounded host content, operation identity and unified patch validation.

pub use crate::review_ticket::Ticket;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// Maximum combined loaded source bytes, independent of host limits.
pub const BYTE_LIMIT: usize = 4 * 1024 * 1024;
/// Maximum source lines across patch and whole file.
pub const LINE_LIMIT: usize = 20_000;
/// Caller focus destination; it determines Tab behavior and return hints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    #[default]
    Chat,
    Tree,
}
/// Identity and presentation supplied before loading starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub revision: u64,
    pub title: String,
    pub summary: String,
    pub language: String,
    pub origin: Origin,
    /// Initial new-side line; old numbering is used for deleted-only patches.
    pub line: Option<usize>,
    /// Whether the host offers opening this exact revision in an editor.
    pub editor: bool,
}
impl Item {
    #[must_use]
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.into(),
            revision: 1,
            title: title.into(),
            summary: String::new(),
            language: String::new(),
            origin: Origin::Chat,
            line: None,
            editor: false,
        }
    }
}
/// Loaded source. A whole new file is optional and never inferred from a patch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Code(String),
    /// Explicitly bounded source excerpt retaining host line numbering.
    CodeAt {
        text: String,
        first_line: usize,
    },
    Diff {
        patch: String,
        new_file: Option<String>,
    },
}
/// Explicit host state; blocked content may offer a supplied bounded preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Loading(String),
    Ready(Content),
    Failed(String),
    Unavailable {
        reason: String,
        preview: Option<Content>,
    },
}
/// Identified host operation; no operation performs I/O in the component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Load,
    Copy {
        text: String,
        lines: usize,
        patch: bool,
    },
    Editor {
        line: Option<usize>,
    },
}
/// Clone the exact request to complete it once on its issuing instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub ticket: Ticket,
    pub item: String,
    pub revision: u64,
    pub operation: Operation,
}
/// Result must match the request kind; malformed/stale replies are inert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Loaded(State),
    Copied,
    Opened,
    Failed(String),
}
/// Work and focus changes returned to the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Request(Request),
    Returned,
}
/// Invalid content leaves the current state and outstanding request intact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invalid {
    Identity,
    Limit,
    Patch,
    Revision,
}
impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid inspection content: {self:?}")
    }
}
impl std::error::Error for Invalid {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Code,
    Context,
    Added,
    Removed,
    Header,
    Meta,
}
#[derive(Clone, Debug)]
pub(super) struct Row {
    pub text: String,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub kind: Kind,
    pub hunk: Option<usize>,
    pub changed: Range<usize>,
}
impl Row {
    fn new(
        text: &str,
        kind: Kind,
        old: Option<usize>,
        new: Option<usize>,
        hunk: Option<usize>,
    ) -> Self {
        Self {
            text: text.into(),
            old,
            new,
            kind,
            hunk,
            changed: if matches!(kind, Kind::Added | Kind::Removed) {
                0..text.len()
            } else {
                0..0
            },
        }
    }
}
#[derive(Clone, Debug, Default)]
pub(super) struct Loaded {
    pub diff: Vec<Row>,
    pub file: Option<Vec<Row>>,
    pub hunks: Vec<usize>,
    pub folds: Vec<Range<usize>>,
    pub is_diff: bool,
}
pub(super) fn item(item: &Item) -> Result<(), Invalid> {
    if item.id.is_empty() || item.id.len() > 128 || item.id.chars().any(char::is_control) {
        return Err(Invalid::Identity);
    }
    if [&item.title, &item.summary, &item.language]
        .iter()
        .any(|s| s.len() > 4096)
    {
        return Err(Invalid::Limit);
    }
    Ok(())
}
fn lines(text: &str) -> Result<Vec<&str>, Invalid> {
    if text.len() > BYTE_LIMIT {
        return Err(Invalid::Limit);
    }
    let mut lines: Vec<_> = text
        .split_terminator('\n')
        .map(|s| s.strip_suffix('\r').unwrap_or(s))
        .collect();
    if text.is_empty() {
        lines.clear();
    }
    if lines.len() > LINE_LIMIT || lines.iter().any(|s| s.len() > 65_536) {
        return Err(Invalid::Limit);
    }
    Ok(lines)
}
fn range(s: &str, sign: char) -> Result<(usize, usize), Invalid> {
    let s = s.strip_prefix(sign).ok_or(Invalid::Patch)?;
    let (start, count) = s.split_once(',').unwrap_or((s, "1"));
    let start = start.parse::<usize>().map_err(|_| Invalid::Patch)?;
    let count = count.parse::<usize>().map_err(|_| Invalid::Patch)?;
    if start.checked_add(count).is_none() || (count > 0 && start == 0) {
        return Err(Invalid::Patch);
    }
    Ok((start, count))
}
type HunkRange = (usize, usize);
fn header(s: &str) -> Result<(HunkRange, HunkRange), Invalid> {
    let body = s
        .strip_prefix("@@ ")
        .and_then(|s| s.split_once(" @@"))
        .ok_or(Invalid::Patch)?
        .0;
    let mut words = body.split_whitespace();
    let old = range(words.next().ok_or(Invalid::Patch)?, '-')?;
    let new = range(words.next().ok_or(Invalid::Patch)?, '+')?;
    if words.next().is_some() {
        return Err(Invalid::Patch);
    }
    Ok((old, new))
}
pub(super) fn parse(content: &Content) -> Result<Loaded, Invalid> {
    match content {
        Content::CodeAt { text, first_line } => {
            if text.len() > BYTE_LIMIT {
                return Err(Invalid::Limit);
            }
            let mut loaded = parse(&Content::Code(text.clone()))?;
            if *first_line == 0 || first_line.checked_add(loaded.diff.len()).is_none() {
                return Err(Invalid::Limit);
            }
            for (i, row) in loaded.diff.iter_mut().enumerate() {
                row.new = Some(first_line + i);
            }
            Ok(loaded)
        }
        Content::Code(text) => Ok(Loaded {
            diff: lines(text)?
                .into_iter()
                .enumerate()
                .map(|(i, s)| Row::new(s, Kind::Code, None, Some(i + 1), None))
                .collect(),
            ..Loaded::default()
        }),
        Content::Diff { patch, new_file } => parse_diff(patch, new_file.as_deref()),
    }
}
fn parse_diff(patch: &str, file: Option<&str>) -> Result<Loaded, Invalid> {
    if patch.len().saturating_add(file.map_or(0, str::len)) > BYTE_LIMIT {
        return Err(Invalid::Limit);
    }
    let mut out = Loaded {
        is_diff: true,
        ..Loaded::default()
    };
    let mut remaining = (0usize, 0usize);
    let mut position = (0usize, 0usize);
    let mut previous_end = (0usize, 0usize);
    let mut active = false;
    for s in lines(patch)? {
        if s.starts_with("@@") {
            if remaining != (0, 0) {
                return Err(Invalid::Patch);
            }
            let (old, new) = header(s)?;
            if active && (old.0 < previous_end.0 || new.0 < previous_end.1) {
                return Err(Invalid::Patch);
            }
            previous_end = (old.0 + old.1, new.0 + new.1);
            position = (old.0, new.0);
            remaining = (old.1, new.1);
            active = true;
            let h = out.hunks.len();
            out.hunks.push(out.diff.len());
            out.diff
                .push(Row::new(s, Kind::Header, None, None, Some(h)));
        } else if s == "\\ No newline at end of file" && active {
            out.diff.push(Row::new(
                s,
                Kind::Meta,
                None,
                None,
                out.hunks.len().checked_sub(1),
            ));
        } else if active && remaining != (0, 0) {
            let sign = s.chars().next().ok_or(Invalid::Patch)?;
            let (old, new, kind) = match sign {
                ' ' if remaining.0 > 0 && remaining.1 > 0 => (true, true, Kind::Context),
                '-' if remaining.0 > 0 => (true, false, Kind::Removed),
                '+' if remaining.1 > 0 => (false, true, Kind::Added),
                _ => return Err(Invalid::Patch),
            };
            out.diff.push(Row::new(
                &s[1..],
                kind,
                old.then_some(position.0),
                new.then_some(position.1),
                out.hunks.len().checked_sub(1),
            ));
            if old {
                remaining.0 -= 1;
                position.0 += 1;
            }
            if new {
                remaining.1 -= 1;
                position.1 += 1;
            }
        } else if !active
            && (s.starts_with("diff --git ")
                || s.starts_with("index ")
                || s.starts_with("--- ")
                || s.starts_with("+++ ")
                || s.starts_with("rename ")
                || s.starts_with("similarity index ")
                || s.starts_with("new file mode ")
                || s.starts_with("deleted file mode ")
                || s.starts_with("old mode ")
                || s.starts_with("new mode "))
        {
            out.diff.push(Row::new(s, Kind::Meta, None, None, None));
        } else {
            return Err(Invalid::Patch);
        }
    }
    if remaining != (0, 0) || (!patch.is_empty() && out.hunks.is_empty()) {
        return Err(Invalid::Patch);
    }
    finish(out, file)
}

fn finish(mut out: Loaded, file: Option<&str>) -> Result<Loaded, Invalid> {
    changes(&mut out.diff);
    if let Some(file) = file {
        let mut rows: Vec<_> = lines(file)?
            .into_iter()
            .enumerate()
            .map(|(i, s)| Row::new(s, Kind::Code, None, Some(i + 1), None))
            .collect();
        if rows.len() + out.diff.len() > LINE_LIMIT {
            return Err(Invalid::Limit);
        }
        for row in &out.diff {
            if let Some(n) = row.new {
                let target = rows.get_mut(n - 1).ok_or(Invalid::Patch)?;
                if target.text != row.text {
                    return Err(Invalid::Patch);
                }
                target.hunk = row.hunk;
                if row.kind == Kind::Added {
                    target.kind = Kind::Added;
                    target.changed = row.changed.clone();
                }
            }
        }
        out.file = Some(rows);
    }
    let mut start = 0;
    while start < out.diff.len() {
        if out.diff[start].kind != Kind::Context {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < out.diff.len() && out.diff[end].kind == Kind::Context {
            end += 1;
        }
        if end - start > 8 {
            out.folds.push(start + 3..end - 3);
        }
        start = end;
    }
    Ok(out)
}
fn changes(rows: &mut [Row]) {
    // Pair adjacent replacements; unpaired additions/removals retain whole-line meaning.
    let mut i = 0;
    while i + 1 < rows.len() {
        if rows[i].kind == Kind::Removed && rows[i + 1].kind == Kind::Added {
            let a: Vec<_> = rows[i].text.grapheme_indices(true).collect();
            let b: Vec<_> = rows[i + 1].text.grapheme_indices(true).collect();
            let prefix = a.iter().zip(&b).take_while(|(x, y)| x.1 == y.1).count();
            let suffix = a[prefix..]
                .iter()
                .rev()
                .zip(b[prefix..].iter().rev())
                .take_while(|(x, y)| x.1 == y.1)
                .count();
            let range = |g: &[(usize, &str)], len: usize| {
                g.get(prefix).map_or(len, |v| v.0)..g.get(g.len() - suffix).map_or(len, |v| v.0)
            };
            rows[i].changed = range(&a, rows[i].text.len());
            rows[i + 1].changed = range(&b, rows[i + 1].text.len());
            i += 2;
        } else {
            i += 1;
        }
    }
}
