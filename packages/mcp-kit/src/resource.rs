//! Immutable resource catalogs, URI validation and bounded read results; no I/O.

use crate::{
    json,
    rpc::{self, RpcError},
};
use iri_string::{template::UriTemplateStr, types::UriStr};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// Maximum combined resource and template count.
pub const MAX_ENTRIES: usize = 1024;
/// Entries returned per catalog page.
pub const PAGE_SIZE: usize = 64;
/// Maximum encoded catalog metadata and aggregate raw read data.
pub const MAX_BYTES: usize = 1024 * 1024;
/// Maximum URI or template length.
pub const MAX_URI_BYTES: usize = 4096;
/// Maximum contents in one read.
pub const MAX_CONTENTS: usize = 64;

/// Invalid resource metadata, catalog or read result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResourceError {
    /// A field violates its syntax or length constraint.
    #[error("invalid resource field: {0}")]
    Field(&'static str),
    /// A catalog repeats an address.
    #[error("duplicate resource address: {0}")]
    Duplicate(String),
    /// A bounded collection or representation is too large (or empty).
    #[error("resource limit exceeded: {0}")]
    Limit(&'static str),
}

/// Resource metadata, validated by [`ResourceCatalog`] or [`ResourceLink`].
/// Deserializing this mutable definition alone does not validate its fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    /// Optional extension metadata.
    #[serde(
        default,
        rename = "_meta",
        deserialize_with = "crate::metadata::optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub meta: Option<crate::Metadata>,
    /// Optional display icons.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub icons: Vec<crate::Icon>,
    /// Exact concrete URI.
    pub uri: String,
    /// Display name.
    pub name: String,
    /// Optional display title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional media type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Optional raw size in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Optional presentation hints, independent of access and cache policy.
    #[serde(
        default,
        deserialize_with = "crate::content::annotations",
        skip_serializing_if = "Option::is_none"
    )]
    pub annotations: Option<crate::ContentAnnotations>,
}

/// Validated resource metadata for a tool-result link, independent of discovery.
/// Creating a link neither publishes it in a catalog nor grants access to its URI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ResourceLink(Resource);

impl ResourceLink {
    pub(crate) fn set_metadata(&mut self, metadata: crate::Metadata) {
        self.0.meta = Some(metadata);
    }
    pub(crate) fn set_annotations(&mut self, annotations: crate::ContentAnnotations) {
        self.0.annotations = Some(annotations);
    }
    /// Validates a concrete URI and the same metadata bounds as a catalog entry.
    ///
    /// # Errors
    /// Returns [`ResourceError`] for invalid URI or metadata.
    pub fn new(resource: Resource) -> Result<Self, ResourceError> {
        crate::metadata::validate_icons(&resource.icons)
            .map_err(|_| ResourceError::Field("icons"))?;
        validate_uri(&resource.uri)?;
        metadata(
            &resource.name,
            resource.title.as_deref(),
            resource.description.as_deref(),
            resource.mime_type.as_deref(),
        )?;
        Ok(Self(resource))
    }

    /// Returns the validated definition without permitting mutation.
    #[must_use]
    pub fn resource(&self) -> &Resource {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ResourceLink {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Resource::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl Resource {
    /// Creates a definition without optional metadata.
    #[must_use]
    pub fn new(uri: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            meta: None,
            icons: vec![],
            name: name.into(),
            title: None,
            description: None,
            mime_type: None,
            size: None,
            annotations: None,
        }
    }
}

/// A client-expanded RFC 6570 resource address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTemplate {
    /// Optional extension metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<crate::Metadata>,
    /// Optional display icons.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub icons: Vec<crate::Icon>,
    /// URI template, including variable expressions.
    pub uri_template: String,
    /// Display name.
    pub name: String,
    /// Optional display title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional media type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Optional presentation hints for the template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<crate::ContentAnnotations>,
}

impl ResourceTemplate {
    /// Creates a template without optional metadata.
    #[must_use]
    pub fn new(uri_template: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri_template: uri_template.into(),
            meta: None,
            icons: vec![],
            name: name.into(),
            title: None,
            description: None,
            mime_type: None,
            annotations: None,
        }
    }
}

