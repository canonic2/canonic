//! Optional immutable prompt catalogs, checked string arguments and typed messages; no I/O.

use crate::{
    Content, Role, json,
    rpc::{self, RpcError},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Maximum number of prompt definitions.
pub const MAX_PROMPTS: usize = 1024;
/// Definitions per list page.
pub const PAGE_SIZE: usize = 64;
/// Maximum encoded catalog or prompt result bytes.
pub const MAX_BYTES: usize = 1024 * 1024;
/// Maximum declared arguments per prompt.
pub const MAX_ARGUMENTS: usize = 64;
/// Maximum UTF-8 bytes in one supplied argument.
pub const MAX_ARGUMENT_BYTES: usize = 65536;
/// Maximum encoded supplied argument map bytes.
pub const MAX_INPUT_BYTES: usize = 262_144;
/// Maximum messages in a result.
pub const MAX_MESSAGES: usize = 64;

/// Invalid prompt declaration or result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PromptError {
    /// A field has invalid syntax or length.
    #[error("invalid prompt field: {0}")]
    Field(&'static str),
    /// A declaration repeats a name.
    #[error("duplicate prompt name: {0}")]
    Duplicate(String),
    /// A collection or encoded representation exceeds its budget.
    #[error("prompt limit exceeded: {0}")]
    Limit(&'static str),
}

/// A string parameter, validated when its catalog is prepared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptArgument {
    /// Argument identity.
    pub name: String,
    /// Optional display title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional usage description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether callers must supply this argument, including an empty string.
    pub required: bool,
}

impl PromptArgument {
    /// Creates a parameter without display metadata.
    #[must_use]
    pub fn new(name: impl Into<String>, required: bool) -> Self {
        Self {
            name: name.into(),
            title: None,
            description: None,
            required,
        }
    }
}

/// A discoverable prompt definition; rendering belongs to the consumer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Prompt {
    /// Unique prompt identity.
    pub name: String,
    /// Optional display title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional usage description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Ordered parameter definitions.
    pub arguments: Vec<PromptArgument>,
}

impl Prompt {
    /// Creates a prompt with no parameters or display metadata.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: None,
            description: None,
            arguments: vec![],
        }
    }
}

/// Immutable catalog in declaration order, with prepared argument indexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCatalog {
    prompts: Vec<Prompt>,
    names: HashMap<String, usize>,
    arguments: Vec<HashSet<String>>,
    digest: String,
}

impl PromptCatalog {
    /// Validates definitions, uniqueness and budgets once.
    ///
    /// # Errors
    /// Returns [`PromptError`] for malformed, duplicate or excessive definitions.
    pub fn new(prompts: Vec<Prompt>) -> Result<Self, PromptError> {
        use sha2::{Digest, Sha256};
        if prompts.len() > MAX_PROMPTS {
            return Err(PromptError::Limit("catalog count"));
        }
        let mut names = HashMap::new();
        let mut arguments = Vec::with_capacity(prompts.len());
        for (index, prompt) in prompts.iter().enumerate() {
            metadata(
                &prompt.name,
                prompt.title.as_deref(),
                prompt.description.as_deref(),
            )?;
            if names.insert(prompt.name.clone(), index).is_some() {
                return Err(PromptError::Duplicate(prompt.name.clone()));
            }
            if prompt.arguments.len() > MAX_ARGUMENTS {
                return Err(PromptError::Limit("argument count"));
            }
            let mut seen = HashSet::new();
            for arg in &prompt.arguments {
                metadata(&arg.name, arg.title.as_deref(), arg.description.as_deref())?;
                if !seen.insert(arg.name.clone()) {
                    return Err(PromptError::Duplicate(arg.name.clone()));
                }
            }
            arguments.push(seen);
        }
        let bytes =
            json::encode(&prompts, MAX_BYTES).map_err(|_| PromptError::Limit("catalog bytes"))?;
        let digest = format!("{:x}", Sha256::digest(bytes));
        Ok(Self {
            prompts,
            names,
            arguments,
            digest,
        })
    }

    /// Lists a bounded page. Cursors are opaque and belong to this catalog.
    ///
    /// # Errors
    /// Returns invalid parameters for unsupported fields or invalid cursors.
    pub fn listing(&self, params: &Value) -> Result<Value, RpcError> {
        if !rpc::only(params, &["cursor", "_meta"]) {
            return Err(invalid("Invalid prompt listing parameters"));
        }
        let start = match params.get("cursor") {
            None => 0,
            Some(Value::String(cursor)) => {
                let prefix = format!("prompts:{}:", self.digest);
                let offset = cursor
                    .strip_prefix(&prefix)
                    .ok_or_else(|| invalid("Invalid prompt cursor"))?;
                let index = offset
                    .parse::<usize>()
                    .map_err(|_| invalid("Invalid prompt cursor"))?;
                if index == 0
                    || index >= self.prompts.len()
                    || !index.is_multiple_of(PAGE_SIZE)
                    || offset != index.to_string()
                {
                    return Err(invalid("Invalid prompt cursor"));
                }
                index
            }
            _ => return Err(invalid("Invalid prompt cursor")),
        };
        let end = (start + PAGE_SIZE).min(self.prompts.len());
        let mut result = json!({"prompts": &self.prompts[start..end]});
        if end < self.prompts.len() {
            result["nextCursor"] = json!(format!("prompts:{}:{end}", self.digest));
        }
        Ok(result)
    }

