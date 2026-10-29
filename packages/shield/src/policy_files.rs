//! Owns the text of project policy files: block-style YAML rendering and
//! format-preserving edits that keep the operator's comments and layout,
//! verified by the strict policy reader.
//!
//! An edit walks the previous text's parsed nodes and the target document
//! together, changing only the text that differs. Any edit the strict reader
//! does not decode to exactly the target is discarded for a full rendering.

use crate::policy::{self, Error};
use granit_parser::{Event, Parser, ScalarStyle, StructureStyle};
use serde_json::{Map, Value};
use std::ops::Range;

#[cfg(test)]
mod tests;

/// Fields people look for first in a configuration or rule mapping; the rest
/// follow in name order.
const LEADING: [&str; 5] = ["schema_version", "kind", "id", "decision", "reason"];

/// A policy file's new text and what writing it cost the previous text.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Written {
    /// The file's new text.
    pub(crate) text: String,
    /// Whether previous text was replaced by a full rendering.
    pub(crate) rendered: bool,
    /// Comments of the previous text the new text does not keep, other than
    /// those removed with their entries.
    pub(crate) dropped_comments: usize,
}

/// Block-style YAML for a document, in the default layout, verified to read
/// back as exactly that document.
///
/// # Errors
/// Refuses values the YAML serializer cannot represent and renderings the
/// strict reader decodes differently.
pub(crate) fn render(value: &Value) -> Result<String, Error> {
    let text = Layout::default()
        .render(value)
        .map_err(|Fallback| Error::Contract("policy file rendering failed"))?;
    if policy::parse::<Value>(&text).ok().as_ref() != Some(value) {
        return Err(Error::Contract(
            "policy file rendering does not read back as its document",
        ));
    }
    Ok(text)
}

/// A retained source as its project file: a JSON source is rendered, and YAML
/// keeps its bytes and comments.
///
/// # Errors
/// Refuses JSON values the YAML serializer cannot represent.
pub(crate) fn projected(source: &str) -> Result<String, Error> {
    match serde_json::from_str::<Value>(source) {
        Ok(value) => render(&value),
        Err(_) => Ok(source.into()),
    }
}

/// The text of a file whose document becomes `target`: the previous text with
/// only the differing nodes edited, the previous text itself when the document
/// is unchanged, or a full rendering below its leading comment block when no
/// verified edit exists. A file without previous text is rendered.
///
/// # Errors
/// Refuses targets the YAML serializer cannot represent.
pub(crate) fn write(previous: Option<&str>, target: &Value) -> Result<Written, Error> {
    let Some(previous) = previous else {
        return Ok(Written {
            text: render(target)?,
            rendered: false,
            dropped_comments: 0,
        });
    };
    let Ok(before) = policy::parse::<Value>(previous) else {
        return fallback(previous, target, 0);
    };
    if before == *target {
        return Ok(Written {
            text: previous.into(),
            rendered: false,
            dropped_comments: 0,
        });
    }
    let Some(source) = Source::parse(previous) else {
        return fallback(previous, target, 0);
    };
    match source.edit(&before, target) {
        Ok(written) => Ok(written),
        Err(Fallback) => fallback(previous, target, source.comments.len()),
    }
}

/// One review line naming what a change costs a file's formatting, when Shield
/// wrote `retained` from `previous` and the change rewrites the file in full or
/// drops comments. A JSON source was already projected in full, and a file
/// without previous text has nothing to lose.
pub(crate) fn notice(name: &str, previous: Option<&str>, retained: Option<&str>) -> Option<String> {
    let (previous, retained) = (previous?, retained?);
    if previous == retained || serde_json::from_str::<Value>(previous).is_ok() {
        return None;
    }
    let target = policy::parse::<Value>(retained).ok()?;
    let written = write(Some(previous), &target).ok()?;
    if written.text != retained {
        return None;
    }
    let comments = match written.dropped_comments {
        0 => None,
        1 => Some("1 comment".to_owned()),
        count => Some(format!("{count} comments")),
    };
    match (written.rendered, comments) {
        (true, None) => Some(format!("{name} will be rewritten in Shield's layout.")),
        (true, Some(comments)) => Some(format!(
            "{name} will be rewritten in Shield's layout; its {comments} will not be kept."
        )),
        (false, Some(comments)) => Some(format!(
            "{name}: {comments} in the changed entries will not be kept."
        )),
        (false, None) => None,
    }
}

