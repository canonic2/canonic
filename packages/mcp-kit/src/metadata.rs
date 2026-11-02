//! Immutable extension metadata, icon descriptors and request trace context; no I/O.
mod icon;
mod trace;
pub use icon::{Icon, IconTheme};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::{Map, Value};
use std::sync::Arc;
pub use trace::TraceContext;

#[cfg(test)]
mod tests;

/// Maximum encoded custom metadata bytes.
pub const MAX_METADATA_BYTES: usize = 65536;
/// Maximum icons on one declaration or implementation.
pub const MAX_ICONS: usize = 8;

/// Invalid metadata, icon or trace declaration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MetadataError {
    /// Invalid shape, key or value.
    #[error("invalid metadata field: {0}")]
    Field(&'static str),
    /// Exceeded byte, count or nesting bound.
    #[error("metadata limit exceeded: {0}")]
    Limit(&'static str),
}

/// Validated immutable JSON metadata. Clones share the same object.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Metadata(Arc<Map<String, Value>>);

impl Metadata {
    /// Validates key grammar, trace fields, encoded bytes and nesting once.
    /// Unknown values are retained unchanged and grant no authority.
    ///
    /// # Errors
    /// Returns [`MetadataError`] for a malformed or excessive object.
    pub fn new(value: Value) -> Result<Self, MetadataError> {
        Self::validate(&value, MAX_METADATA_BYTES, 16)?;
        let Value::Object(map) = value else {
            return Err(MetadataError::Field("_meta"));
        };
        Ok(Self(Arc::new(map)))
    }

    pub(crate) fn request(value: &Value) -> Result<Self, MetadataError> {
        Self::validate(value, 1024 * 1024, 32)?;
        Ok(Self(Arc::new(
            value
                .as_object()
                .ok_or(MetadataError::Field("_meta"))?
                .clone(),
        )))
    }

    pub(crate) fn validate(value: &Value, bytes: usize, depth: usize) -> Result<(), MetadataError> {
        let map = value.as_object().ok_or(MetadataError::Field("_meta"))?;
        crate::modern::bounded(value, bytes, depth).map_err(|_| MetadataError::Limit("object"))?;
        if map.keys().any(|key| !valid_key(key)) {
            return Err(MetadataError::Field("key"));
        }
        trace::validate(map)
    }

    /// The exact supplied metadata, without mutable access.
    #[must_use]
    pub fn as_map(&self) -> &Map<String, Value> {
        &self.0
    }
}

impl Serialize for Metadata {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Metadata {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Value::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

pub(crate) fn optional<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Metadata>, D::Error> {
    Metadata::deserialize(deserializer).map(Some)
}

fn valid_key(key: &str) -> bool {
    let (prefix, name) = key
        .split_once('/')
        .map_or((None, key), |(prefix, name)| (Some(prefix), name));
    let alphanumeric = |b: u8| b.is_ascii_alphanumeric();
    let name_valid = name.is_empty()
        || (name.as_bytes().first().is_some_and(|b| alphanumeric(*b))
            && name.as_bytes().last().is_some_and(|b| alphanumeric(*b))
            && name.bytes().all(|b| alphanumeric(b) || b"-_.".contains(&b)));
    name_valid
        && prefix.is_none_or(|prefix| {
            prefix.split('.').all(|label| {
                label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphabetic)
                    && label
                        .as_bytes()
                        .last()
                        .is_some_and(u8::is_ascii_alphanumeric)
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        })
}

pub(crate) fn website_url(url: &str) -> bool {
    icon::web_url(url, false)
}

pub(crate) fn result_allowed(map: &Map<String, Value>) -> bool {
    [
        crate::modern::VERSION_KEY,
        crate::modern::CAPABILITIES_KEY,
        crate::modern::CLIENT_KEY,
        "io.modelcontextprotocol/serverInfo",
        crate::subscription::SUBSCRIPTION_KEY,
        "io.modelcontextprotocol/logLevel",
        "progressToken",
    ]
    .iter()
    .all(|key| !map.contains_key(*key))
}

pub(crate) fn validate_icons(icons: &[Icon]) -> Result<(), MetadataError> {
    if icons.len() > MAX_ICONS {
        return Err(MetadataError::Limit("icons"));
    }
    crate::json::measure(icons, MAX_METADATA_BYTES, &|| Ok(()))
        .map_err(|_| MetadataError::Limit("icons"))?;
    Ok(())
}

pub(crate) fn implementation(value: &Value) -> Result<(), MetadataError> {
    let map = value
        .as_object()
        .ok_or(MetadataError::Field("implementation"))?;
    crate::modern::bounded(value, 128 * 1024, 16)
        .map_err(|_| MetadataError::Limit("implementation"))?;
    for key in ["name", "version"] {
        if map
            .get(key)
            .and_then(Value::as_str)
            .is_none_or(|s| s.len() > crate::MAX_CLIENT_FIELD_BYTES || s.contains('\0'))
        {
            return Err(MetadataError::Field("implementation identity"));
        }
    }
    for key in ["title", "description", "websiteUrl"] {
        if map.get(key).is_some_and(|v| {
            v.as_str()
                .is_none_or(|s| s.len() > 16384 || s.contains('\0'))
        }) {
            return Err(MetadataError::Field("implementation display"));
        }
    }
    if let Some(url) = map.get("websiteUrl").and_then(Value::as_str)
        && !icon::web_url(url, false)
    {
        return Err(MetadataError::Field("websiteUrl"));
    }
    if let Some(icons) = map.get("icons") {
        let icons: Vec<Icon> =
            serde_json::from_value(icons.clone()).map_err(|_| MetadataError::Field("icons"))?;
        validate_icons(&icons)?;
    }
    Ok(())
}
