//! The Streamable HTTP exchange for protocol revision `2026-07-28`.
//!
//! [`Endpoint`] checks one received HTTP request (method, target, headers and
//! body) and either answers it completely or admits its JSON-RPC request with
//! the response form it gets. [`Response`] and [`sse_event`] encode what a
//! runtime writes back. This module opens no sockets and starts no threads, so
//! any HTTP server can call it; the runtime owns connection bounds, timeouts,
//! scheduling and cancellation.
//!
//! ```
//! use canonic_mcp_kit::http::{Endpoint, EndpointOptions, Exchange, Form, RequestHead};
//!
//! let endpoint = Endpoint::new(EndpointOptions::default())?;
//! let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{
//!     "io.modelcontextprotocol/protocolVersion":"2026-07-28",
//!     "io.modelcontextprotocol/clientCapabilities":{}}}}"#;
//! let headers: &[(&str, &[u8])] = &[
//!     ("Content-Type", b"application/json"),
//!     ("Accept", b"application/json, text/event-stream"),
//!     ("MCP-Protocol-Version", b"2026-07-28"),
//!     ("Mcp-Method", b"tools/list"),
//! ];
//! let head = RequestHead { method: "POST", target: "/mcp", headers };
//! let Exchange::Request(admitted) = endpoint.receive(&head, body) else { unreachable!() };
//! assert_eq!(admitted.form, Form::Json);
//! # Ok::<(), canonic_mcp_kit::http::EndpointError>(())
//! ```

use crate::{
    content::decode_base64,
    json, modern,
    rpc::{self, Message, RpcError},
    session::DEFAULT_MAX_MESSAGE_BYTES,
    tool::{Catalog, ParameterKind},
};
use serde_json::{Value, json};
use std::collections::HashSet;

#[cfg(feature = "http-server")]
pub mod server;
#[cfg(feature = "http-server")]
pub use server::{HttpOptions, Shutdown, serve};

#[cfg(test)]
mod tests;

/// The default MCP endpoint path.
pub const DEFAULT_PATH: &str = "/mcp";
/// The longest endpoint path, in bytes.
pub const MAX_PATH_BYTES: usize = 2048;
/// The most origins an endpoint allows.
pub const MAX_ORIGINS: usize = 64;
/// The longest allowed origin, in bytes.
pub const MAX_ORIGIN_BYTES: usize = 2048;
/// The longest decoded `Mcp-Name` value, in bytes.
pub const MAX_NAME_BYTES: usize = 4096;
/// The header that names the request's protocol revision.
pub const PROTOCOL_VERSION_HEADER: &str = "MCP-Protocol-Version";
/// The header that mirrors the request's `method`.
pub const METHOD_HEADER: &str = "Mcp-Method";
/// The header that mirrors a tool or prompt name or a resource URI.
pub const NAME_HEADER: &str = "Mcp-Name";
/// The prefix of headers that mirror `x-mcp-header` tool arguments.
pub const PARAMETER_HEADER_PREFIX: &str = "Mcp-Param-";
/// The largest integer a parameter header compares exactly, 2^53 − 1.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
/// An SSE comment line that keeps a quiet stream open.
pub const SSE_KEEP_ALIVE: &[u8] = b":\n";

/// Where the endpoint is served and whom it accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointOptions {
    /// The exact endpoint path, compared without the query string.
    pub path: String,
    /// Exact browser origins, such as `https://app.example.com`, that may call
    /// the endpoint. A request with any other `Origin` header is refused.
    pub allowed_origins: Vec<String>,
    /// The largest request body, in bytes.
    pub max_body_bytes: usize,
}

impl Default for EndpointOptions {
    fn default() -> Self {
        Self {
            path: DEFAULT_PATH.to_owned(),
            allowed_origins: Vec::new(),
            max_body_bytes: DEFAULT_MAX_MESSAGE_BYTES,
        }
    }
}

/// Why endpoint options are invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EndpointError {
    /// The path is empty, relative, too long or contains a query, fragment or
    /// non-visible ASCII byte.
    #[error("endpoint path must be an absolute path of visible ASCII without a query")]
    Path,
    /// More than [`MAX_ORIGINS`] origins are allowed.
    #[error("at most {MAX_ORIGINS} origins may be allowed")]
    TooManyOrigins,
    /// The origin at this index is not an `http` or `https` scheme and host.
    #[error("allowed origin {0} must be an http or https scheme and host without a path")]
    Origin(usize),
    /// The body limit is zero.
    #[error("the request body limit must be at least one byte")]
    BodyLimit,
}

