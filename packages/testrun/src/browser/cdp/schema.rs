//! Validation of native CDP commands, results and events against pinned schemas.

use serde_json::Value;
use std::sync::LazyLock;

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 100_000;
static PROTOCOL: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("protocol.json")).expect("Generated pinned CDP schema")
});

#[cfg(test)]
mod tests;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("CDP method is absent from the pinned schema")]
    Method,
    #[error("CDP value differs from the pinned schema")]
    Shape,
    #[error("CDP value exceeds structural bounds")]
    Bounds,
    #[error("Pinned CDP schema contains an unresolved type")]
    Definition,
}

fn domain(name: &str) -> Option<&'static Value> {
    PROTOCOL["domains"]
        .as_array()?
        .iter()
        .find(|domain| domain["domain"] == name)
}

pub(super) fn commit() -> &'static str {
    PROTOCOL["sources"][0]["commit"]
        .as_str()
        .expect("Pinned CDP source commit")
}

fn member<'a>(method: &'a str, family: &str) -> Option<(&'a str, &'static Value)> {
    let (name, method) = method.split_once('.')?;
    let value = domain(name)?[family]
        .as_array()?
        .iter()
        .find(|value| value["name"] == method)?;
    Some((name, value))
}

pub(super) fn command(method: &str, params: &Value) -> Result<(), Error> {
    let (domain, method) = member(method, "commands").ok_or(Error::Method)?;
    object(domain, method["parameters"].as_array(), params, 0)
}

pub(super) fn result(method: &str, result: &Value) -> Result<(), Error> {
    let (domain, method) = member(method, "commands").ok_or(Error::Method)?;
    object(domain, method["returns"].as_array(), result, 0)
}

pub(super) fn event(method: &str, params: &Value) -> Result<(), Error> {
    if let Some((domain, event)) = member(method, "events") {
        object(domain, event["parameters"].as_array(), params, 0)
    } else if method.starts_with("Target.")
        || method.starts_with("Page.frame")
        || method.starts_with("Runtime.executionContext")
    {
        Err(Error::Method)
    } else {
        // An unknown optional event is private diagnostic metadata, not a newly
        // enabled command or a source of resource authority.
        structure(params, 0, &mut 0)
    }
}

/// External browsers can add fields to known messages. Strip those fields
/// before strict validation and before any consumer sees the payload.
pub(super) fn project_incoming(
    method: &str,
    event: bool,
    payload: &mut Value,
) -> Result<(), Error> {
    let (family, fields) = if event {
        ("events", "parameters")
    } else {
        ("commands", "returns")
    };
    if let Some((domain, member)) = member(method, family) {
        structure(payload, 0, &mut 0)?;
        project_object(domain, member[fields].as_array(), payload, 0, false)?;
    }
    Ok(())
}

fn project_object(
    domain: &str,
    fields: Option<&Vec<Value>>,
    value: &mut Value,
    depth: usize,
    target_info: bool,
) -> Result<(), Error> {
    let values = value.as_object_mut().ok_or(Error::Shape)?;
    values.retain(|name, _| {
        // TargetInfo.pid remains reserved for the verified Edge normalization.
        (target_info && name == "pid")
            || fields.is_some_and(|fields| {
                fields
                    .iter()
                    .any(|field| field["name"].as_str() == Some(name.as_str()))
            })
    });
    for field in fields.into_iter().flatten() {
        let name = field["name"].as_str().ok_or(Error::Definition)?;
        if let Some(value) = values.get_mut(name) {
            project_value(domain, field, value, depth + 1)?;
        }
    }
    Ok(())
}

