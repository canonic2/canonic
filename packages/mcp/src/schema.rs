//! Validates tool arguments against the supported JSON Schema subset.
//!
//! [`check`] refuses a schema that uses a keyword this validator does not
//! enforce, so a catalog cannot publish a constraint that is silently ignored.
//! A `$ref` may have annotation siblings, but no assertion siblings.
//! Annotation keywords such as `description` and `default` are accepted and
//! not enforced; `format` is enforced only for `uuid`.

use serde_json::{Map, Value};

/// The deepest schema nesting, including `$ref` hops, that validation follows.
pub const MAX_DEPTH: usize = 64;

const ASSERTIONS: &[&str] = &[
    "$ref",
    "type",
    "enum",
    "const",
    "properties",
    "required",
    "additionalProperties",
    "minProperties",
    "maxProperties",
    "items",
    "minItems",
    "maxItems",
    "minLength",
    "maxLength",
    "format",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "oneOf",
    "anyOf",
    "allOf",
    "not",
    "if",
    "then",
    "else",
];

const ANNOTATIONS: &[&str] = &[
    "$schema",
    "$id",
    "$comment",
    "$defs",
    "definitions",
    "title",
    "description",
    "default",
    "examples",
    "deprecated",
    "readOnly",
    "writeOnly",
];

const TYPES: &[&str] = &[
    "object", "array", "string", "boolean", "integer", "number", "null",
];

/// Arguments that do not match their schema.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ArgumentError {
    /// Where the mismatch is, such as `arguments.point.x`.
    pub path: String,
    /// A sentence naming the path and the expected shape.
    pub message: String,
}

impl ArgumentError {
    fn new(path: &str, message: String) -> Self {
        Self {
            path: path.to_owned(),
            message,
        }
    }
}

/// A schema keyword or value this validator cannot enforce.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("schema at {pointer} {problem}")]
pub struct SchemaError {
    /// JSON pointer of the offending schema within the checked schema.
    pub pointer: String,
    /// What is unsupported.
    pub problem: String,
}

/// Validates `value` against `schema`, naming paths from `arguments`.
///
/// # Errors
///
/// Returns the first mismatch found.
pub fn validate(schema: &Value, value: &Value) -> Result<(), ArgumentError> {
    validate_at(schema, value, "arguments")
}

/// Validates `value` against `schema`, naming paths from `path`.
///
/// # Errors
///
/// Returns the first mismatch found.
pub fn validate_at(schema: &Value, value: &Value, path: &str) -> Result<(), ArgumentError> {
    validate_with(schema, schema, value, path, 0)
}

/// Checks that `schema` uses only supported keywords and well-formed values.
///
/// # Errors
///
/// Returns the first unsupported keyword, malformed keyword value or
/// unresolvable local `$ref`, or assertion beside `$ref`.
pub fn check(schema: &Value) -> Result<(), SchemaError> {
    check_at(schema, schema, "", 0)
}

