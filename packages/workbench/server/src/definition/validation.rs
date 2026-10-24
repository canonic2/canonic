//! Ordered definition validation; no filesystem, process or response I/O.
use super::compatibility::{id, kebab, string, truthy};
use crate::paths::resolve;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
fn failure(message: &str) -> String {
    message.into()
}

pub(super) fn check<'a>(
    raw: &'a Value,
    file: &Path,
    root: &Path,
    mut source_exists: impl FnMut(&Path) -> bool,
    sources: &mut Vec<PathBuf>,
) -> Result<&'a Map<String, Value>, String> {
    let object = raw
        .as_object()
        .ok_or_else(|| failure("default export must be definePreview({...})."))?;
    if !string(object.get("id"), id) {
        return Err(failure("id must use kebab-case segments separated by /."));
    }
    if !string(object.get("adapter"), kebab) {
        return Err(failure("adapter must be a registered kebab-case name."));
    }
    if object.contains_key("icon") && !string(object.get("icon"), kebab) {
        return Err(failure("icon must be a kebab-case Lucide icon name."));
    }
    sources_valid(
        object,
        file.parent().unwrap_or(root),
        root,
        &mut source_exists,
        sources,
    )?;
    if object
        .get("scenarios")
        .is_some_and(|scenarios| !scenarios.is_object())
    {
        return Err(failure("scenarios must be a map."));
    }
    if let Some(scenarios) = object.get("scenarios").and_then(Value::as_object)
        && scenarios
            .iter()
            .any(|(name, scenario)| !kebab(name) || !scenario.is_object())
    {
        return Err(failure(
            "scenarios need kebab-case IDs and object definitions.",
        ));
    }
    sizes(object)?;
    controls(object)?;
    links(object)?;
    Ok(object)
}

fn sources_valid(
    object: &Map<String, Value>,
    directory: &Path,
    root: &Path,
    mut source_exists: impl FnMut(&Path) -> bool,
    sources: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let mut source_values = vec![object.get("source")];
    if let Some(scenarios) = object.get("scenarios").and_then(Value::as_object) {
        source_values.extend(
            scenarios
                .values()
                .filter_map(|scenario| scenario.get("source"))
                .filter(|value| truthy(value))
                .map(Some),
        );
    }
    for source in source_values {
        let entry = source
            .and_then(|value| value.get("entry"))
            .and_then(Value::as_str)
            .filter(|entry| !entry.trim().is_empty())
            .ok_or_else(|| failure("source.entry must name a local source file."))?;
        let target = resolve(directory, entry);
        sources.push(target.clone());
        if !target.starts_with(root) || !source_exists(&target) {
            return Err(failure(&format!(
                "source must exist inside the project: {entry}"
            )));
        }
    }
    Ok(())
}

fn sizes(object: &Map<String, Value>) -> Result<(), String> {
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
            return Err(failure(
                "sizes must be a list of size keys, such as laptop or mobile.",
            ));
        }
    }
    Ok(())
}

fn controls(object: &Map<String, Value>) -> Result<(), String> {
    if let Some(controls) = object.get("controls").and_then(Value::as_object) {
        for (name, control) in controls {
            if !control
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| ["text", "number", "boolean", "select", "json"].contains(&kind))
            {
                return Err(failure(&format!("invalid control {name}")));
            }
            if control["type"] == "select"
                && control
                    .get("options")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty)
            {
                return Err(failure("select controls need options."));
            }
        }
    }
    Ok(())
}

fn links(object: &Map<String, Value>) -> Result<(), String> {
    if object.get("links").is_some_and(|links| !links.is_object()) {
        return Err(failure("links must be a map."));
    }
    if let Some(raw_links) = object.get("links").and_then(Value::as_object) {
        for (href, target) in raw_links {
            let (preview, scenario) = if target.is_string() {
                (Some(target), None)
            } else {
                (target.get("preview"), target.get("scenario"))
            };
            let valid = (target.is_object() || target.is_string())
                && (preview.is_some_and(truthy) || scenario.is_some_and(truthy))
                && preview.is_none_or(|value| string(Some(value), id))
                && scenario.is_none_or(|value| string(Some(value), kebab));
            if !valid {
                return Err(failure(&format!(
                    "link {href} must name a preview ID, or {{ preview, scenario }}."
                )));
            }
        }
    }
    Ok(())
}
