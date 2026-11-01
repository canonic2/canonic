//! Pure schema/reference admission and conservative per-validation work accounting.

use super::{
    ArgumentErrorKind, DIALECT, MAX_DEPTH, MAX_SCHEMA_BYTES, MAX_SCHEMA_NODES, MAX_VALUE_BYTES,
    MAX_VALUE_NODES, SchemaError,
};
use crate::{control::Interrupted, json};
use referencing::{Draft, Registry, Resolver, ResourceRef};
use serde_json::Value;

pub(super) const BASE: &str = "https://schema.invalid/root";
const MAPS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
];
const LISTS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];
const SINGLES: &[&str] = &[
    "items",
    "contains",
    "additionalProperties",
    "unevaluatedProperties",
    "unevaluatedItems",
    "propertyNames",
    "not",
    "if",
    "then",
    "else",
    "contentSchema",
];
const KEYWORDS: &[&str] = &[
    "$schema",
    "$id",
    "$anchor",
    "$dynamicAnchor",
    "$ref",
    "$dynamicRef",
    "$vocabulary",
    "$comment",
    "type",
    "enum",
    "const",
    "required",
    "dependentRequired",
    "minProperties",
    "maxProperties",
    "minItems",
    "maxItems",
    "minContains",
    "maxContains",
    "uniqueItems",
    "minLength",
    "maxLength",
    "pattern",
    "format",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "title",
    "description",
    "default",
    "examples",
    "deprecated",
    "readOnly",
    "writeOnly",
    "contentEncoding",
    "contentMediaType",
    "x-mcp-header",
];

fn children(schema: &Value) -> Vec<&Value> {
    locations(schema)
        .into_iter()
        .map(|(_, value)| value)
        .collect()
}

pub(super) fn locations(schema: &Value) -> Vec<(String, &Value)> {
    let mut children = Vec::new();
    for key in MAPS {
        if let Some(values) = schema[*key].as_object() {
            children.extend(values.iter().map(|(name, value)| {
                (
                    format!("/{key}/{}", name.replace('~', "~0").replace('/', "~1")),
                    value,
                )
            }));
        }
    }
    for key in LISTS {
        if let Some(values) = schema[*key].as_array() {
            children.extend(
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (format!("/{key}/{index}"), value)),
            );
        }
    }
    for key in SINGLES {
        if let Some(value) = schema.get(*key) {
            children.push((format!("/{key}"), value));
        }
    }
    children
}

pub(super) fn prepare(
    schema: &Value,
) -> Result<(Registry<'static>, super::plan::Plan), SchemaError> {
    // Bound all JSON, including defaults/enum/annotation data, before registry preparation.
    count(schema, 0, &mut 0, &|| Ok(()), MAX_SCHEMA_NODES)
        .map_err(|_| SchemaError::new("exceeds the schema depth/node bound"))?;
    json::measure(schema, MAX_SCHEMA_BYTES, &|| Ok(()))
        .map_err(|_| SchemaError::new("exceeds the schema byte bound"))?;
    declarations(schema)?;
    // The referencing crate's default retriever always refuses acquisition.
    let registry = Registry::new()
        .draft(Draft::Draft202012)
        .add(BASE, jsonschema::Resource::from_contents(schema.clone()))
        .and_then(referencing::RegistryBuilder::prepare)
        .map_err(|error| SchemaError::new(error.to_string()))?;
    let resolver = registry.resolver(
        referencing::uri::from_str(BASE).map_err(|error| SchemaError::new(error.to_string()))?,
    );
    let root = resolver
        .lookup("#")
        .map_err(|error| SchemaError::new(error.to_string()))?;
    let (root, root_resolver, _) = root.into_inner();
    let root_resolver = root_resolver
        .in_subresource(ResourceRef::new(root, Draft::Draft202012))
        .map_err(|error| SchemaError::new(error.to_string()))?;
    let plan = super::plan::Plan::new(root, root_resolver)?;
    Ok((registry, plan))
}

pub(super) fn declarations(schema: &Value) -> Result<(), SchemaError> {
    let mut pending = vec![(String::new(), schema)];
    while let Some((pointer, node)) = pending.pop() {
        let fail = |problem: String| SchemaError {
            pointer: if pointer.is_empty() {
                "/".into()
            } else {
                pointer.clone()
            },
            problem,
        };
        if let Some(dialect) = node.get("$schema")
            && dialect
                .as_str()
                .is_none_or(|text| text.strip_suffix('#').unwrap_or(text) != DIALECT)
        {
            return Err(fail(format!("uses unsupported dialect {dialect}")));
        }
        if let Some(object) = node.as_object() {
            for key in object.keys() {
                if !KEYWORDS.contains(&key.as_str())
                    && !MAPS.contains(&key.as_str())
                    && !LISTS.contains(&key.as_str())
                    && !SINGLES.contains(&key.as_str())
                {
                    return Err(fail(format!("uses unsupported keyword {key}")));
                }
            }
        }
        if let Some(vocabularies) = node["$vocabulary"].as_object() {
            for (uri, required) in vocabularies {
                let supported = uri
                    .strip_prefix("https://json-schema.org/draft/2020-12/vocab/")
                    .is_some_and(|name| {
                        [
                            "core",
                            "applicator",
                            "validation",
                            "unevaluated",
                            "meta-data",
                            "format-annotation",
                            "content",
                        ]
                        .contains(&name)
                    });
                if required == true && !supported {
                    return Err(fail(format!("requires unsupported vocabulary {uri}")));
                }
            }
        }
        pending.extend(
            locations(node)
                .into_iter()
                .map(|(child, value)| (format!("{pointer}{child}"), value)),
        );
    }
    Ok(())
}

pub(super) fn anchors<'a>(
    root: &'a Value,
    resolver: Resolver<'a>,
) -> Result<Vec<(&'a Value, Resolver<'a>)>, SchemaError> {
    let mut pending = vec![(root, resolver)];
    let mut anchors = Vec::new();
    while let Some((node, resolver)) = pending.pop() {
        if node.get("$dynamicAnchor").is_some() {
            anchors.push((node, resolver.clone()));
        }
        for child in children(node) {
            let selected = resolver
                .in_subresource(ResourceRef::new(child, Draft::Draft202012))
                .map_err(|error| SchemaError::new(error.to_string()))?;
            pending.push((child, selected));
        }
    }
    Ok(anchors)
}

fn count(
    value: &Value,
    depth: usize,
    nodes: &mut usize,
    check: &dyn Fn() -> Result<(), Interrupted>,
    limit: usize,
) -> Result<(), ArgumentErrorKind> {
    check().map_err(|_| ArgumentErrorKind::Interrupted)?;
    *nodes += 1;
    if depth > MAX_DEPTH || *nodes > limit {
        return Err(ArgumentErrorKind::Limit);
    }
    match value {
        Value::Array(values) => {
            for value in values {
                count(value, depth + 1, nodes, check, limit)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                count(value, depth + 1, nodes, check, limit)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn value(
    value: &Value,
    check: &dyn Fn() -> Result<(), Interrupted>,
) -> Result<(usize, usize), ArgumentErrorKind> {
    let mut nodes = 0;
    count(value, 0, &mut nodes, check, MAX_VALUE_NODES)?;
    let bytes = json::measure(value, MAX_VALUE_BYTES, check).map_err(|error| match error {
        json::EncodeError::Interrupted => ArgumentErrorKind::Interrupted,
        _ => ArgumentErrorKind::Limit,
    })?;
    Ok((nodes, bytes))
}
