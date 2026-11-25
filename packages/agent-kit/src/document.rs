//! Surgical JSON/JSONC entry editing and TOML's formatting-preserving codec.
use super::{Error, Format, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::ops::Range;
use std::rc::Rc;

/// Recovery guidance for a configuration document that contains Git markers.
pub const GIT_CONFLICT_MESSAGE: &str =
    "Unresolved Git conflict markers. Resolve the conflict and retry.";

/// The one-based opening line of the first complete Git conflict block.
/// Checks column-zero markers of equal width, including diff3 and zdiff3
/// blocks. Callers pass their already-read, bounded document after parsing
/// fails. The result contains no branch names or document values.
#[must_use]
pub fn git_conflict_line(text: &str) -> Option<usize> {
    let marker = |line: &str, prefix| {
        let rest = line.trim_start_matches(prefix);
        let width = line.len() - rest.len();
        (width > 0 && (rest.is_empty() || rest.starts_with(' '))).then_some(width)
    };
    let mut opening = None;
    let mut separated = false;
    for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        if let Some(width) = marker(line, '<') {
            opening = Some((index + 1, width));
            separated = false;
        } else if let Some((start, width)) = opening {
            if line.len() == width && line.bytes().all(|byte| byte == b'=') {
                separated = true;
            } else if separated && marker(line, '>') == Some(width) {
                return Some(start);
            }
        }
    }
    None
}

pub(crate) struct Prepared {
    /// Shared with the command's file snapshot rather than copied.
    source: Rc<str>,
    parsed: Parsed,
}

#[cfg(test)]
thread_local! {
    static EDITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn count_edit() {
    #[cfg(test)]
    EDITS.with(|edits| edits.set(edits.get() + 1));
}

/// Complete edited documents produced on this thread, for work-count regressions.
#[cfg(test)]
pub(crate) fn edits() -> usize {
    EDITS.with(std::cell::Cell::get)
}

enum Parsed {
    Json(Node),
    Toml(toml_edit::DocumentMut),
    Yaml(Value),
}

impl Prepared {
    pub(crate) fn parse(source: impl Into<Rc<str>>, format: Format) -> Result<Self> {
        let source = source.into();
        let parsed = match format {
            Format::Json | Format::Jsonc => {
                Parsed::Json(Parser::parse(&source, format == Format::Json)?)
            }
            Format::Toml => Parsed::Toml(
                source
                    .parse()
                    .map_err(|_| Error::invalid("Invalid TOML configuration"))?,
            ),
            Format::Yaml => Parsed::Yaml(
                serde_saphyr::from_str(&source)
                    .map_err(|_| Error::invalid("Invalid YAML configuration"))?,
            ),
        };
        Ok(Self { source, parsed })
    }

    pub(crate) fn edit(
        &self,
        key: &str,
        name: &str,
        desired: Option<&Value>,
    ) -> Result<(Option<Value>, String)> {
        self.edit_path(&[key], name, desired)
    }

    /// The current named entry, with the same validation as [`Self::edit`] but
    /// without producing an edited document.
    pub(crate) fn entry(&self, key: &str, name: &str) -> Result<Option<Value>> {
        match &self.parsed {
            Parsed::Json(root) => Ok(json_group(root, &[key])?
                .ok()
                .and_then(|group| group.members.iter().find(|m| m.key == name))
                .map(|entry| entry.node.to_value())),
            Parsed::Toml(document) => toml_entry(document, key, name),
            Parsed::Yaml(root) => yaml_entry(root, key, name),
        }
    }

    /// Read and validate the current `permissions.allow` rules.
    pub(crate) fn allowed(&self) -> Result<Vec<String>> {
        let Parsed::Json(root) = &self.parsed else {
            return Err(Error::invalid(
                "Permission rules require JSON configuration",
            ));
        };
        permission_rules(root, "allow")
    }

    pub(crate) fn plan_allow_list(
        &self,
        remove: &[String],
        add: &[String],
    ) -> Result<(Vec<String>, Option<Vec<String>>)> {
        let current = self.allowed()?;
        let updated = updated_permission_rules(&current, remove, add);
        Ok((current, updated))
    }

    /// Render changed settings fields together against the original ranges.
    pub(crate) fn edit_settings(
        &self,
        rules: Option<(&str, &[String])>,
        hooks: Option<(&str, &[Value])>,
    ) -> Result<String> {
        count_edit();
        let Parsed::Json(root) = &self.parsed else {
            return Err(Error::invalid("Claude settings require JSON configuration"));
        };
        let mut writes = Vec::new();
        if let Some((decision, rules)) = rules {
            writes.push(JsonWrite::field(
                root,
                "permissions",
                decision,
                serde_json::json!(rules),
            )?);
        }
        if let Some((event, groups)) = hooks {
            writes.push(JsonWrite::field(
                root,
                "hooks",
                event,
                serde_json::json!(groups),
            )?);
        }
        let mut edits = Vec::new();
        if let [first, second] = writes.as_slice()
            && std::ptr::eq(first.object, second.object)
        {
            // Both parents are absent. Append them together at the root.
            append_members(
                &self.source,
                first.object,
                &[(first.key, &first.value), (second.key, &second.value)],
                &mut edits,
            )?;
        } else {
            for write in writes {
                if let Some(entry) = write
                    .object
                    .members
                    .iter()
                    .find(|member| member.key == write.key)
                {
                    edits.push((
                        entry.node.range.clone(),
                        serde_json::to_string_pretty(&write.value)
                            .expect("serializing a JSON value cannot fail"),
                    ));
                } else {
                    append_members(
                        &self.source,
                        write.object,
                        &[(write.key, &write.value)],
                        &mut edits,
                    )?;
                }
            }
        }
        Ok(apply_text_edits(&self.source, edits))
    }

    fn edit_path(
        &self,
        path: &[&str],
        name: &str,
        desired: Option<&Value>,
    ) -> Result<(Option<Value>, String)> {
        count_edit();
        match (&self.parsed, path) {
            (Parsed::Json(root), _) => json_edit(&self.source, root, path, name, desired),
            (Parsed::Toml(document), [key]) => {
                toml_edit(&self.source, document, key, name, desired)
            }
            (Parsed::Yaml(root), [key]) => yaml_edit(&self.source, root, key, name, desired),
            _ => Err(Error::invalid(
                "Nested entry paths are supported only in JSON configuration",
            )),
        }
    }

    fn permission_list(
        &self,
        decision: &str,
        remove: &[String],
        add: &[String],
    ) -> Result<(Vec<String>, String)> {
        let Parsed::Json(root) = &self.parsed else {
            return Err(Error::invalid(
                "Permission rules require JSON configuration",
            ));
        };
        let current = permission_rules(root, decision)?;
        let updated = updated_permission_rules(&current, remove, add);
        let edited = self.edit_settings(updated.as_deref().map(|rules| (decision, rules)), None)?;
        Ok((current, edited))
    }

    /// The groups under `hooks.<event>`: JSON hook files, or Codex's inline
    /// TOML `[[hooks.<event>]]` tables, which are read but never edited.
    pub(crate) fn hook_groups(&self, event: &str) -> Result<Vec<Value>> {
        let list_error = || Error::invalid(format!("Hooks for {event} must be a list"));
        let root = match &self.parsed {
            Parsed::Json(root) => root,
            Parsed::Toml(document) => {
                let Some(item) = document.get("hooks").and_then(|hooks| hooks.get(event)) else {
                    return Ok(Vec::new());
                };
                let Value::Array(groups) = toml_item(item, 0)? else {
                    return Err(list_error());
                };
                return Ok(groups);
            }
            Parsed::Yaml(_) => return Err(Error::invalid("Hooks require JSON configuration")),
        };
        let Some(hooks) = root.members.iter().find(|m| m.key == "hooks") else {
            return Ok(Vec::new());
        };
        if !hooks.node.value.is_object() {
            return Err(Error::invalid("Hooks must be an object of event lists"));
        }
        match hooks.node.members.iter().find(|m| m.key == event) {
            Some(list) if list.node.value.is_array() => {
                Ok(list.node.elements.iter().map(Node::to_value).collect())
            }
            Some(_) => Err(list_error()),
            None => Ok(Vec::new()),
        }
    }
}

/// Replace or remove one named entry, returning its previous value and edited text.
///
/// # Errors
/// Returns an error for malformed or ambiguous configuration or unsupported values.
///
/// ```
/// use canonic_agent_kit::{Format, document};
/// use serde_json::json;
///
/// let source = r#"{"mcpServers":{"other":{"command":"other"}},"theme":"dark"}"#;
/// let desired = json!({"command": "acme", "args": ["mcp", "serve"]});
/// let (previous, edited) = document::edit(
///     source, Format::Json, "mcpServers", "acme", Some(&desired),
/// )?;
/// assert!(previous.is_none());
/// let parsed: serde_json::Value = serde_json::from_str(&edited)?;
/// assert_eq!(parsed["mcpServers"]["acme"], desired);
/// assert_eq!(parsed["mcpServers"]["other"]["command"], "other");
/// assert_eq!(parsed["theme"], "dark");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn edit(
    source: &str,
    format: Format,
    key: &str,
    name: &str,
    desired: Option<&Value>,
) -> Result<(Option<Value>, String)> {
    Prepared::parse(source, format)?.edit(key, name, desired)
}

