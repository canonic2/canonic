//! Tool definitions, catalog rules, call parameters and result content.

use crate::{
    rpc::{self, RpcError},
    schema,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashSet;

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
    /// The name callers use; see [`Catalog::new`] for the allowed characters.
    pub name: String,
    /// A display title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// One short sentence on what the tool returns or changes.
    pub description: String,
    /// An object schema for the arguments, using the [`schema`] subset.
    pub input_schema: Value,
    /// An optional object schema for `structuredContent`. It is published, not enforced.
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
    /// The input or output schema is not an object schema the validator supports.
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
}

impl Catalog {
    /// Validates and freezes `tools` in the given order.
    ///
    /// # Errors
    ///
    /// Returns the first invalid or repeated name, or a schema that is not an
    /// object schema. Input schemas must also pass [`schema::check`].
    pub fn new(tools: Vec<Tool>) -> Result<Self, CatalogError> {
        let mut names = HashSet::new();
        for tool in &tools {
            if !valid_name(&tool.name) {
                return Err(CatalogError::InvalidName(tool.name.clone()));
            }
            if !names.insert(tool.name.as_str()) {
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
            schema::check(&tool.input_schema)
                .map_err(|error| schema_error("inputSchema", error.to_string()))?;
            if let Some(output) = &tool.output_schema
                && output["type"] != "object"
            {
                return Err(schema_error("outputSchema", "must have type object".into()));
            }
        }
        Ok(Self { tools })
    }

    /// Finds a tool by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|tool| tool.name == name)
    }

    /// The tools in publication order.
    #[must_use]
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// The `tools/list` result.
    #[must_use]
    pub fn listing(&self) -> Value {
        json!({"tools": self.tools})
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

/// One content block of a tool result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    /// Plain text.
    Text {
        /// The text.
        text: String,
    },
    /// An inline image.
    Image {
        /// Base64-encoded bytes.
        data: String,
        /// The image's media type, such as `image/png`.
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
}

impl Content {
    /// A text block.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// An image block from raw bytes. Callers bound the bytes they read.
    #[must_use]
    pub fn image(bytes: &[u8], mime_type: impl Into<String>) -> Self {
        Self::Image {
            data: base64(bytes),
            mime_type: mime_type.into(),
        }
    }
}

/// The result of one tool call.
///
/// A tool's own failure, such as invalid arguments or a refused operation, is
/// a result with `is_error` set, so the caller's model can read it. Protocol
/// failures are [`RpcError`]s instead.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    /// Content blocks in display order.
    pub content: Vec<Content>,
    /// Machine-readable data matching the tool's output schema, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    /// Whether the tool failed.
    pub is_error: bool,
}

impl ToolResult {
    /// A successful result.
    #[must_use]
    pub fn success(content: Vec<Content>) -> Self {
        Self {
            content,
            structured_content: None,
            is_error: false,
        }
    }

    /// A failed result.
    #[must_use]
    pub fn error(content: Vec<Content>) -> Self {
        Self {
            content,
            structured_content: None,
            is_error: true,
        }
    }

    /// A result whose structured content is also its first text block, as JSON.
    #[must_use]
    pub fn structured(value: Value, is_error: bool) -> Self {
        Self {
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
}

pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |triple, (index, byte)| {
                triple | u32::from(*byte) << (16 - 8 * index)
            });
        for index in 0..4 {
            if index <= chunk.len() {
                let sextet = (triple >> (18 - 6 * index)) & 0x3f;
                text.push(char::from(ALPHABET[sextet as usize]));
            } else {
                text.push('=');
            }
        }
    }
    text
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
            json!({"type":"object","properties":{"id":{"type":"string","pattern":"^a"}}}),
        );
        assert!(matches!(
            Catalog::new(vec![pattern]),
            Err(CatalogError::Schema {
                field: "inputSchema",
                ..
            })
        ));
        let output = tool("acme").with_output_schema(json!({"type":"array"}));
        assert!(matches!(
            Catalog::new(vec![output]),
            Err(CatalogError::Schema {
                field: "outputSchema",
                ..
            })
        ));
    }

    #[test]
    fn catalog_refuses_reference_siblings_at_the_root_and_in_properties() {
        for schema in [
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
        ] {
            let tool = Tool::new("acme_status", "Return Acme status.", schema);
            assert!(matches!(
                Catalog::new(vec![tool]),
                Err(CatalogError::Schema {
                    field: "inputSchema",
                    ..
                })
            ));
        }
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
}
