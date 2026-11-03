//! Tool definitions, catalog rules, call parameters and result content.

use crate::{
    rpc::{self, RpcError},
    schema,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
mod headers;
mod pagination;
pub use headers::MAX_PARAMETER_HEADERS;
pub(crate) use headers::{Kind as ParameterKind, ParameterHeader};
pub use pagination::{MAX_CURSOR_BYTES, PAGE_SIZE};

/// The longest tool name, in bytes.
pub const MAX_NAME_BYTES: usize = 128;

/// Hints a host may show or use to decide on approval. They grant nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotations {
    /// A display title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The tool changes nothing. The stdio driver lets read-only work finish
    /// after the connection closes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// The tool may delete or overwrite data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    /// Repeating the same call has no further effect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    /// The tool reaches systems outside the host and its project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

impl Annotations {
    fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// One published tool.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// Optional extension metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<crate::Metadata>,
    /// Optional validated display icons.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub icons: Vec<crate::Icon>,
    /// The name callers use; see [`Catalog::new`] for the allowed characters.
    pub name: String,
    /// A display title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// One short sentence on what the tool returns or changes.
    pub description: String,
    /// An object schema for arguments, checked and prepared at catalog construction.
    pub input_schema: Value,
    /// An optional JSON Schema object enforced on structured tool results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    /// Behavior hints.
    #[serde(skip_serializing_if = "Annotations::is_empty")]
    pub annotations: Annotations,
}

impl Tool {
    /// A tool with no title, output schema or annotations.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            meta: None,
            icons: vec![],
            title: None,
            description: description.into(),
            input_schema,
            output_schema: None,
            annotations: Annotations::default(),
        }
    }

    /// Sets the output schema.
    #[must_use]
    pub fn with_output_schema(mut self, schema: Value) -> Self {
        self.output_schema = Some(schema);
        self
    }

    /// Sets the annotations.
    #[must_use]
    pub fn with_annotations(mut self, annotations: Annotations) -> Self {
        self.annotations = annotations;
        self
    }

    /// Whether the tool is annotated as read-only.
    #[must_use]
    pub fn read_only(&self) -> bool {
        self.annotations.read_only_hint == Some(true)
    }
}

/// Why a set of tools cannot be published.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatalogError {
    /// The name is empty, too long or uses characters outside `A-Z a-z 0-9 _ - .`.
    #[error("tool name {0:?} is invalid")]
    InvalidName(String),
    /// Two tools share a name.
    #[error("tool name {0:?} is published twice")]
    Duplicate(String),
    /// The input or output is not a supported JSON Schema object.
    #[error("tool {tool:?} {field}: {problem}")]
    Schema {
        /// The tool's name.
        tool: String,
        /// `inputSchema` or `outputSchema`.
        field: &'static str,
        /// What is wrong.
        problem: String,
    },
}

/// The validated, immutable tool list of one server.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    tools: Vec<Tool>,
    names: HashMap<String, usize>,
    inputs: Vec<schema::PreparedSchema>,
    headers: Vec<Vec<ParameterHeader>>,
    outputs: Vec<ResultSchema>,
    pages: pagination::Pages,
}

