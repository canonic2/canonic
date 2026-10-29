//! Opt-in per-request protocol context, cache policy and revision-aware serialization.

use crate::{
    Decision, ServerInfo,
    control::Interrupted,
    json::{self, EncodeError},
    rpc::{self, RpcError},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;
#[cfg(test)]
mod tests;

/// The request-metadata revision; legacy fallback constants retain their meaning.
pub const PROTOCOL_VERSION: &str = "2026-07-28";
/// Reserved protocol metadata key.
pub const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
/// Reserved per-request client capabilities key.
pub const CAPABILITIES_KEY: &str = "io.modelcontextprotocol/clientCapabilities";
/// Reserved optional client identity key.
pub const CLIENT_KEY: &str = "io.modelcontextprotocol/clientInfo";
const SERVER_KEY: &str = "io.modelcontextprotocol/serverInfo";

mod sealed {
    pub trait Payload {}
    impl Payload for serde_json::Value {}
    impl Payload for crate::ToolResult {}
    impl Payload for crate::ReadResourceResult {}
}

/// Supported domain payloads for modern complete-result encoding.
/// Custom method payloads use a JSON object without protocol-owned fields.
pub trait CompletePayload: Serialize + sealed::Payload {
    /// Whether this payload is a domain object without reserved envelope fields.
    fn valid_payload(&self) -> bool;
}

impl CompletePayload for Value {
    fn valid_payload(&self) -> bool {
        self.is_object()
            && ["resultType", "_meta", "ttlMs", "cacheScope"]
                .iter()
                .all(|key| self.get(*key).is_none())
    }
}
impl CompletePayload for crate::ToolResult {
    fn valid_payload(&self) -> bool {
        true
    }
}
impl CompletePayload for crate::ReadResourceResult {
    fn valid_payload(&self) -> bool {
        true
    }
}

/// Which authorization contexts may share a cached result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheScope {
    /// Content may be shared publicly; the consumer must explicitly select this.
    Public,
    /// Content belongs to its authorization context.
    Private,
}

/// Validated cache hints. Defaults to immediately stale and private.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePolicy {
    ttl_ms: u64,
    cache_scope: CacheScope,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            ttl_ms: 0,
            cache_scope: CacheScope::Private,
        }
    }
}

impl CachePolicy {
    /// Creates policy with an interoperable nonnegative JSON integer TTL.
    ///
    /// # Errors
    /// Returns an invalid-parameters error for TTLs above the JSON safe integer bound.
    pub fn new(ttl_ms: u64, cache_scope: CacheScope) -> Result<Self, RpcError> {
        if ttl_ms > (1_u64 << 53) - 1 {
            return Err(invalid("Cache TTL exceeds safe integer range"));
        }
        Ok(Self {
            ttl_ms,
            cache_scope,
        })
    }
    pub(crate) fn merged(self, other: Self) -> Self {
        Self {
            ttl_ms: self.ttl_ms.min(other.ttl_ms),
            cache_scope: if self.cache_scope == CacheScope::Private
                || other.cache_scope == CacheScope::Private
            {
                CacheScope::Private
            } else {
                CacheScope::Public
            },
        }
    }
}

/// Explicit modern opt-in with server identity independent of legacy initialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModernOptions {
    /// Server identity and optional instructions.
    pub server: ServerInfo,
    /// Discovery cache hints.
    pub discovery_cache: CachePolicy,
    /// Tool catalog cache hints.
    pub tools_cache: CachePolicy,
    /// Ordinary resource/template catalog cache hints.
    pub resources_cache: CachePolicy,
    /// Skill entries and bundled file cache hints.
    pub skills_cache: CachePolicy,
    /// Default consumer-resolved resource read hints.
    pub reads_cache: CachePolicy,
}

impl ModernOptions {
    /// Enables the modern path with conservative cache defaults.
    #[must_use]
    pub fn new(server: ServerInfo) -> Self {
        Self {
            server,
            discovery_cache: CachePolicy::default(),
            tools_cache: CachePolicy::default(),
            resources_cache: CachePolicy::default(),
            skills_cache: CachePolicy::default(),
            reads_cache: CachePolicy::default(),
        }
    }
}

/// Immutable context captured for one admitted modern request.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestContext {
    /// The admitted operation, used to validate interim-result eligibility.
    pub method: String,
    /// This request's protocol revision.
    pub protocol_version: String,
    /// This request's capabilities; never an earlier client's declaration.
    pub capabilities: Value,
    /// Optional self-reported identity; grants no authority.
    pub client_info: Option<Value>,
    /// Opaque echoed continuation state. The consumer must authenticate it.
    pub request_state: Option<String>,
    /// Structurally validated input responses, if supplied.
    pub input_responses: Option<Value>,
    server: Value,
    cache: Option<CachePolicy>,
}