/// Prepared endpoint rules, validated once and shared by every request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    path: String,
    origins: HashSet<String>,
    max_body_bytes: usize,
}

/// The parts of an HTTP request known before its body is read.
#[derive(Debug, Clone, Copy)]
pub struct RequestHead<'a> {
    /// The request method, such as `POST`.
    pub method: &'a str,
    /// The request target, in origin form (`/mcp?x`) or absolute form.
    pub target: &'a str,
    /// Header fields in received order; names compare case-insensitively.
    pub headers: &'a [(&'a str, &'a [u8])],
}

/// What a received request requires of the runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum Exchange {
    /// Write this complete response; nothing is dispatched.
    Respond(Response),
    /// Answer with [`Response::accepted`]. Route the notification only when the
    /// server lists it as a product notification; it never gets a reply.
    Notification {
        /// The notification method.
        method: String,
        /// Its parameters object.
        params: Value,
    },
    /// Dispatch this request and answer in its form.
    Request(Admitted),
}

/// A request whose HTTP framing and headers passed validation.
#[derive(Debug, Clone, PartialEq)]
pub struct Admitted {
    /// The request ID, scoped to this HTTP request.
    pub id: Value,
    /// The method.
    pub method: String,
    /// The request message to pass to `Session::receive_value`.
    pub message: Value,
    /// How the response is written.
    pub form: Form,
    /// `Mcp-Param-*` fields of a `tools/call`: lowercase suffix and raw value.
    parameters: Vec<(String, Vec<u8>)>,
}

/// How a request's response is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// One `application/json` reply, from [`Response::reply`].
    Json,
    /// An SSE stream: [`Response::stream`], then [`sse_event`] per message,
    /// ending after the final reply.
    Stream,
}

/// A complete HTTP response, or the head of a stream.
///
/// The runtime adds framing fields such as `Content-Length` and `Connection`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// The status code.
    pub status: u16,
    /// Header fields to send.
    pub headers: Vec<(&'static str, &'static str)>,
    /// The body; empty for no body and for a stream head.
    pub body: Vec<u8>,
}

impl Endpoint {
    /// Validates `options` once.
    ///
    /// # Errors
    /// Returns [`EndpointError`] for an invalid path, origin or body limit.
    pub fn new(options: EndpointOptions) -> Result<Self, EndpointError> {
        if !valid_path(&options.path) {
            return Err(EndpointError::Path);
        }
        if options.allowed_origins.len() > MAX_ORIGINS {
            return Err(EndpointError::TooManyOrigins);
        }
        if let Some(index) = options
            .allowed_origins
            .iter()
            .position(|origin| !valid_origin(origin))
        {
            return Err(EndpointError::Origin(index));
        }
        if options.max_body_bytes == 0 {
            return Err(EndpointError::BodyLimit);
        }
        Ok(Self {
            path: options.path,
            origins: options.allowed_origins.into_iter().collect(),
            max_body_bytes: options.max_body_bytes,
        })
    }

    /// The largest accepted request body, in bytes.
    #[must_use]
    pub fn max_body_bytes(&self) -> usize {
        self.max_body_bytes
    }

    /// Checks the path, method, `Origin`, `Content-Type` and `Accept` before the
    /// body is read, as a runtime answering `Expect: 100-continue` needs.
    ///
    /// # Errors
    /// Returns the complete refusal to write instead of reading the body.
    pub fn check_head(&self, head: &RequestHead<'_>) -> Result<(), Response> {
        if request_path(head.target) != self.path {
            return Err(Response::empty(404));
        }
        if head.method != "POST" {
            let mut response = Response::empty(405);
            response.headers.push(("Allow", "POST"));
            return Err(response);
        }
        let allowed = match field(head.headers, "origin") {
            Field::Missing => true,
            Field::One(origin) => {
                std::str::from_utf8(origin).is_ok_and(|origin| self.origins.contains(origin))
            }
            Field::Repeated => false,
        };
        if !allowed {
            return Err(Response::failure(
                403,
                Value::Null,
                &RpcError::new(rpc::SERVER_ERROR, "Origin is not allowed"),
            ));
        }
        let json = matches!(field(head.headers, "content-type"), Field::One(value)
            if media_type(value).eq_ignore_ascii_case("application/json"));
        if !json {
            return Err(Response::empty(415));
        }
        if !accepts_both(head.headers) {
            return Err(Response::empty(406));
        }
        Ok(())
    }

