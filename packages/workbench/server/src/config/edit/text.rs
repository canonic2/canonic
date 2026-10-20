//! One block of a workbench.yaml's text replaced, every other line kept as
//! it was, and JSON values written as the YAML subset the reader reads, as
//! `server/node/config.js` and `server/node/yaml.js` do. Pure.
use super::super::js::{self, is_space};
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::OnceLock;

const FILE: &str = "workbench.yaml";

/// A value as workbench YAML, ending in a newline.
pub(super) fn stringify(value: &Value) -> Result<String, String> {
    let mut out = Vec::new();
    write_node(value, 0, &mut out)?;
    Ok(out.join("\n") + "\n")
}

fn is_node(value: &Value) -> bool {
    value.is_object() || value.is_array()
}

fn pad(indent: usize) -> String {
    " ".repeat(indent)
}

fn write_node(value: &Value, indent: usize, out: &mut Vec<String>) -> Result<(), String> {
    match value {
        Value::Array(items) => write_list(items, indent, out),
        Value::Object(map) => write_map(map, indent, out),
        _ => Ok(()),
    }
}

fn write_map(map: &Map<String, Value>, indent: usize, out: &mut Vec<String>) -> Result<(), String> {
    for (key, child) in js::ordered(map.clone()) {
        if is_node(&child) {
            out.push(format!("{}{key}:", pad(indent)));
            write_node(&child, indent + 2, out)?;
        } else {
            out.push(format!("{}{key}: {}", pad(indent), scalar_text(&child)?));
        }
    }
    Ok(())
}

fn write_list(items: &[Value], indent: usize, out: &mut Vec<String>) -> Result<(), String> {
    for child in items {
        match child {
            Value::Array(_) => {
                out.push(format!("{}-", pad(indent)));
                write_node(child, indent + 2, out)?;
            }
            Value::Object(map) => {
                let mut entries = js::ordered(map.clone()).into_iter();
                let Some((first, first_value)) = entries.next() else {
                    return Err("Empty maps are not supported in workbench YAML.".to_owned());
                };
                if is_node(&first_value) {
                    out.push(format!("{}- {first}:", pad(indent)));
                    write_node(&first_value, indent + 4, out)?;
                } else {
                    out.push(format!(
                        "{}- {first}: {}",
                        pad(indent),
                        scalar_text(&first_value)?
                    ));
                }
                write_map(&entries.collect(), indent + 2, out)?;
            }
            scalar => out.push(format!("{}- {}", pad(indent), scalar_text(scalar)?)),
        }
    }
    Ok(())
}

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a valid pattern"))
}

/// Whether a string needs quotes to read back as the same string.
fn needs_quotes(text: &str) -> bool {
    static RESERVED: OnceLock<Regex> = OnceLock::new();
    let reserved = text.is_empty()
        || regex(&RESERVED, r"^(?:true|false|null|~|-?[0-9]+(?:\.[0-9]+)?)$").is_match(text);
    let chars: Vec<char> = text.chars().collect();
    let space_at = |at: usize| chars.get(at).copied().is_some_and(is_space);
    let indicator = matches!(chars.first(), Some('-' | '?' | ':')) && space_at(1);
    let comment_or_key = chars.first() == Some(&'#')
        || chars.windows(2).any(|pair| {
            (pair[0] == ':' && is_space(pair[1])) || (is_space(pair[0]) && pair[1] == '#')
        });
    let edges =
        chars.first().copied().is_some_and(is_space) || chars.last().copied().is_some_and(is_space);
    reserved || indicator || comment_or_key || edges
}

fn scalar_text(value: &Value) -> Result<String, String> {
    let text = match value {
        Value::Bool(flag) => return Ok(flag.to_string()),
        Value::Null => return Ok("null".to_owned()),
        Value::Number(number) => return Ok(js::number_text(number)),
        Value::String(text) => text.as_str(),
        _ => "",
    };
    if text.contains(['\r', '\n']) {
        return Err("Workbench YAML values must stay on one line.".to_owned());
    }
    if !needs_quotes(text) {
        return Ok(text.to_owned());
    }
    if !text.contains('\'') {
        return Ok(format!("'{text}'"));
    }
    if !text.contains('"') {
        return Ok(format!("\"{text}\""));
    }
    Err("A workbench YAML value cannot contain both quote styles.".to_owned())
}