impl Catalog {
    /// Validates and freezes `tools` in the given order.
    ///
    /// # Errors
    ///
    /// Returns the first invalid/repeated name or unsupported schema. Both schemas
    /// must be objects; input schemas must declare `type: "object"`. Each
    /// `x-mcp-header` annotation must be a nonempty HTTP token, unique ignoring
    /// case, on a `string`, `integer` or `boolean` property reached only through
    /// `properties`, with at most [`MAX_PARAMETER_HEADERS`] per tool.
    pub fn new(tools: Vec<Tool>) -> Result<Self, CatalogError> {
        let mut names = HashMap::new();
        let mut inputs = Vec::new();
        let mut headers = Vec::new();
        let mut outputs = Vec::new();
        for (index, tool) in tools.iter().enumerate() {
            crate::metadata::validate_icons(&tool.icons).map_err(|error| CatalogError::Schema {
                tool: tool.name.clone(),
                field: "icons",
                problem: error.to_string(),
            })?;
            if !valid_name(&tool.name) {
                return Err(CatalogError::InvalidName(tool.name.clone()));
            }
            if names.insert(tool.name.clone(), index).is_some() {
                return Err(CatalogError::Duplicate(tool.name.clone()));
            }
            let schema_error = |field, problem| CatalogError::Schema {
                tool: tool.name.clone(),
                field,
                problem,
            };
            if tool.input_schema["type"] != "object" {
                return Err(schema_error("inputSchema", "must have type object".into()));
            }
            inputs.push(
                schema::PreparedSchema::new(&tool.input_schema)
                    .map_err(|error| schema_error("inputSchema", error.to_string()))?,
            );
            headers.push(
                headers::prepare(&tool.input_schema)
                    .map_err(|problem| schema_error("inputSchema", problem))?,
            );
            let output = tool
                .output_schema
                .as_ref()
                .map(|output| {
                    if !output.is_object() {
                        return Err(schema_error(
                            "outputSchema",
                            "must be a JSON Schema object".into(),
                        ));
                    }
                    schema::PreparedSchema::new(output)
                        .map_err(|error| schema_error("outputSchema", error.to_string()))
                })
                .transpose()?;
            outputs.push(ResultSchema {
                schema: output,
                legacy: tool
                    .output_schema
                    .as_ref()
                    .is_none_or(|output| output["type"] == "object"),
            });
        }
        let pages = pagination::Pages::new(&tools, &outputs);
        Ok(Self {
            tools,
            names,
            inputs,
            headers,
            outputs,
            pages,
        })
    }

    /// The prepared `x-mcp-header` annotations of a tool.
    pub(crate) fn parameter_headers(&self, name: &str) -> Option<&[ParameterHeader]> {
        self.names
            .get(name)
            .map(|index| self.headers[*index].as_slice())
    }

    /// Finds a tool by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Tool> {
        self.names.get(name).map(|index| &self.tools[*index])
    }

    /// The tools in publication order.
    #[must_use]
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// The full modern-compatible catalog, without pagination or a wire envelope.
    #[must_use]
    pub fn listing(&self) -> Value {
        json!({"tools": self.tools})
    }

    /// Returns one `tools/list` page for a supported revision, before envelope encoding.
    /// Legacy views omit tools with incompatible output schemas. Cursors belong
    /// to this catalog and revision; callers forward them unchanged.
    ///
    /// # Errors
    /// Returns invalid parameters for unknown fields, unsupported revisions or
    /// malformed, foreign or out-of-range cursors.
    pub fn list_page(&self, params: &Value, protocol_version: &str) -> Result<Value, RpcError> {
        self.pages.page(self, params, protocol_version)
    }

    /// Captures an immutable output contract for ordinary calls and durable task settlement.
    #[must_use]
    pub fn result_schema(&self, name: &str) -> Option<ResultSchema> {
        self.names
            .get(name)
            .map(|index| self.outputs[*index].clone())
    }

    pub(crate) fn validate_arguments(
        &self,
        name: &str,
        value: &Value,
    ) -> Result<(), schema::ArgumentError> {
        let index = self.names.get(name).expect("admitted catalog tool");
        self.inputs[*index].validate(value)
    }
}

/// Prepared tool-result contract. Consumers retain it with a task's producer identity.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultSchema {
    schema: Option<schema::PreparedSchema>,
    legacy: bool,
}

impl ResultSchema {
    /// Whether this contract can be advertised to legacy clients.
    #[must_use]
    pub fn legacy_compatible(&self) -> bool {
        self.legacy
    }

    /// Validates a modern tool result. Error results may omit structured content.
    ///
    /// # Errors
    /// Returns a missing/mismatched structured result or resource-budget refusal.
    pub fn validate(&self, result: &ToolResult) -> Result<(), schema::ArgumentError> {
        self.validate_checked(result, true, &|| Ok(()))
    }