    /// Checks a declared body length before the body is read.
    ///
    /// # Errors
    /// Returns the `413` refusal for a body above [`Self::max_body_bytes`].
    pub fn check_length(&self, length: usize) -> Result<(), Response> {
        if length > self.max_body_bytes {
            return Err(Response::failure(
                413,
                Value::Null,
                &RpcError::new(rpc::PARSE_ERROR, "Message exceeds the size limit"),
            ));
        }
        Ok(())
    }

    /// Checks a complete request and decides what the runtime does with it.
    ///
    /// Tool parameter headers depend on the served catalog; check them with
    /// [`Admitted::check_parameters`] before dispatching a request.
    #[must_use]
    pub fn receive(&self, head: &RequestHead<'_>, body: &[u8]) -> Exchange {
        if let Err(response) = self
            .check_head(head)
            .and_then(|()| self.check_length(body.len()))
        {
            return Exchange::Respond(response);
        }
        let Ok(value) = json::parse(body, self.max_body_bytes) else {
            return refuse(400, rpc::PARSE_ERROR, "Invalid JSON");
        };
        let (id, method, params) = match rpc::classify(value) {
            Ok(Message::Request { id, method, params }) => (id, method, params),
            Ok(Message::Notification { method, params }) => {
                return Exchange::Notification { method, params };
            }
            Ok(Message::Response { .. }) | Err(_) => {
                return refuse(400, rpc::INVALID_REQUEST, "Invalid request");
            }
        };
        if let Err(error) = check_headers(head.headers, &method, &params) {
            return Exchange::Respond(Response::failure(status_for(error.code), id, &error));
        }
        let form = if method == "subscriptions/listen"
            || params
                .get("_meta")
                .is_some_and(|meta| meta.get("progressToken").is_some())
        {
            Form::Stream
        } else {
            Form::Json
        };
        let parameters = if method == "tools/call" {
            head.headers
                .iter()
                .filter_map(|(name, value)| {
                    let prefix = name.get(..PARAMETER_HEADER_PREFIX.len())?;
                    prefix
                        .eq_ignore_ascii_case(PARAMETER_HEADER_PREFIX)
                        .then(|| {
                            let suffix = &name[PARAMETER_HEADER_PREFIX.len()..];
                            (suffix.to_ascii_lowercase(), value.to_vec())
                        })
                })
                .collect()
        } else {
            Vec::new()
        };
        Exchange::Request(Admitted {
            message: rpc::request(id.clone(), &method, params),
            id,
            method,
            form,
            parameters,
        })
    }
}

impl Admitted {
    /// Checks `Mcp-Param-*` headers against the called tool's `x-mcp-header`
    /// annotations in `catalog`, the catalog that will serve the call.
    ///
    /// A header that matches no annotation is ignored. Requests other than
    /// `tools/call`, and calls to unknown tools, pass unchanged.
    ///
    /// # Errors
    /// Returns the `400` `-32020` refusal for a missing, repeated, unexpected,
    /// malformed or mismatched parameter header.
    pub fn check_parameters(&self, catalog: &Catalog) -> Result<(), Response> {
        if self.method != "tools/call" {
            return Ok(());
        }
        let params = &self.message["params"];
        let Some(headers) = params["name"]
            .as_str()
            .and_then(|name| catalog.parameter_headers(name))
        else {
            return Ok(());
        };
        for header in headers {
            let mut values = self
                .parameters
                .iter()
                .filter(|(key, _)| *key == header.key)
                .map(|(_, value)| value.as_slice());
            let (sent, repeated) = (values.next(), values.next().is_some());
            let body = header
                .path
                .iter()
                .try_fold(&params["arguments"], |value, key| value.get(key))
                .filter(|value| !value.is_null());
            let matched = !repeated
                && match (body, sent) {
                    (None, None) => true,
                    (Some(body), Some(sent)) => {
                        decode(sent).is_some_and(|sent| same_value(header.kind, body, &sent))
                    }
                    _ => false,
                };
            if !matched {
                let error = RpcError::new(
                    rpc::HEADER_MISMATCH,
                    format!(
                        "Header mismatch: {PARAMETER_HEADER_PREFIX}{} is missing, repeated, unexpected or does not match the arguments",
                        header.name
                    ),
                );
                return Err(Response::failure(400, self.id.clone(), &error));
            }
        }
        Ok(())
    }
}