fn yaml_entry(root: &Value, key: &str, name: &str) -> Result<Option<Value>> {
    let object = root
        .as_object()
        .ok_or_else(|| Error::invalid("YAML configuration must be an object"))?;
    let Some(group) = object.get(key) else {
        return Ok(None);
    };
    Ok(group
        .as_object()
        .ok_or_else(|| Error::invalid("YAML extensions must be a mapping"))?
        .get(name)
        .cloned())
}

fn yaml_edit(
    source: &str,
    root: &Value,
    key: &str,
    name: &str,
    desired: Option<&Value>,
) -> Result<(Option<Value>, String)> {
    // Validate and compare before cloning the whole document.
    let before = yaml_entry(root, key, name)?;
    if before.as_ref() == desired {
        return Ok((before, source.into()));
    }
    let mut root = root.clone();
    let group = root
        .as_object_mut()
        .and_then(|object| {
            object
                .entry(key)
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
        })
        .expect("yaml_entry validated the root and collection");
    if let Some(desired) = desired {
        group.insert(name.into(), desired.clone());
    } else {
        group.remove(name);
    }
    // JSON is a YAML subset. Preserve all unrelated values, without needing a
    // second YAML serializer; the review explicitly reports this normalization.
    Ok((
        before,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&root).expect("serializing a JSON value cannot fail")
        ),
    ))
}

struct Node {
    value: Value,
    range: Range<usize>,
    members: Vec<Member>,
    elements: Vec<Node>,
    depth: usize,
}

