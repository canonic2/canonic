//! Reusable codec for agents using JSON hook event arrays.
use crate::review::Error;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) type HookGroups = BTreeMap<String, Value>;

#[cfg(test)]
mod tests;

pub(crate) fn shield_callback(entry: &Value) -> bool {
    entry["statusMessage"] == "Shield project enforcement"
        || entry["command"]
            .as_str()
            .is_some_and(|command| command.contains("__internal native-hook"))
}

/// Header scope belongs to the owned group; callbacks from other integrations
/// may share that scope without becoming Shield-owned.
pub(crate) fn same_hook_header(actual: &Value, owned: &Value) -> bool {
    actual
        .as_object()
        .zip(owned.as_object())
        .is_some_and(|(actual, owned)| {
            actual.keys().filter(|key| key.as_str() != "hooks").count()
                == owned.keys().filter(|key| key.as_str() != "hooks").count()
                && owned
                    .iter()
                    .all(|(key, value)| key == "hooks" || actual.get(key) == Some(value))
        })
}

pub(crate) fn hook_group_matches(actual: &Value, owned: &Value) -> bool {
    if !same_hook_header(actual, owned) {
        return false;
    }
    let Some((actual, expected)) = actual["hooks"].as_array().zip(owned["hooks"].as_array()) else {
        return false;
    };
    !expected.is_empty()
        && expected.iter().all(shield_callback)
        && actual.iter().filter(|entry| shield_callback(entry)).count() == expected.len()
        && expected.iter().all(|entry| {
            expected.iter().filter(|other| *other == entry).count() == 1
                && actual.iter().filter(|other| *other == entry).count() == 1
        })
}

fn owned_group_index(groups: &[Value], previous: Option<&Value>) -> Result<Option<usize>, Error> {
    let mut recognized = groups.iter().enumerate().filter(|(_, group)| {
        group["hooks"]
            .as_array()
            .is_some_and(|entries| entries.iter().any(shield_callback))
    });
    let Some((index, group)) = recognized.next() else {
        return Ok(None);
    };
    if recognized.next().is_some()
        || previous.is_none_or(|previous| !hook_group_matches(group, previous))
    {
        return Err(Error::Invalid(
            "Shield hook group is unowned or manually changed",
        ));
    }
    Ok(Some(index))
}

fn merge_event(
    groups: &mut Vec<Value>,
    previous: Option<&Value>,
    desired: Option<&Value>,
) -> Result<(), Error> {
    if [previous, desired]
        .into_iter()
        .flatten()
        .any(|owned| !hook_group_matches(owned, owned))
    {
        return Err(Error::Invalid("invalid owned Shield callbacks"));
    }
    if let Some(index) = owned_group_index(groups, previous)? {
        let mut retained = groups.remove(index);
        let same_header = desired.is_some_and(|desired| same_hook_header(&retained, desired));
        let callbacks = retained["hooks"]
            .as_array_mut()
            .expect("matched callback array");
        let position = callbacks
            .iter()
            .position(shield_callback)
            .expect("owned callback");
        callbacks.retain(|entry| !shield_callback(entry));
        if same_header {
            let desired = desired.expect("matching desired header")["hooks"]
                .as_array()
                .expect("validated owned callbacks");
            drop(callbacks.splice(position..position, desired.iter().cloned()));
        }
        if !callbacks.is_empty() {
            groups.insert(index, retained);
        }
        if same_header {
            return Ok(());
        }
    }
    if let Some(desired) = desired {
        groups.push(desired.clone());
    }
    Ok(())
}

pub(crate) fn parse(source: Option<&str>) -> Result<Value, Error> {
    crate::json::parse(source.unwrap_or("{}").as_bytes())
        .map_err(|_| Error::Invalid("invalid hook settings JSON"))
}

pub(crate) fn capture(source: Option<&str>, expected: &HookGroups) -> Result<HookGroups, Error> {
    let config = parse(source)?;
    let captured = expected
        .iter()
        .filter(|(event, group)| {
            config["hooks"][*event]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|actual| hook_group_matches(actual, group)))
        })
        .map(|(event, group)| (event.clone(), group.clone()))
        .collect();
    // Recognizable edited entries still conflict; capture confers no authority.
    merge(source, &captured, expected)?;
    Ok(captured)
}

pub(crate) fn merge(
    source: Option<&str>,
    previous: &HookGroups,
    desired: &HookGroups,
) -> Result<String, Error> {
    let mut config = parse(source)?;
    let object = config
        .as_object_mut()
        .ok_or(Error::Invalid("hook settings must be an object"))?;
    let hooks = object.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or(Error::Invalid("hooks must be an object"))?;
    let events: std::collections::BTreeSet<_> = previous.keys().chain(desired.keys()).collect();
    for event in events {
        let old = previous.get(event);
        let new = desired.get(event);
        let groups = hooks.entry(event).or_insert_with(|| json!([]));
        let groups = groups
            .as_array_mut()
            .ok_or(Error::Invalid("hook event must be an array"))?;
        merge_event(groups, old, new)?;
        if groups.is_empty() {
            hooks.remove(event);
        }
    }
    if hooks.is_empty() {
        object.remove("hooks");
    }
    serde_json::to_string_pretty(&config)
        .map(|text| format!("{text}\n"))
        .map_err(|_| Error::Invalid("hook settings encoding"))
}

/// Prepare an explicitly requested reconciliation. The surrounding setup plan
/// retains the original bytes and requires human confirmation before writing.
pub(crate) fn repair(source: Option<&str>, desired: &HookGroups) -> Result<String, Error> {
    let mut config = parse(source)?;
    if let Some(hooks) = config.get_mut("hooks") {
        let hooks = hooks
            .as_object_mut()
            .ok_or(Error::Invalid("hooks must be an object"))?;
        for groups in hooks.values_mut() {
            let Some(groups) = groups.as_array_mut() else {
                // The selected events are validated by merge below. Unrelated
                // host-specific values cannot contain callbacks we recognize.
                continue;
            };
            groups.retain_mut(|group| {
                if let Some(entries) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                    let before = entries.len();
                    entries.retain(|entry| !shield_callback(entry));
                    return before == entries.len() || !entries.is_empty();
                }
                true
            });
        }
    }
    let source =
        serde_json::to_string(&config).map_err(|_| Error::Invalid("hook settings encoding"))?;
    merge(Some(&source), &HookGroups::new(), desired)
}

pub(crate) fn diagnose(
    source: Option<&str>,
    owned: &HookGroups,
    diagnostics: &mut Vec<&'static str>,
) -> Result<Value, Error> {
    let config = parse(source)?;
    if owned.iter().any(|(event, group)| {
        !config["hooks"][event].as_array().is_some_and(|groups| {
            owned_group_index(groups, Some(group)).is_ok_and(|index| index.is_some())
        })
    }) {
        diagnostics.push("owned native hook group is missing");
    }
    Ok(config)
}
