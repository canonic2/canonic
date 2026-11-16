//! Separate read-only identities for consumer-declared dry-run tools.
//!
//! Consumers declare which commands genuinely support inspection without effects.
//! Hosts still own permissions; annotations never authorize a mutation.

use super::{Annotations, CatalogError, Tool, ToolCall};
use serde_json::json;

/// The applying tool behind a declared `*_preview` identity.
#[must_use]
pub fn source<'a>(sources: &[&str], name: &'a str) -> Option<&'a str> {
    name.strip_suffix("_preview")
        .filter(|source| sources.contains(source))
}

/// Add read-only previews of the declared sources, preserving argument validation.
///
/// The dry-run field is excluded from both published identities. Deferred work
/// and interactive selection flags are also excluded from previews. The
/// consumer must call [`normalize`] before its normal dispatch and must ensure
/// that `dry_run: true` cannot apply effects or ask for confirmation.
/// Source roots support `type`, `properties`, `required`, boolean
/// `additionalProperties` and schema annotations. Composition, other root
/// constraints and schema references are refused before any tool is changed.
///
/// # Errors
/// Returns a schema error for a missing source/boolean flag or unsupported source shape.
pub fn append(tools: &mut Vec<Tool>, sources: &[&str]) -> Result<(), CatalogError> {
    append_with_flag(tools, sources, "dry_run")
}

/// Like [`append`], for a consumer's existing boolean dry-run field spelling.
///
/// # Errors
/// Returns a schema error for a missing source/boolean flag or unsupported source shape.
pub fn append_with_flag(
    tools: &mut Vec<Tool>,
    sources: &[&str],
    flag: &str,
) -> Result<(), CatalogError> {
    for source in sources {
        if !tools.iter().any(|tool| {
            tool.name == *source && tool.input_schema["properties"][flag]["type"] == "boolean"
        }) {
            return Err(CatalogError::Schema {
                tool: (*source).to_owned(),
                field: "inputSchema",
                problem: format!("preview source must declare boolean {flag}"),
            });
        }
    }
    for tool in tools
        .iter()
        .filter(|tool| sources.contains(&tool.name.as_str()))
    {
        let schema = &tool.input_schema;
        let supported = schema.as_object().is_some_and(|root| {
            root.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "type"
                        | "properties"
                        | "required"
                        | "additionalProperties"
                        | "$schema"
                        | "$id"
                        | "title"
                        | "description"
                        | "$comment"
                        | "default"
                        | "examples"
                        | "deprecated"
                        | "readOnly"
                        | "writeOnly"
                )
            })
        }) && schema
            .get("additionalProperties")
            .is_none_or(serde_json::Value::is_boolean);
        if !supported || has_reference(schema) {
            return Err(CatalogError::Schema {
                tool: tool.name.clone(),
                field: "inputSchema",
                problem: "preview source requires a simple object root without schema references"
                    .into(),
            });
        }
    }
    let previews = tools.iter_mut().filter(|tool| sources.contains(&tool.name.as_str())).map(|tool| {
        let mut preview = Tool::new(
            format!("{}_preview", tool.name),
            format!("Preview {} without applying changes or asking for confirmation; use {} to apply.", tool.name, tool.name),
            tool.input_schema.clone(),
        ).with_annotations(Annotations {
            read_only_hint: Some(true), destructive_hint: Some(false),
            idempotent_hint: Some(true), open_world_hint: tool.annotations.open_world_hint,
            ..Annotations::default()
        });
        for field in [flag, "deferred", "select"] { remove_field(&mut preview.input_schema, field); }
        remove_field(&mut tool.input_schema, flag);
        tool.description = format!(
            "{}; use {}_preview for inspection.",
            tool.description.trim_end_matches('.'),
            tool.name
        );
        preview
    }).collect::<Vec<_>>();
    tools.extend(previews);
    Ok(())
}

fn has_reference(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => {
            object.contains_key("$ref")
                || object.contains_key("$dynamicRef")
                || object.values().any(has_reference)
        }
        serde_json::Value::Array(values) => values.iter().any(has_reference),
        _ => false,
    }
}

fn remove_field(schema: &mut serde_json::Value, field: &str) {
    if let Some(properties) = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
    {
        properties.remove(field);
    }
    if let Some(required) = schema
        .get_mut("required")
        .and_then(serde_json::Value::as_array_mut)
    {
        required.retain(|value| value != field);
    }
}

/// Resolve a declared preview to its existing handler with dry-run forced on.
/// Removes deferred and interactive selection flags even when the source schema
/// allows additional properties, and overrides the caller's dry-run flag.
#[must_use]
pub fn normalize(sources: &[&str], call: ToolCall) -> ToolCall {
    normalize_with_flag(sources, call, "dry_run")
}

