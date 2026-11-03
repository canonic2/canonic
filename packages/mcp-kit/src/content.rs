//! Validated audio and presentation annotations shared by protocol content; no I/O.

mod binary;
#[cfg(test)]
mod tests;

pub(crate) use binary::{BinaryError, base64, decode as decode_base64, decoded_size};
use serde::{Deserialize, Deserializer, Serialize, de, de::DeserializeOwned};
use serde_json::{Map, Number, Value};

/// Maximum raw bytes in an inline audio block.
pub const MAX_AUDIO_BYTES: usize = 1024 * 1024;
/// Maximum UTF-8 bytes in an audio MIME type.
pub const MAX_MIME_BYTES: usize = 256;
/// Maximum UTF-8 bytes in a modification timestamp hint.
pub const MAX_MODIFIED_BYTES: usize = 64;

/// Invalid content data or presentation hints, before encoding or publication.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContentError {
    /// A field has an invalid shape or value.
    #[error("invalid content field: {0}")]
    Field(&'static str),
    /// A value exceeds its operating bound.
    #[error("content limit exceeded: {0}")]
    Limit(&'static str),
}

/// An intended audience for content, without granting authority or changing routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Content intended for the user.
    User,
    /// Content intended for the assistant.
    Assistant,
}

/// Immutable, bounded presentation hints, distinct from tool behavior annotations.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentAnnotations {
    #[serde(skip_serializing_if = "Option::is_none")]
    audience: Option<Vec<Role>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_modified: Option<String>,
}

impl ContentAnnotations {
    /// Builds hints with at most two audience entries and finite priority in 0..=1.
    /// Supply an ISO 8601 modification timestamp; its spelling is retained and its
    /// length is bounded, without interpreting its calendar or timezone.
    ///
    /// # Errors
    /// Returns [`ContentError`] for excessive audiences, invalid priority, or an
    /// oversized or NUL-containing modification hint.
    pub fn new(
        audience: Option<Vec<Role>>,
        priority: Option<f64>,
        last_modified: Option<String>,
    ) -> Result<Self, ContentError> {
        let priority = priority
            .map(|value| Number::from_f64(value).ok_or(ContentError::Field("priority")))
            .transpose()?;
        Self::from_parts(audience, priority, last_modified)
    }

    fn from_parts(
        audience: Option<Vec<Role>>,
        priority: Option<Number>,
        last_modified: Option<String>,
    ) -> Result<Self, ContentError> {
        if audience.as_ref().is_some_and(|roles| roles.len() > 2) {
            return Err(ContentError::Limit("audience entries"));
        }
        if priority.as_ref().is_some_and(|number| {
            number
                .as_f64()
                .is_none_or(|value| !(0.0..=1.0).contains(&value))
        }) {
            return Err(ContentError::Field("priority"));
        }
        if last_modified
            .as_ref()
            .is_some_and(|text| text.len() > MAX_MODIFIED_BYTES || text.contains('\0'))
        {
            return Err(ContentError::Field("lastModified"));
        }
        Ok(Self {
            audience,
            priority,
            last_modified,
        })
    }

    /// Intended audience, retaining an explicitly empty array.
    #[must_use]
    pub fn audience(&self) -> Option<&[Role]> {
        self.audience.as_deref()
    }
    /// Importance hint, from least important (0) through most important (1).
    #[must_use]
    pub fn priority(&self) -> Option<f64> {
        self.priority.as_ref().and_then(Number::as_f64)
    }
    /// Exact modification timestamp hint supplied by the consumer.
    #[must_use]
    pub fn last_modified(&self) -> Option<&str> {
        self.last_modified.as_deref()
    }
}

impl<'de> Deserialize<'de> for ContentAnnotations {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Value::Object(mut fields) = Value::deserialize(deserializer)? else {
            return Err(de::Error::custom("content annotations must be an object"));
        };
        Self::from_parts(
            optional(&mut fields, "audience").map_err(de::Error::custom)?,
            optional(&mut fields, "priority").map_err(de::Error::custom)?,
            optional(&mut fields, "lastModified").map_err(de::Error::custom)?,
        )
        .map_err(de::Error::custom)
    }
}

fn optional<T: DeserializeOwned>(
    fields: &mut Map<String, Value>,
    key: &str,
) -> Result<Option<T>, serde_json::Error> {
    fields.remove(key).map(serde_json::from_value).transpose()
}

pub(crate) fn annotations<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ContentAnnotations>, D::Error> {
    ContentAnnotations::deserialize(deserializer).map(Some)
}

/// Bounded base64 audio with a consumer-selected MIME type; no codec processing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioContent {
    data: String,
    mime_type: String,
}

impl AudioContent {
    /// Encodes raw audio bytes after validating their size and MIME metadata.
    ///
    /// # Errors
    /// Returns [`ContentError`] for excessive bytes or empty, oversized or
    /// NUL-containing MIME metadata. This does not validate the audio container.
    pub fn new(bytes: &[u8], mime_type: impl Into<String>) -> Result<Self, ContentError> {
        let mime_type = mime_type.into();
        Self::validate(bytes.len(), &mime_type)?;
        Ok(Self {
            data: base64(bytes),
            mime_type,
        })
    }

    fn validate(raw_bytes: usize, mime_type: &str) -> Result<(), ContentError> {
        if raw_bytes > MAX_AUDIO_BYTES {
            return Err(ContentError::Limit("audio bytes"));
        }
        if mime_type.is_empty() || mime_type.len() > MAX_MIME_BYTES || mime_type.contains('\0') {
            return Err(ContentError::Field("mimeType"));
        }
        Ok(())
    }

    /// Canonical padded standard base64, retained without transcoding.
    #[must_use]
    pub fn data(&self) -> &str {
        &self.data
    }
    /// Consumer-selected MIME type.
    #[must_use]
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }
}

impl<'de> Deserialize<'de> for AudioContent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            data: String,
            mime_type: String,
        }
        let Wire { data, mime_type } = Wire::deserialize(deserializer)?;
        let raw_bytes = decoded_size(&data, MAX_AUDIO_BYTES).map_err(|error| {
            de::Error::custom(match error {
                BinaryError::Invalid => ContentError::Field("data"),
                BinaryError::TooLarge => ContentError::Limit("audio bytes"),
            })
        })?;
        Self::validate(raw_bytes, &mime_type).map_err(de::Error::custom)?;
        Ok(Self { data, mime_type })
    }
}
