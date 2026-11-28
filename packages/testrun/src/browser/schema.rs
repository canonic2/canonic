//! Pinned protocol schemas and bounded validation at the private driver boundary.
//!
//! This is the wire validator, not page evaluation's serializer. Unlike the
//! upstream JS validator it rejects unknown object fields instead of dropping
//! them. Public task operations must never expose arbitrary protocol forwarding.

use super::objects::ObjectTable;
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, sync::OnceLock};

pub(crate) fn encoded_len(value: &Value, limit: usize) -> Result<usize, Error> {
    structural(value, 0, &mut 0)?;
    struct Counter {
        bytes: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit - self.bytes {
                return Err(std::io::Error::other("Size limit"));
            }
            self.bytes += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { bytes: 0, limit };
    serde_json::to_writer(&mut counter, value).map_err(|_| Error::Bounds)?;
    Ok(counter.bytes)
}

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("Unknown pinned driver schema")]
    Unknown,
    #[error("Driver value does not match its pinned schema")]
    Mismatch,
    #[error("Driver value exceeds structural bounds")]
    Bounds,
    #[error("Driver channel is unknown or has an unexpected type")]
    Channel,
}

#[derive(Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
enum Schema {
    Float,
    Int,
    Boolean,
    String,
    Binary,
    Any,
    Optional(Box<Schema>),
    Array(Box<Schema>),
    Object(BTreeMap<String, Schema>),
    Enum(Vec<Value>),
    Channel(Vec<String>),
    Type(String),
}

#[derive(Deserialize)]
struct Registry {
    version: String,
    schemas: BTreeMap<String, Schema>,
}

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let registry: Registry = serde_json::from_str(include_str!("protocol-schema.json"))
            .expect("Generated pinned protocol schemas must be valid");
        assert_eq!(registry.version, super::DRIVER_VERSION);
        registry
    })
}

pub fn contains(name: &str) -> bool {
    registry().schemas.contains_key(name)
}

/// Validate absence separately from JSON null. Optional upstream fields allow
/// absence, not null. Channel GUIDs must already exist in this driver's graph.
pub fn validate(name: &str, value: Option<&Value>, objects: &ObjectTable) -> Result<(), Error> {
    let schema = registry().schemas.get(name).ok_or(Error::Unknown)?;
    if let Some(value) = value {
        structural(value, 0, &mut 0)?;
    }
    check(schema, value, objects, 0)
}

