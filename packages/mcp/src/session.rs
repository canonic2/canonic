//! The I/O-free connection state machine: lifecycle, routing, outstanding
//! request IDs and server-initiated confirmation requests.

use crate::{
    json::{self, ParseError},
    rpc::{self, Message, RpcError},
    schema::{self, ArgumentError},
    tool::{Catalog, ToolCall},
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

#[cfg(test)]
mod tests;

/// The protocol version offered when a client requests an unsupported one.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";
/// Protocol versions accepted as requested.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", LATEST_PROTOCOL_VERSION];
/// The longest accepted client name or version, in bytes.
pub const MAX_CLIENT_FIELD_BYTES: usize = 4096;
/// The longest accepted requested protocol version, in bytes.
pub const MAX_VERSION_BYTES: usize = 64;
/// The default message size limit, in bytes.
pub const DEFAULT_MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Connection limits and product extension methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOptions {
    /// The longest accepted message, in bytes, excluding the line break.
    pub max_message_bytes: usize,
    /// Messages accepted before the connection ends; `None` for no limit.
    pub max_messages: Option<u64>,
    /// Product request methods, such as `acme/task_end`, routed as [`Inbound::Method`].
    pub extension_methods: Vec<String>,
    /// Product notification methods routed as [`Inbound::Notification`].
    pub extension_notifications: Vec<String>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            max_messages: None,
            extension_methods: Vec::new(),
            extension_notifications: Vec::new(),
        }
    }
}

/// The server identity returned from initialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    /// A stable server name.
    pub name: String,
    /// The server version.
    pub version: String,
    /// A display title.
    pub title: Option<String>,
    /// A few sentences on when and how to use the server.
    pub instructions: Option<String>,
}

impl ServerInfo {
    /// An identity without a title or instructions.
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            title: None,
            instructions: None,
        }
    }

    /// Sets the display title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the instructions.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }
}

/// What the client declared during initialization. It is self-reported and
/// identifies no user.
#[derive(Debug, Clone, PartialEq)]
pub struct Client {
    /// `clientInfo.name`.
    pub name: String,
    /// `clientInfo.version`.
    pub version: String,
    /// The protocol version this session uses.
    pub protocol_version: String,
    /// The client's capabilities object.
    pub capabilities: Value,
    /// Whether the client accepts form `elicitation/create` requests.
    pub form_elicitation: bool,
}

/// How a confirmation request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The host reported that its user accepted.
    Accepted,
    /// The host reported that its user declined or dismissed the form.
    Declined,
    /// No confirmation could be obtained: the client lacks form elicitation,
    /// another confirmation is pending, the client answered with an error, or
    /// the connection closed.
    Unavailable,
}

/// What a received message requires of the caller.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    /// Write this reply.
    Reply(Value),
    /// Write this reply, then end the connection.
    Fatal {
        /// The final reply.
        reply: Value,
        /// Why the connection ends.
        reason: &'static str,
    },
    /// Answer with [`Session::accept_initialize`] or [`Session::reject_initialize`].
    Initialize {
        /// The request ID.
        id: Value,
        /// The client's declaration.
        client: Client,
    },
    /// Run a tool call whose name and arguments are valid; reply through [`Session::finish`].
    Tool {
        /// The request ID.
        id: Value,
        /// The call.
        call: ToolCall,
    },
    /// Reply with a failed tool result describing `error`; nothing runs.
    /// The ID remains outstanding: call [`Session::finish`] with it and send
    /// the reply only if that returns `true`, releasing the ID for reuse.
    InvalidArguments {
        /// The request ID.
        id: Value,
        /// The call.
        call: ToolCall,
        /// Why the arguments do not match the tool's input schema.
        error: ArgumentError,
    },
    /// Run a product extension request; reply through [`Session::finish`].
    Method {
        /// The request ID.
        id: Value,
        /// The method.
        method: String,
        /// Its parameters.
        params: Value,
    },
    /// Run a product extension notification; it has no reply.
    Notification {
        /// The method.
        method: String,
        /// Its parameters.
        params: Value,
    },
    /// The client cancelled an outstanding request. Stop it and send no reply.
    Cancel {
        /// The cancelled request's ID.
        id: Value,
    },
    /// The client answered a confirmation request.
    Confirmation {
        /// The ID returned by [`Session::request_confirmation`].
        id: Value,
        /// The answer.
        decision: Decision,
    },
    /// Nothing to do.
    Ignored,
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    New,
    Initializing { id: Value, client: Client },
    Negotiated,
    Ready,
}

