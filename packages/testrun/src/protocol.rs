//! MCP handshake and dispatch; transport, schemas and presentation have separate owners.
use crate::{error::Error, qa::Server};
use serde_json::{Value, json};
use std::time::Instant;

mod presentation;
mod responsive;
mod scheduled;
mod transport;
pub use transport::serve;
/// Validates the published tools, bundled skills and protocol options without serving.
///
/// # Errors
/// Returns `INTERNAL` when the embedded catalogs or options are invalid.
pub fn check() -> crate::error::Result<()> {
    Protocol::try_new().map(drop)
}
#[cfg(test)]
const VERSION: &str = "2025-11-25";
fn build_catalog() -> Result<canonic_mcp_kit::Catalog, canonic_mcp_kit::CatalogError> {
    let definitions = crate::catalog::catalog()
        .iter()
        .map(|definition| {
            let mut tool = canonic_mcp_kit::Tool::new(
                definition["name"].as_str().expect("Tool name"),
                definition["description"]
                    .as_str()
                    .expect("Tool description"),
                definition["inputSchema"].clone(),
            );
            let hints = &definition["annotations"];
            tool.annotations = canonic_mcp_kit::Annotations {
                title: hints["title"].as_str().map(str::to_owned),
                read_only_hint: hints["readOnlyHint"].as_bool(),
                destructive_hint: hints["destructiveHint"].as_bool(),
                idempotent_hint: hints["idempotentHint"].as_bool(),
                open_world_hint: hints["openWorldHint"].as_bool(),
            };
            tool.title = definition["title"].as_str().map(str::to_owned);
            tool.output_schema = definition.get("outputSchema").cloned();
            tool.meta = definition
                .get("_meta")
                .map(|metadata| canonic_mcp_kit::Metadata::new(metadata.clone()))
                .transpose()
                .map_err(|error| canonic_mcp_kit::CatalogError::Schema {
                    tool: tool.name.clone(),
                    field: "_meta",
                    problem: error.to_string(),
                })?;
            Ok(tool)
        })
        .collect::<Result<Vec<_>, canonic_mcp_kit::CatalogError>>()?;
    canonic_mcp_kit::Catalog::new_with_argument_byte_limits(
        definitions,
        crate::browser::transfer::argument_byte_limit,
    )
}
const INSTRUCTIONS: &str = "Testrun tests iOS and Android apps and isolated Chrome, Firefox and WebKit browsers.\n\
Use a known targetId directly. Find devices with target_list and browsers with browser_list when needed. Connect only when needed. Reuse the device screenshot from connect when current. Get a fresh view with observe, find elements with ui_find, and send supported input with act. Use wait for an expected state. Independent targets can run in parallel.\n\
After interrupted input, check status and observe before deciding what to do next. For a pending browser action, poll status with actionId until operation.settled and read its outcome. Never repeat uncertain input. Use browser_status after a lost browser connect reply.\n\
Connect with engine to start an isolated browser, headless by default. Use an explicit browser targetId and cancel to request cancellation. Use browser_send for browser-specific operations. Coordinate input needs a fresh inputObservation and CSS viewport coordinates. Held-input release invalidates older observations. Use control_acquire for an external viewer on an owned page. Viewer input needs its own fresh observation.\n\
For physical iPhone setup, use doctor, developer_support_install for missing support, and developer_mode_reveal to show the setting the user must turn on. See connect for HID, XCTest and keyboard options.\n\
For project setup, use project_discover as needed, then project_init_preview and project_init with the reviewed planDigest. For investigations, use diagnostics_capabilities and follow nextOperation in results. Reuse submissionId for lost collection or analysis replies. Use diagnostics_query for retained evidence and diagnostics_export to save it locally.\n\
For more results, follow nextCursor with the same filters. Restart without the cursor on invalid_cursor. Use disconnect for requested cleanup. It can stop owned resources and does not reset settings. Tools grant no additional authorization.\n\
For branching UI verification or recovery after interrupted input, open skill://testrun/ui-verification/SKILL.md via MCP resources/read.";
/// The largest encoded reply to a modern request; larger results return an internal error.
pub(crate) const MAX_REPLY: usize = 64 * 1024 * 1024;
/// Skill files are immutable for one binary, so hosts may share them briefly.
const SKILLS_CACHE_MS: u64 = 300_000;

