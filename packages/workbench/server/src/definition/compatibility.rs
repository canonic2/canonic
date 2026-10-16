//! JavaScript coercion rules and the conservative native-validation boundary.
use crate::paths::resolve;
use serde_json::Value;
use std::path::Path;

fn requests_valid(requests: Option<&Value>) -> bool {
    static KEY: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let Some(requests) = requests else {
        return true;
    };
    let Some(requests) = requests.as_object() else {
        return false;
    };
    let key = KEY.get_or_init(|| {
        regex::Regex::new(r"^(?:[A-Z]+\s+)?(?:/\S*|[a-z][a-z0-9+.-]*://\S+)(?:\s+[A-Za-z_]\w*)?$")
            .expect("request key expression")
    });
    requests
        .iter()
        .all(|(name, value)| name.is_ascii() && key.is_match(name.trim()) && value.is_object())
}

pub(super) fn kebab(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
pub(super) fn id(value: &str) -> bool {
    value.split('/').all(kebab)
}
pub(super) fn string(value: Option<&Value>, check: fn(&str) -> bool) -> bool {
    value.and_then(Value::as_str).is_some_and(check)
}
pub(super) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        _ => true,
    }
}
pub(super) fn fallback(value: Option<&Value>, default: Value) -> Value {
    value
        .filter(|value| truthy(value))
        .cloned()
        .unwrap_or(default)
}

pub(super) fn supported(raw: &Value, file: &Path, root: &Path) -> bool {
    if raw.get("kind").and_then(Value::as_str) == Some("docs") {
        return false;
    }
    if !requests_valid(raw.get("requests")) {
        return false;
    }
    if let Some(states) = raw.get("states").and_then(Value::as_object) {
        if states
            .values()
            .any(|state| !state.is_object() || !requests_valid(state.get("requests")))
        {
            return false;
        }
    } else if raw
        .get("states")
        .is_some_and(|value| !value.is_null() && !value.is_object())
    {
        return false;
    }
    if raw
        .get("controls")
        .is_some_and(|value| !value.is_object() && truthy(value))
    {
        return false;
    }
    // JavaScript's realpath and whitespace rules own unusual source paths,
    // including an outside alias that resolves back inside the project.
    let directory = file.parent().unwrap_or(root);
    let mut candidates = vec![raw.get("source")];
    if let Some(states) = raw.get("states").and_then(Value::as_object) {
        candidates.extend(states.values().map(|state| state.get("source")));
    }
    if candidates
        .into_iter()
        .flatten()
        .filter_map(|source| source.get("entry").and_then(Value::as_str))
        .any(|entry| {
            !resolve(directory, entry).starts_with(root) || entry.contains(['\u{feff}', '\u{85}'])
        })
    {
        return false;
    }
    true
}
