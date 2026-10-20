//! Changing a space's workbench.yaml for the page form and the size dialogs
//! (`/_workbench/config-file` and `/_workbench/sizes`), as
//! `server/node/config.js` does: one block of the committed file replaced,
//! every other line and comment kept, and the new text checked to parse
//! before it replaces the file in one rename. Also lists the spaces a
//! workbench.yaml holds. `sizes` decides what the size dialogs write; `text`
//! edits the file's text.
mod sizes;
mod text;

pub(crate) use sizes::{SizeEdit, add_size, update_sizes};

use super::{manifest, read, yaml};
use crate::refused::refused;
use anyhow::anyhow;
use serde_json::{Map, Value, json};
use std::path::Path;

const FILE: &str = "workbench.yaml";

/// Which space's config: the workbench.yaml in `dir`, and its space `key`,
/// or its first space when `None`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Location<'a> {
    pub(crate) dir: &'a Path,
    pub(crate) key: Option<&'a str>,
}

/// The committed file as the page form and the size dialogs start from it.
#[derive(Debug)]
pub(crate) struct Source {
    pub(crate) body: String,
    /// The space the location picked, or `None` for a file without spaces.
    pub(crate) key: Option<String>,
    /// The collections the space shows: its own, or the shared ones it inherits.
    pub(crate) collections: Value,
    /// The space's own `sizes` as written, if any.
    pub(crate) sizes: Option<Value>,
}

/// JavaScript truthiness of a JSON value.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(_) => true,
    }
}