    /// Validates and encodes a legacy result, retaining object-shaped structured data.
    ///
    /// # Errors
    /// Returns an invalid-result, interruption or bounded-encoding error.
    pub fn encode_legacy(
        &self,
        id: &Value,
        result: &ToolResult,
        limit: usize,
        check: &dyn Fn() -> Result<(), crate::Interrupted>,
    ) -> Result<Vec<u8>, crate::json::EncodeError> {
        self.validate_checked(result, false, check)
            .map_err(|error| {
                if error.kind == schema::ArgumentErrorKind::Interrupted {
                    crate::json::EncodeError::Interrupted
                } else if error.kind == schema::ArgumentErrorKind::Limit {
                    crate::json::EncodeError::ValidationLimit
                } else {
                    crate::json::EncodeError::Invalid
                }
            })?;
        rpc::encode_success(id, result, limit, check)
    }

    pub(crate) fn validate_checked(
        &self,
        result: &ToolResult,
        modern: bool,
        check: &dyn Fn() -> Result<(), crate::Interrupted>,
    ) -> Result<(), schema::ArgumentError> {
        if !modern
            && (!self.legacy
                || result
                    .structured_content
                    .as_ref()
                    .is_some_and(|value| !value.is_object()))
        {
            return Err(schema::ArgumentError::new(
                "structuredContent",
                "structuredContent must be an object for legacy calls".into(),
            ));
        }
        self.validate_fields(result.structured_content.as_ref(), result.is_error, check)
    }

    fn validate_fields(
        &self,
        value: Option<&Value>,
        is_error: bool,
        check: &dyn Fn() -> Result<(), crate::Interrupted>,
    ) -> Result<(), schema::ArgumentError> {
        check().map_err(|_| {
            schema::ArgumentError::refusal(
                "structuredContent",
                schema::ArgumentErrorKind::Interrupted,
            )
        })?;
        if let Some(schema) = &self.schema {
            if let Some(value) = value {
                schema.validate_checked(value, "structuredContent", check)?;
            } else if !is_error {
                return Err(schema::ArgumentError::new(
                    "structuredContent",
                    "structuredContent is required by the output schema".into(),
                ));
            }
        }
        Ok(())
    }

