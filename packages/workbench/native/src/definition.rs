//! Preview validation and list projection for literal definitions.
//! Unusual JavaScript coercions and request mocks use the compiler adapter.
use serde_json::{Map, Value, json};
use std::path::{Component, Path, PathBuf};

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

fn kebab(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
fn id(value: &str) -> bool {
    value.split('/').all(kebab)
}
fn string(value: Option<&Value>, check: fn(&str) -> bool) -> bool {
    value.and_then(Value::as_str).is_some_and(check)
}
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        _ => true,
    }
}
fn fallback(value: Option<&Value>, default: Value) -> Value {
    value.filter(|value| truthy(value)).cloned().unwrap_or(default)
}
fn resolve(base: &Path, entry: &str) -> PathBuf {
    let mut result = PathBuf::new();
    for part in base.join(entry).components() {
        match part {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other),
        }
    }
    result
}
fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/")
}
fn label(id: &str) -> String {
    let mut label = String::new();
    let mut characters = id.chars().enumerate().peekable();
    while let Some((at, character)) = characters.next() {
        if at == 0 && character.is_ascii_alphanumeric() {
            label.push(character.to_ascii_uppercase());
        } else if character == '-'
            && characters.peek().is_some_and(|(_, next)| next.is_ascii_alphanumeric())
        {
            label.push(' ');
            if let Some((_, next)) = characters.next() {
                label.push(next.to_ascii_uppercase());
            }
        } else {
            label.push(character);
        }
    }
    label
}

pub(crate) struct Projection {
    pub(crate) entry: Result<Value, String>,
    pub(crate) sources: Vec<PathBuf>,
}