/// The file's lines, split as `/\r?\n/` splits them.
fn lines_of(body: &str) -> Vec<String> {
    body.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect()
}

/// The lines joined, ending in exactly one newline.
fn finish(lines: &[String]) -> String {
    let mut text = lines.join("\n").trim_end_matches('\n').to_owned();
    text.push('\n');
    text
}

fn replace_top_level(body: &str, key: &str, replacement: &str) -> String {
    static NEXT: OnceLock<Regex> = OnceLock::new();
    let next = regex(&NEXT, r"^[^\s#][^:]*\s*:");
    let own = Regex::new(&format!(r"^{}\s*:", regex::escape(key))).expect("a valid pattern");
    let mut lines = lines_of(body);
    let replacement: Vec<String> = replacement.split('\n').map(str::to_owned).collect();
    if let Some(start) = lines.iter().position(|line| own.is_match(line)) {
        let end = lines
            .iter()
            .skip(start + 1)
            .position(|line| next.is_match(line))
            .map_or(lines.len(), |offset| start + 1 + offset);
        lines.splice(start..end, replacement);
    } else {
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(replacement);
    }
    finish(&lines)
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_blank(line: &str) -> bool {
    static BLANK: OnceLock<Regex> = OnceLock::new();
    regex(&BLANK, r"^\s*(#.*)?$").is_match(line)
}

/// A `key:` block among the direct children of lines `[from, to)`.
struct Block {
    /// Its first line, or `None` when there is none.
    start: Option<usize>,
    /// The line after its last, or where a new child would go.
    end: usize,
    indent: usize,
}

fn child_block(lines: &[String], from: usize, to: usize, key: &str, fallback: usize) -> Block {
    let mut depth: Option<(usize, Regex)> = None;
    for i in from..to {
        let line = &lines[i];
        if is_blank(line) {
            continue;
        }
        let indent = indent_of(line);
        let (level, pattern) = depth.get_or_insert_with(|| {
            let pattern = Regex::new(&format!(r"^ {{{indent}}}{}\s*:", regex::escape(key)))
                .expect("a valid pattern");
            (indent, pattern)
        });
        let level = *level;
        if indent < level {
            break;
        }
        if indent != level || !pattern.is_match(line) {
            continue;
        }
        let mut end = i + 1;
        while end < to && (is_blank(&lines[end]) || indent_of(&lines[end]) > level) {
            end += 1;
        }
        while end > i + 1 && lines[end - 1].trim().is_empty() {
            end -= 1;
        }
        return Block {
            start: Some(i),
            end,
            indent: level,
        };
    }
    let mut after = to;
    while after > from && lines[after - 1].trim().is_empty() {
        after -= 1;
    }
    Block {
        start: None,
        end: after,
        indent: depth.map_or(fallback, |(level, _)| level),
    }
}

fn wrapped(name: &str, value: &Value) -> Result<String, String> {
    let mut map = Map::new();
    map.insert(name.to_owned(), value.clone());
    Ok(js::trim(&stringify(&Value::Object(map))?).to_owned())
}

/// `spaces.<key>.<name>` replaced in place, or added at the end of that space.
fn replace_space_block(body: &str, key: &str, name: &str, value: &Value) -> Result<String, String> {
    let mut lines = lines_of(body);
    let spaces = child_block(&lines, 0, lines.len(), "spaces", 0);
    let Some(list_start) = spaces.start else {
        return Err(format!("There is no spaces block in {FILE}."));
    };
    let space = child_block(&lines, list_start + 1, spaces.end, key, spaces.indent + 2);
    let Some(entry_start) = space.start else {
        return Err(format!("There is no space “{key}” in {FILE}."));
    };
    let block = child_block(&lines, entry_start + 1, space.end, name, space.indent + 2);
    let pad = pad(block.indent);
    let replacement: Vec<String> = wrapped(name, value)?
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{pad}{line}")
            }
        })
        .collect();
    match block.start {
        None => {
            lines.splice(block.end..block.end, replacement);
        }
        Some(start) => {
            lines.splice(start..block.end, replacement);
        }
    }
    Ok(finish(&lines))
}