fn project_value(scope: &str, field: &Value, value: &mut Value, depth: usize) -> Result<(), Error> {
    if depth > MAX_DEPTH {
        return Err(Error::Bounds);
    }
    if let Some(reference) = field["$ref"].as_str() {
        let (scope, name) = reference.split_once('.').unwrap_or((scope, reference));
        let definition = domain(scope)
            .and_then(|domain| domain["types"].as_array())
            .and_then(|types| types.iter().find(|value| value["id"] == name))
            .ok_or(Error::Definition)?;
        return project_value(scope, definition, value, depth + 1);
    }
    match field["type"].as_str() {
        Some("object") if field.get("properties").is_some() => project_object(
            scope,
            field["properties"].as_array(),
            value,
            depth + 1,
            scope == "Target" && field["id"] == "TargetInfo",
        ),
        Some("array") => {
            let values = value.as_array_mut().ok_or(Error::Shape)?;
            let item = field.get("items").ok_or(Error::Definition)?;
            for value in values {
                project_value(scope, item, value, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Edge adds a process ID to TargetInfo. Ignore valid IDs on verified Edge
/// connections so resource indexing sees only the pinned TargetInfo shape.
pub(super) fn discard_edge_target_pid(method: &str, payload: &mut Value) {
    match method {
        "Target.targetCreated"
        | "Target.targetInfoChanged"
        | "Target.attachedToTarget"
        | "Target.getTargetInfo" => {
            if let Some(target) = payload.get_mut("targetInfo") {
                discard_pid(target);
            }
        }
        "Target.getTargets" => {
            if let Some(targets) = payload.get_mut("targetInfos").and_then(Value::as_array_mut) {
                for target in targets {
                    discard_pid(target);
                }
            }
        }
        _ => {}
    }
}

fn discard_pid(target: &mut Value) {
    if let Some(target) = target.as_object_mut()
        && target
            .get("pid")
            .and_then(Value::as_u64)
            .is_some_and(|pid| i32::try_from(pid).is_ok())
    {
        target.remove("pid");
    }
}

fn structure(value: &Value, depth: usize, nodes: &mut usize) -> Result<(), Error> {
    *nodes += 1;
    if depth > MAX_DEPTH || *nodes > MAX_NODES {
        return Err(Error::Bounds);
    }
    match value {
        Value::Array(values) => {
            for value in values {
                structure(value, depth + 1, nodes)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                structure(value, depth + 1, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn object(
    domain: &str,
    fields: Option<&Vec<Value>>,
    value: &Value,
    depth: usize,
) -> Result<(), Error> {
    if depth == 0 {
        structure(value, 0, &mut 0)?;
    }
    if depth > MAX_DEPTH {
        return Err(Error::Bounds);
    }
    let values = value.as_object().ok_or(Error::Shape)?;
    if values.keys().any(|name| {
        fields.is_none_or(|fields| {
            !fields
                .iter()
                .any(|field| field["name"].as_str() == Some(name.as_str()))
        })
    }) {
        return Err(Error::Shape);
    }
    for field in fields.into_iter().flatten() {
        let name = field["name"].as_str().ok_or(Error::Definition)?;
        match values.get(name) {
            Some(value) => validate(domain, field, value, depth + 1)?,
            None if field["optional"] == true => {}
            None => return Err(Error::Shape),
        }
    }
    Ok(())
}

fn validate(scope: &str, field: &Value, value: &Value, depth: usize) -> Result<(), Error> {
    if depth > MAX_DEPTH {
        return Err(Error::Bounds);
    }
    if let Some(reference) = field["$ref"].as_str() {
        let (scope, name) = reference.split_once('.').unwrap_or((scope, reference));
        let definition = domain(scope)
            .and_then(|domain| domain["types"].as_array())
            .and_then(|types| types.iter().find(|value| value["id"] == name))
            .ok_or(Error::Definition)?;
        return validate(scope, definition, value, depth + 1);
    }
    let valid = match field["type"].as_str().ok_or(Error::Definition)? {
        "any" => true,
        "string" | "binary" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value
            .as_i64()
            .is_some_and(|value| (-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&value)),
        "number" => value.as_f64().is_some_and(f64::is_finite),
        "object" => {
            if field.get("properties").is_some() {
                object(scope, field["properties"].as_array(), value, depth + 1)?;
                true
            } else {
                value.is_object()
            }
        }
        "array" => {
            let values = value.as_array().ok_or(Error::Shape)?;
            let item = field.get("items").ok_or(Error::Definition)?;
            for value in values {
                validate(scope, item, value, depth + 1)?;
            }
            true
        }
        _ => return Err(Error::Definition),
    };
    if !valid
        || field["enum"]
            .as_array()
            .is_some_and(|values| !values.contains(value))
    {
        return Err(Error::Shape);
    }
    Ok(())
}