impl RequestContext {
    pub(crate) fn parse(
        params: &Value,
        options: &ModernOptions,
        method: &str,
    ) -> Result<Arc<Self>, RpcError> {
        let meta = &params["_meta"];
        let capabilities = meta[CAPABILITIES_KEY]
            .as_object()
            .ok_or_else(|| invalid("Missing or invalid client capabilities"))?;
        bounded(&Value::Object(capabilities.clone()), 65536, 32)?;
        bounded(meta, 1024 * 1024, 32)?;
        if let Some(extensions) = capabilities.get("extensions")
            && !extensions
                .as_object()
                .is_some_and(|map| map.values().all(Value::is_object))
        {
            return Err(invalid("Invalid extension declarations"));
        }
        if let Some(elicitation) = capabilities.get("elicitation")
            && !elicitation.as_object().is_some_and(|map| {
                map.iter().all(|(key, value)| {
                    !["form", "url"].contains(&key.as_str()) || value.is_object()
                })
            })
        {
            return Err(invalid("Invalid elicitation capability"));
        }
        let client_info = meta.get(CLIENT_KEY).cloned();
        if let Some(info) = &client_info
            && (!info.is_object()
                || ["name", "version"].iter().any(|field| {
                    info[field]
                        .as_str()
                        .is_none_or(|s| s.len() > crate::MAX_CLIENT_FIELD_BYTES)
                }))
        {
            return Err(invalid("Invalid client identity"));
        }
        let request_state = params
            .get("requestState")
            .map(|state| {
                state
                    .as_str()
                    .filter(|s| s.len() <= 65536)
                    .map(str::to_owned)
                    .ok_or_else(|| invalid("Invalid requestState"))
            })
            .transpose()?;
        let input_responses = params.get("inputResponses").cloned();
        if let Some(responses) = &input_responses {
            validate_responses(responses)?;
        }
        let cache = match method {
            "server/discover" => Some(options.discovery_cache),
            "tools/list" => Some(options.tools_cache),
            "resources/list" | "resources/templates/list" => Some(options.resources_cache),
            "skills/list" | "skills/get" => Some(options.skills_cache),
            "resources/read" => Some(options.reads_cache),
            _ => None,
        };
        let mut server = json!({"name":options.server.name,"version":options.server.version});
        if let Some(title) = &options.server.title {
            server["title"] = json!(title);
        }
        Ok(Arc::new(Self {
            method: method.into(),
            protocol_version: PROTOCOL_VERSION.into(),
            capabilities: Value::Object(capabilities.clone()),
            client_info,
            request_state,
            input_responses,
            server,
            cache,
        }))
    }

    /// Returns the echoed confirmation decision after checking this request's capability.
    /// This does not validate state integrity or grant authority.
    ///
    /// # Errors
    /// Returns missing-capability or invalid-input errors; consumers validate their state first.
    pub fn confirmation_decision(&self) -> Result<Option<Decision>, RpcError> {
        let Some(responses) = &self.input_responses else {
            return Ok(None);
        };
        if responses.as_object().is_some_and(serde_json::Map::is_empty) {
            return Ok(None);
        }
        self.require_form()?;
        match responses["confirmation"]["action"].as_str() {
            Some("accept") => Ok(Some(Decision::Accepted)),
            Some("decline" | "cancel") => Ok(Some(Decision::Declined)),
            _ => Err(invalid("Invalid confirmation response")),
        }
    }

    /// Creates a bounded yes/no interim result using this call's capabilities.
    /// The consumer supplies and later authenticates opaque workflow state.
    ///
    /// # Errors
    /// Returns a missing-capability error or a size error for excessive state/message.
    pub fn confirmation(
        &self,
        message: &str,
        state: &str,
    ) -> Result<InputRequiredResult, RpcError> {
        self.require_form()?;
        if state.len() > 65536 {
            return Err(invalid("Invalid requestState"));
        }
        let value = json!({"resultType":"input_required","inputRequests":{"confirmation":{"method":"elicitation/create","params":{"mode":"form","message":message,"requestedSchema":{"type":"object","properties":{}}}}},"requestState":state});
        bounded(&value, 1024 * 1024, 32)?;
        Ok(InputRequiredResult { value })
    }

    fn require_form(&self) -> Result<(), RpcError> {
        if self.capabilities["elicitation"]
            .as_object()
            .is_some_and(|map| map.is_empty() || map.get("form").is_some_and(Value::is_object))
        {
            return Ok(());
        }
        Err(RpcError::new(-32021, "Client lacks form elicitation")
            .with_data(json!({"requiredCapabilities":{"elicitation":{"form":{}}}})))
    }