fn read_file(dir: &Path, name: &str) -> Result<Option<String>, String> {
    match std::fs::read_to_string(dir.join(name)) {
        Ok(body) => Ok(Some(body)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Couldn’t read {name}: {error}")),
    }
}

fn parse_file(body: &str, name: &str) -> Result<Value, String> {
    yaml::parse(body).map_err(|error| format!("{name}, {error}"))
}

/// The committed file for the space, or `None` without a workbench.yaml.
///
/// # Errors
/// Returns the reader's message when the file can't be read or parsed.
pub(crate) fn source(at: Location<'_>) -> Result<Option<Source>, String> {
    let Some(body) = read_file(at.dir, FILE)? else {
        return Ok(None);
    };
    let raw = parse_file(&body, FILE)?;
    let (picked, key, _) = manifest::select_space(&raw, at.key, &mut Vec::new());
    let collections = picked
        .get("collections")
        .filter(|value| truthy(Some(value)))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let sizes = picked.get("sizes").cloned();
    Ok(Some(Source {
        body,
        key,
        collections,
        sizes,
    }))
}

fn missing() -> String {
    format!("There is no {FILE} at the project root.")
}

/// The new text, checked to parse, replaces the file in one rename.
fn write_checked(dir: &Path, next: &str) -> Result<(), String> {
    parse_file(next, FILE)?;
    let target = dir.join(FILE);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let temporary = dir.join(format!("{FILE}.canonic-{}-{millis}", std::process::id()));
    let written = std::fs::metadata(&target)
        .and_then(|metadata| {
            std::fs::write(&temporary, next)?;
            std::fs::set_permissions(&temporary, metadata.permissions())?;
            std::fs::rename(&temporary, &target)
        })
        .map_err(|error| format!("Couldn’t write {FILE}: {error}"));
    // Gone after a rename; left behind only by a failed write.
    let _removed = std::fs::remove_file(&temporary);
    written
}

/// The page form's save. A space of a file that lists several gets its own
/// collections, so the others keep theirs, shared or not.
///
/// # Errors
/// Returns why the collections can't be written.
pub(crate) fn update_collections(at: Location<'_>, updated: &Value) -> Result<Source, String> {
    if !updated.is_array() {
        return Err("collections must be a list.".to_owned());
    }
    let current = source(at)?.ok_or_else(missing)?;
    let next = text::replace_block(
        &current.body,
        current.key.as_deref(),
        "collections",
        updated,
    )?;
    write_checked(at.dir, &next)?;
    source(at)?.ok_or_else(missing)
}

/// The size dialogs' save: `edit` gets the space's sizes and collections as
/// written and answers with what to write. Sizes are the space's own block;
/// collections are replaced whole, as the page form replaces them.
///
/// # Errors
/// Returns `edit`'s refusal, or why the file can't be written.
pub(crate) fn update_space_sizes(
    at: Location<'_>,
    edit: impl FnOnce(Option<&Value>, &Value) -> Result<SizeEdit, String>,
) -> Result<SizeEdit, String> {
    let current = source(at)?.ok_or_else(missing)?;
    let change = edit(current.sizes.as_ref(), &current.collections)?;
    let key = current.key.as_deref();
    let mut next = current.body.clone();
    if let Some(sizes) = &change.sizes {
        next = text::replace_block(&next, key, "sizes", &Value::Object(sizes.clone()))?;
    }
    if let Some(collections) = &change.collections {
        next = text::replace_block(&next, key, "collections", collections)?;
    }
    if next != current.body {
        write_checked(at.dir, &next)?;
    }
    Ok(change)
}

/// The space's sizes as the reader resolves them, empty without a workbench.yaml.
fn resolved_sizes(at: Location<'_>) -> Result<Vec<Value>, String> {
    Ok(read::resolved(at.dir, at.key)?
        .and_then(|config| config.get("sizes").and_then(Value::as_array).cloned())
        .unwrap_or_default())
}

fn parse_ask(body: &[u8]) -> anyhow::Result<Value> {
    serde_json::from_slice(body).map_err(|error| anyhow!("invalid JSON: {error}"))
}

/// `GET /_workbench/config-file`: the page form's collections and the
/// space's size keys. Refused with 404 without a workbench.yaml.
///
/// # Errors
/// Errors other than the 404 answer 400.
pub(crate) fn form(at: Location<'_>) -> anyhow::Result<Value> {
    let current = source(at)
        .map_err(|error| anyhow!(error))?
        .ok_or_else(|| refused(404, missing()))?;
    let sizes: Vec<Value> = resolved_sizes(at)
        .map_err(|error| anyhow!(error))?
        .into_iter()
        .map(|size| size["key"].clone())
        .collect();
    Ok(json!({ "ok": true, "collections": current.collections, "sizes": sizes }))
}

/// `POST /_workbench/config-file` with `{ collections }`.
///
/// # Errors
/// Every error answers 400.
pub(crate) fn save_form(at: Location<'_>, body: &[u8]) -> anyhow::Result<Value> {
    let ask = parse_ask(body)?;
    let collections = ask.get("collections").cloned().unwrap_or(Value::Null);
    let saved = update_collections(at, &collections).map_err(|error| anyhow!(error))?;
    Ok(json!({ "ok": true, "collections": saved.collections }))
}

/// `POST /_workbench/sizes` with `{ action: "add", size }` or
/// `{ action: "update", sizes }`, answering with the space's sizes as they
/// now resolve and the new size's key.
///
/// # Errors
/// Every error answers 400.
pub(crate) fn save_sizes(at: Location<'_>, body: &[u8]) -> anyhow::Result<Value> {
    let ask = parse_ask(body)?;
    let before = read::resolved(at.dir, at.key)
        .map_err(|error| anyhow!(error))?
        .ok_or_else(|| anyhow!(missing()))?;
    let local: Vec<String> = before["sizes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|size| size["local"] == true)
        .filter_map(|size| size["key"].as_str().map(str::to_owned))
        .collect();
    let empty = Value::Object(Map::new());
    let change = update_space_sizes(at, |sizes, collections| {
        match ask.get("action").and_then(Value::as_str) {
            Some("add") => {
                let size = ask
                    .get("size")
                    .filter(|size| truthy(Some(size)))
                    .unwrap_or(&empty);
                add_size(sizes, collections, size)
            }
            Some("update") => {
                let rows = ask.get("sizes").cloned().unwrap_or_else(|| json!([]));
                update_sizes(sizes, collections, &rows, &local)
            }
            _ => Err("Ask to add a size or update the sizes.".to_owned()),
        }
    })
    .map_err(|error| anyhow!(error))?;
    let sizes = resolved_sizes(at).map_err(|error| anyhow!(error))?;
    Ok(json!({ "ok": true, "key": change.key, "sizes": sizes }))
}

#[cfg(test)]
mod tests;