    /// Checks a completed task snapshot before persistence or after restore.
    /// The consumer binds this contract to the task's producing tool; management
    /// requests alone cannot identify that tool.
    ///
    /// # Errors
    /// Returns missing/mismatched structured data or a validation-budget refusal.
    pub fn validate_snapshot(
        &self,
        snapshot: &crate::task::Snapshot,
    ) -> Result<(), schema::ArgumentError> {
        if let Some(result) = snapshot.completed_result() {
            self.validate_fields(
                result.get("structuredContent"),
                result["isError"] == true,
                &|| Ok(()),
            )?;
        }
        Ok(())
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

/// The parameters of one `tools/call` request.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    /// The tool's name; the session has checked it is in the catalog.
    pub name: String,
    /// The arguments object; `{}` when absent.
    pub arguments: Value,
    /// The caller's progress token, a string or an integer.
    pub progress_token: Option<Value>,
}

impl ToolCall {
    /// Reads `tools/call` parameters.
    ///
    /// # Errors
    ///
    /// Returns an [`rpc::INVALID_PARAMS`] error for fields other than `name`,
    /// `arguments` and `_meta`, a non-string name, non-object arguments or a
    /// malformed progress token.
    pub fn from_params(params: &Value) -> Result<Self, RpcError> {
        let invalid = || RpcError::new(rpc::INVALID_PARAMS, "Invalid tool call parameters");
        if !rpc::only(params, &["name", "arguments", "_meta"]) {
            return Err(invalid());
        }
        let Some(name) = params["name"].as_str() else {
            return Err(invalid());
        };
        let arguments = match params.get("arguments") {
            None => json!({}),
            Some(arguments @ Value::Object(_)) => arguments.clone(),
            Some(_) => return Err(invalid()),
        };
        let progress_token = match params["_meta"].get("progressToken") {
            None => None,
            Some(token) if token.is_string() || token.is_i64() || token.is_u64() => {
                Some(token.clone())
            }
            Some(_) => return Err(invalid()),
        };
        Ok(Self {
            name: name.to_owned(),
            arguments,
            progress_token,
        })
    }
}

/// One supported content block of a tool result. Deserialization ignores extra
/// object fields; unsupported content variants are refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    /// Plain text.
    Text {
        /// The text.
        text: String,
        /// Optional extension metadata.
        #[serde(
            default,
            rename = "_meta",
            deserialize_with = "crate::metadata::optional",
            skip_serializing_if = "Option::is_none"
        )]
        meta: Option<crate::Metadata>,
        /// Optional audience, priority and modification hints.
        #[serde(
            default,
            deserialize_with = "crate::content::annotations",
            skip_serializing_if = "Option::is_none"
        )]
        annotations: Option<crate::ContentAnnotations>,
    },
    /// An inline image.
    Image {
        /// Base64-encoded bytes.
        data: String,
        /// The image's media type, such as `image/png`.
        #[serde(rename = "mimeType")]
        mime_type: String,
        /// Optional extension metadata.
        #[serde(
            default,
            rename = "_meta",
            deserialize_with = "crate::metadata::optional",
            skip_serializing_if = "Option::is_none"
        )]
        meta: Option<crate::Metadata>,
        /// Optional audience, priority and modification hints.
        #[serde(
            default,
            deserialize_with = "crate::content::annotations",
            skip_serializing_if = "Option::is_none"
        )]
        annotations: Option<crate::ContentAnnotations>,
    },
    /// Inline audio; the consumer supplies the bytes and MIME type.
    Audio {
        /// Validated base64 data and MIME metadata.
        #[serde(flatten)]
        audio: crate::AudioContent,
        /// Optional extension metadata.
        #[serde(
            default,
            rename = "_meta",
            deserialize_with = "crate::metadata::optional",
            skip_serializing_if = "Option::is_none"
        )]
        meta: Option<crate::Metadata>,
        /// Optional audience, priority and modification hints.
        #[serde(
            default,
            deserialize_with = "crate::content::annotations",
            skip_serializing_if = "Option::is_none"
        )]
        annotations: Option<crate::ContentAnnotations>,
    },
    /// A reference to a resource the host may retrieve separately.
    #[serde(rename = "resource_link")]
    ResourceLink {
        /// Validated URI and display metadata, flattened into this block.
        #[serde(flatten)]
        link: crate::resource::ResourceLink,
    },
    /// Inline text or binary resource contents with their URI.
    Resource {
        /// Validated contents; construction performs no I/O.
        resource: crate::resource::ResourceContents,
        /// Optional extension metadata.
        #[serde(
            default,
            rename = "_meta",
            deserialize_with = "crate::metadata::optional",
            skip_serializing_if = "Option::is_none"
        )]
        meta: Option<crate::Metadata>,
        /// Hints on the enclosing block, rather than nested resource contents.
        #[serde(
            default,
            deserialize_with = "crate::content::annotations",
            skip_serializing_if = "Option::is_none"
        )]
        annotations: Option<crate::ContentAnnotations>,
    },
}

impl Content {
    /// Attaches immutable extension metadata to this content block.
    #[must_use]
    pub fn with_metadata(mut self, metadata: crate::Metadata) -> Self {
        match &mut self {
            Self::Text { meta, .. }
            | Self::Image { meta, .. }
            | Self::Audio { meta, .. }
            | Self::Resource { meta, .. } => *meta = Some(metadata),
            Self::ResourceLink { link } => link.set_metadata(metadata),
        }
        self
    }
    /// Extension metadata on this block, distinct from embedded resource metadata.
    #[must_use]
    pub fn metadata(&self) -> Option<&crate::Metadata> {
        match self {
            Self::Text { meta, .. }
            | Self::Image { meta, .. }
            | Self::Audio { meta, .. }
            | Self::Resource { meta, .. } => meta.as_ref(),
            Self::ResourceLink { link } => link.resource().meta.as_ref(),
        }
    }
    /// An audio block from bounded raw bytes, without decoding its container.
    ///
    /// # Errors
    /// Returns [`crate::ContentError`] for excessive audio bytes or invalid MIME metadata.
    pub fn audio(bytes: &[u8], mime_type: impl Into<String>) -> Result<Self, crate::ContentError> {
        Ok(Self::Audio {
            audio: crate::AudioContent::new(bytes, mime_type)?,
            meta: None,
            annotations: None,
        })
    }