/// The full rendering below the previous text's leading comment block.
fn fallback(previous: &str, target: &Value, comments: usize) -> Result<Written, Error> {
    let header = leading_block(previous);
    let kept = header
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .count();
    let crlf = previous.contains("\r\n");
    Ok(Written {
        text: format!("{header}{}", line_endings(render(target)?, crlf)),
        rendered: true,
        dropped_comments: comments.saturating_sub(kept),
    })
}

/// The comment and blank lines before the first YAML token, when they hold a
/// comment.
fn leading_block(text: &str) -> &str {
    let mut end = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            break;
        }
        end += line.len();
    }
    let block = &text[..end];
    if block.contains('#') { block } else { "" }
}

fn line_endings(text: String, crlf: bool) -> String {
    if crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    }
}

/// An edit cannot be made or verified; the file is rendered in full.
#[derive(Debug)]
struct Fallback;

/// A mapping's entries with the [`LEADING`] fields first.
fn ordered(fields: &Map<String, Value>) -> impl Iterator<Item = (&str, &Value)> {
    let leading = LEADING
        .iter()
        .filter_map(|key| fields.get(*key).map(|value| (*key, value)));
    let rest = fields
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .filter(|(key, _)| !LEADING.contains(key));
    leading.chain(rest)
}

/// Indentation and line endings of rendered text.
#[derive(Clone, Copy)]
struct Layout {
    indent_step: usize,
    compact_sequences: bool,
    crlf: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            indent_step: 2,
            compact_sequences: true,
            crlf: false,
        }
    }
}

impl Layout {
    fn render(self, value: &Value) -> Result<String, Fallback> {
        self.block(value, 0)
    }

    /// Rendered block text whose lines start at `column`.
    fn block(self, value: &Value, column: usize) -> Result<String, Fallback> {
        let mut text = String::new();
        self.lines(value, column, true, &mut text)?;
        Ok(line_endings(text, self.crlf))
    }

    /// Appends `value` as block lines at `column`. The first line is not
    /// indented when it continues a sequence entry's dash.
    fn lines(
        self,
        value: &Value,
        column: usize,
        indent_first: bool,
        text: &mut String,
    ) -> Result<(), Fallback> {
        let indent = " ".repeat(column);
        match value {
            Value::Object(fields) if !fields.is_empty() => {
                for (index, (key, value)) in ordered(fields).enumerate() {
                    if indent_first || index > 0 {
                        text.push_str(&indent);
                    }
                    text.push_str(&Self::scalar(&Value::String(key.into()))?);
                    text.push(':');
                    match value {
                        Value::Object(nested) if !nested.is_empty() => {
                            text.push('\n');
                            self.lines(value, column + self.indent_step, true, text)?;
                        }
                        Value::Array(values) if !values.is_empty() => {
                            text.push('\n');
                            let items = if self.compact_sequences {
                                column
                            } else {
                                column + self.indent_step
                            };
                            self.lines(value, items, true, text)?;
                        }
                        value => {
                            text.push(' ');
                            text.push_str(&Self::scalar(value)?);
                            text.push('\n');
                        }
                    }
                }
            }
            Value::Array(values) if !values.is_empty() => {
                for (index, value) in values.iter().enumerate() {
                    if indent_first || index > 0 {
                        text.push_str(&indent);
                    }
                    text.push_str("- ");
                    self.lines(value, column + 2, false, text)?;
                }
            }
            value => {
                if indent_first {
                    text.push_str(&indent);
                }
                text.push_str(&Self::scalar(value)?);
                text.push('\n');
            }
        }
        Ok(())
    }

    /// One scalar's token text; empty collections are written as `[]` and `{}`.
    fn scalar(value: &Value) -> Result<String, Fallback> {
        match value {
            Value::Array(values) if values.is_empty() => return Ok("[]".into()),
            Value::Object(fields) if fields.is_empty() => return Ok("{}".into()),
            Value::Array(_) | Value::Object(_) => return Err(Fallback),
            _ => {}
        }
        let options = serde_saphyr::ser_options! { prefer_block_scalars: false };
        let text = serde_saphyr::to_string_with_options(value, options).map_err(|_| Fallback)?;
        let token = text.trim_end_matches(['\n', '\r']);
        // A string the serializer spreads over lines, such as one holding a
        // line break, is written as one double-quoted line: JSON's escapes are
        // also YAML's.
        if let Value::String(_) = value
            && token.contains('\n')
        {
            return serde_json::to_string(value).map_err(|_| Fallback);
        }
        if token.is_empty()
            || token.contains('\n')
            || token.starts_with("---")
            || token.starts_with("...")
        {
            return Err(Fallback);
        }
        Ok(token.into())
    }
}