fn check_at(root: &Value, schema: &Value, pointer: &str, depth: usize) -> Result<(), SchemaError> {
    let error = |problem: String| SchemaError {
        pointer: if pointer.is_empty() {
            "/".into()
        } else {
            pointer.into()
        },
        problem,
    };
    if depth > MAX_DEPTH {
        return Err(error(format!("nests deeper than {MAX_DEPTH} levels")));
    }
    if schema.is_boolean() {
        return Ok(());
    }
    let Some(object) = schema.as_object() else {
        return Err(error("is not an object".into()));
    };
    for (keyword, value) in object {
        if object.contains_key("$ref")
            && keyword != "$ref"
            && ASSERTIONS.contains(&keyword.as_str())
        {
            return Err(error(format!("has assertion {keyword} beside $ref")));
        }
        let child = |name: &str| format!("{pointer}/{keyword}{name}");
        match keyword.as_str() {
            "$ref" => {
                let target = value
                    .as_str()
                    .and_then(|reference| reference.strip_prefix('#'))
                    .and_then(|path| root.pointer(path));
                if target.is_none() {
                    return Err(error(format!("has an unresolvable $ref {value}")));
                }
            }
            "type" => {
                let valid = |kind: &Value| kind.as_str().is_some_and(|kind| TYPES.contains(&kind));
                if !(valid(value)
                    || value
                        .as_array()
                        .is_some_and(|kinds| kinds.iter().all(valid)))
                {
                    return Err(error(format!("has an unsupported type {value}")));
                }
            }
            "enum" | "required" if !value.is_array() => {
                return Err(error(format!("has a non-array {keyword}")));
            }
            "properties" | "$defs" | "definitions" => {
                let Some(children) = value.as_object() else {
                    return Err(error(format!("has a non-object {keyword}")));
                };
                for (name, schema) in children {
                    check_at(root, schema, &child(&format!("/{name}")), depth + 1)?;
                }
            }
            "additionalProperties" | "items" | "not" | "if" | "then" | "else" => {
                check_at(root, value, &child(""), depth + 1)?;
            }
            "oneOf" | "anyOf" | "allOf" => {
                let Some(branches) = value.as_array() else {
                    return Err(error(format!("has a non-array {keyword}")));
                };
                for (index, branch) in branches.iter().enumerate() {
                    check_at(root, branch, &child(&format!("/{index}")), depth + 1)?;
                }
            }
            "minProperties" | "maxProperties" | "minItems" | "maxItems" | "minLength"
            | "maxLength"
                if !value.is_u64() =>
            {
                return Err(error(format!("has a non-integer {keyword}")));
            }
            "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum"
                if !value.is_number() =>
            {
                return Err(error(format!("has a non-numeric {keyword}")));
            }
            other if ASSERTIONS.contains(&other) || ANNOTATIONS.contains(&other) => {}
            other => return Err(error(format!("uses unsupported keyword {other}"))),
        }
    }
    Ok(())
}

fn validate_with(
    root: &Value,
    schema: &Value,
    value: &Value,
    path: &str,
    depth: usize,
) -> Result<(), ArgumentError> {
    let mismatch = || ArgumentError::new(path, format!("{path} does not match its tool schema"));
    if depth > MAX_DEPTH {
        return Err(mismatch());
    }
    match schema {
        Value::Bool(true) => return Ok(()),
        Value::Bool(false) => return Err(mismatch()),
        _ => {}
    }
    if let Some(reference) = schema["$ref"].as_str() {
        let target = reference
            .strip_prefix('#')
            .and_then(|pointer| root.pointer(pointer))
            .ok_or_else(mismatch)?;
        return validate_with(root, target, value, path, depth + 1);
    }
    let matches = |branch: &Value| validate_with(root, branch, value, path, depth + 1).is_ok();
    if let Some(branches) = schema["oneOf"].as_array() {
        validate_union(root, branches, value, path, depth, true)?;
    }
    if let Some(branches) = schema["anyOf"].as_array() {
        validate_union(root, branches, value, path, depth, false)?;
    }
    if let Some(branches) = schema["allOf"].as_array() {
        for branch in branches {
            validate_with(root, branch, value, path, depth + 1)?;
        }
    }
    if let Some(negated) = schema.get("not")
        && matches(negated)
    {
        return Err(mismatch());
    }
    if let Some(condition) = schema.get("if") {
        let branch = if matches(condition) {
            schema.get("then")
        } else {
            schema.get("else")
        };
        if let Some(branch) = branch {
            validate_with(root, branch, value, path, depth + 1)?;
        }
    }
    if let Some(kinds) = schema["type"].as_array()
        && !kinds
            .iter()
            .any(|kind| kind.as_str().is_some_and(|kind| is_type(kind, value)))
    {
        return Err(mismatch());
    }
    if schema
        .get("const")
        .is_some_and(|expected| expected != value)
        || schema
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|choices| !choices.contains(value))
    {
        let choices = schema
            .get("enum")
            .or_else(|| schema.get("const"))
            .unwrap_or(&Value::Null);
        return Err(ArgumentError::new(
            path,
            format!("{path} must match one of {choices}"),
        ));
    }
    if let Some(kind) = schema["type"].as_str()
        && !is_type(kind, value)
    {
        return Err(ArgumentError::new(path, format!("{path} must be {kind}")));
    }
    if let Some(object) = value.as_object() {
        validate_object(root, schema, object, path, depth)?;
    }
    if let Some(array) = value.as_array() {
        validate_array(root, schema, array, path, depth)?;
    }
    validate_scalar(schema, value, path)
}

