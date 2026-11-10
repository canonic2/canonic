//! Copilot's flat per-event callback arrays and exact callback ownership.
use crate::{agents::setup::HookGroups, review::Error};
use serde_json::{Value, json};
use std::path::Path;

const COEXISTENCE: &str = "Copilot imports incompatible Shield callbacks from another host. Use a separate trusted Copilot checkout/profile without those imported hooks; existing Claude files are unchanged";

fn incompatible(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            if recognized(value) {
                let copilot = value["args"].as_array().is_some_and(|args| {
                    args.windows(2)
                        .any(|pair| pair[0] == "--agent" && pair[1] == "copilot")
                }) || value["command"]
                    .as_str()
                    .is_some_and(|command| command.contains("--agent copilot --event "));
                if !copilot {
                    return true;
                }
            }
            object.values().any(incompatible)
        }
        Value::Array(values) => values.iter().any(incompatible),
        _ => false,
    }
}

fn check_source(source: Option<String>) -> Result<(), Error> {
    if let Some(source) = source {
        let value = crate::json::parse(source.as_bytes())
            .map_err(|_| Error::Invalid("cannot verify imported Copilot hook configuration"))?;
        if incompatible(&value) {
            return Err(Error::Invalid(COEXISTENCE));
        }
    }
    Ok(())
}

pub(super) fn check_project(
    root: &Path,
    pending: &dyn Fn(&str) -> Option<Option<String>>,
) -> Result<(), Error> {
    for name in [
        ".claude/settings.json",
        ".claude/settings.local.json",
        ".github/copilot/settings.json",
        ".github/copilot/settings.local.json",
    ] {
        check_source(match pending(name) {
            Some(source) => source,
            None => super::super::mcp_scopes::read(&root.join(name))?,
        })?;
    }
    Ok(())
}

pub(super) fn check_compatible(
    root: &Path,
    pending: &dyn Fn(&str) -> Option<Option<String>>,
) -> Result<(), Error> {
    check_project(root, pending)?;
    let home = super::super::mcp_scopes::configuration("COPILOT_HOME", ".copilot")?;
    check_source(super::super::mcp_scopes::read(&home.join("settings.json"))?)?;
    let directory = home.join("hooks");
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for (count, entry) in entries.enumerate() {
        if count >= 128 {
            return Err(Error::Invalid(
                "Copilot user hook discovery exceeds setup limit",
            ));
        }
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            check_source(super::super::mcp_scopes::read(&path)?)?;
        }
    }
    Ok(())
}

fn document(source: Option<&str>) -> Result<Value, Error> {
    let mut value = super::super::setup::parse(source)?;
    let object = value
        .as_object_mut()
        .ok_or(Error::Invalid("Copilot hooks must be an object"))?;
    if object.get("version").is_some_and(|version| version != 1)
        || object.get("hooks").is_some_and(|hooks| !hooks.is_object())
    {
        return Err(Error::Invalid("invalid Copilot hook configuration"));
    }
    object.insert("version".into(), json!(1));
    if object.get("hooks").is_none() {
        object.insert("hooks".into(), json!({}));
    }
    if value["hooks"]
        .as_object()
        .expect("validated hooks")
        .values()
        .any(|entries| !entries.is_array())
    {
        return Err(Error::Invalid("Copilot hook events must be arrays"));
    }
    Ok(value)
}

fn recognized(entry: &Value) -> bool {
    entry["command"]
        .as_str()
        .is_some_and(|command| command.contains("__internal native-hook"))
        || entry["args"].as_array().is_some_and(|args| {
            args.windows(2)
                .any(|pair| pair[0] == "__internal" && pair[1] == "native-hook")
        })
}

pub(super) fn capture(source: Option<&str>, expected: &HookGroups) -> Result<HookGroups, Error> {
    let value = document(source)?;
    let mut captured = HookGroups::new();
    for (event, entries) in value["hooks"].as_object().expect("validated hooks") {
        for entry in entries
            .as_array()
            .expect("validated array")
            .iter()
            .filter(|entry| recognized(entry))
        {
            if expected.get(event) != Some(entry)
                || captured.insert(event.clone(), entry.clone()).is_some()
            {
                return Err(Error::Invalid(
                    "Copilot Shield hook is unowned, duplicated or manually changed",
                ));
            }
        }
    }
    Ok(captured)
}

pub(super) fn merge(
    source: Option<&str>,
    old: &HookGroups,
    new: &HookGroups,
    repair: bool,
) -> Result<String, Error> {
    if !repair {
        capture(source, old)?;
    }
    if new.values().any(|entry| !recognized(entry)) {
        return Err(Error::Invalid("invalid owned Copilot callback"));
    }
    let mut value = document(source)?;
    let hooks = value["hooks"].as_object_mut().expect("validated hooks");
    for entries in hooks.values_mut() {
        entries
            .as_array_mut()
            .expect("validated array")
            .retain(|entry| !recognized(entry));
    }
    hooks.retain(|_, entries| !entries.as_array().expect("validated array").is_empty());
    for (event, entry) in new {
        hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("validated array")
            .push(entry.clone());
    }
    serde_json::to_string_pretty(&value)
        .map(|value| format!("{value}\n"))
        .map_err(|_| Error::Invalid("Copilot hooks encoding"))
}