/// One MCP connection's protocol state, without I/O.
///
/// Feed each received frame to [`Session::receive`] and act on the returned
/// [`Inbound`]. Send every reply to an [`Inbound::Tool`],
/// [`Inbound::InvalidArguments`] or [`Inbound::Method`] only after
/// [`Session::finish`] returns `true`: it returns `false` once the client
/// has cancelled that request.
#[derive(Debug)]
pub struct Session {
    catalog: Arc<Catalog>,
    options: SessionOptions,
    phase: Phase,
    client: Option<Client>,
    received: u64,
    outstanding: HashSet<String>,
    confirmation: Option<Value>,
    confirmations: u64,
    closed: bool,
}

impl Session {
    /// A new connection serving `catalog`.
    #[must_use]
    pub fn new(catalog: Arc<Catalog>, options: SessionOptions) -> Self {
        Self {
            catalog,
            options,
            phase: Phase::New,
            client: None,
            received: 0,
            outstanding: HashSet::new(),
            confirmation: None,
            confirmations: 0,
            closed: false,
        }
    }

    /// The catalog this session serves.
    #[must_use]
    pub fn catalog(&self) -> &Arc<Catalog> {
        &self.catalog
    }

    /// The initialized client, once initialization succeeded.
    #[must_use]
    pub fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    /// Whether the client sent `notifications/initialized` after initialization.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.phase == Phase::Ready
    }

    /// Requests awaiting a reply or cancellation.
    #[must_use]
    pub fn outstanding(&self) -> usize {
        self.outstanding.len()
    }

    /// Processes one frame, with or without its trailing line break.
    pub fn receive(&mut self, frame: &[u8]) -> Inbound {
        self.received += 1;
        if self
            .options
            .max_messages
            .is_some_and(|limit| self.received > limit)
        {
            return Inbound::Fatal {
                reply: rpc::failure(
                    Value::Null,
                    &RpcError::new(rpc::SERVER_ERROR, "Session message limit reached"),
                ),
                reason: "session message limit reached",
            };
        }
        let frame = frame.strip_suffix(b"\n").unwrap_or(frame);
        match json::parse(frame, self.options.max_message_bytes) {
            Ok(value) => self.receive_value(value),
            Err(ParseError::TooLarge { .. }) => Inbound::Fatal {
                reply: rpc::failure(
                    Value::Null,
                    &RpcError::new(rpc::PARSE_ERROR, "Message exceeds size limit"),
                ),
                reason: "message exceeds size limit",
            },
            Err(ParseError::Syntax | ParseError::Trailing) => Inbound::Reply(rpc::failure(
                Value::Null,
                &RpcError::new(rpc::PARSE_ERROR, "Invalid JSON message"),
            )),
        }
    }

    /// Processes one parsed message. [`Session::receive`] parses frames strictly first.
    pub fn receive_value(&mut self, value: Value) -> Inbound {
        match rpc::classify(value) {
            Err(error) => Inbound::Reply(rpc::failure(Value::Null, &error)),
            Ok(Message::Response { id, outcome }) => self.response(id, outcome),
            Ok(Message::Notification { method, params }) => self.notification(method, params),
            Ok(Message::Request { id, method, params }) => self.request(id, &method, params),
        }
    }

    /// Completes initialization and returns its reply.
    ///
    /// Call this only for the ID of the pending [`Inbound::Initialize`]; any
    /// other call returns an [`rpc::INVALID_REQUEST`] error reply without
    /// changing the session state.
    pub fn accept_initialize(&mut self, id: Value, info: &ServerInfo) -> Value {
        let client = match &self.phase {
            Phase::Initializing {
                id: pending,
                client,
            } if pending == &id => client.clone(),
            _ => {
                return rpc::failure(
                    id,
                    &RpcError::new(
                        rpc::INVALID_REQUEST,
                        "No matching initialization is pending",
                    ),
                );
            }
        };
        let mut server = json!({"name": info.name, "version": info.version});
        if let Some(title) = &info.title {
            server["title"] = json!(title);
        }
        let mut result = json!({
            "protocolVersion": client.protocol_version,
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": server,
        });
        if let Some(instructions) = &info.instructions {
            result["instructions"] = json!(instructions);
        }
        self.phase = Phase::Negotiated;
        self.client = Some(client);
        rpc::success(id, result)
    }

    /// Refuses initialization; the client may initialize again.
    pub fn reject_initialize(&mut self, id: Value, error: &RpcError) -> Value {
        self.phase = Phase::New;
        rpc::failure(id, error)
    }

    /// Releases an outstanding request ID. Returns whether to send its reply.
    pub fn finish(&mut self, id: &Value) -> bool {
        self.outstanding.remove(&id.to_string())
    }

    /// Starts a form confirmation that shows `message` to the host's user.
    ///
    /// Returns the request's ID and the request to send. Its answer arrives as
    /// [`Inbound::Confirmation`].
    ///
    /// # Errors
    ///
    /// Returns [`Decision::Unavailable`] when the client lacks form
    /// elicitation, another confirmation is pending or the session is closed.
    pub fn request_confirmation(&mut self, message: &str) -> Result<(Value, Value), Decision> {
        let Some(client) = &self.client else {
            return Err(Decision::Unavailable);
        };
        if self.closed || !client.form_elicitation || self.confirmation.is_some() {
            return Err(Decision::Unavailable);
        }
        self.confirmations += 1;
        let id = json!(format!("confirmation-{}", self.confirmations));
        let mut params = json!({
            "message": message,
            "requestedSchema": {"type": "object", "properties": {}},
        });
        if client.protocol_version == LATEST_PROTOCOL_VERSION {
            params["mode"] = json!("form");
        }
        self.confirmation = Some(id.clone());
        let request = rpc::request(id.clone(), "elicitation/create", params);
        Ok((id, request))
    }

    /// Withdraws a pending confirmation. Returns the cancellation notice to send.
    pub fn cancel_confirmation(&mut self, id: &Value) -> Option<Value> {
        if self.confirmation.as_ref() != Some(id) {
            return None;
        }
        self.confirmation = None;
        Some(rpc::notification(
            "notifications/cancelled",
            json!({"requestId": id, "reason": "The confirmation request ended"}),
        ))
    }

    /// Marks the connection closed. Later confirmations are unavailable.
    /// Returns the cancellation notice for a pending confirmation.
    pub fn close(&mut self) -> Option<Value> {
        self.closed = true;
        let pending = self.confirmation.clone()?;
        self.cancel_confirmation(&pending)
    }

    fn response(&mut self, id: Value, outcome: Option<Result<Value, Value>>) -> Inbound {
        if self.confirmation.as_ref() != Some(&id) {
            return Inbound::Ignored;
        }
        self.confirmation = None;
        let decision = match outcome {
            Some(Ok(result)) if result["action"] == "accept" => Decision::Accepted,
            Some(Ok(_)) => Decision::Declined,
            Some(Err(_)) | None => Decision::Unavailable,
        };
        Inbound::Confirmation { id, decision }
    }

    fn notification(&mut self, method: String, params: Value) -> Inbound {
        match method.as_str() {
            "notifications/initialized" => {
                if self.phase == Phase::Negotiated && rpc::only(&params, &["_meta"]) {
                    self.phase = Phase::Ready;
                }
                Inbound::Ignored
            }
            "notifications/cancelled" => {
                let id = &params["requestId"];
                if rpc::only(&params, &["requestId", "reason", "_meta"])
                    && rpc::valid_id(id)
                    && params.get("reason").is_none_or(Value::is_string)
                    && self.finish(id)
                {
                    Inbound::Cancel { id: id.clone() }
                } else {
                    Inbound::Ignored
                }
            }
            _ if self.ready() && self.options.extension_notifications.contains(&method) => {
                Inbound::Notification { method, params }
            }
            _ => Inbound::Ignored,
        }
    }

    fn request(&mut self, id: Value, method: &str, params: Value) -> Inbound {
        if self.outstanding.contains(&id.to_string()) {
            return Inbound::Fatal {
                reply: rpc::failure(
                    id,
                    &RpcError::new(rpc::INVALID_REQUEST, "Duplicate outstanding request ID"),
                ),
                reason: "duplicate outstanding request ID",
            };
        }
        let reply = |error: RpcError| Inbound::Reply(rpc::failure(id.clone(), &error));
        match method {
            "ping" if rpc::only(&params, &["_meta"]) => Inbound::Reply(rpc::success(id, json!({}))),
            "ping" => reply(RpcError::new(
                rpc::INVALID_PARAMS,
                "Invalid ping parameters",
            )),
            "initialize" => match self.phase {
                Phase::New => match client(&params) {
                    Ok(client) => {
                        self.phase = Phase::Initializing {
                            id: id.clone(),
                            client: client.clone(),
                        };
                        Inbound::Initialize { id, client }
                    }
                    Err(error) => reply(error),
                },
                _ => reply(RpcError::new(
                    rpc::INVALID_REQUEST,
                    "Server already initialized",
                )),
            },
            _ if !self.ready() => reply(RpcError::new(rpc::SERVER_ERROR, "Server not initialized")),
            "tools/list" if rpc::only(&params, &["_meta"]) => {
                Inbound::Reply(rpc::success(id, self.catalog.listing()))
            }
            "tools/list" => reply(RpcError::new(
                rpc::INVALID_PARAMS,
                "Invalid tool listing parameters",
            )),
            "tools/call" => self.tool_call(id, &params),
            _ if self
                .options
                .extension_methods
                .iter()
                .any(|name| name == method) =>
            {
                self.outstanding.insert(id.to_string());
                Inbound::Method {
                    id,
                    method: method.to_owned(),
                    params,
                }
            }
            _ => reply(RpcError::new(rpc::METHOD_NOT_FOUND, "Method not found")),
        }
    }

    fn tool_call(&mut self, id: Value, params: &Value) -> Inbound {
        let call = match ToolCall::from_params(params) {
            Ok(call) => call,
            Err(error) => return Inbound::Reply(rpc::failure(id, &error)),
        };
        let Some(tool) = self.catalog.get(&call.name) else {
            return Inbound::Reply(rpc::failure(
                id,
                &RpcError::new(rpc::INVALID_PARAMS, "Unknown tool"),
            ));
        };
        let validation = schema::validate(&tool.input_schema, &call.arguments);
        self.outstanding.insert(id.to_string());
        match validation {
            Ok(()) => Inbound::Tool { id, call },
            Err(error) => Inbound::InvalidArguments { id, call, error },
        }
    }
}

