//! The YAML a workbench.yaml is written in, parsed as the Node reader's
//! `server/node/yaml.js` parses it: block mappings and sequences, scalars,
//! quotes and comments, and nothing else. Errors name the line. Pure.
use super::js::{self, is_space};
use serde_json::{Map, Value};

struct Line {
    number: usize,
    indent: usize,
    text: String,
}

/// The document as a JSON value, or the error the Node reader throws.
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    let lines = significant(text)?;
    let Some(first) = lines.first() else {
        return Ok(Value::Object(Map::new()));
    };
    if first.indent != 0 {
        return Err(fail(first, "the document starts indented."));
    }
    let (value, next) = block(&lines, 0, 0, None)?;
    if let Some(line) = lines.get(next) {
        return Err(fail(
            line,
            "this line is indented in a way the block above doesn’t account for.",
        ));
    }
    Ok(value.unwrap_or(Value::Null))
}

fn fail(line: &Line, message: &str) -> String {
    format!("line {}: {message}", line.number + 1)
}

/// A `#` starts a comment at the line's start or after a space, outside quotes.
fn without_comment(line: &str) -> &str {
    let mut quote = None;
    let mut previous = None;
    for (at, c) in line.char_indices() {
        match quote {
            Some(open) if c == open => quote = None,
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' && (at == 0 || previous == Some(' ')) => return &line[..at],
            _ => {}
        }
        previous = Some(c);
    }
    line
}

/// Every line that carries something, with its indent and text.
fn significant(text: &str) -> Result<Vec<Line>, String> {
    let mut lines = Vec::new();
    for (number, raw) in text.split('\n').enumerate() {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let leading: String = raw.chars().take_while(|c| is_space(*c)).collect();
        if leading.contains('\t') {
            return Err(format!(
                "line {}: indented with a tab — YAML indents with spaces.",
                number + 1
            ));
        }
        let body = without_comment(raw);
        if js::trim(body).is_empty() {
            continue;
        }
        let indent = body.len() - body.trim_start_matches(' ').len();
        lines.push(Line {
            number,
            indent,
            text: js::trim_end(&body[indent..]).to_owned(),
        });
    }
    Ok(lines)
}

/// The line at `at`, or the replacement a sequence entry gives its first key.
fn line_at<'a>(lines: &'a [Line], at: usize, first: Option<&'a Line>) -> Option<&'a Line> {
    if at >= lines.len() {
        return None;
    }
    match first {
        Some(line) if line.number == lines[at].number => Some(line),
        _ => lines.get(at),
    }
}

/// One block of everything indented at `indent` or deeper, from `at`.
fn block(
    lines: &[Line],
    at: usize,
    indent: usize,
    first: Option<&Line>,
) -> Result<(Option<Value>, usize), String> {
    let Some(line) = line_at(lines, at, first) else {
        return Ok((None, at));
    };
    if line.indent < indent {
        return Ok((None, at));
    }
    if line.text.starts_with('-') {
        sequence(lines, at, line.indent).map(|(value, next)| (Some(value), next))
    } else {
        mapping(lines, at, line.indent, first).map(|(value, next)| (Some(value), next))
    }
}

/// `- key: value` opens a mapping: the key must start with neither a space
/// nor a colon, and its first colon ends the line or comes before a space.
fn opens_mapping(rest: &str) -> bool {
    let Some(start) = rest.chars().next() else {
        return false;
    };
    if is_space(start) || start == ':' {
        return false;
    }
    let colon = rest.find(':').unwrap_or(rest.len());
    match rest[colon..].chars().nth(1) {
        _ if colon == rest.len() => false,
        None => true,
        Some(after) => is_space(after),
    }
}