fn identity() -> canonic_mcp_kit::ServerInfo {
    canonic_mcp_kit::ServerInfo::new("canonic-testrun", env!("CARGO_PKG_VERSION"))
        .with_instructions(INSTRUCTIONS)
}

#[derive(Clone)]
pub struct Protocol {
    session: std::sync::Arc<std::sync::Mutex<canonic_mcp_kit::Session>>,
    connection_id: String,
}
impl Default for Protocol {
    fn default() -> Self {
        Self::try_new().expect("Bundled MCP skills and modern options are valid")
    }
}
impl Protocol {
    /// Validates the tool catalog, skill catalog and modern options before any input.
    pub(crate) fn try_new() -> Result<Self, Error> {
        let skills = crate::skills::catalog()
            .map_err(|error| Error::new("INTERNAL", format!("Invalid bundled skills: {error}")))?;
        let mut modern = canonic_mcp_kit::ModernOptions::new(identity());
        modern.skills_cache =
            canonic_mcp_kit::CachePolicy::new(SKILLS_CACHE_MS, canonic_mcp_kit::CacheScope::Public)
                .expect("Skill cache policy is within bounds");
        let session = canonic_mcp_kit::Session::try_new(
            tools(),
            canonic_mcp_kit::SessionOptions {
                modern: Some(modern),
                skills: Some(std::sync::Arc::new(skills)),
                max_message_bytes: transport::MAX_REQUEST,
                extension_methods: vec!["testrun/task_end".into()],
                extension_notifications: vec!["notifications/testrun/task_end".into()],
                ..canonic_mcp_kit::SessionOptions::default()
            },
        )
        .map_err(|error| Error::new("INTERNAL", error.to_string()))?;
        Ok(Self {
            session: std::sync::Arc::new(std::sync::Mutex::new(session)),
            connection_id: uuid::Uuid::new_v4().to_string(),
        })
    }
}
pub fn tools() -> std::sync::Arc<canonic_mcp_kit::Catalog> {
    static CATALOG: std::sync::OnceLock<std::sync::Arc<canonic_mcp_kit::Catalog>> =
        std::sync::OnceLock::new();
    CATALOG
        .get_or_init(|| {
            std::sync::Arc::new(build_catalog().expect("Published MCP catalog is supported"))
        })
        .clone()
}
/// Restore our tool and bundled-resource payloads before MCP-kit's modern
/// validator checks their type against the admitted method and output schema.
fn encode_complete(
    context: &canonic_mcp_kit::RequestContext,
    id: &Value,
    result: &Value,
) -> Result<Vec<u8>, canonic_mcp_kit::json::EncodeError> {
    use canonic_mcp_kit::json::EncodeError;
    if result.get("content").is_some() {
        let result: canonic_mcp_kit::ToolResult =
            serde_json::from_value(result.clone()).map_err(|_| EncodeError::Invalid)?;
        context.encode_success(id, &result, MAX_REPLY, &|| Ok(()))
    } else if let Some(contents) = result.get("contents") {
        let contents =
            serde_json::from_value(contents.clone()).map_err(|_| EncodeError::Invalid)?;
        let result =
            canonic_mcp_kit::ReadResourceResult::new(contents).map_err(|_| EncodeError::Invalid)?;
        context.encode_success(id, &result, MAX_REPLY, &|| Ok(()))
    } else {
        context.encode_success(id, result, MAX_REPLY, &|| Ok(()))
    }
}
fn definition(name: &str) -> Option<Value> {
    tools()
        .get(name)
        .map(|tool| serde_json::to_value(tool).expect("Tool definition serializes"))
}
impl Protocol {
    fn receive(&self, frame: &[u8]) -> canonic_mcp_kit::Inbound {
        self.session
            .lock()
            .expect("MCP session owner")
            .receive(frame)
    }
    fn ready(&self) -> bool {
        self.session.lock().expect("MCP session owner").ready()
    }
    fn context(&self, id: &Value) -> Option<std::sync::Arc<canonic_mcp_kit::RequestContext>> {
        self.session
            .lock()
            .expect("MCP session owner")
            .request_context(id)
    }
    /// Session admitted this request: after the legacy handshake, or with its own modern metadata.
    pub(super) fn accepts(&self, request: &Value) -> bool {
        self.ready()
            || request
                .get("id")
                .is_some_and(|id| self.context(id).is_some())
    }
    fn finish(&self, id: &Value) -> bool {
        self.session.lock().expect("MCP session owner").finish(id)
    }
    /// Releases the reply's ID and encodes it for that request's own revision.
    /// Returns `None` when the request was cancelled or already answered.
    pub(super) fn settle(&self, response: &Value) -> Option<Vec<u8>> {
        let id = response.get("id")?;
        let context = {
            let mut session = self.session.lock().expect("MCP session owner");
            let context = session.request_context(id);
            if !session.finish(id) {
                return None;
            }
            context
        };
        Some(match (context, response.get("result")) {
            (Some(context), Some(result)) => {
                encode_complete(&context, id, result).unwrap_or_else(|_| {
                    frame(&rpc_error(
                        id.clone(),
                        canonic_mcp_kit::rpc::INTERNAL_ERROR,
                        "Reply is invalid or exceeds its output limit",
                    ))
                })
            }
            _ => frame(response),
        })
    }
    /// A read outside the skill namespace: Testrun publishes no other resources.
    fn missing_resource(&self, id: Value, params: &Value) -> Value {
        let uri = params["uri"].as_str().unwrap_or_default();
        let mut error = canonic_mcp_kit::resource::not_found(uri);
        if self.context(&id).is_some() {
            error.code = canonic_mcp_kit::rpc::INVALID_PARAMS;
        }
        canonic_mcp_kit::rpc::failure(id, &error)
    }
    fn close(&self) -> Option<Value> {
        self.session.lock().expect("MCP session owner").close()
    }
    fn initialize(&self, client: &mut crate::managed::Client, id: Value) -> Value {
        client.attach_mcp(self.connection_id.clone());
        self.session
            .lock()
            .expect("MCP session owner")
            .accept_initialize(id, &identity())
    }
    /// Extension requests take no parameters; a modern request may add only its `_meta`.
    fn empty_params(&self, id: &Value, params: &Value) -> bool {
        params.as_object().is_some_and(|params| {
            params.is_empty()
                || (params.len() == 1 && params.contains_key("_meta") && self.context(id).is_some())
        })
    }
    /// Extension parameter validation belongs to Testrun; framing and readiness to Session.
    pub(super) fn task_end(&self, request: &Value) -> bool {
        self.ready()
            && request["jsonrpc"] == "2.0"
            && request
                .get("params")
                .is_none_or(|params| params.as_object().is_some_and(serde_json::Map::is_empty))
            && matches!(
                (request.get("id"), request["method"].as_str()),
                (None, Some("notifications/testrun/task_end"))
                    | (
                        Some(Value::String(_) | Value::Number(_)),
                        Some("testrun/task_end")
                    )
            )
    }
    /// Convenience single-request entry point; the shipping transport also drives this same Session.
    pub async fn request(&mut self, server: &mut Server, request: Value) -> Option<Value> {
        let frame = serde_json::to_vec(&request).expect("JSON request serializes");
        let response = match self.receive(&frame) {
            canonic_mcp_kit::Inbound::Subscription { id, .. } => {
                rpc_error(id, -32601, "Subscriptions are not enabled for this server")
            }
            canonic_mcp_kit::Inbound::Completion { id, .. } => {
                rpc_error(id, -32601, "Completions are not enabled for this server")
            }
            canonic_mcp_kit::Inbound::Prompt { id, .. } => {
                rpc_error(id, -32601, "Prompts are not enabled for this server")
            }
            canonic_mcp_kit::Inbound::Task { id, .. } => {
                rpc_error(id, -32601, "Tasks are not enabled for this server")
            }
            canonic_mcp_kit::Inbound::BundledResource { id, read, .. } => bundled(id, &read),
            canonic_mcp_kit::Inbound::Reply(reply)
            | canonic_mcp_kit::Inbound::Fatal { reply, .. } => {
                return Some(reply);
            }
            canonic_mcp_kit::Inbound::Initialize { id, .. } => {
                return Some(self.initialize(&mut server.managed, id));
            }
            canonic_mcp_kit::Inbound::Tool { id, call } => {
                self.admitted_request(server, tool_request(id, call), Instant::now())
                    .await?
            }
            canonic_mcp_kit::Inbound::InvalidArguments { id, call, error } => {
                invalid_arguments(id, &call, error)
            }
            canonic_mcp_kit::Inbound::Method { id, method, params }
                if method == "resources/read" =>
            {
                self.missing_resource(id, &params)
            }
            canonic_mcp_kit::Inbound::Method { id, method, params } => {
                if !self.empty_params(&id, &params) {
                    rpc_error(id, -32602, "Task end takes no parameters")
                } else {
                    self.admitted_request(
                        server,
                        json!({"jsonrpc":"2.0","id":id,"method":method,"params":{}}),
                        Instant::now(),
                    )
                    .await?
                }
            }
            canonic_mcp_kit::Inbound::Notification { method, params } => {
                return self
                    .admitted_request(
                        server,
                        json!({"jsonrpc":"2.0","method":method,"params":params}),
                        Instant::now(),
                    )
                    .await;
            }
            canonic_mcp_kit::Inbound::Cancel { .. }
            | canonic_mcp_kit::Inbound::Confirmation { .. }
            | canonic_mcp_kit::Inbound::Elicitation { .. }
            | canonic_mcp_kit::Inbound::Ignored => return None,
        };
        let encoded = self.settle(&response)?;
        Some(serde_json::from_slice(&encoded).expect("Settled reply is JSON"))
    }
    /// Execute an already admitted tool or product extension without repeating protocol admission.
    pub(super) async fn admitted_request(
        &mut self,
        server: &mut Server,
        request: Value,
        admitted: Instant,
    ) -> Option<Value> {
        let id = request.get("id").cloned();
        let params = request.get("params").cloned().unwrap_or(json!({}));
        let method = request["method"].as_str().unwrap_or("");
        if matches!(
            method,
            "testrun/task_end" | "notifications/testrun/task_end"
        ) {
            if !self.task_end(&request) {
                // Session routes legacy callbacks only after the handshake, so a
                // callback that is not ready came as a modern request.
                let message = if self.ready() {
                    "Task end takes no parameters"
                } else {
                    "Task end requires an initialized connection"
                };
                return id.map(|id| rpc_error(id, -32602, message));
            }
            let result = server.managed.end_task().await;
            return id.map(|id| match result {
                Ok(value) => canonic_mcp_kit::rpc::success(id, value),
                Err(error) => canonic_mcp_kit::rpc::failure(
                    id,
                    &canonic_mcp_kit::RpcError::new(-32000, error.message.clone())
                        .with_data(json!(error)),
                ),
            });
        }
        let id = id?;
        let name = params["name"].as_str()?;
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        let start = Instant::now();
        server.managed.request_id = Some(format!(
            "mcp-{}",
            crate::managed::request_key(&self.connection_id, &id)
        ));
        server.managed.received = Some(admitted);
        server.progress_token(params["_meta"].get("progressToken").cloned());
        let deadline = requested_deadline(name, &arguments);
        let browser = crate::browser::api::selected(name, &arguments)
            || crate::browser::surface::handles(name);
        server.managed.media_deadline = deadline.filter(|_| crate::playback::operation(name));
        let validation = if name == "act" && !crate::browser::api::selected(name, &arguments) {
            crate::managed::validate_actions(&arguments)
        } else {
            Ok(())
        };
        let result = if let Err(error) = validation {
            Err(crate::managed::invalid_action(error))
        } else if let Some(deadline) = deadline {
            match tokio::time::timeout_at(deadline, server.call(name, arguments)).await {
                Ok(result) => result,
                Err(_) if browser => Err(responsive::timed_out(name, start.elapsed().as_secs())),
                Err(_) => match server.cancel_dispatched().await {
                    Ok(()) => Err(Error::new(
                        "TIMEOUT",
                        "Tool deadline expired; inspect the command journal before retrying a mutation",
                    )),
                    Err(error) => Err(error),
                },
            }
        } else {
            server.call(name, arguments).await
        };
        server.progress_token(None);
        server.managed.request_id = None;
        server.managed.received = None;
        server.managed.media_deadline = None;
        Some(canonic_mcp_kit::rpc::success(
            id,
            presentation::tool_result(name, result, start.elapsed()),
        ))
    }
}
fn tool_request(id: Value, call: canonic_mcp_kit::ToolCall) -> Value {
    let mut params = json!({"name":call.name,"arguments":call.arguments});
    if let Some(token) = call.progress_token {
        params["_meta"] = json!({"progressToken":token});
    }
    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params})
}
fn invalid_arguments(
    id: Value,
    call: &canonic_mcp_kit::ToolCall,
    error: canonic_mcp_kit::schema::ArgumentError,
) -> Value {
    let name = call.name.as_str();
    let error = crate::browser::transfer::fixture_path_guidance(name, &call.arguments)
        .err()
        .unwrap_or_else(|| {
            let error = Error::invalid(error.message);
            if name == "act" && !crate::browser::api::selected(name, &call.arguments) {
                // Preserve the mobile action's validation and dispatch facts.
                crate::managed::invalid_action(
                    crate::managed::validate_actions(&call.arguments)
                        .err()
                        .unwrap_or(error),
                )
            } else {
                error
            }
        });
    canonic_mcp_kit::rpc::success(
        id,
        presentation::tool_result(name, Err(error), std::time::Duration::ZERO),
    )
}
/// The deadline from a tool's explicit `timeoutMs`, starting now; other calls
/// stay live for the connection.
fn requested_deadline(name: &str, arguments: &Value) -> Option<tokio::time::Instant> {
    arguments
        .get("timeoutMs")
        .and_then(Value::as_u64)
        // A wait's or network switch's timeoutMs bounds polling, not transport
        // execution; zero requests one bounded observation.
        .filter(|_| !matches!(name, "wait" | "network_set"))
        .and_then(|ms| {
            tokio::time::Instant::now().checked_add(std::time::Duration::from_millis(ms))
        })
}
fn validate_arguments(name: &str, schema: &Value, arguments: &Value) -> crate::error::Result<()> {
    crate::browser::transfer::fixture_path_guidance(name, arguments)?;
    if name == "act" && !crate::browser::api::selected(name, arguments) {
        crate::managed::validate_actions(arguments)?;
    }
    crate::schema::validate(schema, arguments)
}
fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    canonic_mcp_kit::rpc::failure(id, &canonic_mcp_kit::RpcError::new(code, message))
}
/// A reply carrying a bundled skill file's prepared contents.
fn bundled(id: Value, read: &canonic_mcp_kit::BundledRead) -> Value {
    canonic_mcp_kit::rpc::success(
        id,
        serde_json::to_value(read.result()).expect("Bundled read result serializes"),
    )
}
fn frame(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("JSON reply serializes")
}

#[cfg(test)]
mod tests;