fn client(params: &Value) -> Result<Client, RpcError> {
    let invalid = || RpcError::new(rpc::INVALID_PARAMS, "Invalid initialization parameters");
    if !rpc::only(
        params,
        &["protocolVersion", "capabilities", "clientInfo", "_meta"],
    ) {
        return Err(invalid());
    }
    let requested = params["protocolVersion"]
        .as_str()
        .filter(|version| {
            !version.is_empty() && version.len() <= MAX_VERSION_BYTES && !version.contains('\0')
        })
        .ok_or_else(invalid)?;
    let capabilities = &params["capabilities"];
    let info = &params["clientInfo"];
    let (Some(name), Some(version)) = (info["name"].as_str(), info["version"].as_str()) else {
        return Err(invalid());
    };
    if !capabilities.is_object() || !info.is_object() {
        return Err(invalid());
    }
    if name.len() > MAX_CLIENT_FIELD_BYTES || version.len() > MAX_CLIENT_FIELD_BYTES {
        return Err(RpcError::new(
            rpc::INVALID_PARAMS,
            "Client metadata exceeds limit",
        ));
    }
    let protocol_version = if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
        requested
    } else {
        LATEST_PROTOCOL_VERSION
    };
    let form_elicitation = capabilities["elicitation"]
        .as_object()
        .is_some_and(|elicitation| {
            elicitation.is_empty() || elicitation.get("form").is_some_and(Value::is_object)
        });
    Ok(Client {
        name: name.to_owned(),
        version: version.to_owned(),
        protocol_version: protocol_version.to_owned(),
        capabilities: capabilities.clone(),
        form_elicitation,
    })
}
