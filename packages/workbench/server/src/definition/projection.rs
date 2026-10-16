//! List-entry presentation from validated definition fields.
use super::compatibility::{fallback, truthy};
use crate::paths::{relative, resolve};
use serde_json::{Map, Value, json};
use std::path::Path;

pub(super) fn list(object: &Map<String, Value>, file: &Path, root: &Path) -> Value {
    let preview_id = &object["id"];
    let directory = file.parent().unwrap_or(root);
    let listed = states(object);
    let links = links(object, preview_id);
    let source = object["source"]["entry"]
        .as_str()
        .expect("validated source");
    let mut out = json!({"id": preview_id, "title": fallback(object.get("title"), preview_id.clone()), "adapter": object["adapter"], "file": relative(root, file), "source": relative(root, &resolve(directory, source)), "states": listed, "controls": fallback(object.get("controls"), json!({})), "docs": fallback(object.get("docs"), Value::Null), "links": links});
    for key in ["sizes", "icon"] {
        if let Some(value) = object.get(key) {
            out[key] = value.clone();
        }
    }
    out
}

fn states(object: &Map<String, Value>) -> Vec<Value> {
    let default = json!({"default": {}});
    let states = object
        .get("states")
        .and_then(Value::as_object)
        .filter(|states| !states.is_empty())
        .unwrap_or(default.as_object().expect("default map"));
    let mut listed = vec![];
    for (name, state) in states {
        listed.push(json!({"id": name, "label": fallback(state.get("label"), json!(label(name)))}));
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
            if let Some(state) = normalized.get("state").filter(|value| truthy(value)) {
                link["state"] = state.clone();
            }
            links.insert(href.clone(), link);
        }
    }
    links
}

fn label(id: &str) -> String {
    let mut label = String::new();
    let mut characters = id.chars().enumerate().peekable();
    while let Some((at, character)) = characters.next() {
        if at == 0 && character.is_ascii_alphanumeric() {
            label.push(character.to_ascii_uppercase());
        } else if character == '-'
            && characters
                .peek()
                .is_some_and(|(_, next)| next.is_ascii_alphanumeric())
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