/// A parsed node: its byte range from the first to the last byte of its
/// content, excluding comments and line breaks after it.
struct Node {
    kind: Kind,
    flow: bool,
    range: Range<usize>,
}

enum Kind {
    /// A scalar with its decoded text; an empty scalar has no token to replace.
    Scalar {
        value: String,
        style: ScalarStyle,
        empty: bool,
    },
    Sequence(Vec<Node>),
    Mapping(Vec<(Node, Node)>),
}

/// Previous text parsed into nodes with byte ranges, its comment positions and
/// its layout.
struct Source<'a> {
    text: &'a str,
    root: Node,
    comments: Vec<usize>,
    layout: Layout,
}

impl<'a> Source<'a> {
    /// Parse one YAML document without anchors, aliases or tags.
    fn parse(text: &'a str) -> Option<Self> {
        let mut events = Vec::new();
        let mut comments = Vec::new();
        let mut documents = 0;
        for next in Parser::new_from_str(text) {
            let (event, span) = next.ok()?;
            let range = span.byte_range()?;
            match event {
                Event::Comment(..) => comments.push(range.start),
                Event::DocumentStart(..) => {
                    documents += 1;
                    if documents > 1 {
                        return None;
                    }
                }
                Event::StreamStart | Event::StreamEnd | Event::DocumentEnd => {}
                event => events.push((event, range)),
            }
        }
        let mut builder = Builder {
            events: &events,
            next: 0,
        };
        let root = builder.node()?;
        if builder.next != events.len() {
            return None;
        }
        let mut layout = Layout {
            crlf: text.contains("\r\n"),
            ..Layout::default()
        };
        let mut found = (None, None);
        detect(text, &root, &mut found);
        if let Some(step) = found.0 {
            layout.indent_step = step;
        }
        if let Some(compact) = found.1 {
            layout.compact_sequences = compact;
        }
        Some(Self {
            text,
            root,
            comments,
            layout,
        })
    }

    fn edit(&self, before: &Value, target: &Value) -> Result<Written, Fallback> {
        let mut editor = Editor {
            source: self,
            edits: Vec::new(),
            dropped: 0,
        };
        if !matches!(self.root.kind, Kind::Mapping(_)) || self.root.flow {
            return Err(Fallback);
        }
        if editor.diff(&self.root, before, target, &mut Vec::new())? == Change::Replace {
            return Err(Fallback);
        }
        let text = editor.apply()?;
        if policy::parse::<Value>(&text).ok().as_ref() != Some(target) {
            return Err(Fallback);
        }
        Ok(Written {
            text,
            rendered: false,
            dropped_comments: editor.dropped,
        })
    }
}

struct Builder<'e, 'a> {
    events: &'e [(Event<'a>, Range<usize>)],
    next: usize,
}

impl Builder<'_, '_> {
    fn node(&mut self) -> Option<Node> {
        let events = self.events;
        let (event, range) = events.get(self.next)?;
        self.next += 1;
        match event {
            Event::Scalar(value, style, anchor, tag) => {
                if *anchor != 0 || tag.is_some() {
                    return None;
                }
                Some(Node {
                    kind: Kind::Scalar {
                        value: value.to_string(),
                        style: *style,
                        empty: range.is_empty(),
                    },
                    flow: false,
                    range: range.clone(),
                })
            }
            Event::SequenceStart(style, anchor, tag) => {
                if *anchor != 0 || tag.is_some() {
                    return None;
                }
                let mut items = Vec::new();
                loop {
                    if let (Event::SequenceEnd, end) = events.get(self.next)? {
                        self.next += 1;
                        return Some(collection(
                            *style,
                            range,
                            end,
                            items.last().map(|item: &Node| item.range.end),
                            Kind::Sequence(items),
                        ));
                    }
                    items.push(self.node()?);
                }
            }
            Event::MappingStart(style, anchor, tag) => {
                if *anchor != 0 || tag.is_some() {
                    return None;
                }
                let mut entries = Vec::new();
                loop {
                    if let (Event::MappingEnd, end) = events.get(self.next)? {
                        self.next += 1;
                        let last = entries
                            .last()
                            .map(|(key, value): &(Node, Node)| key.range.end.max(value.range.end));
                        return Some(collection(*style, range, end, last, Kind::Mapping(entries)));
                    }
                    let key = self.node()?;
                    if !matches!(key.kind, Kind::Scalar { empty: false, .. }) {
                        return None;
                    }
                    let mut value = self.node()?;
                    // An empty value has no token; it ends with its key.
                    if matches!(value.kind, Kind::Scalar { empty: true, .. }) {
                        value.range = key.range.end..key.range.end;
                    }
                    entries.push((key, value));
                }
            }
            _ => None,
        }
    }
}