    /// Attaches validated presentation hints, replacing any existing hints.
    #[must_use]
    pub fn with_annotations(mut self, hints: crate::ContentAnnotations) -> Self {
        match &mut self {
            Self::Text { annotations, .. }
            | Self::Image { annotations, .. }
            | Self::Audio { annotations, .. }
            | Self::Resource { annotations, .. } => *annotations = Some(hints),
            Self::ResourceLink { link } => link.set_annotations(hints),
        }
        self
    }

    /// Returns presentation hints when present.
    #[must_use]
    pub fn annotations(&self) -> Option<&crate::ContentAnnotations> {
        match self {
            Self::Text { annotations, .. }
            | Self::Image { annotations, .. }
            | Self::Audio { annotations, .. }
            | Self::Resource { annotations, .. } => annotations.as_ref(),
            Self::ResourceLink { link } => link.resource().annotations.as_ref(),
        }
    }

    /// A resource link independent of the resource catalog.
    ///
    /// # Errors
    /// Returns [`crate::resource::ResourceError`] for invalid URI or metadata.
    pub fn resource_link(
        resource: crate::resource::Resource,
    ) -> Result<Self, crate::resource::ResourceError> {
        Ok(Self::ResourceLink {
            link: crate::resource::ResourceLink::new(resource)?,
        })
    }

    /// Embeds already validated resource contents without fetching their URI.
    #[must_use]
    pub fn resource(resource: crate::resource::ResourceContents) -> Self {
        Self::Resource {
            resource,
            meta: None,
            annotations: None,
        }
    }

    /// A text block.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            meta: None,
            annotations: None,
        }
    }

    /// An image block from raw bytes. Callers bound the bytes they read.
    #[must_use]
    pub fn image(bytes: &[u8], mime_type: impl Into<String>) -> Self {
        Self::Image {
            data: base64(bytes),
            mime_type: mime_type.into(),
            meta: None,
            annotations: None,
        }
    }
}

/// The result of one tool call.
///
/// A tool's own failure, such as invalid arguments or a refused operation, is
/// a result with `is_error` set, so the caller's model can read it. Protocol
/// failures are [`RpcError`]s instead.
/// Deserialization defaults an absent `isError` to false and ignores extra
/// object fields. This payload type is not a lossless protocol-frame decoder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    /// Optional result metadata, distinct from structured tool data.
    #[serde(
        default,
        rename = "_meta",
        deserialize_with = "crate::metadata::optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub meta: Option<crate::Metadata>,
    /// Content blocks in display order.
    pub content: Vec<Content>,
    /// Machine-readable data matching the tool's output schema, when it has one.
    #[serde(
        default,
        deserialize_with = "structured_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub structured_content: Option<Value>,
    /// Whether the tool failed.
    #[serde(default)]
    pub is_error: bool,
}

impl ToolResult {
    /// A successful result.
    #[must_use]
    pub fn success(content: Vec<Content>) -> Self {
        Self {
            meta: None,
            content,
            structured_content: None,
            is_error: false,
        }
    }

    /// A failed result.
    #[must_use]
    pub fn error(content: Vec<Content>) -> Self {
        Self {
            meta: None,
            content,
            structured_content: None,
            is_error: true,
        }
    }

    /// A result whose structured content is also its first text block, as JSON.
    #[must_use]
    pub fn structured(value: Value, is_error: bool) -> Self {
        Self {
            meta: None,
            content: vec![Content::text(value.to_string())],
            structured_content: Some(value),
            is_error,
        }
    }

    /// Appends a content block.
    #[must_use]
    pub fn with_content(mut self, content: Content) -> Self {
        self.content.push(content);
        self
    }
    /// Attaches immutable custom result metadata; modern encoders refuse owned protocol keys.
    #[must_use]
    pub fn with_metadata(mut self, meta: crate::Metadata) -> Self {
        self.meta = Some(meta);
        self
    }
}

pub(crate) use crate::content::base64;

