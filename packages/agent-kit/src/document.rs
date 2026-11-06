//! Surgical JSON/JSONC entry editing and TOML's formatting-preserving codec.
use super::{Error, Format, Result};
use serde_json::Value;
use std::ops::Range;

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
    match format {
        Format::Json => {
            serde_json::from_str::<Value>(source)
                .map_err(|_| Error::invalid("Invalid JSON configuration"))?;
            json_edit(source, &[key], name, desired)
        }
        Format::Jsonc => json_edit(source, &[key], name, desired),
        Format::Toml => toml_edit(source, key, name, desired),
        Format::Yaml => yaml_edit(source, key, name, desired),
    }
}

fn yaml_edit(
    source: &str,
    key: &str,
    name: &str,
    desired: Option<&Value>,
) -> Result<(Option<Value>, String)> {
    let mut root: Value =
        serde_saphyr::from_str(source).map_err(|_| Error::invalid("Invalid YAML configuration"))?;
    let object = root
        .as_object_mut()
        .ok_or_else(|| Error::invalid("YAML configuration must be an object"))?;
    let group = object.entry(key).or_insert_with(|| serde_json::json!({}));
    let group = group
        .as_object_mut()
        .ok_or_else(|| Error::invalid("YAML extensions must be a mapping"))?;
    let before = group.get(name).cloned();
    if before.as_ref() == desired {
        return Ok((before, source.into()));
    }
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
}
impl Parser<'_> {
    fn byte(&self) -> Option<u8> {
        self.source.as_bytes().get(self.position).copied()
    }
    fn skip(&mut self) -> Result<()> {
        loop {
            while self.byte().is_some_and(|b| b.is_ascii_whitespace()) {
                self.position += 1;
            }
            if self.source[self.position..].starts_with("//") {
                while self.byte().is_some_and(|b| b != b'\n') {
                    self.position += 1;
                }
            } else if self.source[self.position..].starts_with("/*") {
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
        let value = match self.byte() {
            Some(b'{') => {
                self.position += 1;
                self.skip()?;
                let mut map = serde_json::Map::new();
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
                    if map.insert(key.clone(), node.value.clone()).is_some() {
                        return Err(Error::invalid("Duplicate JSON key"));
                    }
                    self.skip()?;
                    let comma = if self.byte() == Some(b',') {
                        let index = self.position;
                        self.position += 1;
                        self.skip()?;
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
                Value::Object(map)
            }
            Some(b'[') => {
                self.position += 1;
                self.skip()?;
                let mut values = Vec::new();
                while self.byte() != Some(b']') {
                    values.push(self.node(depth + 1)?.value);
                    self.skip()?;
                    if self.byte() == Some(b',') {
                        self.position += 1;
                        self.skip()?;
                    } else if self.byte() != Some(b']') {
                        return Err(Error::invalid("Expected JSON array comma"));
                    }
                }
                self.position += 1;
                Value::Array(values)
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
        })
    }
}

fn insert(source: &str, object: &Node, name: &str, value: &Value) -> Result<String> {
    if !object.value.is_object() {
        return Err(Error::invalid("MCP server collection must be an object"));
    }
    let mut result = source.to_owned();
    let new = format!(
        "\n  {}: {}\n",
        serde_json::to_string(name).expect("serializing a string cannot fail"),
        serde_json::to_string_pretty(value).expect("serializing a JSON value cannot fail")
    );
    result.insert_str(object.range.end - 1, &new);
    if let Some(last) = object.members.last()
        && last.comma.is_none()
    {
        result.insert(last.node.range.end, ',');
    }
    Ok(result)
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
    match (format, path) {
        (Format::Json, _) => {
            serde_json::from_str::<Value>(source)
                .map_err(|_| Error::invalid("Invalid JSON configuration"))?;
            json_edit(source, path, name, desired)
        }
        (Format::Jsonc, _) => json_edit(source, path, name, desired),
        (_, [key]) => edit(source, format, key, name, desired),
        _ => Err(Error::invalid(
            "Nested entry paths are supported only in JSON configuration",
        )),
    }
}

/// Add or remove `item` in the top-level string list `key` of a JSON object.
/// Returns whether the item was present and the edited document.
///
/// # Errors
/// Returns an error for malformed or ambiguous JSON, a non-object root, or a
/// selected member that is not a list of strings.
pub fn list_item(source: &str, key: &str, item: &str, present: bool) -> Result<(bool, String)> {
    serde_json::from_str::<Value>(source)
        .map_err(|_| Error::invalid("Invalid JSON configuration"))?;
    let mut parser = Parser {
        source,
        position: 0,
    };
    let root = parser.node(0)?;
    if !root.value.is_object() {
        return Err(Error::invalid("Host configuration must be one JSON object"));
    }
    let member = root.members.iter().find(|m| m.key == key);
    let current: Vec<String> = match member {
        Some(member) => serde_json::from_value(member.node.value.clone())
            .map_err(|_| Error::invalid(format!("{key} must be a list of strings")))?,
        None => Vec::new(),
    };
    let was = current.iter().any(|value| value == item);
    if was == present {
        return Ok((was, source.into()));
    }
    let after: Vec<&String> = current.iter().filter(|value| *value != item).collect();
    let mut after: Vec<String> = after.into_iter().cloned().collect();
    if present {
        after.push(item.to_owned());
    }
    let list = serde_json::json!(after);
    let edited = match member {
        Some(member) => {
            let mut result = source.to_owned();
            result.replace_range(
                member.node.range.clone(),
                &serde_json::to_string_pretty(&list).map_err(|e| Error::invalid(e.to_string()))?,
            );
            result
        }
        None => insert(source, &root, key, &list)?,
    };
    Ok((was, edited))
}

fn json_edit(
    source: &str,
    path: &[&str],
    name: &str,
    desired: Option<&Value>,
) -> Result<(Option<Value>, String)> {
    let mut parser = Parser {
        source,
        position: 0,
    };
    let root = parser.node(0)?;
    parser.skip()?;
    if parser.position != source.len() || !root.value.is_object() {
        return Err(Error::invalid("Host configuration must be one JSON object"));
    }
    let mut group = &root;
    for (depth, key) in path.iter().enumerate() {
        let Some(member) = group.members.iter().find(|m| m.key == *key) else {
            // Create the missing levels around the new entry.
            let Some(value) = desired else {
                return Ok((None, source.into()));
            };
            let nested = path[depth + 1..].iter().rev().fold(
                serde_json::json!({name: value}),
                |inner, key| serde_json::json!({*key: inner}),
            );
            return Ok((None, insert(source, group, key, &nested)?));
        };
        if !member.node.value.is_object() {
            return Err(Error::invalid("MCP server collection must be an object"));
        }
        group = &member.node;
    }
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
    let before = entry.node.value.clone();
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

fn toml_edit(
    source: &str,
    key: &str,
    name: &str,
    desired: Option<&Value>,
) -> Result<(Option<Value>, String)> {
    use toml_edit::{DocumentMut, Item, Table, value};
    let mut document = source
        .parse::<DocumentMut>()
        .map_err(|_| Error::invalid("Invalid TOML configuration"))?;
    let before = document
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
        .transpose()?;
    if before.as_ref() == desired {
        return Ok((before, source.into()));
    }
    if document.get(key).is_some_and(|g| !g.is_table_like()) {
        return Err(Error::invalid("MCP collection must be a TOML table"));
    }
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
    serde_json::from_str::<Value>(source)
        .map_err(|_| Error::invalid("Invalid JSON configuration"))?;
    let mut parser = Parser {
        source,
        position: 0,
    };
    let root = parser.node(0)?;
    if !root.value.is_object() {
        return Err(Error::invalid("Host configuration must be one JSON object"));
    }
    let hooks = root.members.iter().find(|m| m.key == "hooks");
    if hooks.is_some_and(|h| !h.node.value.is_object()) {
        return Err(Error::invalid("Hooks must be an object of event lists"));
    }
    let list = hooks.and_then(|h| h.node.members.iter().find(|m| m.key == event));
    let current: &[Value] = match list {
        Some(list) => list
            .node
            .value
            .as_array()
            .ok_or_else(|| Error::invalid(format!("Hooks for {event} must be a list")))?,
        None => &[],
    };
    let was = current.contains(group);
    if was == present {
        return Ok((was, source.into()));
    }
    let mut after: Vec<Value> = current.iter().filter(|v| *v != group).cloned().collect();
    if present {
        after.push(group.clone());
    }
    let after = Value::Array(after);
    let edited = match (hooks, list) {
        (_, Some(list)) => {
            let mut result = source.to_owned();
            result.replace_range(
                list.node.range.clone(),
                &serde_json::to_string_pretty(&after).map_err(|e| Error::invalid(e.to_string()))?,
            );
            result
        }
        (Some(hooks), None) => insert(source, &hooks.node, event, &after)?,
        (None, None) => insert(source, &root, "hooks", &serde_json::json!({ event: after }))?,
    };
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
    serde_json::from_str::<Value>(source)
        .map_err(|_| Error::invalid("Invalid JSON configuration"))?;
    let mut parser = Parser {
        source,
        position: 0,
    };
    let root = parser.node(0)?;
    if !root.value.is_object() {
        return Err(Error::invalid("Host configuration must be one JSON object"));
    }
    let permissions = root.members.iter().find(|m| m.key == "permissions");
    let allow = permissions.and_then(|p| p.node.members.iter().find(|m| m.key == "allow"));
    if permissions.is_some_and(|p| !p.node.value.is_object()) {
        return Err(Error::invalid("Permissions must be an object"));
    }
    let current: Vec<String> = match allow {
        Some(allow) => serde_json::from_value(allow.node.value.clone())
            .map_err(|_| Error::invalid("Permission rules must be a list of strings"))?,
        None => Vec::new(),
    };
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
    if after == current {
        return Ok((current, source.into()));
    }
    let list = serde_json::json!(after);
    let edited = match (permissions, allow) {
        (_, Some(allow)) => {
            let mut result = source.to_owned();
            result.replace_range(
                allow.node.range.clone(),
                &serde_json::to_string_pretty(&list).map_err(|e| Error::invalid(e.to_string()))?,
            );
            result
        }
        (Some(permissions), None) => insert(source, &permissions.node, "allow", &list)?,
        (None, None) => insert(
            source,
            &root,
            "permissions",
            &serde_json::json!({"allow":list}),
        )?,
    };
    Ok((current, edited))
}
