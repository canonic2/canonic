//! Map shared argument validation errors to Testrun's stable boundary error.
use crate::error::{Error, Result};
use canonic_mcp_kit::schema::PreparedSchema;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

// Definitions are product-owned, but keep retention bounded for ad-hoc callers.
// Exact content keys also distinguish entry-point variants of the same tool.
const MAX_CACHED_SCHEMAS: usize = 512;
const MAX_CACHED_BYTES: usize = 8 * 1024 * 1024;
static SCHEMAS: LazyLock<Mutex<Schemas>> = LazyLock::new(|| Mutex::new(Schemas::default()));

#[derive(Default)]
struct Schemas {
    entries: HashMap<Value, PreparedSchema>,
    bytes: usize,
    #[cfg(test)]
    preparations: usize,
}

impl Schemas {
    fn prepare(&mut self, schema: &Value) -> Result<PreparedSchema> {
        if let Some(prepared) = self.entries.get(schema) {
            return Ok(prepared.clone());
        }
        let prepared =
            PreparedSchema::new(schema).map_err(|error| Error::invalid(error.to_string()))?;
        #[cfg(test)]
        {
            self.preparations += 1;
        }
        let bytes = serde_json::to_vec(schema)
            .map_err(|error| Error::invalid(error.to_string()))?
            .len();
        if self.entries.len() < MAX_CACHED_SCHEMAS
            && self.bytes.saturating_add(bytes) <= MAX_CACHED_BYTES
        {
            self.entries.insert(schema.clone(), prepared.clone());
            self.bytes += bytes;
        }
        Ok(prepared)
    }
}

/// A schema prepared once by its owner, for validation on a hot path
/// without hashing the whole schema to find it in the shared cache.
pub(crate) struct Prepared(PreparedSchema);
impl Prepared {
    pub(crate) fn new(schema: &Value, max_value_bytes: usize) -> Result<Self> {
        PreparedSchema::new(schema)
            .map(|prepared| Self(prepared.with_value_byte_limit(max_value_bytes)))
            .map_err(|error| Error::invalid(error.to_string()))
    }
    pub(crate) fn validate(&self, value: &Value) -> Result<()> {
        self.0
            .validate_at(value, "arguments")
            .map_err(|error| Error::invalid(error.message))
    }
}

/// Cache a fixed tool surface by name so calls avoid hashing the full schema.
#[derive(Default)]
pub(crate) struct PreparedTools(Mutex<HashMap<String, Arc<Prepared>>>);

impl PreparedTools {
    pub(crate) fn get(
        &self,
        name: &str,
        max_value_bytes: usize,
        schema: impl FnOnce() -> Result<Value>,
    ) -> Result<Arc<Prepared>> {
        let mut entries = self.0.lock().expect("Prepared tool schemas");
        if let Some(prepared) = entries.get(name) {
            return Ok(prepared.clone());
        }
        let prepared = Arc::new(Prepared::new(&schema()?, max_value_bytes)?);
        entries.insert(name.to_owned(), prepared.clone());
        Ok(prepared)
    }
}

pub fn validate(schema: &Value, value: &Value) -> Result<()> {
    validate_at(schema, value, "arguments")
}
pub(crate) fn validate_at(schema: &Value, value: &Value, path: &str) -> Result<()> {
    let prepared = SCHEMAS
        .lock()
        .map_err(|_| Error::invalid("Schema preparation unavailable"))?
        .prepare(schema)?;
    prepared
        .validate_at(value, path)
        .map_err(|error| Error::invalid(error.message))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn repeated_definitions_prepare_once_and_retain_validation() {
        let mut schemas = Schemas::default();
        let schema =
            json!({"type":"object","properties":{"count":{"type":"integer"}},"required":["count"]});
        for _ in 0..4 {
            let prepared = schemas.prepare(&schema.clone()).unwrap();
            prepared.validate(&json!({"count":1})).unwrap();
            assert!(prepared.validate(&json!({"count":"wrong"})).is_err());
        }
        assert_eq!(schemas.preparations, 1);
        let changed = json!({"type":"boolean"});
        schemas
            .prepare(&changed)
            .unwrap()
            .validate(&json!(true))
            .unwrap();
        assert_eq!(schemas.preparations, 2);
    }
    #[test]
    fn tagged_unions_nullable_values_and_uuid_references_validate_before_dispatch() {
        let schema = crate::browser::contract::schemas();
        for request in [
            json!({"operation":"context_list"}),
            json!({"operation":"context_create"}),
            json!({"operation":"page_create","context":{"connectionId":uuid::Uuid::new_v4(),"id":uuid::Uuid::new_v4(),"generation":uuid::Uuid::new_v4()}}),
        ] {
            validate(&schema["request"], &request).unwrap();
        }
        for request in [
            json!({"operation":"context_list","unknown":true}),
            json!({"operation":"page_create","context":{}}),
            json!({"operation":"handle_dispose","handle":{"connectionId":"wrong","id":"wrong","generation":"wrong"}}),
        ] {
            assert!(validate(&schema["request"], &request).is_err());
        }
        let nullable = json!({"type":["string","null"]});
        assert!(validate(&nullable, &Value::Null).is_ok());
        assert!(validate(&nullable, &json!(5)).is_err());
    }
    #[test]
    fn rejects_nested_unknown_fields_and_wrong_optional_types() {
        let schema = json!({"type":"object","properties":{"point":{"type":"object","properties":{"x":{"type":"number","minimum":0,"maximum":1}},"required":["x"],"additionalProperties":false},"enabled":{"type":"boolean"}},"required":["point"],"additionalProperties":false});
        assert!(validate(&schema, &json!({"point":{"x":0.5}})).is_ok());
        for value in [
            json!({"point":{"x":0.5,"shell":"ignored"}}),
            json!({"point":{"x":2}}),
            json!({"point":{"x":0},"enabled":"false"}),
        ] {
            assert_eq!(
                validate(&schema, &value).unwrap_err().code,
                "INVALID_ARGUMENT"
            );
        }
        let list = json!({"type":"array","items":schema});
        for (value, expected) in [
            (json!([{"point":{}}]), "arguments[0].point.x is required"),
            (
                json!([{"point":{"x":"bad"}}]),
                "arguments[0].point.x must be number",
            ),
            (
                json!([{"point":{"x":0,"extra":1}}]),
                "arguments[0].point.extra is not a supported field",
            ),
            (
                json!([{"point":{"x":2}}]),
                "arguments[0].point.x must be between 0 and 1; received 2",
            ),
        ] {
            assert_eq!(validate(&list, &value).unwrap_err().message, expected);
        }
    }
}