impl Node {
    fn to_value(&self) -> Value {
        match &self.value {
            Value::Object(_) => self
                .members
                .iter()
                .map(|m| (m.key.clone(), m.node.to_value()))
                .collect::<serde_json::Map<_, _>>()
                .into(),
            Value::Array(_) => Value::Array(self.elements.iter().map(Self::to_value).collect()),
            value => value.clone(),
        }
    }
}
struct Member {
    key: String,
    start: usize,
    node: Node,
    comma: Option<usize>,
}
struct Parser<'a> {
    source: &'a str,
    position: usize,
    strict: bool,
}
impl Parser<'_> {
    fn parse(source: &str, strict: bool) -> Result<Node> {
        let mut parser = Parser {
            source,
            position: 0,
            strict,
        };
        let root = parser.node(0)?;
        parser.skip()?;
        if parser.position != source.len() || !root.value.is_object() {
            return Err(Error::invalid("Host configuration must be one JSON object"));
        }
        Ok(root)
    }

    fn byte(&self) -> Option<u8> {
        self.source.as_bytes().get(self.position).copied()
    }
    fn skip(&mut self) -> Result<()> {
        loop {
            while self.byte().is_some_and(|b| b.is_ascii_whitespace()) {
                if self.strict && !self.byte().is_some_and(|b| b" \t\r\n".contains(&b)) {
                    return Err(Error::invalid("Invalid JSON whitespace"));
                }
                self.position += 1;
            }
            if self.source[self.position..].starts_with("//") {
                if self.strict {
                    return Err(Error::invalid("Comments require JSONC"));
                }
                while self.byte().is_some_and(|b| b != b'\n') {
                    self.position += 1;
                }
            } else if self.source[self.position..].starts_with("/*") {
                if self.strict {
                    return Err(Error::invalid("Comments require JSONC"));
                }
                let end = self.source[self.position + 2..]
                    .find("*/")
                    .ok_or_else(|| Error::invalid("Unclosed JSON comment"))?;
                self.position += end + 4;
            } else {
                return Ok(());
            }
        }
    }
    fn string(&mut self) -> Result<String> {
        let start = self.position;
        self.position += 1;
        while let Some(byte) = self.byte() {
            self.position += 1;
            if byte == b'\\' {
                if self.byte().is_none() {
                    break;
                }
                self.position += 1;
            } else if byte == b'"' {
                return serde_json::from_str(&self.source[start..self.position])
                    .map_err(|_| Error::invalid("Invalid JSON string"));
            }
        }
        Err(Error::invalid("Unclosed JSON string"))
    }
    fn node(&mut self, depth: usize) -> Result<Node> {
        if depth > 128 {
            return Err(Error::invalid("JSON nesting limit exceeded"));
        }
        self.skip()?;
        let start = self.position;
        let mut members = Vec::new();
        let mut elements = Vec::new();
        let value = match self.byte() {
            Some(b'{') => {
                self.position += 1;
                self.skip()?;
                let mut keys = BTreeSet::new();
                while self.byte() != Some(b'}') {
                    if self.byte() != Some(b'"') {
                        return Err(Error::invalid("Expected JSON object key"));
                    }
                    let member_start = self.position;
                    let key = self.string()?;
                    self.skip()?;
                    if self.byte() != Some(b':') {
                        return Err(Error::invalid("Expected JSON colon"));
                    }
                    self.position += 1;
                    let node = self.node(depth + 1)?;
                    if !keys.insert(key.clone()) {
                        return Err(Error::invalid("Duplicate JSON key"));
                    }
                    self.skip()?;
                    let comma = if self.byte() == Some(b',') {
                        let index = self.position;
                        self.position += 1;
                        self.skip()?;
                        if self.strict && self.byte() == Some(b'}') {
                            return Err(Error::invalid("Trailing commas require JSONC"));
                        }
                        Some(index)
                    } else {
                        None
                    };
                    members.push(Member {
                        key,
                        start: member_start,
                        node,
                        comma,
                    });
                    if comma.is_none() && self.byte() != Some(b'}') {
                        return Err(Error::invalid("Expected JSON comma"));
                    }
                }
                self.position += 1;
                Value::Object(serde_json::Map::new())
            }
            Some(b'[') => {
                self.position += 1;
                self.skip()?;
                while self.byte() != Some(b']') {
                    elements.push(self.node(depth + 1)?);
                    self.skip()?;
                    if self.byte() == Some(b',') {
                        self.position += 1;
                        self.skip()?;
                        if self.strict && self.byte() == Some(b']') {
                            return Err(Error::invalid("Trailing commas require JSONC"));
                        }
                    } else if self.byte() != Some(b']') {
                        return Err(Error::invalid("Expected JSON array comma"));
                    }
                }
                self.position += 1;
                Value::Array(Vec::new())
            }
            Some(b'"') => Value::String(self.string()?),
            Some(_) => {
                while self
                    .byte()
                    .is_some_and(|b| !b.is_ascii_whitespace() && !b",}]/".contains(&b))
                {
                    self.position += 1;
                }
                serde_json::from_str(&self.source[start..self.position])
                    .map_err(|_| Error::invalid("Invalid JSON value"))?
            }
            None => return Err(Error::invalid("Incomplete JSON document")),
        };
        Ok(Node {
            value,
            range: start..self.position,
            members,
            elements,
            depth,
        })
    }
}

struct JsonWrite<'a> {
    object: &'a Node,
    key: &'a str,
    value: Value,
}

impl<'a> JsonWrite<'a> {
    fn field(root: &'a Node, parent: &'a str, key: &'a str, value: Value) -> Result<Self> {
        Ok(match json_group(root, &[parent])? {
            Ok(object) => Self { object, key, value },
            Err(_) => Self {
                object: root,
                key: parent,
                value: serde_json::json!({key: value}),
            },
        })
    }
}

type TextEdit = (Range<usize>, String);

fn apply_text_edits(source: &str, mut edits: Vec<TextEdit>) -> String {
    // Preserve the comma before a member inserted at the same byte.
    edits.sort_by_key(|(range, _)| range.start);
    let capacity = edits.iter().fold(source.len(), |len, (range, text)| {
        len - range.len() + text.len()
    });
    let mut result = String::with_capacity(capacity);
    let mut cursor = 0;
    for (range, text) in edits {
        result.push_str(&source[cursor..range.start]);
        result.push_str(&text);
        cursor = range.end;
    }
    result.push_str(&source[cursor..]);
    result
}

fn insert(source: &str, object: &Node, name: &str, value: &Value) -> Result<String> {
    let mut edits = Vec::new();
    append_members(source, object, &[(name, value)], &mut edits)?;
    Ok(apply_text_edits(source, edits))
}

