//! Prepared `x-mcp-header` annotations: tool arguments mirrored into HTTP headers.
//!
//! [`prepare`] validates every annotation in one input schema and records the
//! property path, header name and declared type once, at catalog construction.

use crate::http::is_token;
use serde_json::Value;

/// The most `x-mcp-header` annotations one tool may declare.
pub const MAX_PARAMETER_HEADERS: usize = 16;

const ANNOTATION: &str = "x-mcp-header";

/// The declared JSON type of an annotated property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    String,
    Integer,
    Boolean,
}

/// One annotated property, prepared for request-time comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParameterHeader {
    /// The annotation value, as declared; the header is `Mcp-Param-{name}`.
    pub(crate) name: String,
    /// The lowercase name, for case-insensitive header matching.
    pub(crate) key: String,
    /// Property names from the arguments object to the value.
    pub(crate) path: Vec<String>,
    pub(crate) kind: Kind,
}

/// Validates and prepares the annotations of one input schema.
pub(crate) fn prepare(schema: &Value) -> Result<Vec<ParameterHeader>, String> {
    let mut found = Vec::new();
    visit(schema, &mut Vec::new(), true, &mut found)?;
    if found.len() > MAX_PARAMETER_HEADERS {
        return Err(format!(
            "declares more than {MAX_PARAMETER_HEADERS} {ANNOTATION} annotations"
        ));
    }
    for (index, header) in found.iter().enumerate() {
        if found[..index].iter().any(|other| other.key == header.key) {
            return Err(format!(
                "{ANNOTATION} {:?} is declared twice, ignoring case",
                header.name
            ));
        }
    }
    Ok(found)
}

/// Walks `schema`; `reachable` holds while every step from the root was a
/// `properties` entry.
fn visit(
    schema: &Value,
    path: &mut Vec<String>,
    reachable: bool,
    found: &mut Vec<ParameterHeader>,
) -> Result<(), String> {
    let object = match schema {
        Value::Object(object) => object,
        Value::Array(items) => {
            for item in items {
                visit(item, path, false, found)?;
            }
            return Ok(());
        }
        _ => return Ok(()),
    };
    if let Some(value) = object.get(ANNOTATION) {
        if !reachable || path.is_empty() {
            return Err(format!(
                "{ANNOTATION} is allowed only on a property reached through properties"
            ));
        }
        let name = value
            .as_str()
            .filter(|name| is_token(name))
            .ok_or_else(|| format!("{ANNOTATION} must be a nonempty HTTP token"))?;
        let declared = match object.get("type") {
            Some(Value::String(kind)) => Some(kind.as_str()),
            Some(Value::Array(kinds)) if kinds.len() == 1 => kinds[0].as_str(),
            _ => None,
        };
        let kind = match declared {
            Some("string") => Kind::String,
            Some("integer") => Kind::Integer,
            Some("boolean") => Kind::Boolean,
            _ => {
                return Err(format!(
                    "{ANNOTATION} {name:?} must annotate one type: string, integer or boolean"
                ));
            }
        };
        found.push(ParameterHeader {
            name: name.to_owned(),
            key: name.to_ascii_lowercase(),
            path: path.clone(),
            kind,
        });
    }
    for (keyword, child) in object {
        match keyword.as_str() {
            // Annotation values and instance data are not schemas.
            ANNOTATION | "const" | "enum" | "default" | "examples" => {}
            "properties" => {
                for (name, property) in child.as_object().into_iter().flatten() {
                    path.push(name.clone());
                    visit(property, path, reachable, found)?;
                    path.pop();
                }
            }
            "$defs" | "definitions" | "patternProperties" | "dependentSchemas" => {
                for property in child.as_object().into_iter().flat_map(|map| map.values()) {
                    visit(property, path, false, found)?;
                }
            }
            _ => visit(child, path, false, found)?,
        }
    }
    Ok(())
}
