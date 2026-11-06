//! Admission and selector positions of finite, data-only selection models.
use crate::policy::{Error, selectors as text};
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn name(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn map(value: &Value, max: usize, empty: bool) -> Result<&serde_json::Map<String, Value>, Error> {
    let map = value.as_object().ok_or(Error::Syntax)?;
    if map.len() > max || !empty && map.is_empty() {
        return Err(Error::Limit);
    }
    Ok(map)
}
#[expect(
    clippy::too_many_lines,
    reason = "one admission pass checks bounded model declarations and graph reachability together"
)]
pub(crate) fn validate(value: &Value) -> Result<(), Error> {
    text::fields(value, &["carrier", "operation", "selection"])?;
    if value["carrier"] != "gh-api" || value["operation"] != "query" {
        return Err(Error::Contract(
            "GraphQL requires carrier gh-api and operation query",
        ));
    }
    let Some(model) = value.get("selection") else {
        return Ok(());
    };
    text::fields(model, &["root", "states"])?;
    let states = map(&model["states"], 256, false)?;
    let root = model["root"].as_str().ok_or(Error::Syntax)?;
    if !states.contains_key(root) {
        return Err(Error::Contract("unknown GraphQL root state"));
    }
    let mut total = 0;
    for (name, state) in states {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        {
            return Err(Error::Contract("invalid GraphQL state name"));
        }
        text::fields(state, &["types", "fields"])?;
        let types = state["types"].as_array().ok_or(Error::Syntax)?;
        let mut seen = BTreeSet::new();
        if types.is_empty() || types.len() > 64 {
            return Err(Error::Limit);
        }
        for ty in types {
            let ty = ty.as_str().ok_or(Error::Syntax)?;
            if !self::name(ty) || !seen.insert(ty) {
                return Err(Error::Contract("invalid GraphQL type conditions"));
            }
        }
        for (name, field) in map(&state["fields"], 256, false)? {
            total += 1;
            if total > 4096 {
                return Err(Error::Limit);
            }
            if !self::name(name) {
                return Err(Error::Contract("invalid GraphQL field name"));
            }
            text::fields(field, &["arguments", "select", "leaf"])?;
            match (field.get("select"), field.get("leaf")) {
                (Some(next), None) if next.as_str().is_some_and(|n| states.contains_key(n)) => {}
                (None, Some(Value::Bool(true))) => {}
                _ => {
                    return Err(Error::Contract(
                        "GraphQL field needs a valid select or leaf true",
                    ));
                }
            }
            if let Some(arguments) = field.get("arguments") {
                for (name, argument) in map(arguments, 64, true)? {
                    if !self::name(name) {
                        return Err(Error::Contract("invalid GraphQL argument name"));
                    }
                    text::fields(argument, &["match", "required", "nullable", "cursor"])?;
                    for key in ["required", "nullable", "cursor"] {
                        if argument.get(key).is_some_and(|v| !v.is_boolean()) {
                            return Err(Error::Syntax);
                        }
                    }
                    let matcher = &argument["match"];
                    text::fields(matcher, &["text", "exact", "scalar"])?;
                    if map(matcher, 1, false)?.len() != 1 {
                        return Err(Error::Syntax);
                    }
                    if let Some(selector) = matcher.get("text") {
                        text::text(selector, false)?;
                    } else if let Some(value) = matcher.get("exact") {
                        if !matches!(value, Value::String(_) | Value::Bool(_) | Value::Number(_)) {
                            return Err(Error::Syntax);
                        }
                    } else if !matches!(
                        matcher["scalar"].as_str(),
                        Some("string" | "integer" | "number" | "boolean" | "enum")
                    ) {
                        return Err(Error::Syntax);
                    }
                    if argument["cursor"] == true
                        && (argument["nullable"] != true
                            || argument["required"] == true
                            || *matcher != serde_json::json!({"scalar":"string"}))
                    {
                        return Err(Error::Contract(
                            "GraphQL cursor must be optional nullable scalar string",
                        ));
                    }
                }
            }
        }
    }
    let mut reached = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(state) = pending.pop() {
        if !reached.insert(state) {
            continue;
        }
        for field in states[state]["fields"]
            .as_object()
            .ok_or(Error::Syntax)?
            .values()
        {
            if let Some(next) = field["select"].as_str() {
                pending.push(next);
            }
        }
    }
    if reached.len() != states.len() {
        return Err(Error::Contract("unused GraphQL model state"));
    }
    Ok(())
}

pub(crate) fn selectors(value: &Value) -> impl Iterator<Item = &Value> {
    value
        .get("selection")
        .and_then(|m| m.get("states"))
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(serde_json::Map::values)
        .flat_map(|s| {
            s.get("fields")
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(serde_json::Map::values)
        })
        .flat_map(|f| {
            f.get("arguments")
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(serde_json::Map::values)
        })
        .filter_map(|a| a.get("match").and_then(|m| m.get("text")))
}
pub(crate) fn selectors_mut(value: &mut Value) -> impl Iterator<Item = &mut Value> {
    value
        .get_mut("selection")
        .and_then(|m| m.get_mut("states"))
        .and_then(Value::as_object_mut)
        .into_iter()
        .flat_map(serde_json::Map::values_mut)
        .flat_map(|s| {
            s.get_mut("fields")
                .and_then(Value::as_object_mut)
                .into_iter()
                .flat_map(serde_json::Map::values_mut)
        })
        .flat_map(|f| {
            f.get_mut("arguments")
                .and_then(Value::as_object_mut)
                .into_iter()
                .flat_map(serde_json::Map::values_mut)
        })
        .filter_map(|a| a.get_mut("match").and_then(|m| m.get_mut("text")))
}
