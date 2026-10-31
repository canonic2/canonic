//! A small, bounded form schema profile; no general JSON Schema evaluation.
use super::{formats, invalid, key};
use crate::{RpcError, modern, rpc};
use serde_json::{Value, json};

/// An immutable flat form with supported, enforced constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    schema: Value,
    choices: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
}

impl Form {
    /// Validates a flat schema and every default before publication.
    ///
    /// # Errors
    /// Refuses unsupported keywords, bad constraints/defaults or schema/choice budgets.
    pub fn new(schema: Value) -> Result<Self, RpcError> {
        modern::bounded(&schema, 65536, 16)?;
        work_budget(&schema)?;
        if !rpc::only(&schema, &["type", "properties", "required"]) || schema["type"] != "object" {
            return Err(invalid("form root"));
        }
        let properties = schema["properties"]
            .as_object()
            .filter(|map| map.len() <= 64)
            .ok_or_else(|| invalid("form fields"))?;
        let mut choices = 0;
        let mut compiled = std::collections::BTreeMap::new();
        for (name, field) in properties {
            key(name)?;
            check_field(field)?;
            let selected = options(field)?;
            choices += selected.len();
            compiled.insert(
                name.clone(),
                selected.into_iter().map(str::to_owned).collect(),
            );
            if choices > 512 {
                return Err(invalid("aggregate choices"));
            }
            if let Some(default) = field.get("default") {
                validate_field(field, default)?;
            }
        }
        if let Some(required) = schema.get("required") {
            let required = required
                .as_array()
                .ok_or_else(|| invalid("required fields"))?;
            let mut names = std::collections::HashSet::new();
            for name in required {
                let name = name
                    .as_str()
                    .filter(|name| properties.contains_key(*name))
                    .ok_or_else(|| invalid("required field membership"))?;
                if !names.insert(name) {
                    return Err(invalid("duplicate required field"));
                }
            }
        }
        Ok(Self {
            schema,
            choices: compiled,
        })
    }

    /// The validated schema, without copying it.
    #[must_use]
    pub fn as_value(&self) -> &Value {
        &self.schema
    }

    pub(super) fn validate(&self, value: Option<&Value>) -> Result<Value, RpcError> {
        let empty = json!({});
        let properties = self.schema["properties"]
            .as_object()
            .ok_or_else(|| invalid("form fields"))?;
        let value = value
            .or_else(|| properties.is_empty().then_some(&empty))
            .ok_or_else(|| invalid("missing content"))?;
        modern::bounded(value, super::MAX_RESPONSE_BYTES, 32)?;
        work_budget(value)?;
        let values = value.as_object().ok_or_else(|| invalid("content object"))?;
        for name in self.schema["required"].as_array().into_iter().flatten() {
            if !values.contains_key(name.as_str().ok_or_else(|| invalid("required field"))?) {
                return Err(invalid("missing required field"));
            }
        }
        for (name, value) in values {
            let field = properties
                .get(name)
                .ok_or_else(|| invalid("undeclared field"))?;
            let choices = self
                .choices
                .get(name)
                .ok_or_else(|| invalid("undeclared field"))?;
            validate_prepared(field, value, choices)
                .map_err(|_| invalid(&format!("field {name} does not match its definition")))?;
        }
        Ok(value.clone())
    }

    pub(super) fn legacy(&self) -> Result<Value, RpcError> {
        let mut schema = self.schema.clone();
        let fields = schema["properties"]
            .as_object_mut()
            .ok_or_else(|| invalid("form fields"))?;
        for field in fields.values_mut() {
            if field["type"] == "array"
                || (field.get("default").is_some() && field["type"] != "boolean")
            {
                return Err(invalid("field unavailable in this revision"));
            }
            if let Some(alternatives) = field.get("oneOf").and_then(Value::as_array) {
                let values: Vec<_> = alternatives
                    .iter()
                    .map(|entry| entry["const"].clone())
                    .collect();
                let names: Vec<_> = alternatives
                    .iter()
                    .map(|entry| entry["title"].clone())
                    .collect();
                field["enum"] = json!(values);
                field["enumNames"] = json!(names);
                field
                    .as_object_mut()
                    .ok_or_else(|| invalid("field"))?
                    .remove("oneOf");
            }
        }
        Ok(schema)
    }
}

fn check_field(field: &Value) -> Result<(), RpcError> {
    let common = ["type", "title", "description", "default"];
    let extra: &[&str] = match field["type"].as_str() {
        Some("string") => &[
            "minLength",
            "maxLength",
            "format",
            "enum",
            "enumNames",
            "oneOf",
        ],
        Some("number" | "integer") => &["minimum", "maximum"],
        Some("boolean") => &[],
        Some("array") => &["items", "minItems", "maxItems"],
        _ => return Err(invalid("field type")),
    };
    if !rpc::only(
        field,
        &common
            .into_iter()
            .chain(extra.iter().copied())
            .collect::<Vec<_>>(),
    ) {
        return Err(invalid("field keyword"));
    }
    for key in ["title", "description"] {
        if field
            .get(key)
            .is_some_and(|v| v.as_str().is_none_or(|s| s.len() > 4096))
        {
            return Err(invalid("field metadata"));
        }
    }
    for (low, high) in [("minLength", "maxLength"), ("minItems", "maxItems")] {
        for key in [low, high] {
            if field
                .get(key)
                .is_some_and(|v| v.as_u64().is_none_or(|v| v > 9_007_199_254_740_991))
            {
                return Err(invalid("count constraint"));
            }
        }
        if let (Some(low), Some(high)) = (field[low].as_u64(), field[high].as_u64())
            && low > high
        {
            return Err(invalid("inverted bounds"));
        }
    }
    for key in ["minimum", "maximum"] {
        if let Some(v) = field.get(key) {
            number(v, field["type"] == "integer")?;
        }
    }
    if let (Some(low), Some(high)) = (field["minimum"].as_f64(), field["maximum"].as_f64())
        && low > high
    {
        return Err(invalid("inverted bounds"));
    }
    if let Some(format) = field.get("format")
        && !matches!(
            format.as_str(),
            Some("email" | "uri" | "date" | "date-time")
        )
    {
        return Err(invalid("string format"));
    }
    let choices = options(field)?;
    if field["type"] == "array" && choices.is_empty() {
        return Err(invalid("array requires choices"));
    }
    if field["type"] == "array"
        && field["minItems"]
            .as_u64()
            .is_some_and(|n| n > choices.len() as u64)
    {
        return Err(invalid("impossible selection count"));
    }
    Ok(())
}

