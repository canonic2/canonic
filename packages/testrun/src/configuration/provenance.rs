//! Value-free field provenance and source diagnostics across configuration layers.
use super::*;

pub(crate) fn fields(
    value: &Value,
    pointer: &str,
    label: &str,
    origins: &mut BTreeMap<String, String>,
    replace: bool,
) {
    if value.is_null() {
        return;
    }
    if let Some(map) = value.as_object()
        && !map.is_empty()
    {
        for (key, value) in map {
            fields(
                value,
                &format!("{pointer}/{}", escape(key)),
                label,
                origins,
                replace,
            );
        }
    } else if replace {
        origins.insert(pointer.into(), label.into());
    } else {
        origins
            .entry(pointer.into())
            .or_insert_with(|| label.into());
    }
}
pub(crate) fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
pub(crate) fn with_file(mut error: Error, file: impl Serialize) -> Error {
    let mut details = error.details.take().unwrap_or(json!({}));
    details["file"] = json!(file);
    if details.get("pointer").is_none() {
        details["pointer"] = json!("");
    }
    error.details(details)
}
pub(crate) fn argument(error: canonic_mcp_kit::schema::ArgumentError, value: &Value) -> Error {
    let mut remaining = error.path.strip_prefix("arguments").unwrap_or(&error.path);
    let mut current = value;
    let mut pointer = String::new();
    while !remaining.is_empty() {
        if let Some(map) = current.as_object() {
            let key = map
                .keys()
                .filter(|key| {
                    remaining
                        .strip_prefix(&format!(".{key}"))
                        .is_some_and(|rest| {
                            rest.is_empty() || rest.starts_with('.') || rest.starts_with('[')
                        })
                })
                .max_by_key(|key| key.len());
            if let Some(key) = key {
                remaining = &remaining[key.len() + 1..];
                pointer.push('/');
                pointer.push_str(&escape(key));
                current = &map[key];
                continue;
            }
            if let Some(missing) = remaining.strip_prefix('.') {
                pointer.push('/');
                pointer.push_str(&escape(missing));
            }
            break;
        }
        if let Some(rest) = remaining.strip_prefix('[')
            && let Some((index, tail)) = rest.split_once(']')
            && let Ok(index) = index.parse::<usize>()
        {
            pointer.push('/');
            pointer.push_str(&index.to_string());
            remaining = tail;
            current = current.get(index).unwrap_or(&Value::Null);
            continue;
        }
        break;
    }
    Error::new("CONFIG_INVALID", error.message).details(json!({"pointer":pointer}))
}