fn append_members(
    source: &str,
    object: &Node,
    members: &[(&str, &Value)],
    edits: &mut Vec<TextEdit>,
) -> Result<()> {
    if !object.value.is_object() {
        return Err(Error::invalid("MCP server collection must be an object"));
    }
    let unit = source
        .lines()
        .filter_map(|line| {
            let indent = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
            (!indent.is_empty() && line.trim_start().starts_with('"')).then_some(indent)
        })
        .min_by_key(|indent| indent.len())
        .unwrap_or("  ");
    let parent_indent = unit.repeat(object.depth);
    let child_indent = object
        .members
        .iter()
        .find_map(|member| {
            let line = source[..member.start].rsplit('\n').next()?;
            (line.chars().all(|c| c == ' ' || c == '\t') && !line.is_empty()).then_some(line)
        })
        .map_or_else(|| format!("{parent_indent}{unit}"), str::to_owned);
    let closing = object.range.end - 1;
    let line_start = source[..closing].rfind('\n').map_or(0, |index| index + 1);
    let own_line = source[line_start..closing]
        .chars()
        .all(|c| c == ' ' || c == '\t');
    let position = if own_line { line_start } else { closing };
    let prefix = if position > 0 && source.as_bytes()[position - 1] == b'\n' {
        ""
    } else {
        "\n"
    };
    let suffix = if own_line { "" } else { &parent_indent };
    let mut new = prefix.to_owned();
    for (index, (name, value)) in members.iter().enumerate() {
        if index > 0 {
            new.push_str(",\n");
        }
        let mut bytes = Vec::new();
        let mut serializer = serde_json::Serializer::with_formatter(
            &mut bytes,
            serde_json::ser::PrettyFormatter::with_indent(unit.as_bytes()),
        );
        value
            .serialize(&mut serializer)
            .map_err(|e| Error::invalid(e.to_string()))?;
        let encoded = String::from_utf8(bytes).map_err(|e| Error::invalid(e.to_string()))?;
        let encoded = encoded.replace('\n', &format!("\n{child_indent}"));
        new.push_str(&child_indent);
        new.push_str(&serde_json::to_string(name).expect("serializing a string cannot fail"));
        new.push_str(": ");
        new.push_str(&encoded);
    }
    new.push('\n');
    new.push_str(suffix);
    if let Some(last) = object.members.last()
        && last.comma.is_none()
    {
        let end = last.node.range.end;
        edits.push((end..end, ",".into()));
    }
    edits.push((position..position, new));
    Ok(())
}

/// Edit `name` in the object at `path` (one key per level), creating missing
/// levels only when writing. Other bytes, including comments, are kept.
///
/// # Errors
/// Returns an error for malformed or ambiguous configuration, unsupported values,
/// or nested paths in formats other than JSON/JSONC.
pub fn edit_path(
    source: &str,
    format: Format,
    path: &[&str],
    name: &str,
    desired: Option<&Value>,
) -> Result<(Option<Value>, String)> {
    Prepared::parse(source, format)?.edit_path(path, name, desired)
}

/// Add or remove `item` in the top-level string list `key` of a JSON object.
/// Returns whether the item was present and the edited document.
///
/// # Errors
/// Returns an error for malformed or ambiguous JSON, a non-object root, or a
/// selected member that is not a list of strings.
pub fn list_item(source: &str, key: &str, item: &str, present: bool) -> Result<(bool, String)> {
    let root = Parser::parse(source, true)?;
    let member = root.members.iter().find(|m| m.key == key);
    let current: Vec<String> = match member {
        Some(member) => serde_json::from_value(member.node.to_value())
            .map_err(|_| Error::invalid(format!("{key} must be a list of strings")))?,
        None => Vec::new(),
    };
    let was = current.iter().any(|value| value == item);
    if was == present {
        return Ok((was, source.into()));
    }
    let mut after: Vec<String> = current
        .iter()
        .filter(|value| *value != item)
        .cloned()
        .collect();
    if present {
        after.push(item.to_owned());
    }
    let list = serde_json::json!(after);
    let (_, edited) = json_edit(source, &root, &[], key, Some(&list))?;
    Ok((was, edited))
}

fn json_edit(
    source: &str,
    root: &Node,
    path: &[&str],
    name: &str,
    desired: Option<&Value>,
) -> Result<(Option<Value>, String)> {
    let group = match json_group(root, path)? {
        Ok(group) => group,
        Err((depth, parent)) => {
            // Create the missing levels around the new entry.
            let Some(value) = desired else {
                return Ok((None, source.into()));
            };
            let nested = path[depth + 1..].iter().rev().fold(
                serde_json::json!({name: value}),
                |inner, key| serde_json::json!({*key: inner}),
            );
            return Ok((None, insert(source, parent, path[depth], &nested)?));
        }
    };
    let Some((index, entry)) = group
        .members
        .iter()
        .enumerate()
        .find(|(_, m)| m.key == name)
    else {
        return Ok((
            None,
            match desired {
                Some(value) => insert(source, group, name, value)?,
                None => source.into(),
            },
        ));
    };
    let before = entry.node.to_value();
    if desired == Some(&before) {
        return Ok((Some(before), source.into()));
    }
    let mut result = source.to_string();
    if let Some(value) = desired {
        result.replace_range(
            entry.node.range.clone(),
            &serde_json::to_string_pretty(value).expect("serializing a JSON value cannot fail"),
        );
    } else {
        if let Some(comma) = entry.comma {
            result.remove(comma);
        }
        result.replace_range(entry.start..entry.node.range.end, "");
        if entry.comma.is_none()
            && index > 0
            && let Some(comma) = group.members[index - 1].comma
        {
            result.remove(comma);
        }
    }
    Ok((Some(before), result))
}

/// The object at `path`, or the depth and parent object of its first missing level.
fn json_group<'a>(
    root: &'a Node,
    path: &[&str],
) -> Result<std::result::Result<&'a Node, (usize, &'a Node)>> {
    let mut group = root;
    for (depth, key) in path.iter().enumerate() {
        let Some(member) = group.members.iter().find(|m| m.key == *key) else {
            return Ok(Err((depth, group)));
        };
        if !member.node.value.is_object() {
            return Err(Error::invalid("MCP server collection must be an object"));
        }
        group = &member.node;
    }
    Ok(Ok(group))
}