/// Keep a unique deeper failure when no branch matches. Ambiguous alternatives
/// and overlapping oneOf branches still report the union itself as invalid.
fn validate_union(
    root: &Value,
    branches: &[Value],
    value: &Value,
    path: &str,
    depth: usize,
    exactly_one: bool,
) -> Result<(), ArgumentError> {
    let path_depth = |path: &str| path.bytes().filter(|b| matches!(b, b'.' | b'[')).count();
    let mut matched = 0;
    let mut deepest = path_depth(path);
    let mut failure = None;
    let mut ambiguous = false;
    for branch in branches {
        match validate_with(root, branch, value, path, depth + 1) {
            Ok(()) => {
                matched += 1;
                if !exactly_one {
                    return Ok(());
                }
            }
            Err(error) => {
                let level = path_depth(&error.path);
                if level > deepest || failure.is_none() {
                    deepest = level;
                    failure = Some(error);
                    ambiguous = false;
                } else if level == deepest && failure.as_ref().is_some_and(|known| *known != error)
                {
                    ambiguous = true;
                }
            }
        }
    }
    if matched == 1 {
        return Ok(());
    }
    if matched == 0
        && !ambiguous
        && let Some(error) = failure
    {
        return Err(error);
    }
    Err(ArgumentError::new(
        path,
        format!("{path} does not match its tool schema"),
    ))
}

fn is_type(kind: &str, value: &Value) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "null" => value.is_null(),
        _ => false,
    }
}

fn validate_object(
    root: &Value,
    schema: &Value,
    object: &Map<String, Value>,
    path: &str,
    depth: usize,
) -> Result<(), ArgumentError> {
    if let Some(fields) = schema["required"].as_array() {
        for field in fields.iter().filter_map(Value::as_str) {
            if !object.contains_key(field) {
                return Err(ArgumentError::new(
                    path,
                    format!("{path}.{field} is required"),
                ));
            }
        }
    }
    let properties = schema["properties"].as_object();
    for (key, value) in object {
        let child = format!("{path}.{key}");
        if let Some(property) = properties.and_then(|properties| properties.get(key)) {
            validate_with(root, property, value, &child, depth + 1)?;
        } else {
            match schema.get("additionalProperties") {
                Some(Value::Bool(false)) => {
                    return Err(ArgumentError::new(
                        &child,
                        format!("{child} is not a supported field"),
                    ));
                }
                Some(additional @ Value::Object(_)) => {
                    validate_with(root, additional, value, &child, depth + 1)?;
                }
                _ => {}
            }
        }
    }
    for (bound, comparison, at_least) in [
        ("minProperties", "at least", true),
        ("maxProperties", "at most", false),
    ] {
        if let Some(size) = schema[bound].as_u64()
            && out_of_bounds(object.len(), size, at_least)
        {
            return Err(ArgumentError::new(
                path,
                format!("{path} must contain {comparison} {size} fields"),
            ));
        }
    }
    Ok(())
}

fn validate_array(
    root: &Value,
    schema: &Value,
    array: &[Value],
    path: &str,
    depth: usize,
) -> Result<(), ArgumentError> {
    for (bound, comparison, at_least) in [
        ("minItems", "at least", true),
        ("maxItems", "at most", false),
    ] {
        if let Some(size) = schema[bound].as_u64()
            && out_of_bounds(array.len(), size, at_least)
        {
            return Err(ArgumentError::new(
                path,
                format!(
                    "{path} must contain {comparison} {size} items; received {}",
                    array.len()
                ),
            ));
        }
    }
    if let Some(item) = schema.get("items") {
        for (index, value) in array.iter().enumerate() {
            validate_with(root, item, value, &format!("{path}[{index}]"), depth + 1)?;
        }
    }
    Ok(())
}

fn out_of_bounds(length: usize, bound: u64, at_least: bool) -> bool {
    let length = u64::try_from(length).unwrap_or(u64::MAX);
    if at_least {
        length < bound
    } else {
        length > bound
    }
}