fn structural(value: &Value, depth: usize, nodes: &mut usize) -> Result<(), Error> {
    *nodes += 1;
    if depth > MAX_DEPTH || *nodes > MAX_NODES {
        return Err(Error::Bounds);
    }
    match value {
        Value::Array(values) => {
            for value in values {
                structural(value, depth + 1, nodes)?;
            }
        }
        Value::Object(fields) => {
            for value in fields.values() {
                structural(value, depth + 1, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn check(
    schema: &Schema,
    value: Option<&Value>,
    objects: &ObjectTable,
    depth: usize,
) -> Result<(), Error> {
    if depth > MAX_DEPTH * 4 {
        return Err(Error::Bounds);
    }
    if let Schema::Optional(inner) = schema {
        return value.map_or(Ok(()), |value| {
            check(inner, Some(value), objects, depth + 1)
        });
    }
    if let Schema::Type(name) = schema {
        let target = registry().schemas.get(name).ok_or(Error::Unknown)?;
        return check(target, value, objects, depth + 1);
    }
    let value = value.ok_or(Error::Mismatch)?;
    let valid = match schema {
        Schema::Any => true,
        Schema::Float => value.is_number(),
        Schema::Int => value
            .as_f64()
            .is_some_and(|number| number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0),
        Schema::Boolean => value.is_boolean(),
        Schema::String => value.is_string(),
        Schema::Binary => value.as_str().is_some_and(|text| {
            base64::engine::general_purpose::STANDARD
                .decode(text)
                .is_ok()
        }),
        Schema::Enum(values) => values.contains(value),
        Schema::Array(inner) => {
            let values = value.as_array().ok_or(Error::Mismatch)?;
            for value in values {
                check(inner, Some(value), objects, depth + 1)?;
            }
            true
        }
        Schema::Object(fields) => {
            let values = value.as_object().ok_or(Error::Mismatch)?;
            if values.keys().any(|key| !fields.contains_key(key)) {
                return Err(Error::Mismatch);
            }
            for (key, schema) in fields {
                check(schema, values.get(key), objects, depth + 1)?;
            }
            true
        }
        Schema::Channel(types) => {
            let fields = value.as_object().ok_or(Error::Channel)?;
            let guid = fields
                .get("guid")
                .and_then(Value::as_str)
                .ok_or(Error::Channel)?;
            if fields.len() != 1 {
                return Err(Error::Channel);
            }
            let kind = objects.wire_kind(guid).map_err(|_| Error::Channel)?;
            if !types
                .iter()
                .any(|expected| expected == "*" || expected == kind)
            {
                return Err(Error::Channel);
            }
            true
        }
        Schema::Optional(_) | Schema::Type(_) => unreachable!("Handled schema wrappers"),
    };
    if valid { Ok(()) } else { Err(Error::Mismatch) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn optional_absence_is_distinct_from_null_and_unknown_fields_fail() {
        let table = ObjectTable::default();
        assert!(validate("DisposableInitializer", None, &table).is_ok());
        assert!(validate("DisposableInitializer", Some(&json!({})), &table).is_ok());
        assert_eq!(
            validate("DisposableInitializer", Some(&Value::Null), &table),
            Err(Error::Mismatch)
        );
        assert_eq!(
            validate(
                "DisposableInitializer",
                Some(&json!({"unknown":true})),
                &table
            ),
            Err(Error::Mismatch)
        );
        assert!(
            validate(
                "RootInitializeParams",
                Some(&json!({"sdkLanguage":"javascript"})),
                &table
            )
            .is_ok()
        );
        assert_eq!(
            validate(
                "RootInitializeParams",
                Some(&json!({"sdkLanguage":"rust"})),
                &table
            ),
            Err(Error::Mismatch)
        );
        assert_eq!(
            validate("InventedSchema", None, &table),
            Err(Error::Unknown)
        );
    }

    #[test]
    fn channel_references_require_live_objects_of_the_expected_type() {
        let mut table = ObjectTable::default();
        let root = table.lookup("", "Root").unwrap();
        let object = table.create(&root, "playwright", "Playwright").unwrap();
        let value = json!({"playwright":{"guid":"playwright"}});
        assert!(validate("RootInitializeResult", Some(&value), &table).is_ok());
        assert_eq!(
            validate(
                "RootInitializeResult",
                Some(&json!({"playwright":{"guid":"unknown"}})),
                &table
            ),
            Err(Error::Channel)
        );
        table.dispose(&object).unwrap();
        table.create(&root, "playwright", "Page").unwrap();
        assert_eq!(
            validate("RootInitializeResult", Some(&value), &table),
            Err(Error::Channel)
        );
    }

    #[test]
    fn binary_numeric_and_structural_bounds_reject_lossy_or_unbounded_values() {
        let table = ObjectTable::default();
        assert!(
            validate(
                "StreamReadResult",
                Some(&json!({"binary":"YWNtZQ=="})),
                &table
            )
            .is_ok()
        );
        assert_eq!(
            validate(
                "StreamReadResult",
                Some(&json!({"binary":"invalid!"})),
                &table
            ),
            Err(Error::Mismatch)
        );
        let mut value = Value::Null;
        for _ in 0..66 {
            value = json!([value]);
        }
        assert_eq!(
            validate("SerializedValue", Some(&value), &table),
            Err(Error::Bounds)
        );
        assert_eq!(
            validate("Point", Some(&json!({"x":1,"y":"2"})), &table),
            Err(Error::Mismatch)
        );
        assert!(validate("Point", Some(&json!({"x":1.5,"y":2})), &table).is_ok());
        assert_eq!(
            validate(
                "PageScreencastFrameAckParams",
                Some(&json!({"frameId":9_007_199_254_740_992_u64})),
                &table
            ),
            Err(Error::Mismatch)
        );
        assert_eq!(
            validate(
                "PageScreencastFrameAckParams",
                Some(&json!({"frameId":1.5})),
                &table
            ),
            Err(Error::Mismatch)
        );
        assert!(
            validate(
                "PageScreencastFrameAckParams",
                Some(&json!({"frameId":1})),
                &table
            )
            .is_ok()
        );
        assert_eq!(
            validate(
                "SerializedValue",
                Some(&Value::Array(vec![Value::Null; 100_001])),
                &table
            ),
            Err(Error::Bounds)
        );
    }
}
