//! Reusable codec for agents using JSON hook event arrays.
use crate::review::Error;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) type HookGroups = BTreeMap<String, Value>;
pub(crate) type PortableHookGroups = BTreeMap<String, Vec<Value>>;

/// Automatic maintenance accepts only exact known callbacks and headers. The
/// broad reviewed repair codec remains separate: resemblance is not ownership.
pub(crate) fn migrate(
    source: &str,
    permitted: &PortableHookGroups,
    desired: &HookGroups,
) -> Result<String, Error> {
    let config = parse(Some(source))?;
    let hooks = config["hooks"]
        .as_object()
        .ok_or(Error::Invalid("hooks must be an object"))?;
    let mut observed = std::collections::BTreeSet::new();
    for (event, groups) in hooks {
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for group in groups {
            let Some(callbacks) = group["hooks"].as_array() else {
                continue;
            };
            for callback in callbacks.iter().filter(|entry| shield_callback(entry)) {
                let known = permitted.get(event).is_some_and(|groups| {
                    groups.iter().any(|known| {
                        same_hook_header(group, known)
                            && known["hooks"]
                                .as_array()
                                .is_some_and(|entries| entries.contains(callback))
                    })
                });
                if !known {
                    return Err(Error::Invalid(
                        "unknown Shield callback is preserved; review explicit hook repair",
                    ));
                }
                observed.insert(event.as_str());
            }
        }
    }
    if desired
        .keys()
        .any(|event| !observed.contains(event.as_str()))
    {
        return Err(Error::Invalid(
            "owned callback is missing; review explicit hook repair",
        ));
    }
    repair(Some(source), desired)
}

/// The arguments of Shield's generated MCP server entry for `agent` in the
/// project at `root`: `mcp serve --project . --agent <agent>`, followed by the
/// project-relative `--config` of an explicit configuration selection, so the
/// server selects the same policy files as the command that set it up.
pub(crate) fn serve_args(root: &std::path::Path, agent: &str) -> Vec<String> {
    serve_args_for_project(root, agent, ".")
}

pub(super) fn serve_args_for_project(
    root: &std::path::Path,
    agent: &str,
    project: &str,
) -> Vec<String> {
    let mut args: Vec<String> = ["mcp", "serve", "--project", project, "--agent", agent]
        .into_iter()
        .map(str::to_owned)
        .collect();
    args.extend(crate::layout::launch_args(root).unwrap_or_default());
    args
}

/// Recognize only the supported generated stdio argument shape. Other settings
/// remain byte/semantic data of the caller's own codec, never regenerated here.
pub(crate) fn portable_mcp_args(
    command: &str,
    args: &[&str],
    agent: &str,
    aliases: &[&str],
    binary: &std::path::Path,
    roots: &[&std::path::Path],
) -> Result<Vec<String>, Error> {
    if !(command == "shield" || binary.to_str() == Some(command))
        || args.len() != 6
        || !matches!(&args[..2], ["mcp", "serve"] | ["__internal", "mcp"])
        || args[2] != "--project"
        || args[4] != "--agent"
        || !(args[5] == agent || aliases.contains(&args[5]))
        || !(args[3] == "." || roots.iter().any(|root| root.to_str() == Some(args[3])))
    {
        return Err(Error::Invalid("unknown MCP launcher is preserved"));
    }
    Ok(["mcp", "serve", "--project", ".", "--agent", agent]
        .into_iter()
        .map(str::to_owned)
        .collect())
}

pub(crate) fn portable_json_mcp(
    source: &str,
    agent: &str,
    aliases: &[&str],
    binary: &std::path::Path,
    roots: &[&std::path::Path],
) -> Result<String, Error> {
    let mut value = parse(Some(source))?;
    let command = value["command"]
        .as_str()
        .ok_or(Error::Invalid("MCP command missing"))?;
    let args = value["args"]
        .as_array()
        .ok_or(Error::Invalid("MCP arguments missing"))?
        .iter()
        .map(|arg| {
            arg.as_str()
                .ok_or(Error::Invalid("MCP argument is not text"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let args = portable_mcp_args(command, &args, agent, aliases, binary, roots)?;
    value["command"] = json!("shield");
    value["args"] = json!(args);
    serde_json::to_string(&value).map_err(|_| Error::Invalid("portable MCP entry encoding"))
}

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
        return Err(Error::HookOwnership(
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
