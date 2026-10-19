//! A space's and a page's `sizes`, read as `contracts/src/sizes/schema.ts`
//! reads them: malformed entries are reported and left out. Pure.
use super::js::{self, label_of};
use serde_json::{Map, Value, json};

const FIELDS: [&str; 5] = ["width", "height", "label", "icon", "button"];

fn defaults() -> Vec<Value> {
    vec![
        json!({ "key": "fit", "label": "Fit", "icon": "minimize-2", "button": true, "kind": "fit", "width": null, "height": null }),
        json!({ "key": "laptop", "label": "Laptop", "icon": "monitor", "button": true, "kind": "fixed", "width": 1512, "height": 982 }),
        json!({ "key": "mobile", "label": "Mobile", "icon": "smartphone", "button": true, "kind": "fixed", "width": 393, "height": 852 }),
        json!({ "key": "resizable", "label": "Resizable", "icon": "scaling", "button": true, "kind": "resizable", "width": null, "height": null }),
    ]
}

fn default_size(key: &str) -> Option<Value> {
    defaults().into_iter().find(|size| size["key"] == key)
}

/// Kebab-case, and not all digits, so it can't be read as a width.
pub(super) fn is_size_key(key: &str) -> bool {
    let kebab = !key.is_empty()
        && !key.starts_with('-')
        && !key.ends_with('-')
        && !key.contains("--")
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    kebab
        && !key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
}

fn is_length(value: &Value) -> bool {
    value == "fill"
        || value
            .as_f64()
            .is_some_and(|number| js::is_integer(number) && (1.0..=8192.0).contains(&number))
}