/// Like [`normalize`], forcing the consumer's existing dry-run field spelling.
#[must_use]
pub fn normalize_with_flag(sources: &[&str], mut call: ToolCall, flag: &str) -> ToolCall {
    if let Some(source) = source(sources, &call.name) {
        call.name = source.to_owned();
        if let Some(arguments) = call.arguments.as_object_mut() {
            for field in [flag, "deferred", "select"] {
                arguments.remove(field);
            }
        }
        call.arguments[flag] = json!(true);
    }
    call
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Catalog, schema};

    #[test]
    fn previews_strip_execution_flags_when_source_allows_extra_properties() {
        for additional in [None, Some(json!(true))] {
            let mut input = json!({"type":"object", "properties":{
                "path":{"type":"string"}, "inspect":{"type":"boolean"}
            }});
            if let Some(additional) = additional {
                input["additionalProperties"] = additional;
            }
            let mut tools = vec![Tool::new("acme_remove", "Remove files.", input)];
            append_with_flag(&mut tools, &["acme_remove"], "inspect").unwrap();
            let arguments = json!({"path":"a", "deferred":true, "select":true,
                "inspect":false, "extra":{"keep":true}});
            schema::validate(&tools[1].input_schema, &arguments).unwrap();
            let call = ToolCall {
                name: "acme_remove_preview".into(),
                arguments,
                progress_token: Some(json!("progress")),
            };
            let applying = ToolCall {
                name: "acme_remove".into(),
                ..call.clone()
            };
            assert_eq!(
                normalize_with_flag(&["acme_remove"], applying.clone(), "inspect"),
                applying
            );
            let normalized = normalize_with_flag(&["acme_remove"], call, "inspect");
            assert_eq!(normalized.name, "acme_remove");
            assert_eq!(
                normalized.arguments,
                json!({"path":"a", "inspect":true, "extra":{"keep":true}})
            );
            assert_eq!(normalized.progress_token, Some(json!("progress")));
        }
    }

    #[test]
    fn previews_reject_composed_constraints_before_mutating_any_source() {
        let simple = json!({"type":"object", "properties":{"dry_run":{"type":"boolean"}},
            "required":["dry_run"], "additionalProperties":false});
        for constraint in [
            json!({"allOf":[{"required":["dry_run"]}]}),
            json!({"anyOf":[{"required":["select"]}]}),
            json!({"dependentRequired":{"path":["dry_run"]}}),
            json!({"minProperties":1}),
            json!({"properties":{"dry_run":{"type":"boolean"},
                "other":{"$ref":"#/properties/dry_run"}}}),
        ] {
            let mut composed = simple.clone();
            composed
                .as_object_mut()
                .unwrap()
                .extend(constraint.as_object().unwrap().clone());
            schema::check(&composed).unwrap();
            let mut tools = vec![
                Tool::new("first", "Apply.", simple.clone()),
                Tool::new("last", "Apply.", composed),
            ];
            let original = tools.clone();
            assert!(append(&mut tools, &["first", "last"]).is_err());
            assert_eq!(tools, original);
        }
    }

    #[test]
    fn previews_preserve_catalog_order_and_validate_before_mutating() {
        let make_tool = |name| {
            Tool::new(
                name,
                "Apply changes.",
                json!({
                    "type":"object",
                    "properties":{"inspect":{"type":"boolean"},"deferred":{"type":"boolean"},"select":{"type":"boolean"}},
                    "required":["inspect","deferred","select"],
                    "additionalProperties":false
                }),
            )
        };
        let mut tools = vec![
            make_tool("first"),
            make_tool("untouched"),
            make_tool("last"),
        ];
        let original = tools.clone();
        assert!(append_with_flag(&mut tools, &["first", "missing"], "inspect").is_err());
        assert_eq!(tools, original);

        append_with_flag(&mut tools, &["last", "first", "first"], "inspect").unwrap();
        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            [
                "first",
                "untouched",
                "last",
                "first_preview",
                "last_preview"
            ]
        );
        assert_eq!(tools[1], original[1]);
        for tool in [&tools[0], &tools[2]] {
            assert_eq!(tool.input_schema["required"], json!(["deferred", "select"]));
            assert!(
                tool.description
                    .ends_with(&format!("use {}_preview for inspection.", tool.name))
            );
        }
        for tool in &tools[3..] {
            schema::validate(&tool.input_schema, &json!({})).unwrap();
            assert_eq!(tool.input_schema["properties"], json!({}));
        }
        let call = normalize_with_flag(
            &["first", "last"],
            ToolCall {
                name: "last_preview".into(),
                arguments: json!({}),
                progress_token: None,
            },
            "inspect",
        );
        assert_eq!(call.name, "last");
        assert_eq!(call.arguments, json!({"inspect":true}));
    }

    #[test]
    fn previews_have_separate_permissions_and_force_inspection() {
        let mut tools = vec![Tool::new(
            "acme_remove",
            "Remove selected files.",
            json!({
                "type":"object","properties":{"path":{"type":"string"},"dry_run":{"type":"boolean"},"deferred":{"type":"boolean"},"select":{"type":"boolean"}},
                "required":["path"],"additionalProperties":false
            }),
        )];
        assert!(append(&mut tools, &["unknown"]).is_err());
        assert!(append_with_flag(&mut tools, &["acme_remove"], "not_a_preview_flag").is_err());
        append(&mut tools, &["acme_remove"]).unwrap();
        let catalog = Catalog::new(tools).unwrap();
        let tool = catalog.get("acme_remove_preview").unwrap();
        assert!(tool.read_only());
        assert_eq!(tool.annotations.destructive_hint, Some(false));
        assert!(!catalog.get("acme_remove").unwrap().read_only());
        for tool in catalog.tools() {
            assert!(tool.input_schema["properties"].get("dry_run").is_none());
            for value in [true, false] {
                assert!(
                    schema::validate(&tool.input_schema, &json!({"path":"a","dry_run":value}))
                        .is_err()
                );
            }
        }
        for field in ["dry_run", "deferred", "select"] {
            assert!(
                schema::validate(&tool.input_schema, &json!({"path":"a",field:false})).is_err()
            );
        }
        schema::validate(&tool.input_schema, &json!({"path":"a"})).unwrap();
        let call = normalize(
            &["acme_remove"],
            ToolCall {
                name: tool.name.clone(),
                arguments: json!({"path":"a"}),
                progress_token: None,
            },
        );
        assert_eq!(call.name, "acme_remove");
        assert_eq!(call.arguments, json!({"path":"a","dry_run":true}));
        assert!(source(&["acme_remove"], "unknown_preview").is_none());
    }
}