/// Return `None` for rules delegated to JavaScript, preserving its coercions.
#[expect(
    clippy::too_many_lines,
    reason = "ordered validation mirrors the compiler's public contract"
)]
pub(crate) fn project(
    raw: &Value,
    file: &Path,
    root: &Path,
    mut source_exists: impl FnMut(&Path) -> bool,
) -> Option<Projection> {
    if raw.get("kind").and_then(Value::as_str) == Some("docs") {
        return None;
    }
    if !requests_valid(raw.get("requests")) {
        return None;
    }
    if let Some(states) = raw.get("states").and_then(Value::as_object) {
        if states.values().any(|state| !state.is_object() || !requests_valid(state.get("requests")))
        {
            return None;
        }
    } else if raw.get("states").is_some_and(|value| !value.is_null() && !value.is_object()) {
        return None;
    }
    if raw.get("controls").is_some_and(|value| !value.is_object() && truthy(value)) {
        return None;
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
        return None;
    }
    let mut sources = vec![];
    let result = (|| {
        let error = |message: &str| format!("{}: {message}", file.display());
        let object =
            raw.as_object().ok_or_else(|| error("default export must be definePreview({...})."))?;
        if !string(object.get("id"), id) {
            return Err(error("id must use kebab-case segments separated by /."));
        }
        let preview_id = &object["id"];
        if !string(object.get("adapter"), kebab) {
            return Err(error("adapter must be a registered kebab-case name."));
        }
        if object.contains_key("icon") && !string(object.get("icon"), kebab) {
            return Err(error("icon must be a kebab-case Lucide icon name."));
        }
        let mut source_values = vec![object.get("source")];
        if let Some(states) = object.get("states").and_then(Value::as_object) {
            source_values.extend(
                states
                    .values()
                    .filter_map(|state| state.get("source"))
                    .filter(|value| truthy(value))
                    .map(Some),
            );
        }
        let directory = file.parent().unwrap_or(root);
        for source in source_values {
            let entry = source
                .and_then(|value| value.get("entry"))
                .and_then(Value::as_str)
                .filter(|entry| !entry.trim().is_empty())
                .ok_or_else(|| error("source.entry must name a local source file."))?;
            let target = resolve(directory, entry);
            sources.push(target.clone());
            if !target.starts_with(root) || !source_exists(&target) {
                return Err(error(&format!("source must exist inside the project: {entry}")));
            }
        }
        if object.get("states").is_some_and(|states| !states.is_object()) {
            return Err(error("states must be a map."));
        }
        let default = json!({"default": {}});
        let states = object
            .get("states")
            .and_then(Value::as_object)
            .filter(|states| !states.is_empty())
            .unwrap_or(default.as_object().expect("default map"));
        let mut listed = vec![];
        for (name, state) in states {
            if !kebab(name) || !state.is_object() {
                return Err(error("states need kebab-case IDs and object definitions."));
            }
            listed.push(
                json!({"id": name, "label": fallback(state.get("label"), json!(label(name)))}),
            );
        }
        if let Some(sizes) = object.get("sizes") {
            let valid = sizes.as_array().is_some_and(|sizes| {
                !sizes.is_empty()
                    && sizes.iter().all(|size| {
                        size.as_str().is_some_and(|size| {
                            kebab(size)
                                && !size.starts_with('-')
                                && !size.ends_with('-')
                                && !size.contains("--")
                                && size.bytes().any(|byte| byte.is_ascii_lowercase())
                        })
                    })
            });
            if !valid {
                return Err(error("sizes must be a list of size keys, such as laptop or mobile."));
            }
        }
        if let Some(controls) = object.get("controls").and_then(Value::as_object) {
            for (name, control) in controls {
                if !control.get("type").and_then(Value::as_str).is_some_and(|kind| {
                    ["text", "number", "boolean", "select", "json"].contains(&kind)
                }) {
                    return Err(error(&format!("invalid control {name}")));
                }
                if control["type"] == "select"
                    && control.get("options").and_then(Value::as_array).is_none_or(Vec::is_empty)
                {
                    return Err(error("select controls need options."));
                }
            }
        }
        if object.get("links").is_some_and(|links| !links.is_object()) {
            return Err(error("links must be a map."));
        }
        let mut links = Map::new();
        if let Some(raw_links) = object.get("links").and_then(Value::as_object) {
            for (href, target) in raw_links {
                let normalized =
                    if target.is_string() { json!({"preview": target}) } else { target.clone() };
                let valid = normalized.is_object()
                    && (normalized.get("preview").is_some_and(truthy)
                        || normalized.get("state").is_some_and(truthy))
                    && normalized.get("preview").is_none_or(|value| string(Some(value), id))
                    && normalized.get("state").is_none_or(|value| string(Some(value), kebab));
                if !valid {
                    return Err(error(&format!(
                        "link {href} must name a preview ID, or {{ preview, state }}."
                    )));
                }
                let mut link =
                    json!({"preview": fallback(normalized.get("preview"), preview_id.clone())});
                if let Some(state) = normalized.get("state").filter(|value| truthy(value)) {
                    link["state"] = state.clone();
                }
                links.insert(href.clone(), link);
            }
        }
        let source = object["source"]["entry"].as_str().expect("validated source");
        let mut out = json!({"id": preview_id, "title": fallback(object.get("title"), preview_id.clone()), "adapter": object["adapter"], "file": relative(root, file), "source": relative(root, &resolve(directory, source)), "states": listed, "controls": fallback(object.get("controls"), json!({})), "docs": fallback(object.get("docs"), Value::Null), "links": links});
        for key in ["sizes", "icon"] {
            if let Some(value) = object.get(key) {
                out[key] = value.clone();
            }
        }
        Ok(out)
    })();
    Some(Projection { entry: result, sources })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_errors_keep_the_definition_path() {
        let root = Path::new("/acme");
        let file = root.join("page.workbench.ts");
        for (raw, message) in [
            (json!(null), "default export"),
            (json!({}), "id must"),
            (json!({"id":"page"}), "adapter must"),
            (json!({"id":"page","adapter":"html","icon":"BAD"}), "icon must"),
        ] {
            assert!(
                project(&raw, &file, root, |_| true)
                    .unwrap()
                    .entry
                    .unwrap_err()
                    .starts_with(&format!("{}: {message}", file.display()))
            );
        }
    }

    #[test]
    fn projection_and_validation_match_list_fields() {
        let root = Path::new("/acme");
        let file = root.join("page.workbench.ts");
        let raw = json!({"id":"page","adapter":"html","source":{"entry":"page.html"},"states":{"a--b":{},"-ready":{},"empty":{}},"links":{"/next":{"state":"empty"}},"sizes":["mobile"]});
        let entry = project(&raw, &file, root, |_| true).unwrap().entry.unwrap();
        assert_eq!(entry["states"][0]["label"], "A- B");
        assert_eq!(entry["states"][1]["label"], " Ready");
        assert_eq!(entry["links"]["/next"]["preview"], "page");
        assert_eq!(entry["title"], "page");
        for (key, value, message) in [
            ("sizes", json!([]), "sizes must"),
            ("controls", json!({"tone":{"type":"select","options":[]}}), "select controls"),
            ("links", json!({"/bad":null}), "link /bad must"),
        ] {
            let mut invalid = raw.clone();
            invalid[key] = value;
            assert!(
                project(&invalid, &file, root, |_| true)
                    .unwrap()
                    .entry
                    .unwrap_err()
                    .contains(message)
            );
        }
        assert!(
            project(&raw, &file, root, |_| false)
                .unwrap()
                .entry
                .unwrap_err()
                .contains("source must exist")
        );
    }
}
