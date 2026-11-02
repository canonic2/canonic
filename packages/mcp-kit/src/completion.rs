//! Catalog-checked completion requests and immutable bounded suggestions; no I/O.

use crate::{
    PromptCatalog, ResourceCatalog, json,
    rpc::{self, RpcError},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Maximum suggestions in one response, as required by the protocol.
pub const MAX_VALUES: usize = 100;
/// Maximum UTF-8 bytes per suggestion.
pub const MAX_VALUE_BYTES: usize = 4096;
/// Maximum partial/context string bytes.
pub const MAX_INPUT_VALUE_BYTES: usize = 65536;
/// Maximum contextual arguments.
pub const MAX_CONTEXT_ARGUMENTS: usize = 64;
/// Maximum encoded request fields (excluding metadata) or result bytes.
pub const MAX_BYTES: usize = 262_144;
/// Largest integer total represented exactly by all JSON consumers.
pub const MAX_TOTAL: u64 = (1_u64 << 53) - 1;

/// Published prompt or resource-template identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum CompletionReference {
    /// A published prompt.
    #[serde(rename = "ref/prompt")]
    Prompt {
        /// Exact prompt name.
        name: String,
        /// Optional caller-supplied display label, without authority.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// An exact published template address, not an expanded resource URI.
    #[serde(rename = "ref/resource")]
    Resource {
        /// URI template including expressions.
        uri: String,
    },
}

/// An argument being filled in, distinct from resolved context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionArgument {
    /// Declared argument or template variable name.
    pub name: String,
    /// Partial value, retained exactly.
    pub value: String,
}

/// A catalog-checked completion request. Parsing does not resolve any data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteRequest {
    /// Published target identity.
    pub reference: CompletionReference,
    /// Active field and partial value.
    pub argument: CompletionArgument,
    /// Supplied resolved fields of the same target.
    pub context: BTreeMap<String, String>,
    /// Optional validated progress token.
    pub progress_token: Option<Value>,
}

#[derive(Serialize)]
struct InputFields<'a> {
    #[serde(rename = "ref")]
    reference: &'a Value,
    argument: &'a Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<&'a Value>,
}

impl CompleteRequest {
    /// Checks wire shapes, published targets, declared arguments and budgets.
    ///
    /// # Errors
    /// Returns invalid parameters for malformed, unknown or excessive input.
    pub fn parse(
        params: &Value,
        prompts: Option<&PromptCatalog>,
        resources: Option<&ResourceCatalog>,
    ) -> Result<Self, RpcError> {
        if !rpc::only(params, &["ref", "argument", "context", "_meta"]) {
            return Err(invalid("Invalid completion parameters"));
        }
        let reference: CompletionReference = serde_json::from_value(params["ref"].clone())
            .map_err(|_| invalid("Invalid completion reference"))?;
        if params["ref"]
            .get("title")
            .is_some_and(|title| !title.is_string())
        {
            return Err(invalid("Invalid completion reference title"));
        }
        let names = match &reference {
            CompletionReference::Prompt { name, title } => {
                if title
                    .as_ref()
                    .is_some_and(|title| title.len() > 4096 || title.contains('\0'))
                {
                    return Err(invalid("Invalid completion reference title"));
                }
                prompts.and_then(|catalog| catalog.argument_names(name))
            }
            CompletionReference::Resource { uri } => {
                resources.and_then(|catalog| catalog.argument_names(uri))
            }
        }
        .ok_or_else(|| invalid("Unknown completion target"))?;
        let argument: CompletionArgument = serde_json::from_value(params["argument"].clone())
            .map_err(|_| invalid("Invalid completion argument"))?;
        if !names.contains(&argument.name) || argument.value.len() > MAX_INPUT_VALUE_BYTES {
            return Err(invalid("Unknown or excessive completion argument"));
        }
        let context = context(params.get("context"))?;
        if context.keys().any(|name| !names.contains(name)) {
            return Err(invalid("Unknown completion context argument"));
        }
        json::measure(
            &InputFields {
                reference: &params["ref"],
                argument: &params["argument"],
                context: params.get("context"),
            },
            MAX_BYTES,
            &|| Ok(()),
        )
        .map_err(|_| invalid("Completion input exceeds its byte limit"))?;
        let progress_token = params["_meta"].get("progressToken").cloned();
        if progress_token
            .as_ref()
            .is_some_and(|token| !(token.is_string() || token.is_i64() || token.is_u64()))
        {
            return Err(invalid("Invalid completion progress token"));
        }
        Ok(Self {
            reference,
            argument,
            context,
            progress_token,
        })
    }
}

fn context(value: Option<&Value>) -> Result<BTreeMap<String, String>, RpcError> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    if !rpc::only(value, &["arguments"]) {
        return Err(invalid("Invalid completion context"));
    }
    let Some(arguments) = value.get("arguments") else {
        return Ok(BTreeMap::new());
    };
    let arguments = arguments
        .as_object()
        .filter(|args| args.len() <= MAX_CONTEXT_ARGUMENTS)
        .ok_or_else(|| invalid("Invalid completion context arguments"))?;
    arguments
        .iter()
        .map(|(name, value)| {
            let value = value
                .as_str()
                .filter(|value| value.len() <= MAX_INPUT_VALUE_BYTES)
                .ok_or_else(|| invalid("Invalid completion context value"))?;
            Ok((name.clone(), value.to_owned()))
        })
        .collect()
}

/// Invalid completion results, refused rather than truncated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompletionError {
    /// The value collection or representation exceeds its budget.
    #[error("completion limit exceeded: {0}")]
    Limit(&'static str),
    /// Optional total and hasMore disagree with the returned values.
    #[error("inconsistent completion counts")]
    Counts,
}

/// Validated immutable completion result, preserving consumer relevance order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompleteResult {
    completion: Suggestions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Suggestions {
    values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    has_more: Option<bool>,
}

impl CompleteResult {
    /// Checks count, per-value/aggregate budgets and optional count consistency.
    /// Empty arrays and unknown totals are valid; values are never truncated.
    ///
    /// # Errors
    /// Returns [`CompletionError`] for exceeded bounds or inconsistent counts.
    pub fn new(
        values: Vec<String>,
        total: Option<u64>,
        has_more: Option<bool>,
    ) -> Result<Self, CompletionError> {
        if values.len() > MAX_VALUES || values.iter().any(|value| value.len() > MAX_VALUE_BYTES) {
            return Err(CompletionError::Limit("values"));
        }
        if let Some(total) = total {
            let count = values.len() as u64;
            if total > MAX_TOTAL
                || total < count
                || has_more.is_some_and(|more| more != (total > count))
            {
                return Err(CompletionError::Counts);
            }
        }
        let result = Self {
            completion: Suggestions {
                values,
                total,
                has_more,
            },
        };
        json::measure(&result, MAX_BYTES, &|| Ok(()))
            .map_err(|_| CompletionError::Limit("result bytes"))?;
        Ok(result)
    }

    /// Encodes a complete legacy reply with the caller's budget and checkpoint.
    ///
    /// # Errors
    /// Returns the bounded encoder's overflow, interruption or serialization error.
    pub fn encode_legacy(
        &self,
        id: &Value,
        limit: usize,
        check: &dyn Fn() -> Result<(), crate::Interrupted>,
    ) -> Result<Vec<u8>, json::EncodeError> {
        rpc::encode_success(id, self, limit, check)
    }
}

fn invalid(message: &str) -> RpcError {
    RpcError::new(rpc::INVALID_PARAMS, message)
}

#[cfg(test)]
mod tests;
