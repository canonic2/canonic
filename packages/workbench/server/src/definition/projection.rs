//! List-entry presentation from validated definition fields.
use super::compatibility::{fallback, truthy};
use crate::paths::{relative, resolve};
use serde_json::{Map, Value, json};
use std::path::Path;

pub(super) fn list(object: &Map<String, Value>, file: &Path, root: &Path) -> Value {
    let preview_id = &object["id"];
    let directory = file.parent().unwrap_or(root);
    let listed = scenarios(object);
    let links = links(object, preview_id);
    let source = object["source"]["entry"]
        .as_str()
        .expect("validated source");
    let mut out = json!({"id": preview_id, "title": fallback(object.get("title"), preview_id.clone()), "adapter": object["adapter"], "file": relative(root, file), "source": relative(root, &resolve(directory, source)), "scenarios": listed, "controls": fallback(object.get("controls"), json!({})), "docs": fallback(object.get("docs"), Value::Null), "links": links});
    for key in ["sizes", "icon"] {
        if let Some(value) = object.get(key) {
            out[key] = value.clone();
        }
    }
    out
}

fn scenarios(object: &Map<String, Value>) -> Vec<Value> {
    let default = json!({"default": {}});
    let scenarios = object
        .get("scenarios")
        .and_then(Value::as_object)
        .filter(|scenarios| !scenarios.is_empty())
        .unwrap_or(default.as_object().expect("default map"));
    let mut listed = vec![];
    for (name, scenario) in scenarios {
        listed.push(
            json!({"id": name, "label": fallback(scenario.get("label"), json!(label(name)))}),
        );
    }
    listed
}

fn links(object: &Map<String, Value>, preview_id: &Value) -> Map<String, Value> {
    let mut links = Map::new();
    if let Some(raw_links) = object.get("links").and_then(Value::as_object) {
        for (href, target) in raw_links {
            let normalized = if target.is_string() {
                json!({"preview": target})
            } else {
                target.clone()
            };
            let mut link =
                json!({"preview": fallback(normalized.get("preview"), preview_id.clone())});
            if let Some(scenario) = normalized.get("scenario").filter(|value| truthy(value)) {
                link["scenario"] = scenario.clone();
            }
            links.insert(href.clone(), link);
        }
    }
    links
}

/// The label a scenario shows without its own: its ID in sentence case
/// (`with-filters` → "With filters"), as the compiler's `titled` writes it.
fn label(id: &str) -> String {
    let words = id.replace('-', " ");
    let mut characters = words.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}