/// A collection node: flow collections end at their closing bracket, block
/// collections at their last content.
fn collection(
    style: StructureStyle,
    start: &Range<usize>,
    end: &Range<usize>,
    last: Option<usize>,
    kind: Kind,
) -> Node {
    let flow = style == StructureStyle::Flow;
    let end = if flow {
        end.end
    } else {
        last.unwrap_or(start.start)
    };
    Node {
        kind,
        flow,
        range: start.start..end,
    }
}

/// The first nested block mapping's indentation step and whether the first
/// block sequence under a key starts at the key's column.
fn detect(text: &str, node: &Node, found: &mut (Option<usize>, Option<bool>)) {
    match &node.kind {
        Kind::Mapping(entries) => {
            for (key, value) in entries {
                if !value.flow {
                    let key_column = column(text, key.range.start);
                    match &value.kind {
                        Kind::Mapping(inner) if found.0.is_none() => {
                            if let Some((first, _)) = inner.first() {
                                let step = column(text, first.range.start)
                                    .checked_sub(key_column)
                                    .filter(|step| (1..=8).contains(step));
                                found.0 = step;
                            }
                        }
                        Kind::Sequence(items) if found.1.is_none() => {
                            // An indentless sequence's span starts at its first item.
                            if let Some(dash) = items.first().and_then(|item| dash(text, item)) {
                                found.1 = Some(column(text, dash) == key_column);
                            }
                        }
                        _ => {}
                    }
                }
                detect(text, value, found);
            }
        }
        Kind::Sequence(items) => {
            for item in items {
                detect(text, item, found);
            }
        }
        Kind::Scalar { .. } => {}
    }
}

/// The offset of a block sequence entry's dash.
fn dash(text: &str, item: &Node) -> Option<usize> {
    text[..item.range.start]
        .trim_end_matches([' ', '\n', '\r'])
        .strip_suffix('-')
        .map(str::len)
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |index| index + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..]
        .find('\n')
        .map_or(text.len(), |index| at + index + 1)
}

fn column(text: &str, at: usize) -> usize {
    at - line_start(text, at)
}

/// The start of the own-line comments directly above the line at `start`.
fn leading_start(text: &str, start: usize) -> usize {
    let mut start = start;
    while start > 0 {
        let previous = line_start(text, start - 1);
        if !text[previous..start].trim_start().starts_with('#') {
            break;
        }
        start = previous;
    }
    start
}

/// Whether `range` holds only spaces.
fn blank(text: &str, range: Range<usize>) -> bool {
    text[range].bytes().all(|byte| byte == b' ')
}

/// Entries of identity sequences match by identity; other sequences are values.
fn identity(path: &[String], value: &Value) -> Option<String> {
    match path {
        // Selections match by ID alone, so adding `registry_identity` edits
        // an entry; two selections of one ID fall back to a full rendering.
        [field] if field == "rules" || field == "packs" => value["id"].as_str().map(str::to_owned),
        [field] if field == "capabilities" => value.as_str().map(str::to_owned),
        [parent, field] if parent == "access_paths" && (field == "read" || field == "write") => {
            value.as_str().map(str::to_owned)
        }
        _ => None,
    }
}

fn identified(path: &[String]) -> bool {
    matches!(path, [field] if field == "rules" || field == "packs" || field == "capabilities")
        || matches!(path, [parent, field] if parent == "access_paths" && (field == "read" || field == "write"))
}

/// Unique identities of a sequence's entries, when every entry has one.
fn identities(path: &[String], values: &[Value]) -> Result<Vec<String>, Fallback> {
    let ids: Vec<String> = values
        .iter()
        .map(|value| identity(path, value))
        .collect::<Option<_>>()
        .ok_or(Fallback)?;
    let mut sorted: Vec<&String> = ids.iter().collect();
    sorted.sort();
    sorted.dedup();
    if sorted.len() != ids.len() {
        return Err(Fallback);
    }
    Ok(ids)
}

