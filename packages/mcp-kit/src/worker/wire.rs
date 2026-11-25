//! Strict bounded private assignment messages. This is not host JSON-RPC.
//! Native adapters own channel authentication, deadline I/O, cancellation and
//! process cleanup; no codec function retries an exchange.
use crate::{
    Catalog, SessionOptions, json,
    rpc::RpcError,
    stdio::{Context, Outcome, Request},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

mod context;
pub use context::Decoder;
mod work;

/// An assignment-bound, nonblocking native progress sink. The adapter owns its
/// bounded queue and must reject updates after the assignment settles. Returning
/// `true` means queued, not delivered. No host progress token crosses this API.
pub type ProgressSink = Arc<dyn Fn(f64, Option<f64>, Option<&str>) -> bool + Send + Sync>;

/// Private wire version, independent from MCP revisions.
pub const VERSION: u32 = 2;

/// A channel-authenticated identity independent of the host's reusable RPC ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub frontend: String,
    pub worker: String,
    pub sequence: u64,
}

/// Validated request data. `kind` is one of the variants supported by `restore`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub kind: String,
    pub params: Value,
}

/// One private command. Adapters bound the complete encoded value and authenticate
/// both incarnations before dispatching it. Remaining time never renews a deadline.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub version: u32,
    pub assignment: Assignment,
    pub work: Work,
    pub remaining_ms: u64,
    pub metadata: Value,
}

/// One typed domain response. Interaction tokens are consumer-owned; they never
/// contain a closure, pointer, host question ID or executable code.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub version: u32,
    pub assignment: Assignment,
    pub kind: String,
    pub value: Value,
}

fn invalid() -> RpcError {
    RpcError::new(
        crate::rpc::INTERNAL_ERROR,
        "Invalid private worker message; effects may have occurred",
    )
}

/// Encode without allocating past the explicit frame budget.
/// # Errors
/// Returns the core encoder's overflow or invalid-value error.
pub fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, json::EncodeError> {
    json::encode(value, limit)
}

/// Strict duplicate-key/depth/byte checking before typed decoding.
/// # Errors
/// Refuses malformed or oversized values, including unknown envelope fields.
pub fn decode<T: DeserializeOwned>(bytes: &[u8], limit: usize) -> Result<T, RpcError> {
    let value = json::parse(bytes, limit).map_err(|_| invalid())?;
    serde_json::from_value(value).map_err(|_| invalid())
}

impl Work {
    /// Copy only typed input, never a host envelope or prepared schema pointer.
    /// # Errors
    /// Bundled reads belong to the frontend and cannot be sent to an implementation.
    pub fn new(request: &Request) -> Result<Self, RpcError> {
        let view = work::View::new(request)?;
        Ok(Self {
            kind: view.kind.to_owned(),
            params: serde_json::to_value(view.params).map_err(|_| invalid())?,
        })
    }
    /// Rebuild through the owning request validators and prepared catalogs.
    /// # Errors
    /// Refuses unsupported variants, invalid arguments or undeclared extensions.
    pub fn restore(
        &self,
        catalog: &Catalog,
        options: &SessionOptions,
    ) -> Result<Request, RpcError> {
        match self.kind.as_str() {
            "tool" => {
                let call = crate::tool::ToolCall::from_params(&self.params)?;
                if catalog.get(&call.name).is_none() {
                    return Err(invalid());
                }
                // Reuse the catalog's already prepared validator on the worker.
                catalog
                    .validate_arguments(&call.name, &call.arguments)
                    .map_err(|_| invalid())?;
                Ok(Request::Tool(call))
            }
            "prompt" => Ok(Request::Prompt(
                options
                    .prompts
                    .as_ref()
                    .ok_or_else(invalid)?
                    .call(&self.params)?,
            )),
            "completion" if options.completions => {
                Ok(Request::Completion(crate::CompleteRequest::parse(
                    &self.params,
                    options.prompts.as_deref(),
                    options.resources.as_deref(),
                )?))
            }
            "tasks/get" | "tasks/update" | "tasks/cancel" if options.tasks.is_some() => Ok(
                Request::Task(crate::task::TaskRequest::parse(&self.kind, &self.params)?),
            ),
            "method" | "notification" => {
                let method = self.params["method"]
                    .as_str()
                    .ok_or_else(invalid)?
                    .to_owned();
                let params = self.params["params"].clone();
                if !params.is_object() || !crate::rpc::only(&self.params, &["method", "params"]) {
                    return Err(invalid());
                }
                if method == "resources/read" {
                    if options.resources.is_none()
                        && options.skills.is_none()
                        && options.apps.is_none()
                    {
                        return Err(invalid());
                    }
                    crate::resource::read_params(&params)?;
                } else if method == "resources/directory/read" {
                    options
                        .skills
                        .as_ref()
                        .ok_or_else(invalid)?
                        .directory(&params)?;
                }
                if self.kind == "notification" && options.extension_notifications.contains(&method)
                {
                    Ok(Request::Notification { method, params })
                } else if self.kind == "method"
                    && (options.extension_methods.contains(&method)
                        || method == "resources/read"
                        || method == "resources/directory/read")
                {
                    Ok(Request::Method { method, params })
                } else {
                    Err(invalid())
                }
            }
            _ => Err(invalid()),
        }
    }
}

