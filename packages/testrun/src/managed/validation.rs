//! Pure managed-action validation and translation to admitted batch steps.
use crate::error::{Error, Result};
use serde_json::{Value, json};

pub(crate) fn steps(args: &Value) -> Result<Value> {
    let definitions = crate::catalog::internal_tools();
    let actions = args["actions"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 8)
        .ok_or_else(|| Error::invalid("Supply 1..8 actions"))?;
    let mut steps = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        steps.push(
            step(index, action, &definitions)
                .map_err(|error| error.at_action(index, action["action"].as_str()))?,
        );
    }
    crate::qa::duration_budget(
        steps
            .iter()
            .map(|s| (s["tool"].as_str().unwrap_or(""), &s["arguments"])),
        crate::qa::BATCH_TIMEOUT_MS,
    )?;
    Ok(json!(steps))
}

fn step(index: usize, action: &Value, definitions: &[Value]) -> Result<Value> {
    let operation = action["action"]
        .as_str()
        .ok_or_else(|| Error::invalid("Choose an action"))?;
    let allowed = crate::catalog::action_fields(operation).ok_or_else(|| {
        Error::invalid(
            "Unsupported action; choose tap, swipe, long_press, key, type_text, app_launch, app_terminate, app_restart, open_url or delay",
        )
    })?;
    let mut input = action
        .as_object()
        .cloned()
        .ok_or_else(|| Error::invalid("An action must be an object"))?;
    if let Some(key) = input
        .keys()
        .find(|key| *key != "action" && !allowed.contains(&key.as_str()))
    {
        let mut error = Error::invalid(format!(
            "actions[{index}] ({operation}).{key} is not a supported field; allowed fields: action, {}",
            allowed.join(", ")
        ));
        if matches!(operation, "tap" | "long_press") && matches!(key.as_str(), "x" | "y") {
            error=error.details(json!({"example":{"action":operation,"point":{"x":0.5,"y":0.5}},"coordinateSource":"Use the observed target position; example coordinates are illustrative"}));
        }
        return Err(error);
    }
    input.remove("action");
    if matches!(operation, "tap" | "long_press") {
        crate::ui::semantic::validate_pointer(&json!(input))?;
    }
    if matches!(operation, "swipe" | "long_press") && !input.contains_key("durationMs") {
        input.insert(
            "durationMs".into(),
            json!(if operation == "swipe" { 150 } else { 500 }),
        );
    }
    // Validate caller arguments before creating any mutation receipt. The
    // retained owner supplies only authority, scope and display references.
    let mut schema = if operation == "delay" {
        crate::catalog::delay_schema()
    } else {
        definitions
            .iter()
            .find(|tool| tool["name"] == operation)
            .unwrap()["inputSchema"]
            .clone()
    };
    if let Some(required) = schema["required"].as_array_mut() {
        required.retain(|field| {
            !matches!(
                field.as_str(),
                Some(
                    "runId"
                        | "goalId"
                        | "sessionId"
                        | "leaseId"
                        | "commandId"
                        | "scope"
                        | "display"
                )
            )
        });
    }
    crate::schema::validate_at(
        &schema,
        &json!(input),
        &format!("actions[{index}] ({operation})"),
    )?;
    Ok(json!({"tool":operation,"arguments":input}))
}

/// `android_chords` is the selected controller's recorded keycombination probe.
pub(super) fn backend_options(
    steps: &Value,
    kind: &str,
    control_path: Option<&str>,
    android_chords: bool,
) -> Result<()> {
    for (index, step) in steps.as_array().into_iter().flatten().enumerate() {
        let name = step["tool"].as_str().unwrap_or("");
        if crate::qa::delay_step(name) {
            continue;
        }
        if kind == "android" {
            crate::mobile::validate_android(name, &step["arguments"], android_chords)
        } else {
            crate::mobile::validate_backend_options(kind, control_path, name, &step["arguments"])
        }
        .map_err(|error| error.at_action(index, Some(name)))?;
    }
    Ok(())
}