fn toml_entry(document: &toml_edit::DocumentMut, key: &str, name: &str) -> Result<Option<Value>> {
    document
        .get(key)
        .and_then(|g| g.get(name))
        .map(|entry| {
            toml_object(
                entry
                    .as_table_like()
                    .ok_or_else(|| Error::invalid("MCP server must be a TOML table"))?,
                0,
            )
        })
        .transpose()
}

fn toml_edit(
    source: &str,
    document: &toml_edit::DocumentMut,
    key: &str,
    name: &str,
    desired: Option<&Value>,
) -> Result<(Option<Value>, String)> {
    use toml_edit::{Item, Table, value};
    let before = toml_entry(document, key, name)?;
    if before.as_ref() == desired {
        return Ok((before, source.into()));
    }
    if document.get(key).is_some_and(|g| !g.is_table_like()) {
        return Err(Error::invalid("MCP collection must be a TOML table"));
    }
    let mut document = document.clone();
    if let Some(desired) = desired {
        if document.get(key).is_none() {
            document[key] = Item::Table(Table::new());
        }
        let mut table = Table::new();
        for (key, v) in desired
            .as_object()
            .ok_or_else(|| Error::invalid("Expected MCP object"))?
        {
            table[key] = match v {
                // Per-tool settings read best as one inline table per line.
                Value::Object(map) => {
                    let mut nested = Table::new();
                    for (key, v) in map {
                        nested[key] = value(toml_value(v)?);
                    }
                    Item::Table(nested)
                }
                v => value(toml_value(v)?),
            };
        }
        if let Some(inline) = document.get_mut(key).and_then(Item::as_inline_table_mut) {
            // An inline collection can only hold inline tables, at every depth.
            inline.insert(name, toml_value(desired)?);
        } else {
            document[key][name] = Item::Table(table);
        }
    } else if let Some(group) = document.get_mut(key).and_then(Item::as_table_like_mut) {
        group.remove(name);
    }
    Ok((before, document.to_string()))
}

/// Read an owned server table, including Codex's per-tool settings.
fn toml_object(table: &dyn toml_edit::TableLike, depth: usize) -> Result<Value> {
    if depth > 2 {
        return Err(Error::invalid(
            "Unsupported nested MCP settings; resolve the conflicting entry manually",
        ));
    }
    let mut map = serde_json::Map::new();
    for (key, item) in table.iter() {
        let converted = if let Some(nested) = item.as_table_like() {
            toml_object(nested, depth + 1)?
        } else if let Some(v) = item.as_value() {
            json_value(v, depth)?
        } else {
            return Err(Error::invalid(
                "Unsupported MCP settings; resolve the conflicting entry manually",
            ));
        };
        map.insert(key.into(), converted);
    }
    Ok(Value::Object(map))
}

fn json_value(v: &toml_edit::Value, depth: usize) -> Result<Value> {
    Ok(if let Some(text) = v.as_str() {
        Value::String(text.into())
    } else if let Some(boolean) = v.as_bool() {
        Value::Bool(boolean)
    } else if let Some(integer) = v.as_integer() {
        Value::from(integer)
    } else if let Some(array) = v.as_array() {
        Value::Array(
            array
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(|s| Value::String(s.into()))
                        .ok_or_else(|| Error::invalid("Unsupported MCP array value"))
                })
                .collect::<Result<_>>()?,
        )
    } else if let Some(inline) = v.as_inline_table() {
        toml_object(inline, depth + 1)?
    } else {
        return Err(Error::invalid(
            "Unsupported MCP settings; resolve the conflicting entry manually",
        ));
    })
}

/// A bounded TOML item as JSON, including arrays of tables, for reading hook
/// groups. Floats and datetimes are not part of the hook subset and refuse.
fn toml_item(item: &toml_edit::Item, depth: usize) -> Result<Value> {
    use toml_edit::Item;
    if depth > 4 {
        return Err(Error::invalid("Unsupported nested TOML hooks"));
    }
    let table = |table: &dyn toml_edit::TableLike| -> Result<Value> {
        table
            .iter()
            .map(|(key, item)| Ok((key.to_owned(), toml_item(item, depth + 1)?)))
            .collect::<Result<serde_json::Map<_, _>>>()
            .map(Value::Object)
    };
    match item {
        Item::None => Ok(Value::Null),
        Item::Table(t) => table(t),
        Item::ArrayOfTables(tables) => tables
            .iter()
            .map(|t| table(t))
            .collect::<Result<_>>()
            .map(Value::Array),
        Item::Value(v) => {
            if let Some(text) = v.as_str() {
                Ok(Value::String(text.into()))
            } else if let Some(boolean) = v.as_bool() {
                Ok(Value::Bool(boolean))
            } else if let Some(integer) = v.as_integer() {
                Ok(Value::from(integer))
            } else if let Some(array) = v.as_array() {
                array
                    .iter()
                    .map(|v| toml_item(&Item::Value(v.clone()), depth + 1))
                    .collect::<Result<_>>()
                    .map(Value::Array)
            } else if let Some(inline) = v.as_inline_table() {
                table(inline)
            } else {
                Err(Error::invalid("Unsupported TOML hook value"))
            }
        }
    }
}

fn toml_value(v: &Value) -> Result<toml_edit::Value> {
    Ok(match v {
        Value::String(s) => s.as_str().into(),
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| Error::invalid("Unsupported generated TOML number"))?
            .into(),
        Value::Array(values) => {
            let mut array = toml_edit::Array::new();
            for v in values {
                array.push(
                    v.as_str()
                        .ok_or_else(|| Error::invalid("Expected string argument"))?,
                );
            }
            array.into()
        }
        Value::Object(map) => {
            let mut table = toml_edit::InlineTable::new();
            for (key, v) in map {
                table.insert(key.as_str(), toml_value(v)?);
            }
            table.into()
        }
        Value::Null => return Err(Error::invalid("Unsupported generated TOML value")),
    })
}