pub(crate) fn request_bytes(request: &Request, limit: usize) -> Result<usize, json::EncodeError> {
    let view = work::View::new(request).map_err(|_| json::EncodeError::Invalid)?;
    json::measure(&view, limit, &|| Ok(()))
}

impl Call {
    /// Capture the originating modern context and token-free authorization data.
    /// Opaque grant extensions require a consumer codec; this helper refuses
    /// them instead of silently discarding authority-relevant state.
    /// # Errors
    /// Refuses invalid identities or an input that belongs to the frontend.
    pub fn new(
        assignment: Assignment,
        request: &Request,
        context: &Context,
    ) -> Result<Self, RpcError> {
        context.check().map_err(|_| invalid())?;
        if assignment.sequence == 0
            || !valid_id(&assignment.frontend)
            || !valid_id(&assignment.worker)
            || context.authorization().is_some_and(|grant| {
                grant.has_extension()
                    || !grant.within_bounds()
                    || grant
                        .expires_at()
                        .is_some_and(|expiry| expiry.duration_since(std::time::UNIX_EPOCH).is_err())
            })
        {
            return Err(invalid());
        }
        let protocol = context.request_context().map(|context| {
            json!({
                "version":context.protocol_version, "capabilities":context.capabilities,
                "metadata":context.metadata().as_map(), "requestState":context.request_state,
                "clientInfo":context.client_info, "inputResponses":context.input_responses,
                "tasks":context.task_options().is_some(),
                "cache":context.worker_cache(),
            })
        });
        let grant = context.authorization().map(|grant| json!({"audiences":grant.audiences(),
            "scopes":grant.scopes().collect::<Vec<_>>(),"subject":grant.subject(),"clientId":grant.client_id(),
            "expiresAt":grant.expires_at().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|time|json!({"seconds":time.as_secs(),"nanoseconds":time.subsec_nanos()}))}));
        Ok(Self {
            version: VERSION,
            assignment,
            work: Work::new(request)?,
            remaining_ms: u64::try_from(context.remaining().as_nanos().div_ceil(1_000_000))
                .unwrap_or(u64::MAX),
            metadata: json!({"protocol":protocol,"grant":grant,"apps":context.supports_apps(),
                "id":context.request_id()}),
        })
    }
    /// Validate the expected channel identities before any effects.
    /// # Errors
    /// Refuses foreign versions, incarnations or exhausted assignment identities.
    pub fn check(&self, frontend: &str, worker: &str) -> Result<(), RpcError> {
        if self.version != VERSION
            || self.assignment.sequence == 0
            || self.assignment.frontend != frontend
            || self.assignment.worker != worker
            || self.remaining_ms == 0
            || Duration::from_millis(self.remaining_ms) > crate::control::MAX_TIMEOUT
            || !valid_id(frontend)
            || !valid_id(worker)
            || !self.metadata.is_object()
        {
            return Err(invalid());
        }
        Ok(())
    }
}

