//! Reusable codec for agents using JSON hook event arrays.
use crate::review::Error;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) type HookGroups = BTreeMap<String, Value>;

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
                .is_some_and(|rows| rows.contains(group))
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
        for group in groups.iter() {
            let shield = group["hooks"].as_array().is_some_and(|entries| {
                entries.iter().any(|h| {
                    h["statusMessage"] == "Shield project enforcement"
                        || h["command"]
                            .as_str()
                            .is_some_and(|s| s.contains("__internal native-hook"))
                })
            });
            if shield && Some(group) != old {
                return Err(Error::Invalid(
                    "Shield hook group is unowned or manually changed",
                ));
            }
        }
        if let Some(old) = old {
            groups.retain(|g| g != old);
        }
        if let Some(new) = new {
            groups.push(new.clone());
        }
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

pub(crate) fn diagnose(
    source: Option<&str>,
    owned: &HookGroups,
    diagnostics: &mut Vec<&'static str>,
) -> Result<Value, Error> {
    let config = parse(source)?;
    if owned.iter().any(|(event, group)| {
        !config["hooks"][event]
            .as_array()
            .is_some_and(|g| g.contains(group))
    }) {
        diagnostics.push("owned native hook group is missing");
    }
    Ok(config)
}