fn validate_scalar(schema: &Value, value: &Value, path: &str) -> Result<(), ArgumentError> {
    let mismatch = || ArgumentError::new(path, format!("{path} does not match its tool schema"));
    if let Some(text) = value.as_str() {
        if schema["format"] == "uuid" && !is_uuid(text) {
            return Err(mismatch());
        }
        let size = u64::try_from(text.chars().count()).unwrap_or(u64::MAX);
        if schema["minLength"].as_u64().is_some_and(|min| size < min)
            || schema["maxLength"].as_u64().is_some_and(|max| size > max)
        {
            return Err(mismatch());
        }
    }
    if let Some(number) = value.as_f64()
        && (!number.is_finite()
            || schema["minimum"].as_f64().is_some_and(|min| number < min)
            || schema["maximum"].as_f64().is_some_and(|max| number > max)
            || schema["exclusiveMinimum"]
                .as_f64()
                .is_some_and(|min| number <= min)
            || schema["exclusiveMaximum"]
                .as_f64()
                .is_some_and(|max| number >= max))
    {
        if let (Some(minimum), Some(maximum)) = (schema.get("minimum"), schema.get("maximum")) {
            return Err(ArgumentError::new(
                path,
                format!("{path} must be between {minimum} and {maximum}; received {value}"),
            ));
        }
        return Err(mismatch());
    }
    Ok(())
}

