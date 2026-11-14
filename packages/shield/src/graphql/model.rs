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
/// Nesting bound for mutation input object literals and their matchers.
pub(super) const MAX_OBJECT_DEPTH: usize = 4;

pub(crate) fn validate(value: &Value) -> Result<(), Error> {
    text::fields(value, &["carrier", "operation", "selection", "root_fields"])?;
    let mutation = match (&value["carrier"], value["operation"].as_str()) {
        (Value::String(c), Some("query")) if c == "gh-api" => false,
        (Value::String(c), Some("mutation")) if c == "gh-api" => true,
        _ => {
            return Err(Error::Contract(
                "GraphQL requires carrier gh-api and operation query or mutation",
            ));
        }
    };
    if !mutation {
        if value.get("root_fields").is_some() {
            return Err(Error::Contract("GraphQL root_fields requires a mutation"));
        }
        return value.get("selection").map_or(Ok(()), |model| {
            if is_mutation_model(model) {
                return Err(Error::Contract("GraphQL query model is rooted at Mutation"));
            }
            validate_model(model)
        });
    }
    let fields = value["root_fields"]
        .as_array()
        .ok_or(Error::Contract("GraphQL mutation requires root_fields"))?;
    if fields.is_empty() || fields.len() > 256 {
        return Err(Error::Limit);
    }
    let mut seen = BTreeSet::new();
    for field in fields {
        if !field.as_str().is_some_and(|f| name(f) && seen.insert(f)) {
            return Err(Error::Contract("invalid GraphQL root_fields"));
        }
    }
    let model = value.get("selection").ok_or(Error::Contract(
        "GraphQL mutation requires a selection model",
    ))?;
    if !is_mutation_model(model) {
        return Err(Error::Contract(
            "GraphQL mutation model root types must be exactly [Mutation]",
        ));
    }
    validate_model(model)
}

/// A model whose root state admits exactly the `Mutation` type; only these may
/// declare object matchers.
fn is_mutation_model(model: &Value) -> bool {
    model["root"]
        .as_str()
        .is_some_and(|root| model["states"][root]["types"] == serde_json::json!(["Mutation"]))
}

/// Admit a prepared model image alone; its predicate was admitted at compile time.
pub(crate) fn validate_model(model: &Value) -> Result<(), Error> {
    let objects = is_mutation_model(model);
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
                let mut count = 0;
                arguments_map(arguments, objects, 0, &mut count)?;
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

/// Admit one field's argument map, or one object matcher's input fields at
/// `depth`. Input fields count toward the field's 64-argument bound.
fn arguments_map(
    arguments: &Value,
    objects: bool,
    depth: usize,
    count: &mut usize,
) -> Result<(), Error> {
    for (name, argument) in map(arguments, 64, depth == 0)? {
        *count += 1;
        if *count > 64 {
            return Err(Error::Limit);
        }
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
        text::fields(matcher, &["text", "exact", "scalar", "object"])?;
        if map(matcher, 1, false)?.len() != 1 {
            return Err(Error::Syntax);
        }
        if let Some(selector) = matcher.get("text") {
            text::text(selector, false)?;
        } else if let Some(value) = matcher.get("exact") {
            if !matches!(value, Value::String(_) | Value::Bool(_) | Value::Number(_)) {
                return Err(Error::Syntax);
            }
        } else if let Some(fields) = matcher.get("object") {
            if !objects {
                return Err(Error::Contract(
                    "GraphQL object matchers require a mutation model",
                ));
            }
            if depth + 1 > MAX_OBJECT_DEPTH {
                return Err(Error::Limit);
            }
            if argument["cursor"] == true {
                return Err(Error::Contract("GraphQL cursor must be a scalar string"));
            }
            arguments_map(fields, objects, depth + 1, count)?;
        } else if !matches!(
            matcher["scalar"].as_str(),
            Some("string" | "integer" | "number" | "boolean" | "enum")
        ) {
            return Err(Error::Syntax);
        }
        if argument["cursor"] == true
            && (depth > 0
                || argument["nullable"] != true
                || argument["required"] == true
                || *matcher != serde_json::json!({"scalar":"string"}))
        {
            return Err(Error::Contract(
                "GraphQL cursor must be an optional nullable scalar string field argument",
            ));
        }
    }
    Ok(())
}

fn argument_text<'a>(arguments: &'a Value, out: &mut Vec<&'a Value>) {
    for argument in arguments
        .as_object()
        .into_iter()
        .flat_map(serde_json::Map::values)
    {
        let matcher = &argument["match"];
        if let Some(text) = matcher.get("text") {
            out.push(text);
        } else if let Some(fields) = matcher.get("object") {
            argument_text(fields, out);
        }
    }
}
fn argument_text_mut<'a>(arguments: &'a mut Value, out: &mut Vec<&'a mut Value>) {
    for argument in arguments
        .as_object_mut()
        .into_iter()
        .flat_map(serde_json::Map::values_mut)
    {
        let Some(matcher) = argument.get_mut("match").and_then(Value::as_object_mut) else {
            continue;
        };
        if matcher.contains_key("text") {
            out.extend(matcher.get_mut("text"));
        } else if let Some(fields) = matcher.get_mut("object") {
            argument_text_mut(fields, out);
        }
    }
}

/// Every text selector in a predicate's model, including object matcher fields.
pub(crate) fn selectors(value: &Value) -> Vec<&Value> {
    let mut out = vec![];
    for field in value["selection"]["states"]
        .as_object()
        .into_iter()
        .flat_map(serde_json::Map::values)
        .flat_map(|s| {
            s["fields"]
                .as_object()
                .into_iter()
                .flat_map(serde_json::Map::values)
        })
    {
        argument_text(&field["arguments"], &mut out);
    }
    out
}
pub(crate) fn selectors_mut(value: &mut Value) -> Vec<&mut Value> {
    let mut out = vec![];
    for field in value
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
    {
        if let Some(arguments) = field.get_mut("arguments") {
            argument_text_mut(arguments, &mut out);
        }
    }
    out
}