/// Add or remove `group` in the hook list `hooks.<event>` of a JSON settings
/// document, keeping every other byte. Groups match by value. Returns whether
/// the group was present and the edited document.
///
/// # Errors
/// Returns an error for malformed or ambiguous JSON, a non-object root or hooks
/// member, or a selected event that is not a list.
pub fn hook_group(
    source: &str,
    event: &str,
    group: &Value,
    present: bool,
) -> Result<(bool, String)> {
    let document = Prepared::parse(source, Format::Json)?;
    let mut groups = document.hook_groups(event)?;
    let was = groups.contains(group);
    if was == present {
        return Ok((was, source.into()));
    }
    if present {
        groups.push(group.clone());
    } else {
        groups.retain(|value| value != group);
    }
    let edited = document.edit_settings(None, Some((event, &groups)))?;
    Ok((was, edited))
}

/// Edit the string list at `permissions.allow`, keeping every other byte.
/// Returns the current list and the document after removing `remove` and
/// appending the missing entries of `add`.
///
/// # Errors
/// Returns an error for malformed or ambiguous JSON, a non-object root or
/// permissions member, or an allow list containing non-string values.
pub fn allow_list(
    source: &str,
    remove: &[String],
    add: &[String],
) -> Result<(Vec<String>, String)> {
    permission_list(source, "allow", remove, add)
}

/// Read or edit an exact Claude Code permission list. Unrelated rules and
/// settings are preserved.
///
/// # Errors
/// Returns an error for malformed JSON, unsupported decisions, or a selected
/// permission list that is not a string list.
pub fn permission_list(
    source: &str,
    decision: &str,
    remove: &[String],
    add: &[String],
) -> Result<(Vec<String>, String)> {
    validate_permission_decision(decision)?;
    Prepared::parse(source, Format::Json)?.permission_list(decision, remove, add)
}

/// Read Claude Code permission lists from one strict JSON document.
/// Returns the requested decisions in order, with empty lists for absent fields.
///
/// # Errors
/// Returns an error for malformed JSON, unsupported decisions, or a selected
/// permission list that is not a list of strings.
pub fn read_permission_lists<'a>(
    source: &str,
    decisions: &[&'a str],
) -> Result<Vec<(&'a str, Vec<String>)>> {
    let root = Parser::parse(source, true)?;
    decisions
        .iter()
        .map(|&decision| {
            validate_permission_decision(decision)?;
            Ok((decision, permission_rules(&root, decision)?))
        })
        .collect()
}

fn validate_permission_decision(decision: &str) -> Result<()> {
    if matches!(decision, "deny" | "ask" | "allow") {
        Ok(())
    } else {
        Err(Error::invalid("Unknown permission list"))
    }
}

fn permission_rules(root: &Node, decision: &str) -> Result<Vec<String>> {
    let permissions = root.members.iter().find(|m| m.key == "permissions");
    let list = permissions.and_then(|p| p.node.members.iter().find(|m| m.key == decision));
    if permissions.is_some_and(|p| !p.node.value.is_object()) {
        return Err(Error::invalid("Permissions must be an object"));
    }
    match list {
        Some(list) => serde_json::from_value(list.node.to_value())
            .map_err(|_| Error::invalid("Permission rules must be a list of strings")),
        None => Ok(Vec::new()),
    }
}