/// Whether `text` is a hyphenated UUID in 8-4-4-4-12 hexadecimal groups.
fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(group, length)| {
            group.len() == length && group.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(schema: &Value, value: &Value) -> String {
        validate(schema, value).unwrap_err().message
    }

    #[test]
    fn nested_objects_report_the_failing_path() {
        let point = json!({"type":"object","properties":{"x":{"type":"number","minimum":0,"maximum":1}},"required":["x"],"additionalProperties":false});
        let list = json!({"type":"array","items":point,"maxItems":2});
        assert!(validate(&list, &json!([{"x":0.5}])).is_ok());
        assert_eq!(message(&list, &json!([{}])), "arguments[0].x is required");
        assert_eq!(
            message(&list, &json!([{"x":"a"}])),
            "arguments[0].x must be number"
        );
        assert_eq!(
            message(&list, &json!([{"x":0,"y":1}])),
            "arguments[0].y is not a supported field"
        );
        assert_eq!(
            message(&list, &json!([{"x":2}])),
            "arguments[0].x must be between 0 and 1; received 2"
        );
        assert_eq!(
            message(&list, &json!([{"x":0},{"x":0},{"x":0}])),
            "arguments must contain at most 2 items; received 3"
        );
    }

    #[test]
    fn unions_keep_unique_nested_errors_and_refuse_ambiguous_matches() {
        for union in ["oneOf", "anyOf"] {
            let location = json!({"oneOf":[
                {"type":"object","properties":{"exact":{"oneOf":[
                    {"type":"object","properties":{"root":{"const":"project"}},"required":["root"]},
                    {"type":"object","properties":{"root":{"const":"absolute"},"path":{"type":"string"}},"required":["root","path"]}
                ]}},"required":["exact"]},
                {"type":"object","properties":{"within":{"type":"object"}},"required":["within"]}
            ]});
            let mut schema = json!({});
            schema[union] = json!([
                {"type":"object","properties":{"kind":{"const":"create"},"location":location},"required":["kind","location"]},
                {"type":"object","properties":{"kind":{"const":"remove"}},"required":["kind"]}
            ]);
            let bad = json!({"kind":"create","location":{"exact":"/acme/project"}});
            let error = validate(&schema, &bad).unwrap_err();
            assert_eq!(error.path, "arguments.location.exact");
            assert_eq!(error.message, "arguments.location.exact must be object");
            assert!(
                validate(
                    &schema,
                    &json!({"kind":"create","location":{"exact":{"root":"project"}}})
                )
                .is_ok()
            );
            assert!(validate(&schema, &json!({"kind":"other"})).is_err());
        }
        let ambiguous = json!({"oneOf":[
            {"type":"object","properties":{"left":{"type":"number"}}},
            {"type":"object","properties":{"right":{"type":"number"}}}
        ]});
        assert_eq!(
            validate(&ambiguous, &json!({"left":"bad","right":"bad"}))
                .unwrap_err()
                .path,
            "arguments"
        );
        assert!(
            validate(
                &json!({"oneOf":[{"type":"integer"},{"type":"number"}]}),
                &json!(1)
            )
            .is_err()
        );
    }

    #[test]
    fn unions_references_conditionals_and_formats_validate() {
        let schema = json!({
            "$defs": {"id": {"type": "string", "format": "uuid"}},
            "type": "object",
            "properties": {
                "domain": {"enum": ["sample", "literal"]},
                "input": {"type": ["string", "null"]},
                "handle": {"$ref": "#/$defs/id"},
                "labels": {"type": "object", "additionalProperties": {"type": "string"}, "maxProperties": 2}
            },
            "if": {"properties": {"domain": {"const": "sample"}}},
            "then": {"required": ["input"]},
            "else": {"not": {"required": ["input"]}}
        });
        assert!(validate(&schema, &json!({"domain":"sample","input":null})).is_ok());
        assert!(validate(&schema, &json!({"domain":"sample"})).is_err());
        assert!(validate(&schema, &json!({"domain":"literal","input":"a"})).is_err());
        assert!(validate(&schema, &json!({"input":5,"domain":"sample"})).is_err());
        assert!(
            validate(
                &schema,
                &json!({"domain":"literal","handle":"123e4567-e89b-12d3-a456-426614174000"})
            )
            .is_ok()
        );
        assert!(validate(&schema, &json!({"domain":"literal","handle":"123e4567"})).is_err());
        assert!(validate(&schema, &json!({"domain":"literal","labels":{"a":1}})).is_err());
        assert!(
            validate(
                &schema,
                &json!({"domain":"literal","labels":{"a":"1","b":"2","c":"3"}})
            )
            .is_err()
        );
        assert_eq!(
            message(&schema, &json!({"domain":"other"})),
            r#"arguments.domain must match one of ["sample","literal"]"#
        );
        let exactly_one = json!({"oneOf":[{"type":"integer"},{"type":"number"}]});
        assert!(validate(&exactly_one, &json!(1.5)).is_ok());
        assert!(validate(&exactly_one, &json!(1)).is_err());
    }

    #[test]
    fn check_refuses_assertions_beside_references_but_keeps_annotations() {
        let schema = json!({
            "type": "object",
            "$defs": {"id": {"type": "string"}},
            "properties": {
                "id": {
                    "$ref": "#/$defs/id",
                    "title": "Acme ID",
                    "description": "The Acme identifier.",
                    "default": "acme",
                    "examples": ["acme"]
                }
            }
        });
        assert!(check(&schema).is_ok());
        assert!(validate(&schema, &json!({"id": "acme"})).is_ok());
        assert!(validate(&schema, &json!({"id": 1})).is_err());
        for (keyword, value) in [
            ("maxLength", json!(8)),
            ("type", json!("string")),
            ("enum", json!(["acme"])),
            ("allOf", json!([{"minLength": 1}])),
        ] {
            let mut constrained = schema.clone();
            constrained["properties"]["id"][keyword] = value;
            let error = check(&constrained).unwrap_err();
            assert_eq!(error.pointer, "/properties/id");
            assert_eq!(
                error.problem,
                format!("has assertion {keyword} beside $ref")
            );
        }
        let mut composed = schema;
        composed["properties"]["id"] = json!({
            "allOf": [{"$ref": "#/$defs/id"}, {"maxLength": 8}]
        });
        assert!(check(&composed).is_ok());
        assert!(validate(&composed, &json!({"id": "acme"})).is_ok());
        assert!(validate(&composed, &json!({"id": "acme-long"})).is_err());
        assert!(validate(&composed, &json!({"id": 1})).is_err());
    }

    #[test]
    fn check_refuses_keywords_validation_would_ignore() {
        let supported = json!({"type":"object","properties":{"a":{"type":"string","description":"An Acme label.","default":"x"}},"additionalProperties":false});
        assert!(check(&supported).is_ok());
        let error =
            check(&json!({"type":"object","properties":{"a":{"type":"string","pattern":"^a"}}}))
                .unwrap_err();
        assert_eq!(error.pointer, "/properties/a");
        assert!(error.problem.contains("pattern"));
        assert!(check(&json!({"$ref":"#/$defs/missing"})).is_err());
        assert!(check(&json!({"type":"tuple"})).is_err());
        assert!(check(&json!({"maxItems":-1})).is_err());
    }
}