#[derive(Debug, PartialEq, Eq)]
enum Change {
    /// The node's text was kept or edited in place.
    Done,
    /// The node cannot be edited in place; its parent replaces it.
    Replace,
}

struct Edit {
    range: Range<usize>,
    text: String,
}

struct Editor<'s, 'a> {
    source: &'s Source<'a>,
    edits: Vec<Edit>,
    dropped: usize,
}

impl Editor<'_, '_> {
    fn text(&self) -> &str {
        self.source.text
    }

    fn layout(&self) -> Layout {
        self.source.layout
    }

    /// Edit `node` from `before` to `after`. Edits recorded for a node that
    /// its parent must replace are discarded.
    fn diff(
        &mut self,
        node: &Node,
        before: &Value,
        after: &Value,
        path: &mut Vec<String>,
    ) -> Result<Change, Fallback> {
        if before == after {
            return Ok(Change::Done);
        }
        let (edits, dropped) = (self.edits.len(), self.dropped);
        let change = match (&node.kind, before, after) {
            (Kind::Mapping(entries), Value::Object(old), Value::Object(new)) if !node.flow => {
                self.mapping(entries, old, new, path)?
            }
            (Kind::Sequence(items), Value::Array(old), Value::Array(new))
                if !node.flow && identified(path) =>
            {
                self.sequence(items, old, new, path)?
            }
            (
                Kind::Scalar {
                    style:
                        ScalarStyle::Plain | ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted,
                    empty: false,
                    ..
                },
                _,
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_),
            ) if !self.text()[node.range.clone()].contains('\n') => {
                let token = Layout::scalar(after)?;
                self.replace(node.range.clone(), token);
                Change::Done
            }
            _ => Change::Replace,
        };
        if change == Change::Replace {
            self.edits.truncate(edits);
            self.dropped = dropped;
        }
        Ok(change)
    }

    fn mapping(
        &mut self,
        entries: &[(Node, Node)],
        old: &Map<String, Value>,
        new: &Map<String, Value>,
        path: &mut Vec<String>,
    ) -> Result<Change, Fallback> {
        for (key, value) in entries {
            let Kind::Scalar { value: name, .. } = &key.kind else {
                return Err(Fallback);
            };
            let before = old.get(name).ok_or(Fallback)?;
            match new.get(name) {
                None => {
                    let Some(lines) = self.entry_lines(key, value) else {
                        return Ok(Change::Replace);
                    };
                    // The file's leading comment block is not the first key's.
                    let start = leading_start(self.text(), lines.start)
                        .max(leading_block(self.text()).len());
                    self.remove(start..lines.end);
                }
                Some(after) => {
                    path.push(name.clone());
                    let change = self.diff(value, before, after, path)?;
                    path.pop();
                    if change == Change::Replace {
                        let Some(lines) = self.entry_lines(key, value) else {
                            return Ok(Change::Replace);
                        };
                        let mut single = Map::new();
                        single.insert(name.clone(), after.clone());
                        let indent = column(self.text(), key.range.start);
                        let text = self.layout().block(&Value::Object(single), indent)?;
                        let removed = self.emptied_comments(value, after);
                        self.replace(lines, text);
                        self.dropped = self.dropped.saturating_sub(removed);
                    }
                }
            }
        }
        let added: Map<String, Value> = new
            .iter()
            .filter(|(name, _)| !old.contains_key(*name))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        if !added.is_empty() {
            let (first, _) = entries.first().ok_or(Fallback)?;
            let end = entries
                .iter()
                .map(|(key, value)| key.range.end.max(value.range.end))
                .max()
                .ok_or(Fallback)?;
            let indent = column(self.text(), first.range.start);
            let text = self.layout().block(&Value::Object(added), indent)?;
            self.insert(line_end(self.text(), end), text);
        }
        Ok(Change::Done)
    }

    fn sequence(
        &mut self,
        items: &[Node],
        old: &[Value],
        new: &[Value],
        path: &mut Vec<String>,
    ) -> Result<Change, Fallback> {
        if items.len() != old.len() {
            return Err(Fallback);
        }
        // A block sequence cannot be empty; its parent writes `[]`.
        if new.is_empty() {
            return Ok(Change::Replace);
        }
        let before = identities(path, old)?;
        let after = identities(path, new)?;
        // Existing entries keep their order.
        let mut previous = None;
        for id in &after {
            if let Some(index) = before.iter().position(|known| known == id) {
                if previous.is_some_and(|last| index < last) {
                    return Err(Fallback);
                }
                previous = Some(index);
            }
        }
        let Some(lines) = items
            .iter()
            .map(|item| self.item_lines(item))
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(Change::Replace);
        };
        let mut pending = Vec::new();
        for (index, id) in after.iter().enumerate() {
            let Some(at) = before.iter().position(|known| known == id) else {
                pending.push(new[index].clone());
                continue;
            };
            if !pending.is_empty() {
                let text = self.items(&std::mem::take(&mut pending), lines[at].column)?;
                self.insert(lines[at].leading, text);
            }
            path.push("[]".into());
            let change = self.diff(&items[at], &old[at], &new[index], path)?;
            path.pop();
            if change == Change::Replace {
                let text = self.items(std::slice::from_ref(&new[index]), lines[at].column)?;
                self.replace(lines[at].start..lines[at].end, text);
            }
        }
        for (index, id) in before.iter().enumerate() {
            if !after.contains(id) {
                self.remove(lines[index].leading..lines[index].end);
            }
        }
        if !pending.is_empty() {
            let last = lines.last().ok_or(Fallback)?;
            let text = self.items(&pending, last.column)?;
            self.insert(last.end, text);
        }
        Ok(Change::Done)
    }

    /// The whole lines of a mapping entry, when its key starts its line.
    fn entry_lines(&self, key: &Node, value: &Node) -> Option<Range<usize>> {
        let start = line_start(self.text(), key.range.start);
        blank(self.text(), start..key.range.start)
            .then(|| start..line_end(self.text(), key.range.end.max(value.range.end)))
    }

    /// The lines of a block sequence entry, when its dash starts its line.
    fn item_lines(&self, item: &Node) -> Option<ItemLines> {
        let text = self.text();
        let dash = dash(text, item)?;
        let start = line_start(text, dash);
        blank(text, start..dash).then(|| ItemLines {
            leading: leading_start(text, start),
            start,
            end: line_end(text, item.range.end),
            column: dash - start,
        })
    }

    /// Comments of the entries of a block sequence that becomes empty: they
    /// go with their entries, as for any removed entry.
    fn emptied_comments(&self, node: &Node, after: &Value) -> usize {
        let Kind::Sequence(items) = &node.kind else {
            return 0;
        };
        if node.flow || after.as_array().is_none_or(|values| !values.is_empty()) {
            return 0;
        }
        items
            .iter()
            .filter_map(|item| self.item_lines(item))
            .map(|lines| {
                self.source
                    .comments
                    .iter()
                    .filter(|at| (lines.leading..lines.end).contains(at))
                    .count()
            })
            .sum()
    }

    /// Rendered sequence entries with their dashes at `column`.
    fn items(&self, values: &[Value], column: usize) -> Result<String, Fallback> {
        self.layout().block(&Value::Array(values.to_vec()), column)
    }

    fn replace(&mut self, range: Range<usize>, text: String) {
        self.dropped += self
            .source
            .comments
            .iter()
            .filter(|at| range.contains(at))
            .count();
        self.edits.push(Edit { range, text });
    }

    fn remove(&mut self, range: Range<usize>) {
        self.edits.push(Edit {
            range,
            text: String::new(),
        });
    }

    fn insert(&mut self, at: usize, mut text: String) {
        let source = self.text();
        if at == source.len() && !source.is_empty() && !source.ends_with('\n') {
            text.insert_str(0, if self.layout().crlf { "\r\n" } else { "\n" });
        }
        self.edits.push(Edit {
            range: at..at,
            text,
        });
    }

    /// The previous text with every edit applied. Edits at the same position
    /// keep the order they were made in; overlapping edits cannot be applied.
    fn apply(&self) -> Result<String, Fallback> {
        let mut order: Vec<usize> = (0..self.edits.len()).collect();
        order.sort_by_key(|index| (self.edits[*index].range.start, *index));
        let mut text = String::with_capacity(self.text().len());
        let mut at = 0;
        for index in order {
            let edit = &self.edits[index];
            if edit.range.start < at {
                return Err(Fallback);
            }
            text.push_str(&self.text()[at..edit.range.start]);
            text.push_str(&edit.text);
            at = edit.range.end;
        }
        text.push_str(&self.text()[at..]);
        Ok(text)
    }
}

/// Where a block sequence entry's text lies: its own-line comments above, its
/// dash line, the end of its last line and its dash column.
struct ItemLines {
    leading: usize,
    start: usize,
    end: usize,
    column: usize,
}