fn structured_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object() -> Value {
        json!({"type":"object","additionalProperties":false})
    }

    #[test]
    fn catalog_refuses_bad_names_duplicates_and_unsupported_schemas() {
        let tool = |name: &str| Tool::new(name, "Return Acme data.", object());
        assert!(Catalog::new(vec![tool("acme_status"), tool("acme.v2-list")]).is_ok());
        for name in [
            "",
            "acme status",
            "acme/status",
            &"a".repeat(MAX_NAME_BYTES + 1),
        ] {
            assert_eq!(
                Catalog::new(vec![tool(name)]),
                Err(CatalogError::InvalidName(name.into()))
            );
        }
        assert_eq!(
            Catalog::new(vec![tool("acme"), tool("acme")]),
            Err(CatalogError::Duplicate("acme".into()))
        );
        let untyped = Tool::new("acme", "Return Acme data.", json!({}));
        assert!(matches!(
            Catalog::new(vec![untyped]),
            Err(CatalogError::Schema {
                field: "inputSchema",
                ..
            })
        ));
        let pattern = Tool::new(
            "acme",
            "Return Acme data.",
            json!({"type":"object","properties":{"id":{"type":"string","pattern":"("}}}),
        );
        assert!(matches!(
            Catalog::new(vec![pattern]),
            Err(CatalogError::Schema {
                field: "inputSchema",
                ..
            })
        ));
        let output = tool("acme").with_output_schema(json!([]));
        assert!(matches!(
            Catalog::new(vec![output]),
            Err(CatalogError::Schema {
                field: "outputSchema",
                ..
            })
        ));
    }

    #[test]
    fn catalog_prepares_parameter_headers_and_refuses_invalid_annotations() {
        let tool = |properties: Value| {
            Tool::new(
                "acme_deploy",
                "Deploy an Acme service.",
                json!({"type":"object","properties":properties}),
            )
        };
        let catalog = Catalog::new(vec![tool(json!({
            "region": {"type":"string","x-mcp-header":"Region"},
            "target": {"type":"object","properties":{
                "replicas": {"type":["integer"],"x-mcp-header":"Replicas"},
                "dry": {"type":"boolean","x-mcp-header":"Dry-Run"}
            }},
            "note": {"type":"string","default":{"x-mcp-header":"Ignored"}}
        }))])
        .unwrap();
        let headers = catalog.parameter_headers("acme_deploy").unwrap();
        let prepared: Vec<_> = headers
            .iter()
            .map(|header| (header.key.as_str(), header.path.join("."), header.kind))
            .collect();
        assert_eq!(
            prepared,
            [
                ("region", "region".to_owned(), ParameterKind::String),
                ("dry-run", "target.dry".to_owned(), ParameterKind::Boolean),
                (
                    "replicas",
                    "target.replicas".to_owned(),
                    ParameterKind::Integer
                ),
            ]
        );
        let too_many: serde_json::Map<String, Value> = (0..=MAX_PARAMETER_HEADERS)
            .map(|index| {
                (
                    format!("p{index}"),
                    json!({"type":"string","x-mcp-header":format!("P{index}")}),
                )
            })
            .collect();
        for properties in [
            json!({"a":{"type":"string","x-mcp-header":""}}),
            json!({"a":{"type":"string","x-mcp-header":"Acme Region"}}),
            json!({"a":{"type":"string","x-mcp-header":7}}),
            json!({"a":{"type":"number","x-mcp-header":"A"}}),
            json!({"a":{"type":["string","null"],"x-mcp-header":"A"}}),
            json!({"a":{"x-mcp-header":"A"}}),
            json!({"a":{"type":"string","x-mcp-header":"A"},"b":{"type":"string","x-mcp-header":"a"}}),
            json!({"a":{"type":"array","items":{"type":"string","x-mcp-header":"A"}}}),
            json!({"a":{"anyOf":[{"type":"string","x-mcp-header":"A"}]}}),
            Value::Object(too_many),
        ] {
            assert!(
                matches!(
                    Catalog::new(vec![tool(properties.clone())]),
                    Err(CatalogError::Schema { ref tool, field: "inputSchema", .. }) if tool == "acme_deploy"
                ),
                "{properties}"
            );
        }
        let root = Tool::new(
            "acme_deploy",
            "Deploy an Acme service.",
            json!({"type":"object","x-mcp-header":"Root"}),
        );
        assert!(Catalog::new(vec![root]).is_err());
    }

    #[test]
    fn schema_catalog_enforces_reference_siblings_at_root_and_in_properties() {
        for (index, schema) in [
            json!({
                "type": "object",
                "$defs": {"id": {"type": "string"}},
                "$ref": "#/$defs/id"
            }),
            json!({
                "type": "object",
                "$defs": {"id": {"type": "string"}},
                "properties": {"id": {"$ref": "#/$defs/id", "maxLength": 8}}
            }),
        ]
        .into_iter()
        .enumerate()
        {
            let tool = Tool::new("acme_status", "Return Acme status.", schema);
            let catalog = Catalog::new(vec![tool]).unwrap();
            assert!(
                catalog
                    .validate_arguments("acme_status", &json!({"id":"acme-long"}))
                    .is_err()
            );
            assert_eq!(
                catalog
                    .validate_arguments("acme_status", &json!({"id":"acme"}))
                    .is_ok(),
                index == 1
            );
        }
    }

    #[test]
    fn catalog_lookup_keeps_definition_and_contract_together_across_clones_and_reordering() {
        let definitions = vec![
            Tool::new("acme_z", "Return a string.", object())
                .with_output_schema(json!({"type":"string"})),
            Tool::new("acme_a", "Return an integer.", object())
                .with_output_schema(json!({"type":"integer"})),
        ];
        let original = Catalog::new(definitions.clone()).unwrap();
        let retained = original.result_schema("acme_z").unwrap();
        let mut reordered = definitions;
        reordered.reverse();
        let replacement = Catalog::new(reordered).unwrap();
        for catalog in [original.clone(), replacement] {
            assert!(catalog.get("acme_missing").is_none());
            assert!(catalog.result_schema("acme_missing").is_none());
            for (name, valid, invalid) in [
                ("acme_z", json!("Acme"), json!(1)),
                ("acme_a", json!(1), json!("Acme")),
            ] {
                assert_eq!(catalog.get(name).unwrap().name, name);
                let contract = catalog.result_schema(name).unwrap();
                assert!(
                    contract
                        .validate(&ToolResult::structured(valid, false))
                        .is_ok()
                );
                assert!(
                    contract
                        .validate(&ToolResult::structured(invalid, false))
                        .is_err()
                );
                catalog.validate_arguments(name, &json!({})).unwrap();
                assert!(
                    catalog
                        .validate_arguments(name, &json!({"extra":true}))
                        .is_err()
                );
            }
        }
        assert!(
            retained
                .validate(&ToolResult::structured(json!("Acme"), false))
                .is_ok()
        );
        assert_eq!(original.tools()[0].name, "acme_z");
    }

    #[test]
    fn schema_result_contracts_validate_and_keep_explicit_null() {
        let catalog = Catalog::new(vec![
            Tool::new("acme_object", "Return object.", object()).with_output_schema(json!({"type":"object","properties":{"passed":{"type":"boolean"}},"required":["passed"],"additionalProperties":false})),
            Tool::new("acme_array", "Return array.", object()).with_output_schema(json!({"type":"array","items":{"type":"integer"}})),
            Tool::new("acme_null", "Return null.", object()).with_output_schema(json!({"type":"null"})),
        ]).unwrap();
        assert_eq!(
            catalog
                .list_page(&json!({}), crate::LATEST_PROTOCOL_VERSION)
                .unwrap()["tools"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let output = catalog.result_schema("acme_object").unwrap();
        assert!(
            output
                .validate(&ToolResult::structured(json!({"passed":true}), false))
                .is_ok()
        );
        for result in [
            ToolResult::success(vec![]),
            ToolResult::structured(json!({"passed":"yes"}), false),
            ToolResult::structured(json!({"other":true}), false),
        ] {
            assert!(output.validate(&result).is_err());
        }
        assert!(
            output
                .validate(&ToolResult::error(vec![Content::text("Refused")]))
                .is_ok()
        );
        assert!(
            output
                .validate(&ToolResult::structured(json!({"passed":"yes"}), true))
                .is_err()
        );
        let array = catalog.result_schema("acme_array").unwrap();
        let result = ToolResult::structured(json!([1, 2]), false);
        assert!(array.validate(&result).is_ok());
        assert_eq!(
            array.encode_legacy(&json!(1), &result, 4096, &|| Ok(())),
            Err(crate::json::EncodeError::Invalid)
        );
        let result = ToolResult::structured(Value::Null, false);
        let restored: ToolResult = serde_json::from_value(json!(&result)).unwrap();
        assert_eq!(restored.structured_content, Some(Value::Null));
        assert!(
            catalog
                .result_schema("acme_null")
                .unwrap()
                .validate(&restored)
                .is_ok()
        );
        assert_eq!(
            output.encode_legacy(
                &json!(1),
                &ToolResult::structured(json!({"passed":true}), false),
                4096,
                &|| Err(crate::Interrupted)
            ),
            Err(crate::json::EncodeError::Interrupted)
        );
    }

    #[test]
    fn listing_uses_protocol_field_names_and_omits_empty_fields() {
        let catalog = Catalog::new(vec![
            Tool::new("acme_status", "Return Acme status.", object()),
            Tool::new("acme_stop", "Stop an Acme service.", object()).with_annotations(
                Annotations {
                    read_only_hint: Some(false),
                    destructive_hint: Some(true),
                    ..Annotations::default()
                },
            ),
        ])
        .unwrap();
        assert_eq!(
            catalog.listing(),
            json!({"tools":[
                {"name":"acme_status","description":"Return Acme status.","inputSchema":object()},
                {"name":"acme_stop","description":"Stop an Acme service.","inputSchema":object(),
                 "annotations":{"readOnlyHint":false,"destructiveHint":true}}
            ]})
        );
        assert!(!catalog.get("acme_stop").unwrap().read_only());
    }

    #[test]
    fn call_parameters_are_strict() {
        let call = ToolCall::from_params(
            &json!({"name":"acme","arguments":{"a":1},"_meta":{"progressToken":"p1"}}),
        )
        .unwrap();
        assert_eq!(call.arguments, json!({"a":1}));
        assert_eq!(call.progress_token, Some(json!("p1")));
        assert_eq!(
            ToolCall::from_params(&json!({"name":"acme"}))
                .unwrap()
                .arguments,
            json!({})
        );
        for params in [
            json!({"name":"acme","extra":true}),
            json!({"name":1}),
            json!({"name":"acme","arguments":[]}),
            json!({"name":"acme","_meta":[]}),
            json!({"name":"acme","_meta":{"progressToken":1.5}}),
        ] {
            assert_eq!(
                ToolCall::from_params(&params).unwrap_err().code,
                rpc::INVALID_PARAMS
            );
        }
    }

    #[test]
    fn results_encode_text_images_and_structured_content() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        let result = ToolResult::structured(json!({"ok":true}), false)
            .with_content(Content::image(b"foo", "image/png"));
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({"content":[{"type":"text","text":"{\"ok\":true}"},{"type":"image","data":"Zm9v","mimeType":"image/png"}],
                   "structuredContent":{"ok":true},"isError":false})
        );
        assert_eq!(
            serde_json::to_value(ToolResult::error(vec![Content::text("No Acme service")]))
                .unwrap(),
            json!({"content":[{"type":"text","text":"No Acme service"}],"isError":true})
        );
    }

    #[test]
    fn results_deserialize_optional_error_and_ignore_extra_payload_fields() {
        let result: ToolResult = serde_json::from_value(json!({"content":[]})).unwrap();
        assert_eq!(result, ToolResult::success(vec![]));
        let result: ToolResult = serde_json::from_value(json!({
            "content":[{"type":"text","text":"Acme","extra":true}],
            "isError":true,"extra":{"future":true}
        }))
        .unwrap();
        assert_eq!(result, ToolResult::error(vec![Content::text("Acme")]));
        assert!(
            serde_json::from_value::<ToolResult>(json!({"content":[],"isError":null})).is_err()
        );
        assert!(
            serde_json::from_value::<Content>(json!({"type":"unsupported","extra":true})).is_err()
        );
    }
}