/// Whether a decoded header equals the body value. A body value of another
/// type is left to argument validation.
fn same_value(kind: ParameterKind, body: &Value, sent: &str) -> bool {
    match kind {
        ParameterKind::String => body.as_str().is_none_or(|body| body == sent),
        ParameterKind::Boolean => body
            .as_bool()
            .is_none_or(|body| sent == if body { "true" } else { "false" }),
        ParameterKind::Integer => {
            let Some(body) = body.as_f64().filter(|body| body.fract() == 0.0) else {
                return true;
            };
            let sent = serde_json::from_str::<serde_json::Number>(sent)
                .ok()
                .and_then(|number| number.as_f64())
                .filter(|sent| sent.fract() == 0.0 && sent.abs() <= MAX_SAFE_INTEGER);
            body.abs() <= MAX_SAFE_INTEGER && sent == Some(body)
        }
    }
}

impl Response {
    fn empty(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn failure(status: u16, id: Value, error: &RpcError) -> Self {
        // A small error object always encodes; an empty body is the fallback.
        Self::json(
            status,
            serde_json::to_vec(&rpc::failure(id, error)).unwrap_or_default(),
        )
    }

    /// `202 Accepted` with no body, for an accepted notification.
    #[must_use]
    pub fn accepted() -> Self {
        Self::empty(202)
    }

    /// An `application/json` response with `body`.
    #[must_use]
    pub fn json(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: vec![
                ("Content-Type", "application/json"),
                ("Cache-Control", "no-store"),
            ],
            body,
        }
    }

    /// The JSON response for an encoded reply. Pass the JSON-RPC error code
    /// when the reply is an error, so the status follows [`status_for`].
    #[must_use]
    pub fn reply(body: Vec<u8>, error_code: Option<i32>) -> Self {
        Self::json(error_code.map_or(200, status_for), body)
    }

    /// Encodes a reply value within `limit` bytes and chooses its status.
    ///
    /// # Errors
    /// Returns [`json::EncodeError`] when the reply cannot be encoded within `limit`.
    pub fn reply_value(value: &Value, limit: usize) -> Result<Self, json::EncodeError> {
        let code = value
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64)
            .and_then(|code| i32::try_from(code).ok());
        Ok(Self::reply(json::encode(value, limit)?, code))
    }

    /// The head of an SSE response stream.
    #[must_use]
    pub fn stream() -> Self {
        Self {
            status: 200,
            headers: vec![
                ("Content-Type", "text/event-stream"),
                ("Cache-Control", "no-store"),
                ("X-Accel-Buffering", "no"),
            ],
            body: Vec::new(),
        }
    }
}

/// The HTTP status for a JSON-RPC error code.
///
/// Header, version and capability errors and malformed messages are `400`, an
/// unknown method is `404`, and every other error is `200`.
#[must_use]
pub fn status_for(code: i32) -> u16 {
    match code {
        rpc::HEADER_MISMATCH
        | rpc::MISSING_CLIENT_CAPABILITY
        | rpc::UNSUPPORTED_PROTOCOL_VERSION
        | rpc::INVALID_REQUEST
        | rpc::PARSE_ERROR => 400,
        rpc::METHOD_NOT_FOUND => 404,
        _ => 200,
    }
}

/// One SSE event carrying one compact JSON message.
#[must_use]
pub fn sse_event(frame: &[u8]) -> Vec<u8> {
    let mut event = Vec::with_capacity(frame.len() + 23);
    event.extend_from_slice(b"event: message\ndata: ");
    event.extend_from_slice(frame);
    event.extend_from_slice(b"\n\n");
    event
}

fn refuse(status: u16, code: i32, message: &str) -> Exchange {
    Exchange::Respond(Response::failure(
        status,
        Value::Null,
        &RpcError::new(code, message),
    ))
}