fn sequence(lines: &[Line], at: usize, indent: usize) -> Result<(Value, usize), String> {
    let mut out = Vec::new();
    let mut i = at;
    while let Some(line) = lines
        .get(i)
        .filter(|line| line.indent == indent && line.text.starts_with('-'))
    {
        let rest = &line.text[1..];
        if !rest.is_empty() && !rest.starts_with(' ') {
            return Err(fail(
                line,
                "a \"-\" starts a list item and needs a space after it.",
            ));
        }
        let rest = rest.trim_start_matches(' ');
        if opens_mapping(rest) {
            let key_indent = indent + (line.text.len() - rest.len());
            let patched = Line {
                number: line.number,
                indent: key_indent,
                text: rest.to_owned(),
            };
            let (value, next) = mapping(lines, i, key_indent, Some(&patched))?;
            out.push(value);
            i = next;
            continue;
        }
        if !rest.is_empty() {
            out.push(scalar(rest));
            i += 1;
            continue;
        }
        let (nested, next) = block(lines, i + 1, indent + 1, None)?;
        out.push(nested.unwrap_or(Value::Null));
        i = next;
    }
    Ok((Value::Array(out), i))
}

fn mapping(
    lines: &[Line],
    at: usize,
    indent: usize,
    first: Option<&Line>,
) -> Result<(Value, usize), String> {
    let mut out = Map::new();
    let mut i = at;
    while let Some(line) = line_at(lines, i, first).filter(|line| line.indent == indent) {
        if line.text.starts_with('-') {
            break;
        }
        let Some(colon) = line.text.find(':') else {
            return Err(fail(
                line,
                "expected \"key: value\" — there is no colon on this line.",
            ));
        };
        let key = js::trim(&line.text[..colon]);
        let value = js::trim(&line.text[colon + 1..]);
        if key.is_empty() {
            return Err(fail(line, "a key can’t be empty."));
        }
        if !value.is_empty() {
            out.insert(key.to_owned(), scalar(value));
            i += 1;
            continue;
        }
        let (nested, next) = block(lines, i + 1, indent + 1, None)?;
        out.insert(
            key.to_owned(),
            nested.unwrap_or_else(|| Value::String(String::new())),
        );
        i = next;
    }
    Ok((Value::Object(js::ordered(out)), i))
}

/// Strings stay strings, except the three literals and plain numbers.
fn scalar(raw: &str) -> Value {
    let text = js::trim(raw);
    let Some(first) = text.chars().next() else {
        return Value::String(String::new());
    };
    if (first == '"' || first == '\'') && text.ends_with(first) {
        let inner = if text.chars().count() < 2 {
            ""
        } else {
            &text[1..text.len() - 1]
        };
        return Value::String(inner.to_owned());
    }
    match text {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" | "~" => return Value::Null,
        _ => {}
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "1"));
    let numeric = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if numeric(whole) && numeric(fraction) {
        return js::number(text.parse::<f64>().unwrap_or(f64::NAN));
    }
    Value::String(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::parse;
    use serde_json::json;

    #[test]
    fn reads_the_subset_a_workbench_config_uses() {
        let value = parse(
            "name: Acme # the space\ncolor: \"#2f7d55\"\nport: 4000\nflag: true\nempty:\nlist:\n  - one\n  - label: Sign in\n    src: pages/a.html\n  -\n    nested: yes\n",
        )
        .unwrap();
        assert_eq!(
            value,
            json!({ "name": "Acme", "color": "#2f7d55", "port": 4000, "flag": true, "empty": "",
                "list": ["one", { "label": "Sign in", "src": "pages/a.html" }, { "nested": "yes" }] })
        );
    }

    #[test]
    fn names_the_line_of_each_error() {
        assert_eq!(
            parse("a: 1\n\tb: 2").unwrap_err(),
            "line 2: indented with a tab — YAML indents with spaces."
        );
        assert_eq!(
            parse("  a: 1").unwrap_err(),
            "line 1: the document starts indented."
        );
        assert_eq!(
            parse("a: 1\nnope").unwrap_err(),
            "line 2: expected \"key: value\" — there is no colon on this line."
        );
        assert_eq!(
            parse("list:\n  -x").unwrap_err(),
            "line 2: a \"-\" starts a list item and needs a space after it."
        );
    }

    #[test]
    fn keeps_duplicate_keys_in_their_first_place() {
        let value = parse("a: 1\nb: 2\na: 3").unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), r#"{"a":3,"b":2}"#);
    }
}