/// A validated, immutable resource catalog in publication order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceCatalog {
    resources: Vec<Resource>,
    templates: Vec<ResourceTemplate>,
    template_arguments: HashMap<String, HashSet<String>>,
}

impl ResourceCatalog {
    pub(crate) fn argument_names(&self, uri: &str) -> Option<&HashSet<String>> {
        self.template_arguments.get(uri)
    }
    pub(crate) fn definitions(&self) -> (Vec<Resource>, Vec<ResourceTemplate>) {
        (self.resources.clone(), self.templates.clone())
    }
    /// Validates addresses, uniqueness, metadata and aggregate bounds once.
    ///
    /// # Errors
    /// Returns [`ResourceError`] for invalid metadata, duplicates or limits.
    pub fn new(
        resources: Vec<Resource>,
        templates: Vec<ResourceTemplate>,
    ) -> Result<Self, ResourceError> {
        if resources.len().saturating_add(templates.len()) > MAX_ENTRIES {
            return Err(ResourceError::Limit("catalog entries"));
        }
        let mut seen = HashSet::new();
        for resource in &resources {
            crate::metadata::validate_icons(&resource.icons)
                .map_err(|_| ResourceError::Field("icons"))?;
            validate_uri(&resource.uri)?;
            metadata(
                &resource.name,
                resource.title.as_deref(),
                resource.description.as_deref(),
                resource.mime_type.as_deref(),
            )?;
            if !seen.insert(&resource.uri) {
                return Err(ResourceError::Duplicate(resource.uri.clone()));
            }
        }
        let mut template_arguments = HashMap::new();
        for template in &templates {
            crate::metadata::validate_icons(&template.icons)
                .map_err(|_| ResourceError::Field("icons"))?;
            let parsed = validate_template(&template.uri_template)?;
            metadata(
                &template.name,
                template.title.as_deref(),
                template.description.as_deref(),
                template.mime_type.as_deref(),
            )?;
            if template_arguments
                .insert(
                    template.uri_template.clone(),
                    parsed
                        .variables()
                        .map(|name| name.as_str().to_owned())
                        .collect(),
                )
                .is_some()
            {
                return Err(ResourceError::Duplicate(template.uri_template.clone()));
            }
        }
        json::encode(&(&resources, &templates), MAX_BYTES)
            .map_err(|_| ResourceError::Limit("catalog bytes"))?;
        Ok(Self {
            resources,
            templates,
            template_arguments,
        })
    }

    pub(crate) fn listing(&self, params: &Value, templates: bool) -> Result<Value, RpcError> {
        if !rpc::only(params, &["cursor", "_meta"]) {
            return Err(invalid());
        }
        let (kind, count, key) = if templates {
            ("templates", self.templates.len(), "resourceTemplates")
        } else {
            ("resources", self.resources.len(), "resources")
        };
        let start = match params.get("cursor") {
            None => 0,
            Some(cursor) => {
                let cursor = cursor.as_str().ok_or_else(invalid)?;
                let offset = cursor
                    .strip_prefix(&format!("{kind}:"))
                    .ok_or_else(invalid)?;
                let index = offset.parse::<usize>().map_err(|_| invalid())?;
                if index == 0
                    || index >= count
                    || index % PAGE_SIZE != 0
                    || offset != index.to_string()
                {
                    return Err(invalid());
                }
                index
            }
        };
        let end = start.saturating_add(PAGE_SIZE).min(count);
        let entries = if templates {
            json!(&self.templates[start..end])
        } else {
            json!(&self.resources[start..end])
        };
        let mut result = json!({key: entries});
        if end < count {
            result["nextCursor"] = json!(format!("{kind}:{end}"));
        }
        Ok(result)
    }
}

