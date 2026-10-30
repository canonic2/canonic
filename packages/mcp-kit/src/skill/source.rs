//! Bounded YAML frontmatter parsing without aliases, tags or ambiguous mappings.

use super::SkillError;
use serde_json::{Map, Value};
use yaml_rust2::{
    parser::{Event, Parser},
    scanner::TScalarStyle,
};

pub(super) const FRONTMATTER_BYTES: usize = 65536;

pub(super) fn frontmatter(bytes: &[u8]) -> Result<Value, SkillError> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("SKILL.md must be UTF-8"))?;
    let mut lines = text.split_inclusive('\n');
    if lines.next().map(|line| line.trim_end_matches(['\n', '\r'])) != Some("---") {
        return Err(invalid("missing opening frontmatter delimiter"));
    }
    let start = text
        .find('\n')
        .ok_or_else(|| invalid("missing frontmatter"))?
        + 1;
    let mut end = start;
    let mut closed = false;
    for line in lines {
        if line.trim_end_matches(['\n', '\r']) == "---" {
            closed = true;
            break;
        }
        end += line.len();
        if end - start > FRONTMATTER_BYTES {
            return Err(SkillError::Limit("frontmatter bytes"));
        }
    }
    if !closed {
        return Err(invalid("missing closing frontmatter delimiter"));
    }
    let mut parser = Parser::new_from_str(&text[start..end]);
    expect(&mut parser, &Event::StreamStart)?;
    expect(&mut parser, &Event::DocumentStart)?;
    let first = next(&mut parser)?;
    let value = node(&mut parser, first, 0)?;
    expect(&mut parser, &Event::DocumentEnd)?;
    expect(&mut parser, &Event::StreamEnd)?;
    validate(&value)?;
    crate::json::measure(&value, FRONTMATTER_BYTES, &|| Ok(()))
        .map_err(|_| SkillError::Limit("frontmatter JSON bytes"))?;
    Ok(value)
}

fn next(parser: &mut Parser<std::str::Chars<'_>>) -> Result<Event, SkillError> {
    parser
        .next_token()
        .map(|(event, _)| event)
        .map_err(|_| invalid("invalid YAML"))
}

fn expect(parser: &mut Parser<std::str::Chars<'_>>, expected: &Event) -> Result<(), SkillError> {
    if &next(parser)? != expected {
        return Err(invalid("expected one YAML document"));
    }
    Ok(())
}

fn node(
    parser: &mut Parser<std::str::Chars<'_>>,
    event: Event,
    depth: usize,
) -> Result<Value, SkillError> {
    match event {
        Event::Scalar(text, style, 0, None) => scalar(text, style),
        Event::SequenceStart(0, None) if depth < 32 => {
            let mut values = Vec::new();
            loop {
                let event = next(parser)?;
                if event == Event::SequenceEnd {
                    break;
                }
                values.push(node(parser, event, depth + 1)?);
            }
            Ok(Value::Array(values))
        }
        Event::MappingStart(0, None) if depth < 32 => {
            let mut values = Map::new();
            loop {
                let event = next(parser)?;
                if event == Event::MappingEnd {
                    break;
                }
                let Value::String(key) = node(parser, event, depth + 1)? else {
                    return Err(invalid("mapping keys must be strings"));
                };
                if key == "<<" || values.contains_key(&key) {
                    return Err(invalid("duplicate or merge key"));
                }
                let event = next(parser)?;
                values.insert(key, node(parser, event, depth + 1)?);
            }
            Ok(Value::Object(values))
        }
        _ => Err(invalid("unsupported YAML syntax or nesting limit")),
    }
}