/// The space's block `name` replaced: at the top level of a file without
/// spaces, or inside the space `key` of a file that lists several.
pub(super) fn replace_block(
    body: &str,
    key: Option<&str>,
    name: &str,
    value: &Value,
) -> Result<String, String> {
    match key {
        Some(key) => replace_space_block(body, key, name, value),
        None => Ok(replace_top_level(body, name, &wrapped(name, value)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_are_written_so_they_read_back_the_same() {
        let written = stringify(&json!({
            "name": "Acme",
            "collections": [{ "name": "Pages", "items": [
                { "label": "Sign in", "src": "pages/sign-in.html", "sizes": ["laptop", { "popover": { "width": 280, "height": "fill" } }] },
            ] }],
            "quoted": ["true", "12", "a: b", "#00a1ff", " padded", "it's: here", "plain's", ""],
            "flags": [true, null, 1.5],
        }))
        .expect("writable");
        assert_eq!(
            written,
            "name: Acme\ncollections:\n  - name: Pages\n    items:\n      - label: Sign in\n        src: pages/sign-in.html\n        sizes:\n          - laptop\n          - popover:\n              width: 280\n              height: fill\nquoted:\n  - 'true'\n  - '12'\n  - 'a: b'\n  - '#00a1ff'\n  - ' padded'\n  - \"it's: here\"\n  - plain's\n  - ''\nflags:\n  - true\n  - null\n  - 1.5\n"
        );
        let read = super::super::super::yaml::parse(&written).expect("parses");
        assert_eq!(
            read["collections"][0]["items"][0]["sizes"][1]["popover"]["height"],
            "fill"
        );
        let values = json!({ "a": "true", "b": "12", "c": "a: b", "d": "#00a1ff", "e": " padded", "f": "it's: here", "g": "" });
        let read = super::super::super::yaml::parse(&stringify(&values).expect("writable"))
            .expect("parses");
        assert_eq!(read, values);
    }

    #[test]
    fn values_the_subset_cannot_hold_are_refused() {
        assert!(
            stringify(&json!({ "a": "two\nlines" }))
                .expect_err("refused")
                .contains("one line")
        );
        assert!(
            stringify(&json!({ "a": "it's: \"both\"" }))
                .expect_err("refused")
                .contains("both quote styles")
        );
        assert!(
            stringify(&json!({ "a": [{}] }))
                .expect_err("refused")
                .contains("Empty maps")
        );
    }

    #[test]
    fn a_top_level_block_is_replaced_or_appended() {
        // A comment line before the next key belongs to the block replaced.
        let body = "# about sizes\nname: Acme # keep\nsizes:\n  fit: true\n# about pages\ncollections:\n  - name: Old\n";
        assert_eq!(
            replace_block(body, None, "sizes", &json!({ "mobile": true })).expect("replaced"),
            "# about sizes\nname: Acme # keep\nsizes:\n  mobile: true\ncollections:\n  - name: Old\n"
        );
        assert_eq!(
            replace_block("name: Acme\n\n\n", None, "sizes", &json!({ "fit": true }))
                .expect("added"),
            "name: Acme\n\nsizes:\n  fit: true\n"
        );
    }

    #[test]
    fn a_space_block_is_replaced_or_added_inside_that_space() {
        let body = "spaces:\n  web:\n    name: Web\n    sizes:\n      fit: true\n  ui:\n    name: UI\n\nprevious: false\n";
        assert_eq!(
            replace_block(body, Some("web"), "sizes", &json!({ "mobile": true }))
                .expect("replaced"),
            "spaces:\n  web:\n    name: Web\n    sizes:\n      mobile: true\n  ui:\n    name: UI\n\nprevious: false\n"
        );
        assert_eq!(
            replace_block(body, Some("ui"), "sizes", &json!({ "fit": true })).expect("added"),
            "spaces:\n  web:\n    name: Web\n    sizes:\n      fit: true\n  ui:\n    name: UI\n    sizes:\n      fit: true\n\nprevious: false\n"
        );
        assert_eq!(
            replace_block("name: Acme\n", Some("ui"), "sizes", &json!({})).expect_err("refused"),
            "There is no spaces block in workbench.yaml."
        );
        assert_eq!(
            replace_block(body, Some("docs"), "sizes", &json!({})).expect_err("refused"),
            "There is no space “docs” in workbench.yaml."
        );
    }
}