fn options(field: &Value) -> Result<Vec<&str>, RpcError> {
    let array = field["type"] == "array";
    let source = if array { &field["items"] } else { field };
    if array
        && (!rpc::only(source, &["type", "enum", "anyOf"])
            || source.get("type").is_some_and(|v| v != "string"))
    {
        return Err(invalid("choice items"));
    }
    let alternatives = if array { "anyOf" } else { "oneOf" };
    let values = if let Some(values) = source.get("enum") {
        if source.get(alternatives).is_some() {
            return Err(invalid("ambiguous choices"));
        }
        if array && source["type"] != "string" {
            return Err(invalid("choice item type"));
        }
        values
            .as_array()
            .ok_or_else(|| invalid("choices"))?
            .iter()
            .map(|v| v.as_str().ok_or_else(|| invalid("choice value")))
            .collect::<Result<Vec<_>, _>>()?
    } else if let Some(values) = source.get(alternatives) {
        if source.get("enumNames").is_some() {
            return Err(invalid("ambiguous choice labels"));
        }
        values
            .as_array()
            .ok_or_else(|| invalid("choices"))?
            .iter()
            .map(|v| {
                if !rpc::only(v, &["const", "title"])
                    || v["title"].as_str().is_none_or(|s| s.len() > 4096)
                {
                    return Err(invalid("choice label"));
                }
                v["const"].as_str().ok_or_else(|| invalid("choice value"))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        if source.get("enumNames").is_some() {
            return Err(invalid("labels require choices"));
        }
        return Ok(Vec::new());
    };
    if values.is_empty() || values.len() > 128 {
        return Err(invalid("choice count"));
    }
    let mut seen = std::collections::HashSet::new();
    for value in &values {
        if value.is_empty() || value.len() > 4096 || !seen.insert(*value) {
            return Err(invalid("choice uniqueness or size"));
        }
    }
    if let Some(labels) = source.get("enumNames") {
        let labels = labels
            .as_array()
            .filter(|v| v.len() == values.len())
            .ok_or_else(|| invalid("choice labels"))?;
        if labels
            .iter()
            .any(|v| v.as_str().is_none_or(|s| s.len() > 4096))
        {
            return Err(invalid("choice label"));
        }
    }
    Ok(values)
}

fn number(value: &Value, integer: bool) -> Result<f64, RpcError> {
    let value = value
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid("number"))?;
    if integer && (value.fract() != 0.0 || value.abs() > 9_007_199_254_740_991.0) {
        return Err(invalid("safe integer"));
    }
    Ok(value)
}

fn count(field: &Value, length: usize, low: &str, high: &str) -> Result<(), RpcError> {
    if field[low].as_u64().is_some_and(|n| (length as u64) < n)
        || field[high].as_u64().is_some_and(|n| (length as u64) > n)
    {
        return Err(invalid("length constraint"));
    }
    Ok(())
}

fn validate_field(field: &Value, value: &Value) -> Result<(), RpcError> {
    let choices = options(field)?.into_iter().map(str::to_owned).collect();
    validate_prepared(field, value, &choices)
}

fn work_budget(value: &Value) -> Result<(), RpcError> {
    fn visit(value: &Value, remaining: &mut usize) -> Result<(), RpcError> {
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| invalid("validation work budget"))?;
        match value {
            Value::Array(values) => {
                for value in values {
                    visit(value, remaining)?;
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    visit(value, remaining)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    visit(value, &mut 16_384)
}

fn validate_prepared(
    field: &Value,
    value: &Value,
    choices: &std::collections::BTreeSet<String>,
) -> Result<(), RpcError> {
    match field["type"].as_str() {
        Some("string") => {
            let value = value.as_str().ok_or_else(|| invalid("string"))?;
            count(field, value.chars().count(), "minLength", "maxLength")?;
            if !choices.is_empty() && !choices.contains(value) {
                return Err(invalid("choice membership"));
            }
            if let Some(format) = field["format"].as_str() {
                formats::validate(format, value)?;
            }
        }
        Some("number" | "integer") => {
            let number = number(value, field["type"] == "integer")?;
            if field["minimum"].as_f64().is_some_and(|n| number < n)
                || field["maximum"].as_f64().is_some_and(|n| number > n)
            {
                return Err(invalid("numeric constraint"));
            }
        }
        Some("boolean") if value.is_boolean() => {}
        Some("array") => {
            let values = value.as_array().ok_or_else(|| invalid("selection array"))?;
            count(field, values.len(), "minItems", "maxItems")?;
            let mut seen = std::collections::HashSet::new();
            for value in values {
                let value = value
                    .as_str()
                    .filter(|v| choices.contains(*v))
                    .ok_or_else(|| invalid("choice membership"))?;
                if !seen.insert(value) {
                    return Err(invalid("duplicate selection"));
                }
            }
        }
        _ => return Err(invalid("field type")),
    }
    Ok(())
}