fn kebab_icon(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// A fixed size's width and height, each its own or the default size's.
fn fixed_lengths(
    fields: &Map<String, Value>,
    base: Option<&Value>,
    where_: &str,
    problems: &mut Vec<String>,
) -> Option<(Value, Value)> {
    let mut lengths = [Value::Null, Value::Null];
    for (slot, name) in lengths.iter_mut().zip(["width", "height"]) {
        let value = match (fields.get(name), base) {
            (None, Some(base)) => base[name].clone(),
            (value, _) => value.cloned().unwrap_or(Value::Null),
        };
        if !is_length(&value) {
            problems.push(format!(
                "{where_}: {name} must be a whole number from 1 to 8192, or fill."
            ));
            return None;
        }
        *slot = value;
    }
    let [width, height] = lengths;
    if width == "fill" && height == "fill" {
        problems.push(format!(
            "{where_}: width and height can’t both be fill; use fit."
        ));
        return None;
    }
    Some((width, height))
}

/// One size from its key and value, or `None` when it can't be used.
fn read_size(
    key: &str,
    raw: &Value,
    where_: &str,
    problems: &mut Vec<String>,
    page: bool,
) -> Option<Value> {
    let base = default_size(key);
    if raw == &Value::Bool(true) {
        let Some(mut base) = base else {
            problems.push(format!("{where_}: true only includes a default size (fit, laptop, mobile, or resizable); give this size a width and height."));
            return None;
        };
        if page {
            base["button"] = Value::Bool(false);
        }
        return Some(base);
    }
    let Some(fields) = raw.as_object() else {
        problems.push(format!(
            "{where_}: must be true or a map of width, height, label, icon, and button."
        ));
        return None;
    };
    for field in fields.keys() {
        if !FIELDS.contains(&field.as_str()) {
            problems.push(format!(
                "{where_}: “{field}” isn’t a size field (width, height, label, icon, button)."
            ));
        }
    }
    let kind = base
        .as_ref()
        .and_then(|size| size["kind"].as_str())
        .unwrap_or("fixed")
        .to_owned();
    if kind != "fixed" && (fields.contains_key("width") || fields.contains_key("height")) {
        let how = if kind == "fit" {
            " fills the canvas"
        } else {
            " is dragged to any size"
        };
        problems.push(format!(
            "{where_}: {key}{how}, so it takes no width or height."
        ));
        return None;
    }
    let (width, height) = if kind == "fixed" {
        fixed_lengths(fields, base.as_ref(), where_, problems)?
    } else {
        (Value::Null, Value::Null)
    };
    let label = match fields.get("label").and_then(Value::as_str).map(js::trim) {
        Some(label) if !label.is_empty() => label.to_owned(),
        _ => base
            .as_ref()
            .and_then(|size| size["label"].as_str())
            .map_or_else(|| label_of(key), str::to_owned),
    };
    let default_icon = base
        .as_ref()
        .and_then(|size| size["icon"].as_str())
        .unwrap_or("frame");
    let icon = match fields.get("icon") {
        None => default_icon.to_owned(),
        Some(raw_icon) => match raw_icon.as_str().map(js::trim) {
            Some(text) if kebab_icon(text) => text.to_owned(),
            _ => {
                problems.push(format!(
                    "{where_}: icon “{}” must be a kebab-case Lucide icon name.",
                    js::string(Some(raw_icon))
                ));
                default_icon.to_owned()
            }
        },
    };
    let mut button = if page {
        false
    } else {
        base.as_ref().is_some_and(|size| size["button"] == true)
    };
    if let Some(raw_button) = fields.get("button") {
        if page {
            problems.push(format!("{where_}: button doesn’t apply to a page’s own size; the size switcher’s buttons belong to the space."));
        } else if let Some(flag) = raw_button.as_bool() {
            button = flag;
        } else {
            problems.push(format!("{where_}: button must be true or false."));
        }
    }
    Some(
        json!({ "key": key, "label": label, "icon": icon, "button": button, "kind": kind, "width": width, "height": height }),
    )
}

/// A space's sizes in order, or the defaults when none can be used.
/// `local` names the keys workbench.local.yaml set.
pub(super) fn space_sizes(
    raw: Option<&Value>,
    problems: &mut Vec<String>,
    local: &[String],
) -> Vec<Value> {
    let Some(raw) = raw.filter(|raw| !raw.is_null()) else {
        return defaults();
    };
    let Some(entries) = raw.as_object() else {
        problems.push("Sizes: must be a map of size keys to sizes.".to_owned());
        return defaults();
    };
    let mut out = Vec::new();
    for (key, value) in entries {
        if !is_size_key(key) {
            problems.push(format!(
                "Sizes: “{key}” must be a kebab-case key that isn’t all digits."
            ));
            continue;
        }
        if let Some(mut size) = read_size(key, value, &format!("Sizes › {key}"), problems, false)
        {
            if local.contains(key) {
                size["local"] = Value::Bool(true);
            }
            out.push(size);
        }
    }
    if out.is_empty() {
        problems.push("Sizes: no size can be used, so the default sizes are.".to_owned());
        return defaults();
    }
    out
}

/// A page's sizes: `{ sizes, ownSizes? }`, or `None` when it supports every
/// size of the space.
pub(super) fn page_sizes(
    raw: Option<&Value>,
    space: &[Value],
    where_: &str,
    problems: &mut Vec<String>,
) -> Option<Value> {
    let raw = raw.filter(|raw| !raw.is_null() && raw.as_str() != Some(""))?;
    let entries = match raw {
        Value::Array(items) => items.clone(),
        other => vec![other.clone()],
    };
    let in_space = |key: &str| space.iter().any(|size| size["key"] == key);
    let mut sizes: Vec<String> = Vec::new();
    let mut own = Vec::new();
    for entry in &entries {
        if let Some(text) = entry.as_str() {
            let key = js::trim(text);
            if !in_space(key) {
                problems.push(format!(
                    "{where_}: size “{key}” isn’t one of the space’s sizes."
                ));
                continue;
            }
            if !sizes.iter().any(|size| size == key) {
                sizes.push(key.to_owned());
            }
            continue;
        }
        let Some((key, value)) = entry
            .as_object()
            .filter(|map| map.len() == 1)
            .and_then(|map| map.iter().next())
        else {
            problems.push(format!("{where_}: each size is a key of the space’s sizes, or one key with the fields of a size of the page’s own."));
            continue;
        };
        if !is_size_key(key) {
            problems.push(format!(
                "{where_}: size “{key}” must be a kebab-case key that isn’t all digits."
            ));
            continue;
        }
        if in_space(key) {
            problems.push(format!("{where_}: size “{key}” is one of the space’s sizes; a page can list it but not change it."));
            continue;
        }
        if sizes.contains(key) {
            problems.push(format!("{where_}: size “{key}” is listed twice."));
            continue;
        }
        let Some(size) = read_size(key, value, &format!("{where_} › {key}"), problems, true)
        else {
            continue;
        };
        sizes.push(key.clone());
        own.push(size);
    }
    if sizes.is_empty() {
        return None;
    }
    let mut out = Map::new();
    out.insert("sizes".to_owned(), json!(sizes));
    if !own.is_empty() {
        out.insert("ownSizes".to_owned(), Value::Array(own));
    }
    Some(Value::Object(out))
}
