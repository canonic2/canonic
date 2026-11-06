//! Named hook definitions, preserving every unrelated Antigravity hook.
use crate::{agents::setup::HookGroups, review::Error};
use serde_json::{Value, json};
const NAME: &str = "shield-project-enforcement";

fn expected(groups: &HookGroups) -> Value {
    let mut value = json!({"enabled":true});
    for (event, group) in groups {
        value[event] = json!([group]);
    }
    value
}

pub(super) fn diagnose(
    source: Option<&str>,
    groups: &HookGroups,
    diagnostics: &mut Vec<&'static str>,
) -> Result<(), Error> {
    let config = super::super::setup::parse(source)?;
    let Some(actual) = config.get(NAME) else {
        diagnostics.push("owned native hook group is missing");
        return Ok(());
    };
    let mut wanted = expected(groups);
    if actual["enabled"] == false {
        diagnostics.push("Shield native hook is disabled in project settings");
        wanted["enabled"] = json!(false);
    }
    if actual != &wanted {
        diagnostics.push("owned native hook definition was changed");
    }
    Ok(())
}

pub(super) fn capture(source: Option<&str>, groups: &HookGroups) -> Result<HookGroups, Error> {
    let config = super::super::setup::parse(source)?;
    let captured = if config.get(NAME) == Some(&expected(groups)) {
        groups.clone()
    } else {
        HookGroups::new()
    };
    merge(source, &captured, groups)?;
    Ok(captured)
}

pub(super) fn merge(
    source: Option<&str>,
    old: &HookGroups,
    new: &HookGroups,
) -> Result<String, Error> {
    let mut config = super::super::setup::parse(source)?;
    let object = config
        .as_object_mut()
        .ok_or(Error::Invalid("Antigravity hooks must be an object"))?;
    if let Some(actual) = object.get(NAME)
        && (old.is_empty() || actual != &expected(old))
    {
        return Err(Error::Invalid(
            "Shield hook definition is unowned or manually changed",
        ));
    }
    // A renamed Shield definition must not silently survive removal or repair.
    for (_, value) in object.iter().filter(|(name, _)| name.as_str() != NAME) {
        if contains_command(value) {
            return Err(Error::Invalid(
                "Shield hook definition is unowned or manually changed",
            ));
        }
    }
    object.remove(NAME);
    if !new.is_empty() {
        object.insert(NAME.into(), expected(new));
    }
    serde_json::to_string_pretty(&config)
        .map(|s| format!("{s}\n"))
        .map_err(|_| Error::Invalid("Antigravity hooks encoding"))
}

fn contains_command(value: &Value) -> bool {
    match value {
        Value::Object(fields) => {
            fields
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(|s| s.contains("__internal native-hook"))
                || fields.values().any(contains_command)
        }
        Value::Array(values) => values.iter().any(contains_command),
        _ => false,
    }
}