    pub(crate) fn with_cache(&self, cache: CachePolicy) -> Arc<Self> {
        let mut context = self.clone();
        if context.cache.is_some() {
            context.cache = Some(cache);
        }
        Arc::new(context)
    }

    /// Encodes a modern success with captured identity, result type and applicable cache hints.
    ///
    /// # Errors
    /// Returns the bounded/interruptible encoder's error without truncation.
    pub fn encode_success(
        &self,
        id: &Value,
        result: &(impl CompletePayload + ?Sized),
        limit: usize,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<Vec<u8>, EncodeError> {
        #[derive(Serialize)]
        struct ResultEnvelope<'a, T: ?Sized> {
            #[serde(rename = "resultType")]
            kind: &'static str,
            #[serde(flatten)]
            result: &'a T,
            #[serde(rename = "_meta")]
            meta: Value,
            #[serde(flatten)]
            cache: Option<CachePolicy>,
        }
        if !result.valid_payload() {
            return Err(EncodeError::Invalid);
        }
        let cache = self.cache.map(|policy| {
            if self.request_state.is_some() || self.input_responses.is_some() {
                CachePolicy::default()
            } else {
                policy
            }
        });
        rpc::encode_success(
            id,
            &ResultEnvelope {
                kind: "complete",
                result,
                meta: json!({SERVER_KEY:self.server}),
                cache,
            },
            limit,
            check,
        )
    }

    /// Encodes an input-required reply with this request's server identity.
    /// This checks the operation and current form capability; workflow state
    /// integrity, authorization and revocation remain the consumer's responsibility.
    ///
    /// # Errors
    /// Returns [`EncodeError::Invalid`] if this operation or its capabilities
    /// cannot request input, and the bounded encoder's error on overflow or interruption.
    pub fn encode_interim(
        &self,
        id: &Value,
        result: &InputRequiredResult,
        limit: usize,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<Vec<u8>, EncodeError> {
        let value = self.interim(result).map_err(|_| EncodeError::Invalid)?;
        rpc::encode_success(id, &value, limit, check)
    }

    pub(crate) fn complete_value(&self, mut result: Value) -> Value {
        result["resultType"] = json!("complete");
        result["_meta"] = json!({SERVER_KEY:self.server});
        if let Some(cache) = self.cache {
            result["ttlMs"] = json!(cache.ttl_ms);
            result["cacheScope"] = json!(cache.cache_scope);
        }
        result
    }

    pub(crate) fn interim(&self, result: &InputRequiredResult) -> Result<Value, RpcError> {
        if !["tools/call", "resources/read"].contains(&self.method.as_str()) {
            return Err(invalid("Operation cannot request input"));
        }
        self.require_form()?;
        let mut value = result.value.clone();
        value["_meta"] = json!({SERVER_KEY:self.server});
        Ok(value)
    }
}

/// A typed interim result. Consumer workflow state remains consumer-owned.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct InputRequiredResult {
    value: Value,
}

impl InputRequiredResult {
    /// The validated interim payload without server metadata.
    /// Use [`RequestContext::encode_interim`] for an independent runtime's reply.
    #[must_use]
    pub fn as_value(&self) -> &Value {
        &self.value
    }
}

fn validate_responses(value: &Value) -> Result<(), RpcError> {
    bounded(value, 16384, 32)?;
    let map = value
        .as_object()
        .ok_or_else(|| invalid("Invalid inputResponses"))?;
    if map.is_empty() {
        return Ok(());
    }
    if map.len() != 1 || !map.contains_key("confirmation") {
        return Err(invalid("Unexpected input response"));
    }
    let response = &map["confirmation"];
    if !rpc::only(response, &["action", "content", "_meta"])
        || !matches!(
            response["action"].as_str(),
            Some("accept" | "decline" | "cancel")
        )
        || response
            .get("content")
            .is_some_and(|v| !v.as_object().is_some_and(serde_json::Map::is_empty))
    {
        return Err(invalid("Invalid confirmation response"));
    }
    Ok(())
}

pub(crate) fn bounded(value: &Value, bytes: usize, depth: usize) -> Result<(), RpcError> {
    fn nested(value: &Value, depth: usize) -> bool {
        match value {
            Value::Object(map) => depth > 0 && map.values().all(|value| nested(value, depth - 1)),
            Value::Array(values) => {
                depth > 0 && values.iter().all(|value| nested(value, depth - 1))
            }
            _ => true,
        }
    }
    if !nested(value, depth) || json::measure(value, bytes, &|| Ok(())).is_err() {
        return Err(invalid("Metadata or input limit exceeded"));
    }
    Ok(())
}

pub(crate) fn invalid(message: &str) -> RpcError {
    RpcError::new(rpc::INVALID_PARAMS, message)
}