/// Validates an absolute RFC 3986 URI, preserving its exact spelling.
///
/// # Errors
/// Returns [`ResourceError::Field`] for invalid or oversized URIs.
pub fn validate_uri(uri: &str) -> Result<(), ResourceError> {
    if uri.len() > MAX_URI_BYTES || UriStr::new(uri).is_err() {
        return Err(ResourceError::Field("uri"));
    }
    Ok(())
}

fn validate_template(template: &str) -> Result<&UriTemplateStr, ResourceError> {
    if template.len() > MAX_URI_BYTES {
        return Err(ResourceError::Field("uriTemplate"));
    }
    let parsed = UriTemplateStr::new(template).map_err(|_| ResourceError::Field("uriTemplate"))?;
    // iri-string 0.7.14 permits zero-prefixed and five-digit prefix lengths.
    // RFC 6570 section 2.4.1 requires a nonzero first digit and at most four.
    for expression in template.split('{').skip(1) {
        let body = expression.split('}').next().unwrap_or_default();
        for variable in body.split(',') {
            if let Some((_, length)) = variable.split_once(':')
                && (length.len() > 4 || length.starts_with('0'))
            {
                return Err(ResourceError::Field("uriTemplate"));
            }
        }
    }
    Ok(parsed)
}

fn field(value: &str, limit: usize, name: &'static str) -> Result<(), ResourceError> {
    if value.len() > limit || value.contains('\0') {
        return Err(ResourceError::Field(name));
    }
    Ok(())
}

fn metadata(
    name: &str,
    title: Option<&str>,
    description: Option<&str>,
    mime: Option<&str>,
) -> Result<(), ResourceError> {
    field(name, 4096, "name")?;
    if let Some(title) = title {
        field(title, 4096, "title")?;
    }
    if let Some(description) = description {
        field(description, 16384, "description")?;
    }
    if let Some(mime) = mime {
        field(mime, 256, "mimeType")?;
    }
    Ok(())
}

pub(crate) fn read_params(params: &Value) -> Result<(), RpcError> {
    if !rpc::only(params, &["uri", "_meta"]) {
        return Err(invalid());
    }
    validate_uri(params["uri"].as_str().ok_or_else(invalid)?).map_err(|_| invalid())?;
    if params["_meta"]
        .get("progressToken")
        .is_some_and(|token| !(token.is_string() || token.is_i64() || token.is_u64()))
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> RpcError {
    RpcError::new(rpc::INVALID_PARAMS, "Invalid resource parameters")
}

/// Legacy protocol error for an unknown or inaccessible resource.
/// Prefer `stdio::Outcome::ResourceNotFound` in shared-driver handlers for
/// revision-aware replies. The driver also recognizes this exact error shape
/// on modern resource reads; other runtimes must select their revision's code.
#[must_use]
pub fn not_found(uri: &str) -> RpcError {
    RpcError::new(rpc::RESOURCE_NOT_FOUND, "Resource not found").with_data(json!({"uri": uri}))
}

/// Validated text or binary resource contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContents {
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    meta: Option<crate::Metadata>,
    uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mime_type: Option<String>,
    #[serde(flatten)]
    data: Data,
    #[serde(skip)]
    raw_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
enum Data {
    Text { text: String },
    Blob { blob: String },
}

impl ResourceContents {
    /// Builds bounded text contents.
    ///
    /// # Errors
    /// Returns [`ResourceError`] for invalid URI, MIME metadata or excessive data.
    pub fn text(
        uri: impl Into<String>,
        mime_type: Option<String>,
        text: impl Into<String>,
    ) -> Result<Self, ResourceError> {
        let text = text.into();
        Self::new(uri.into(), mime_type, text.len(), || Data::Text { text })
    }

    /// Encodes raw bytes as base64 after checking the raw size.
    ///
    /// # Errors
    /// Returns [`ResourceError`] for invalid URI, MIME metadata or excessive data.
    pub fn blob(
        uri: impl Into<String>,
        mime_type: Option<String>,
        bytes: &[u8],
    ) -> Result<Self, ResourceError> {
        Self::new(uri.into(), mime_type, bytes.len(), || Data::Blob {
            blob: crate::tool::base64(bytes),
        })
    }

    fn new(
        uri: String,
        mime_type: Option<String>,
        raw_bytes: usize,
        data: impl FnOnce() -> Data,
    ) -> Result<Self, ResourceError> {
        validate_uri(&uri)?;
        if let Some(mime) = &mime_type {
            field(mime, 256, "mimeType")?;
        }
        if raw_bytes > MAX_BYTES {
            return Err(ResourceError::Limit("content bytes"));
        }
        Ok(Self {
            uri,
            meta: None,
            mime_type,
            data: data(),
            raw_bytes,
        })
    }
}

impl<'de> Deserialize<'de> for ResourceContents {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Value::Object(mut fields) = Value::deserialize(deserializer)? else {
            return Err(de::Error::custom("resource contents must be an object"));
        };
        let string = |value, key| -> Result<String, D::Error> {
            match value {
                Some(Value::String(text)) => Ok(text),
                _ => Err(de::Error::custom(format!("invalid resource field: {key}"))),
            }
        };
        let uri = string(fields.remove("uri"), "uri")?;
        let mime_type = fields
            .remove("mimeType")
            .map(|value| string(Some(value), "mimeType"))
            .transpose()?;
        let meta = fields
            .remove("_meta")
            .map(serde_json::from_value::<crate::Metadata>)
            .transpose()
            .map_err(de::Error::custom)?;
        let mut result = match (fields.remove("text"), fields.remove("blob")) {
            (Some(text), None) => {
                Self::text(uri, mime_type, string(Some(text), "text")?).map_err(de::Error::custom)
            }
            (None, Some(blob)) => {
                let blob = string(Some(blob), "blob")?;
                let raw_bytes = base64_size(&blob).map_err(de::Error::custom)?;
                Self::new(uri, mime_type, raw_bytes, || Data::Blob { blob })
                    .map_err(de::Error::custom)
            }
            _ => Err(de::Error::custom(
                "resource requires exactly one of text or blob",
            )),
        }?;
        result.meta = meta;
        Ok(result)
    }
}