/// Checks the standard MCP headers against the request body.
fn check_headers(headers: &[(&str, &[u8])], method: &str, params: &Value) -> Result<(), RpcError> {
    let mismatch = |header: &str, problem: &str| {
        RpcError::new(
            rpc::HEADER_MISMATCH,
            format!("Header mismatch: {header} {problem}"),
        )
    };
    let Field::One(version) = field(headers, PROTOCOL_VERSION_HEADER) else {
        return Err(mismatch(PROTOCOL_VERSION_HEADER, "is missing or repeated"));
    };
    let Some(version) = plain(version) else {
        return Err(mismatch(PROTOCOL_VERSION_HEADER, "is not a plain value"));
    };
    if version != modern::PROTOCOL_VERSION {
        return Err(RpcError::new(
            rpc::UNSUPPORTED_PROTOCOL_VERSION,
            "Unsupported protocol version",
        )
        .with_data(json!({
            "requested": version,
            "supported": [modern::PROTOCOL_VERSION],
        })));
    }
    let body_version = params
        .get("_meta")
        .and_then(|meta| meta.get(modern::VERSION_KEY))
        .and_then(Value::as_str);
    if body_version != Some(version) {
        return Err(mismatch(PROTOCOL_VERSION_HEADER, "does not match the body"));
    }
    if !matches!(field(headers, METHOD_HEADER), Field::One(value) if value == method.as_bytes()) {
        return Err(mismatch(
            METHOD_HEADER,
            "is missing or does not match the body",
        ));
    }
    let source = match method {
        "tools/call" | "prompts/get" => Some("name"),
        "resources/read" => Some("uri"),
        _ => None,
    };
    let named = match (source, field(headers, NAME_HEADER)) {
        (None, Field::Missing) => true,
        (Some(key), Field::One(value)) => decode(value)
            .is_some_and(|name| params.get(key).and_then(Value::as_str) == Some(name.as_str())),
        _ => false,
    };
    if !named {
        return Err(mismatch(
            NAME_HEADER,
            "is missing, unexpected or does not match the body",
        ));
    }
    Ok(())
}

enum Field<'a> {
    Missing,
    One(&'a [u8]),
    Repeated,
}

fn field<'a>(headers: &[(&str, &'a [u8])], name: &str) -> Field<'a> {
    let mut found = Field::Missing;
    for (field_name, value) in headers {
        if field_name.eq_ignore_ascii_case(name) {
            found = match found {
                Field::Missing => Field::One(value),
                Field::One(_) | Field::Repeated => Field::Repeated,
            };
        }
    }
    found
}

/// A nonempty value of visible ASCII and inner spaces.
fn plain(value: &[u8]) -> Option<&str> {
    let printable = value.iter().all(|byte| (0x20..=0x7e).contains(byte));
    let trimmed = value.first() != Some(&b' ') && value.last() != Some(&b' ');
    if value.is_empty() || !printable || !trimmed {
        return None;
    }
    std::str::from_utf8(value).ok()
}

/// A plain value, or the UTF-8 text of a `=?base64?…?=` value.
fn decode(value: &[u8]) -> Option<String> {
    let text = plain(value)?;
    match text
        .strip_prefix("=?base64?")
        .and_then(|rest| rest.strip_suffix("?="))
    {
        Some(encoded) => String::from_utf8(decode_base64(encoded, MAX_NAME_BYTES).ok()?).ok(),
        None => (text.len() <= MAX_NAME_BYTES).then(|| text.to_owned()),
    }
}

/// An RFC 9110 token, as in a method or header name.
pub(crate) fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

fn media_type(value: &[u8]) -> &str {
    std::str::from_utf8(value)
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
}

/// Whether the `Accept` fields allow both JSON and SSE responses.
fn accepts_both(headers: &[(&str, &[u8])]) -> bool {
    let (mut json, mut stream) = (false, false);
    let ranges = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("accept"))
        .filter_map(|(_, value)| std::str::from_utf8(value).ok())
        .flat_map(|value| value.split(','));
    for range in ranges {
        let mut parts = range.split(';');
        let media = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
        let refused = parts.any(|parameter| {
            let parameter = parameter.trim();
            parameter
                .get(..2)
                .is_some_and(|name| name.eq_ignore_ascii_case("q="))
                && parameter[2..]
                    .trim()
                    .parse::<f32>()
                    .is_ok_and(|quality| quality <= 0.0)
        });
        if refused {
            continue;
        }
        match media.as_str() {
            "*/*" => (json, stream) = (true, true),
            "application/*" | "application/json" => json = true,
            "text/*" | "text/event-stream" => stream = true,
            _ => {}
        }
    }
    json && stream
}

/// The path of an origin-form or absolute-form target, without query or fragment.
fn request_path(target: &str) -> &str {
    let path = target
        .strip_prefix("http://")
        .or_else(|| target.strip_prefix("https://"))
        .map_or(target, |rest| {
            rest.find('/').map_or("/", |index| &rest[index..])
        });
    path.split(['?', '#']).next().unwrap_or(path)
}

fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= MAX_PATH_BYTES
        && path
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'?' && byte != b'#')
}

fn valid_origin(origin: &str) -> bool {
    origin.len() <= MAX_ORIGIN_BYTES
        && origin.bytes().all(|byte| byte.is_ascii_graphic())
        && origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
            .is_some_and(|host| !host.is_empty() && !host.contains(['/', '?', '#', '@']))
}
