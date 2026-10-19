//! JavaScript's rules where the Node config reader relies on them: its
//! whitespace, `trim`, `Number(...)`, `String(...)` and the order of an
//! object's keys. Pure.
use serde_json::{Map, Number, Value};

/// A character JavaScript's `\s` and `trim()` treat as whitespace.
pub(super) fn is_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// `String.prototype.trim`.
pub(super) fn trim(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// `text.replace(/\s+$/, '')`.
pub(super) fn trim_end(text: &str) -> &str {
    text.trim_end_matches(is_space)
}

/// An array-index key, which JavaScript orders before every other key.
fn index_key(key: &str) -> Option<u32> {
    if key == "0" {
        return Some(0);
    }
    if key.is_empty() || key.starts_with('0') || !key.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    key.parse::<u32>().ok().filter(|index| *index < u32::MAX)
}

/// An object's keys in JavaScript's order: array indexes ascending, then
/// the rest in insertion order.
pub(super) fn ordered(map: Map<String, Value>) -> Map<String, Value> {
    if !map.keys().any(|key| index_key(key).is_some()) {
        return map;
    }
    let mut indexes = Vec::new();
    let mut rest = Vec::new();
    for (key, value) in map {
        match index_key(&key) {
            Some(index) => indexes.push((index, key, value)),
            None => rest.push((key, value)),
        }
    }
    indexes.sort_by_key(|entry| entry.0);
    indexes
        .into_iter()
        .map(|(_, key, value)| (key, value))
        .chain(rest)
        .collect()
}

/// A JavaScript number as JSON: whole numbers as integers.
pub(super) fn number(value: f64) -> Value {
    const SAFE: f64 = 9_007_199_254_740_992.0;
    if value.fract() == 0.0 && value.abs() < SAFE {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a whole number below 2^53 fits an i64 exactly"
        )]
        return Value::from(value as i64);
    }
    Number::from_f64(value).map_or(Value::Null, Value::Number)
}

/// `String(number)`.
pub(super) fn number_text(value: &Number) -> String {
    if let Some(whole) = value.as_i64() {
        return whole.to_string();
    }
    if let Some(whole) = value.as_u64() {
        return whole.to_string();
    }
    let float = value.as_f64().unwrap_or(f64::NAN);
    if float.fract() == 0.0 && float.abs() < 1e21 {
        format!("{float:.0}")
    } else {
        format!("{float}")
    }
}

/// `String(value)`, for the values YAML produces.
pub(super) fn string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_owned(),
        Some(Value::Null) => "null".to_owned(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number_text(number),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_owned(),
    }
}

/// `Number(value)`.
pub(super) fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Object(_)) => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => string_number(text),
        Some(Value::Array(items)) => match items.as_slice() {
            [] => 0.0,
            [only] => string_number(&string(Some(only))),
            _ => f64::NAN,
        },
    }
}

static DECIMAL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

fn string_number(text: &str) -> f64 {
    let text = trim(text);
    if text.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix).map_or(f64::NAN, |value| {
                #[expect(clippy::cast_precision_loss, reason = "JavaScript numbers are doubles")]
                let value = value as f64;
                value
            });
        }
    }
    match text {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let decimal = DECIMAL.get_or_init(|| {
        regex::Regex::new(r"^[+-]?(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][+-]?[0-9]+)?$")
            .expect("decimal expression")
    });
    if decimal.is_match(text) {
        text.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

/// `Number.isInteger(value)`.
pub(super) fn is_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0
}

/// "local-dev" to "Local dev", as `labelOf` writes a label from a key.
pub(super) fn label_of(key: &str) -> String {
    let words = key.replace('-', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_read_and_print_as_javascript_does() {
        assert_eq!(to_number(Some(&json!(" 12 "))), 12.0);
        assert_eq!(to_number(Some(&json!("0x10"))), 16.0);
        assert_eq!(to_number(Some(&json!(""))), 0.0);
        assert!(to_number(Some(&json!("12px"))).is_nan());
        assert!(to_number(Some(&json!("inf"))).is_nan());
        assert_eq!(to_number(Some(&json!(".5"))), 0.5);
        assert_eq!(to_number(Some(&json!("1e3"))), 1000.0);
        assert_eq!(number(7.0), json!(7));
        assert_eq!(number_text(json!(1.5).as_number().unwrap()), "1.5");
        assert_eq!(string(Some(&json!([1, null, "a"]))), "1,,a");
    }

    #[test]
    fn index_keys_come_first() {
        let map: Map<String, Value> = [("b", 1), ("10", 2), ("a", 3), ("2", 4), ("01", 5)]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), json!(value)))
            .collect();
        let keys: Vec<_> = ordered(map).keys().cloned().collect();
        assert_eq!(keys, ["2", "10", "b", "a", "01"]);
    }

    #[test]
    fn whitespace_is_javascripts() {
        assert_eq!(trim("\u{feff} a \u{a0}"), "a");
        assert_eq!(trim("\u{85}a"), "\u{85}a");
    }
}