fn base64_size(blob: &str) -> Result<usize, ResourceError> {
    crate::content::decoded_size(blob, MAX_BYTES).map_err(|error| match error {
        crate::content::BinaryError::Invalid => ResourceError::Field("blob"),
        crate::content::BinaryError::TooLarge => ResourceError::Limit("content bytes"),
    })
}

/// A validated resource read result, serializable without an intermediate JSON tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadResourceResult {
    pub(crate) contents: Vec<ResourceContents>,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub(crate) meta: Option<crate::Metadata>,
}

impl ReadResourceResult {
    /// Checks nonempty content count and aggregate raw byte limits.
    ///
    /// # Errors
    /// Returns [`ResourceError::Limit`] for empty, excessive or oversized contents.
    pub fn new(contents: Vec<ResourceContents>) -> Result<Self, ResourceError> {
        if contents.is_empty() || contents.len() > MAX_CONTENTS {
            return Err(ResourceError::Limit("content count"));
        }
        if contents
            .iter()
            .map(|content| content.raw_bytes)
            .sum::<usize>()
            > MAX_BYTES
        {
            return Err(ResourceError::Limit("content bytes"));
        }
        Ok(Self {
            contents,
            meta: None,
        })
    }
    /// Attaches bounded custom metadata; the encoder enforces the whole frame budget.
    #[must_use]
    pub fn with_metadata(mut self, meta: crate::Metadata) -> Self {
        self.meta = Some(meta);
        self
    }
}

impl ResourceContents {
    /// Attaches immutable metadata to these contents.
    #[must_use]
    pub fn with_metadata(mut self, meta: crate::Metadata) -> Self {
        self.meta = Some(meta);
        self
    }
    /// Metadata retained with these resource bytes.
    #[must_use]
    pub fn metadata(&self) -> Option<&crate::Metadata> {
        self.meta.as_ref()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod content_tests;