fn scalar(text: String, style: TScalarStyle) -> Result<Value, SkillError> {
    if text.contains('\0') {
        return Err(invalid("NUL in frontmatter"));
    }
    if style != TScalarStyle::Plain {
        return Ok(Value::String(text));
    }
    match text.as_str() {
        "" | "~" | "null" | "Null" | "NULL" => return Ok(Value::Null),
        "true" | "True" | "TRUE" => return Ok(Value::Bool(true)),
        "false" | "False" | "FALSE" => return Ok(Value::Bool(false)),
        _ => {}
    }
    let lower = text.to_ascii_lowercase();
    if matches!(lower.as_str(), ".nan" | ".inf" | "-.inf" | "+.inf") {
        return Err(invalid("non-finite number"));
    }
    let unsigned = text
        .strip_prefix('+')
        .or_else(|| text.strip_prefix('-'))
        .unwrap_or(&text);
    let radix = if unsigned.starts_with("0x") {
        Some(16)
    } else if unsigned.starts_with("0o") {
        Some(8)
    } else {
        None
    };
    if let Some(radix) = radix {
        let digits = &unsigned[2..];
        if !digits.is_empty() && digits.chars().all(|c| c.is_digit(radix)) {
            let number =
                u64::from_str_radix(digits, radix).map_err(|_| invalid("integer out of range"))?;
            return integer(number, text.starts_with('-'));
        }
    }
    if !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()) {
        let number = unsigned
            .parse::<u64>()
            .map_err(|_| invalid("integer out of range"))?;
        return integer(number, text.starts_with('-'));
    }
    if unsigned.contains(['.', 'e', 'E'])
        && let Ok(number) = text.parse::<f64>()
    {
        let number =
            serde_json::Number::from_f64(number).ok_or_else(|| invalid("non-finite number"))?;
        if decimal(&text) != decimal(&number.to_string()) {
            return Err(invalid("number loses precision in JSON"));
        }
        return Ok(Value::Number(number));
    }
    Ok(Value::String(text))
}

fn integer(number: u64, negative: bool) -> Result<Value, SkillError> {
    if !negative {
        return Ok(Value::from(number));
    }
    let signed = i64::try_from(-i128::from(number)).map_err(|_| invalid("integer out of range"))?;
    Ok(Value::from(signed))
}

fn decimal(text: &str) -> Option<(bool, String, i64)> {
    let negative = text.starts_with('-');
    let text = text.trim_start_matches(['+', '-']);
    let (mantissa, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
    let exponent = exponent.parse::<i64>().ok()?;
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Some((false, "0".into(), 0));
    }
    let trimmed = digits.trim_end_matches('0');
    let scale = exponent
        .checked_sub(i64::try_from(fraction.len()).ok()?)?
        .checked_add(i64::try_from(digits.len() - trimmed.len()).ok()?)?;
    Some((negative, trimmed.into(), scale))
}

fn validate(value: &Value) -> Result<(), SkillError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("frontmatter must be a mapping"))?;
    let name = value["name"]
        .as_str()
        .ok_or_else(|| invalid("name must be a string"))?;
    if name.is_empty()
        || name.len() > 64
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(invalid("invalid skill name"));
    }
    let description = value["description"]
        .as_str()
        .ok_or_else(|| invalid("description must be a string"))?;
    if description.trim().is_empty() || description.chars().count() > 1024 {
        return Err(invalid("invalid description"));
    }
    if let Some(compatibility) = object.get("compatibility") {
        let text = compatibility
            .as_str()
            .ok_or_else(|| invalid("compatibility must be a string"))?;
        if text.is_empty() || text.chars().count() > 500 {
            return Err(invalid("invalid compatibility"));
        }
    }
    for key in ["license", "allowed-tools"] {
        if object.get(key).is_some_and(|value| !value.is_string()) {
            return Err(invalid("invalid optional field"));
        }
    }
    if let Some(metadata) = object.get("metadata")
        && !metadata
            .as_object()
            .is_some_and(|map| map.values().all(Value::is_string))
    {
        return Err(invalid("metadata must map strings to strings"));
    }
    Ok(())
}

fn invalid(reason: &'static str) -> SkillError {
    SkillError::Source(reason)
}