    /// Validates a get after revision-specific retry fields have been consumed.
    ///
    /// # Errors
    /// Returns invalid parameters for unknown names/arguments, missing required
    /// strings, malformed tokens or exceeded input budgets.
    pub fn call(&self, params: &Value) -> Result<PromptCall, RpcError> {
        if !rpc::only(params, &["name", "arguments", "_meta"]) {
            return Err(invalid("Invalid prompt parameters"));
        }
        let name = params["name"]
            .as_str()
            .ok_or_else(|| invalid("Invalid prompt name"))?;
        let index = *self
            .names
            .get(name)
            .ok_or_else(|| invalid("Unknown prompt"))?;
        let fields = match params.get("arguments") {
            None => None,
            Some(Value::Object(fields)) if fields.len() <= MAX_ARGUMENTS => Some(fields),
            _ => return Err(invalid("Prompt arguments must be a bounded string map")),
        };
        let mut arguments = BTreeMap::new();
        if let Some(fields) = fields {
            for (key, value) in fields {
                let value = value
                    .as_str()
                    .filter(|value| value.len() <= MAX_ARGUMENT_BYTES)
                    .ok_or_else(|| invalid("Prompt arguments must be bounded strings"))?;
                if !self.arguments[index].contains(key) {
                    return Err(invalid("Unknown prompt argument"));
                }
                arguments.insert(key.clone(), value.to_owned());
            }
        }
        if self.prompts[index]
            .arguments
            .iter()
            .any(|arg| arg.required && !arguments.contains_key(&arg.name))
        {
            return Err(invalid("Missing required prompt argument"));
        }
        json::encode(&arguments, MAX_INPUT_BYTES)
            .map_err(|_| invalid("Prompt arguments exceed input limit"))?;
        let progress_token = params["_meta"].get("progressToken").cloned();
        if progress_token
            .as_ref()
            .is_some_and(|token| !(token.is_string() || token.is_i64() || token.is_u64()))
        {
            return Err(invalid("Invalid prompt progress token"));
        }
        Ok(PromptCall {
            name: name.into(),
            arguments,
            progress_token,
        })
    }
}

/// A validated retrieval. Strings remain uninterpreted consumer input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCall {
    /// Known catalog name.
    pub name: String,
    /// Supplied arguments; omitted optional arguments stay absent.
    pub arguments: BTreeMap<String, String>,
    /// Optional progress correlation.
    pub progress_token: Option<Value>,
}

/// One user or assistant message containing one content block.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PromptMessage {
    /// Speaker.
    pub role: Role,
    /// Text, image, audio, resource link or embedded resource.
    pub content: Content,
}

/// Validated immutable prompt messages; encoding does not revalidate content.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GetPromptResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    messages: Vec<PromptMessage>,
    #[serde(skip)]
    legacy_compatible: bool,
}

impl GetPromptResult {
    /// Validates content, metadata, message count and aggregate encoded bytes.
    ///
    /// # Errors
    /// Returns [`PromptError`] for invalid images, fields or exceeded budgets.
    pub fn new(
        description: Option<String>,
        messages: Vec<PromptMessage>,
    ) -> Result<Self, PromptError> {
        if let Some(description) = &description {
            field(description, 16384, "description")?;
        }
        if messages.len() > MAX_MESSAGES {
            return Err(PromptError::Limit("message count"));
        }
        for message in &messages {
            if let Content::Image {
                data, mime_type, ..
            } = &message.content
            {
                if mime_type.len() > 256
                    || !mime_type.strip_prefix("image/").is_some_and(|subtype| {
                        !subtype.is_empty()
                            && subtype.bytes().all(|byte| {
                                byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
                            })
                    })
                {
                    return Err(PromptError::Field("image MIME type"));
                }
                crate::content::decoded_size(data, MAX_BYTES)
                    .map_err(|_| PromptError::Field("image data"))?;
            }
        }
        let legacy_compatible = messages
            .iter()
            .all(|m| !matches!(m.content, Content::ResourceLink { .. }));
        let result = Self {
            description,
            messages,
            legacy_compatible,
        };
        json::encode(&result, MAX_BYTES).map_err(|_| PromptError::Limit("result bytes"))?;
        Ok(result)
    }

    /// Encodes for a legacy client with its prompt-specific content union.
    ///
    /// # Errors
    /// Returns invalid for resource links, or the bounded/interruptible encoder's error.
    pub fn encode_legacy(
        &self,
        id: &Value,
        limit: usize,
        check: &dyn Fn() -> Result<(), crate::Interrupted>,
    ) -> Result<Vec<u8>, json::EncodeError> {
        if !self.legacy_compatible {
            return Err(json::EncodeError::Invalid);
        }
        rpc::encode_success(id, self, limit, check)
    }
}

fn metadata(name: &str, title: Option<&str>, description: Option<&str>) -> Result<(), PromptError> {
    field(name, 128, "name")?;
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(PromptError::Field("name"));
    }
    if let Some(title) = title {
        field(title, 4096, "title")?;
    }
    if let Some(description) = description {
        field(description, 16384, "description")?;
    }
    Ok(())
}

fn field(value: &str, limit: usize, name: &'static str) -> Result<(), PromptError> {
    if value.len() > limit || value.contains('\0') {
        return Err(PromptError::Field(name));
    }
    Ok(())
}

fn invalid(message: &str) -> RpcError {
    RpcError::new(rpc::INVALID_PARAMS, message)
}

#[cfg(test)]
mod tests;