impl Reply {
    /// Encode a terminal handler outcome under an explicit payload budget.
    /// Interaction closures and frontend-owned bundled reads are refused without
    /// consuming `outcome`; native adapters retain/proxy their own interaction tokens.
    /// The complete reply must still pass [`encode`] with the channel's frame limit.
    /// # Errors
    /// Refuses unsupported outcomes, invalid assignments or oversized payloads.
    pub fn terminal(
        assignment: Assignment,
        outcome: &Outcome,
        limit: usize,
    ) -> Result<Self, RpcError> {
        fn value(value: &impl Serialize, limit: usize) -> Result<Value, RpcError> {
            let bytes = encode(value, limit).map_err(|_| invalid())?;
            json::parse(&bytes, limit).map_err(|_| invalid())
        }
        if assignment.sequence == 0
            || !valid_id(&assignment.frontend)
            || !valid_id(&assignment.worker)
        {
            return Err(invalid());
        }
        let (kind, payload) = match outcome {
            Outcome::Tool(result) => ("tool", value(result, limit)?),
            Outcome::Method(Ok(result)) => ("method", value(result, limit)?),
            Outcome::Method(Err(error)) | Outcome::Task(Err(error)) => {
                ("error", value(error, limit)?)
            }
            Outcome::Done => ("done", value(&(), limit)?),
            Outcome::ResourceNotFound(uri) => ("resource_not_found", value(uri, limit)?),
            Outcome::Resource(result) => ("resource", value(result, limit)?),
            Outcome::ResourceWithCache(result, policy) => {
                let mut payload = value(policy, limit)?;
                payload["result"] = value(result, limit)?;
                ("resource_cache", value(&payload, limit)?)
            }
            Outcome::Completion(result) => ("completion", value(result, limit)?),
            Outcome::Prompt(result) => ("prompt", value(result, limit)?),
            Outcome::Directory(result) => ("directory", value(result, limit)?),
            Outcome::InputRequired(result) => ("input_required", value(result.as_value(), limit)?),
            Outcome::Task(Ok(task)) => match task {
                crate::task::TaskResult::Created(snapshot) => {
                    ("task_created", value(snapshot, limit)?)
                }
                crate::task::TaskResult::Snapshot(snapshot) => {
                    ("task_snapshot", value(snapshot, limit)?)
                }
                crate::task::TaskResult::Updated => ("task_updated", value(&(), limit)?),
                crate::task::TaskResult::Cancelled => ("task_cancelled", value(&(), limit)?),
            },
            Outcome::Confirm(_)
            | Outcome::Elicit(_)
            | Outcome::Bundled(_)
            | Outcome::PendingInput(_) => {
                return Err(invalid());
            }
        };
        Ok(Self {
            version: VERSION,
            assignment,
            kind: kind.into(),
            value: payload,
        })
    }