fn updated_permission_rules(
    current: &[String],
    remove: &[String],
    add: &[String],
) -> Option<Vec<String>> {
    let mut after: Vec<String> = current
        .iter()
        .filter(|rule| !remove.contains(rule))
        .cloned()
        .collect();
    for rule in add {
        if !after.contains(rule) {
            after.push(rule.clone());
        }
    }
    (after != current).then_some(after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_hook_and_approval_edits_share_the_original_settings_ranges() {
        const EVENT: &str = "UserPromptSubmit";
        let rules = ["mcp__acme__observe".to_owned()];
        let groups = [serde_json::json!({"hooks":[{"type":"command","command":"acme hook"}]})];
        for source in [
            "{}",
            r#"{"unrelated": [ 1, 2 ]}"#,
            r#"{"permissions":{"deny":[]}}"#,
            r#"{
  "permissions": {
    "deny": []
  }
}"#,
            r#"{"permissions":{"deny": [ "Bash(other:*)" ]},"hooks":{}}"#,
            r#"{"hooks":{"UserPromptSubmit":[]},"permissions":{"allow":[]}}"#,
            r#"{"permissions":{"allow":[]},"unrelated": [ 1, 2 ]}"#,
        ] {
            let document = Prepared::parse(source, Format::Json).unwrap();
            let edited = document
                .edit_settings(Some(("allow", &rules)), Some((EVENT, &groups)))
                .unwrap();
            let mut expected: Value = serde_json::from_str(source).unwrap();
            expected["permissions"]["allow"] = serde_json::json!(rules);
            expected["hooks"][EVENT] = serde_json::json!(groups);
            assert_eq!(serde_json::from_str::<Value>(&edited).unwrap(), expected);
            for literal in [
                r#""unrelated": [ 1, 2 ]"#,
                r#""deny":[]"#,
                r#""deny": []"#,
                r#""deny": [ "Bash(other:*)" ]"#,
            ] {
                if source.contains(literal) {
                    assert!(edited.contains(literal));
                }
            }
            let document = Prepared::parse(edited.as_str(), Format::Json).unwrap();
            assert_eq!(document.edit_settings(None, None).unwrap(), edited);
        }
    }

    #[test]
    fn json_list_helpers_preserve_unrelated_bytes_and_validate_selected_lists() {
        let group = serde_json::json!({"hooks":[{"type":"command","command":"acme hook"}]});
        for source in [
            "{}",
            r#"{"keep": [1, 2]}"#,
            r#"{"hooks":{},"permissions":{}}"#,
        ] {
            let (was, listed) = list_item(source, "items", "acme", true).unwrap();
            assert!(!was);
            assert_eq!(
                list_item(&listed, "items", "acme", true).unwrap(),
                (true, listed.clone())
            );
            let (was, removed) = list_item(&listed, "items", "acme", false).unwrap();
            assert!(was);
            assert_eq!(
                serde_json::from_str::<Value>(&removed).unwrap()["items"],
                serde_json::json!([])
            );
            let (was, hooked) = hook_group(source, "UserPromptSubmit", &group, true).unwrap();
            assert!(!was);
            assert_eq!(
                hook_group(&hooked, "UserPromptSubmit", &group, true).unwrap(),
                (true, hooked.clone())
            );
            let (was, unhooked) = hook_group(&hooked, "UserPromptSubmit", &group, false).unwrap();
            assert!(was);
            assert_eq!(
                serde_json::from_str::<Value>(&unhooked).unwrap()["hooks"]["UserPromptSubmit"],
                serde_json::json!([])
            );
            let (before, allowed) = allow_list(source, &[], &["acme".into()]).unwrap();
            assert_eq!(before, Vec::<String>::new());
            assert_eq!(
                allow_list(&allowed, &[], &["acme".into()]).unwrap(),
                (vec!["acme".into()], allowed.clone())
            );
            let (_, disallowed) = allow_list(&allowed, &["acme".into()], &[]).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&disallowed).unwrap()["permissions"]["allow"],
                serde_json::json!([])
            );
            if source.contains("keep") {
                for text in [&listed, &removed, &hooked, &unhooked, &allowed, &disallowed] {
                    assert!(text.contains(r#""keep": [1, 2]"#), "{text}");
                }
            }
        }
        let source =
            r#"{"items":["acme","keep","acme"],"permissions":{"allow":["acme","keep","acme"]}}"#;
        let (_, edited) = list_item(source, "items", "acme", false).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&edited).unwrap()["items"],
            serde_json::json!(["keep"])
        );
        let (_, edited) =
            allow_list(source, &["acme".into()], &["new".into(), "new".into()]).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&edited).unwrap()["permissions"]["allow"],
            serde_json::json!(["keep", "new"])
        );
        for source in [r#"{"items":1}"#, r#"{"items":[1]}"#] {
            assert!(list_item(source, "items", "acme", false).is_err());
        }
        for source in [r#"{"hooks":1}"#, r#"{"hooks":{"UserPromptSubmit":1}}"#] {
            assert!(hook_group(source, "UserPromptSubmit", &group, false).is_err());
        }
        for source in [r#"{"permissions":1}"#, r#"{"permissions":{"allow":[1]}}"#] {
            assert!(allow_list(source, &[], &[]).is_err());
        }
        assert_eq!(
            list_item("{}", "items", "acme", false).unwrap(),
            (false, "{}".into())
        );
        assert_eq!(
            hook_group("{}", "UserPromptSubmit", &group, false).unwrap(),
            (false, "{}".into())
        );
        assert_eq!(allow_list("{}", &[], &[]).unwrap(), (vec![], "{}".into()));
    }

    #[test]
    fn permission_list_preserves_other_decisions() {
        let source = r#"{"permissions":{"allow": [ "keep" ],"ask":[1],"deny":["old"]}}"#;
        let (previous, edited) =
            permission_list(source, "deny", &["old".into()], &["new".into()]).unwrap();
        assert_eq!(previous, ["old"]);
        assert_eq!(edited, source.replace(r#"["old"]"#, "[\n  \"new\"\n]"));
        assert_eq!(
            permission_list(&edited, "deny", &[], &["new".into()]).unwrap(),
            (vec!["new".into()], edited)
        );
        assert!(permission_list(source, "ask", &[], &[]).is_err());
        assert!(permission_list(source, "unknown", &[], &[]).is_err());
    }

    #[test]
    fn read_permission_lists_preserves_order_and_validates_selected_lists() {
        let source = r#"{"permissions":{"allow":["one","two"],"ask":[1],"deny":["three"]}}"#;
        assert_eq!(
            read_permission_lists(source, &["deny", "allow", "deny"]).unwrap(),
            [
                ("deny", vec!["three".to_owned()]),
                ("allow", vec!["one".to_owned(), "two".to_owned()]),
                ("deny", vec!["three".to_owned()]),
            ]
        );
        assert_eq!(
            read_permission_lists(r#"{"permissions":{"allow":["one"]}}"#, &["ask"]).unwrap(),
            [("ask", Vec::<String>::new())]
        );
        for (source, decisions) in [
            (source, &["deny", "ask"][..]),
            (source, &["unknown"][..]),
            (r#"{"permissions":1}"#, &["allow"][..]),
            (r#"{"permissions":{},}"#, &["allow"][..]),
        ] {
            assert!(read_permission_lists(source, decisions).is_err());
        }
    }

    #[test]
    fn git_conflict_line_requires_a_complete_column_zero_block() {
        let block = "<<<<<<<<< HEAD\r\nmode: ours\r\n||||||||| base\r\nmode: base\r\n=========\r\nmode: theirs\r\n>>>>>>>>> topic";
        assert_eq!(
            git_conflict_line(&format!("\u{feff}version: 2\r\n{block}")),
            Some(2)
        );
        assert_eq!(
            git_conflict_line(&block.replace(">>>>>>>>>", ">>>>>>>")),
            None
        );
        assert_eq!(
            git_conflict_line(&format!("env: |\n  {}", block.replace('\n', "\n  "))),
            None
        );
    }

    #[test]
    fn inserted_json_entries_follow_nested_space_and_tab_indentation() {
        for unit in ["  ", "    ", "\t"] {
            let source = format!(
                "{{\n{unit}\"mcpServers\": {{\n{unit}{unit}\"other\": {{\"command\":\"other\"}}\n{unit}}},\n{unit}\"keep\": true\n}}\n"
            );
            let desired = serde_json::json!({"command":"acme", "args":["mcp", "serve"]});
            let (_, edited) =
                edit(&source, Format::Json, "mcpServers", "acme", Some(&desired)).unwrap();
            assert!(
                edited.contains(&format!(
                    "\n{unit}{unit}\"acme\": {{\n{unit}{unit}{unit}\"args\": ["
                )),
                "{edited}"
            );
            assert!(edited.contains(&format!(
                "\n{unit}{unit}\"other\": {{\"command\":\"other\"}},"
            )));
            let parsed: Value = serde_json::from_str(&edited).unwrap();
            assert_eq!(parsed["mcpServers"]["acme"], desired);
            assert_eq!(parsed["keep"], true);
        }
        let compact = "{\"mcpServers\":{}}";
        let (_, edited) = edit(
            compact,
            Format::Json,
            "mcpServers",
            "acme",
            Some(&serde_json::json!({"command":"acme"})),
        )
        .unwrap();
        assert!(
            edited.contains("\n    \"acme\": {\n      \"command\": \"acme\"\n    }\n  }"),
            "{edited}"
        );
        assert!(serde_json::from_str::<Value>(&edited).is_ok());
    }

    #[test]
    fn prepared_json_retains_each_payload_once_and_preserves_nested_arrays() {
        fn payload_bytes(node: &Node) -> usize {
            fn value_bytes(value: &Value) -> usize {
                match value {
                    Value::String(text) => text.len(),
                    Value::Object(map) => map.values().map(value_bytes).sum(),
                    Value::Array(values) => values.iter().map(value_bytes).sum(),
                    _ => 0,
                }
            }
            value_bytes(&node.value)
                + node
                    .members
                    .iter()
                    .map(|m| payload_bytes(&m.node))
                    .sum::<usize>()
                + node.elements.iter().map(payload_bytes).sum::<usize>()
        }
        for depth in [1, 32] {
            let payload = "x".repeat(256 * 1024);
            let mut history = serde_json::json!([{"payload": payload}]);
            for _ in 0..depth {
                history = serde_json::json!({"nested": history});
            }
            let value = serde_json::json!({"history": history, "mcpServers": {"acme": {"args": ["serve", "with space"]}}});
            let prepared = Prepared::parse(value.to_string(), Format::Json).unwrap();
            let Parsed::Json(root) = &prepared.parsed else {
                panic!("JSON tree expected");
            };
            assert_eq!(
                payload_bytes(root),
                256 * 1024 + "serve".len() + "with space".len()
            );
            assert_eq!(root.to_value(), value);
            let (previous, edited) = prepared
                .edit(
                    "mcpServers",
                    "acme",
                    Some(&serde_json::json!({"command":"acme"})),
                )
                .unwrap();
            assert_eq!(previous, Some(value["mcpServers"]["acme"].clone()));
            let edited: Value = serde_json::from_str(&edited).unwrap();
            assert_eq!(edited["history"], value["history"]);
        }
    }

    #[test]
    fn entry_lookup_matches_removal_edit_values_and_refusals_without_editing() {
        let cases = [
            (
                Format::Json,
                r#"{"s":{"acme":{"command":"acme"},"other":1}}"#,
            ),
            (Format::Json, r#"{"s":{"other":1}}"#),
            (Format::Json, r#"{"other":1}"#),
            (Format::Json, r#"{"s":[1]}"#),
            (
                Format::Toml,
                "[s.acme]\ncommand = \"acme\"\n[s.other]\ncommand = \"x\"\n",
            ),
            (Format::Toml, "[s]\nacme = 1\n"),
            (Format::Toml, "s = 1\n"),
            (Format::Yaml, "s:\n  acme:\n    command: acme\n"),
            (Format::Yaml, "other: 1\n"),
            (Format::Yaml, "s: [1]\n"),
            (Format::Yaml, "- 1\n"),
        ];
        for (format, source) in cases {
            let prepared = Prepared::parse(source, format).unwrap();
            let start = edits();
            let entry = prepared.entry("s", "acme").map_err(|e| e.to_string());
            assert_eq!(edits(), start, "{source}");
            let removed = prepared
                .edit("s", "acme", None)
                .map(|(previous, _)| previous)
                .map_err(|e| e.to_string());
            assert_eq!(entry, removed, "{format:?} {source}");
        }
        let settings = Prepared::parse(r#"{"permissions":{"allow":["a"]}}"#, Format::Json).unwrap();
        assert_eq!(settings.allowed().unwrap(), ["a"]);
        for source in [r#"{"permissions":1}"#, r#"{"permissions":{"allow":[1]}}"#] {
            let prepared = Prepared::parse(source, Format::Json).unwrap();
            assert_eq!(
                prepared.allowed().map_err(|e| e.to_string()),
                prepared
                    .permission_list("allow", &[], &[])
                    .map(|(rules, _)| rules)
                    .map_err(|e| e.to_string())
            );
        }
    }

    #[test]
    fn prepared_strict_json_rejects_jsonc_and_invalid_syntax() {
        for text in [
            "{\"x\":1,}",
            "{\"x\":[1,]}",
            "{/* comment */}",
            "{\"x\":1 // comment\n}",
            "{\"x\":1,\"x\":2}",
            "{} trailing",
            "{\"x\":NaN}",
            "{\"x\":01}",
            "{\"x\":\"\\q\"}",
            "{\u{b}\"x\":1}",
        ] {
            assert!(Prepared::parse(text, Format::Json).is_err(), "{text}");
        }
        for text in ["{\"x\":1,}", "{\"x\":[1,]}", "{/* comment */}"] {
            assert!(Prepared::parse(text, Format::Jsonc).is_ok(), "{text}");
        }
    }
}