    /// Bind the terminal response to its exact originating assignment.
    /// # Errors
    /// Refuses stale/foreign messages before attempting outcome reconstruction.
    pub fn check(&self, assignment: &Assignment) -> Result<(), RpcError> {
        if self.version != VERSION || &self.assignment != assignment {
            return Err(invalid());
        }
        Ok(())
    }
    /// Reconstruct a terminal typed value; the scheduler then applies the original
    /// producer contract and revision encoder. Interaction envelopes are handled
    /// by the native adapter, keeping continuation tokens on the source process.
    /// # Errors
    /// Refuses unknown kinds or any failed core constructor without replay.
    pub fn outcome(&self, assignment: &Assignment, context: &Context) -> Result<Outcome, RpcError> {
        self.check(assignment)?;
        let value = &self.value;
        match self.kind.as_str() {
            "tool" => Ok(Outcome::Tool(from(value)?)),
            "method" => Ok(Outcome::Method(Ok(value.clone()))),
            "error" => {
                let mut error = RpcError::new(
                    value["code"]
                        .as_i64()
                        .and_then(|v| i32::try_from(v).ok())
                        .ok_or_else(invalid)?,
                    value["message"].as_str().ok_or_else(invalid)?,
                );
                if let Some(data) = value.get("data") {
                    error = error.with_data(data.clone());
                }
                Ok(Outcome::Method(Err(error)))
            }
            "done" if value.is_null() => Ok(Outcome::Done),
            "resource_not_found" => Ok(Outcome::ResourceNotFound(
                value.as_str().ok_or_else(invalid)?.to_owned(),
            )),
            "resource" => Ok(Outcome::Resource(resource(value)?)),
            "resource_cache" => {
                let scope = match value["cacheScope"].as_str() {
                    Some("private") => crate::CacheScope::Private,
                    Some("public") => crate::CacheScope::Public,
                    _ => return Err(invalid()),
                };
                Ok(Outcome::ResourceWithCache(
                    resource(&value["result"])?,
                    crate::CachePolicy::new(value["ttlMs"].as_u64().ok_or_else(invalid)?, scope)?,
                ))
            }
            "completion" => completion(value),
            "prompt" => {
                let mut messages = Vec::new();
                for message in value["messages"].as_array().ok_or_else(invalid)? {
                    let role = match message["role"].as_str() {
                        Some("user") => crate::Role::User,
                        Some("assistant") => crate::Role::Assistant,
                        _ => return Err(invalid()),
                    };
                    messages.push(crate::PromptMessage {
                        role,
                        content: from(&message["content"])?,
                    });
                }
                let mut result =
                    crate::GetPromptResult::new(optional(value, "description")?, messages)
                        .map_err(|_| invalid())?;
                if let Some(meta) = optional(value, "_meta")? {
                    result = result.with_metadata(meta).map_err(|_| invalid())?;
                }
                Ok(Outcome::Prompt(result))
            }
            "directory" => Ok(Outcome::Directory(
                crate::ReadDirectoryResult::new(
                    from(&value["resources"])?,
                    optional(value, "nextCursor")?,
                )
                .map_err(|_| invalid())?,
            )),
            "task_created" => Ok(Outcome::Task(Ok(crate::task::TaskResult::Created(
                crate::task::Snapshot::restore(value).map_err(|_| invalid())?,
            )))),
            "task_snapshot" => Ok(Outcome::Task(Ok(crate::task::TaskResult::Snapshot(
                crate::task::Snapshot::restore(value).map_err(|_| invalid())?,
            )))),
            "task_updated" if value.is_null() => {
                Ok(Outcome::Task(Ok(crate::task::TaskResult::Updated)))
            }
            "task_cancelled" if value.is_null() => {
                Ok(Outcome::Task(Ok(crate::task::TaskResult::Cancelled)))
            }
            "input_required" => {
                let context = context.request_context().ok_or_else(invalid)?;
                if value["resultType"] != "input_required"
                    || !crate::rpc::only(value, &["resultType", "inputRequests", "requestState"])
                {
                    return Err(invalid());
                }
                let requests = crate::elicitation::Requests::restore(&value["inputRequests"])?;
                Ok(Outcome::InputRequired(
                    context.elicitation(
                        requests,
                        value
                            .get("requestState")
                            .map(|state| state.as_str().ok_or_else(invalid))
                            .transpose()?,
                    )?,
                ))
            }
            _ => Err(invalid()),
        }
    }
}

fn from<T: DeserializeOwned>(value: &Value) -> Result<T, RpcError> {
    serde_json::from_value(value.clone()).map_err(|_| invalid())
}
fn optional<T: DeserializeOwned>(value: &Value, key: &str) -> Result<Option<T>, RpcError> {
    value.get(key).map(from).transpose()
}
fn resource(value: &Value) -> Result<crate::ReadResourceResult, RpcError> {
    let mut result =
        crate::ReadResourceResult::new(from(&value["contents"])?).map_err(|_| invalid())?;
    if let Some(meta) = optional(value, "_meta")? {
        result = result.with_metadata(meta);
    }
    Ok(result)
}

fn completion(value: &Value) -> Result<Outcome, RpcError> {
    let data = &value["completion"];
    let mut result = crate::CompleteResult::new(
        from(&data["values"])?,
        optional(data, "total")?,
        optional(data, "hasMore")?,
    )
    .map_err(|_| invalid())?;
    if let Some(meta) = optional(value, "_meta")? {
        result = result.with_metadata(meta).map_err(|_| invalid())?;
    }
    Ok(Outcome::Completion(result))
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}
