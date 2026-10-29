//! JSON-RPC 2.0 message shapes, identifiers, error codes and reply encoding.

use crate::{
    control::Interrupted,
    json::{self, EncodeError},
};
use serde::Serialize;
use serde_json::{Value, json};

/// The input is not JSON or exceeds the message limit.
pub const PARSE_ERROR: i32 = -32700;
/// The message is not a valid JSON-RPC request, or reuses an outstanding ID.
pub const INVALID_REQUEST: i32 = -32600;
/// The method is not supported.
pub const METHOD_NOT_FOUND: i32 = -32601;
/// The method's parameters are invalid, including an unknown tool name.
pub const INVALID_PARAMS: i32 = -32602;
/// The server failed to produce a reply.
pub const INTERNAL_ERROR: i32 = -32603;
/// The server refused the request: not initialized, at capacity or closing.
pub const SERVER_ERROR: i32 = -32000;
/// The request reached its deadline before it produced a reply.
pub const DEADLINE_EXCEEDED: i32 = -32001;
/// A consumer could not resolve or authorize a resource URI.
pub const RESOURCE_NOT_FOUND: i32 = -32002;
/// The request was cancelled because the connection closed.
pub const REQUEST_CANCELLED: i32 = -32800;

/// The longest accepted string request ID, in bytes.
pub const MAX_ID_BYTES: usize = 128;

/// A JSON-RPC error object.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    /// One of the codes in this module, or a product-defined server error code.
    pub code: i32,
    /// A short description of the error.
    pub message: String,
    /// Optional structured details.
    pub data: Option<Value>,
}

impl RpcError {
    /// Creates an error without data.
    #[must_use]
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Attaches structured details.
    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// The JSON error object.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut error = json!({"code": self.code, "message": self.message});
        if let Some(data) = &self.data {
            error["data"] = data.clone();
        }
        error
    }
}

/// One classified incoming message.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A request expecting a reply.
    Request {
        /// The caller's request ID.
        id: Value,
        /// The method name.
        method: String,
        /// The parameters object; `{}` when absent.
        params: Value,
    },
    /// A notification, which receives no reply.
    Notification {
        /// The method name.
        method: String,
        /// The parameters object; `{}` when absent.
        params: Value,
    },
    /// A reply to a request this server sent.
    Response {
        /// The ID of the server's request.
        id: Value,
        /// `Ok(result)` or `Err(error object)`; `None` when the reply is malformed.
        outcome: Option<Result<Value, Value>>,
    },
}

/// Classifies one parsed message.
///
/// Requests and notifications may contain only `jsonrpc`, `id`, `method` and
/// `params`; `params` must be an object.
///
/// # Errors
///
/// Returns an [`INVALID_REQUEST`] error for anything else.
pub fn classify(value: Value) -> Result<Message, RpcError> {
    let invalid = || RpcError::new(INVALID_REQUEST, "Invalid request");
    let Value::Object(mut object) = value else {
        return Err(invalid());
    };
    if object.get("jsonrpc") != Some(&json!("2.0")) {
        return Err(invalid());
    }
    if !object.contains_key("method") && object.contains_key("id") {
        let id = object.remove("id").unwrap_or(Value::Null);
        let well_formed = object
            .keys()
            .all(|key| ["jsonrpc", "result", "error"].contains(&key.as_str()))
            && object.contains_key("result") != object.contains_key("error");
        let outcome = well_formed.then(|| match object.remove("result") {
            Some(result) => Ok(result),
            None => Err(object.remove("error").unwrap_or(Value::Null)),
        });
        return Ok(Message::Response { id, outcome });
    }
    if !object
        .keys()
        .all(|key| ["jsonrpc", "id", "method", "params"].contains(&key.as_str()))
    {
        return Err(invalid());
    }
    let Some(Value::String(method)) = object.remove("method") else {
        return Err(invalid());
    };
    let params = match object.remove("params") {
        None => json!({}),
        Some(params @ Value::Object(_)) => params,
        Some(_) => return Err(invalid()),
    };
    match object.remove("id") {
        None => Ok(Message::Notification { method, params }),
        Some(id) if valid_id(&id) => Ok(Message::Request { id, method, params }),
        Some(_) => Err(invalid()),
    }
}

/// Whether `id` is a string of at most [`MAX_ID_BYTES`] or an integer.
#[must_use]
pub fn valid_id(id: &Value) -> bool {
    id.as_str().is_some_and(|text| text.len() <= MAX_ID_BYTES)
        || id.as_i64().is_some()
        || id.as_u64().is_some()
}

/// Whether `value` is an object containing only `fields`, with any `_meta` an object.
#[must_use]
pub fn only(value: &Value, fields: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.keys().all(|key| fields.contains(&key.as_str()))
            && object.get("_meta").is_none_or(Value::is_object)
    })
}

/// A success reply.
#[must_use]
pub fn success(id: Value, result: Value) -> Value {
    message([("id", id), ("result", result)])
}

/// An error reply.
#[must_use]
pub fn failure(id: Value, error: &RpcError) -> Value {
    message([("id", id), ("error", error.to_value())])
}

/// A notification.
#[must_use]
pub fn notification(method: &str, params: Value) -> Value {
    message([("method", json!(method)), ("params", params)])
}

/// A request sent by the server.
#[must_use]
pub fn request(id: Value, method: &str, params: Value) -> Value {
    message([("id", id), ("method", json!(method)), ("params", params)])
}

/// A JSON-RPC 2.0 message moving `fields` into place without copying them.
fn message<const N: usize>(fields: [(&str, Value); N]) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("jsonrpc".into(), json!("2.0"));
    for (key, value) in fields {
        object.insert(key.into(), value);
    }
    Value::Object(object)
}

/// Encodes a success reply for `result` without first copying it into a [`Value`].
///
/// # Errors
///
/// As [`json::encode_checked`].
pub fn encode_success(
    id: &Value,
    result: &(impl Serialize + ?Sized),
    limit: usize,
    check: &dyn Fn() -> Result<(), Interrupted>,
) -> Result<Vec<u8>, EncodeError> {
    #[derive(Serialize)]
    struct Envelope<'a, T: ?Sized> {
        jsonrpc: &'static str,
        id: &'a Value,
        result: &'a T,
    }
    json::encode_checked(
        &Envelope {
            jsonrpc: "2.0",
            id,
            result,
        },
        limit,
        check,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_notifications_and_responses_are_classified() {
        assert_eq!(
            classify(json!({"jsonrpc":"2.0","id":"a","method":"ping"})),
            Ok(Message::Request {
                id: json!("a"),
                method: "ping".into(),
                params: json!({})
            })
        );
        assert_eq!(
            classify(json!({"jsonrpc":"2.0","method":"notifications/initialized"})),
            Ok(Message::Notification {
                method: "notifications/initialized".into(),
                params: json!({})
            })
        );
        assert_eq!(
            classify(json!({"jsonrpc":"2.0","id":3,"result":{"action":"accept"}})),
            Ok(Message::Response {
                id: json!(3),
                outcome: Some(Ok(json!({"action":"accept"})))
            })
        );
        assert_eq!(
            classify(json!({"jsonrpc":"2.0","id":3,"result":{},"error":{}})),
            Ok(Message::Response {
                id: json!(3),
                outcome: None
            })
        );
    }

    #[test]
    fn malformed_requests_are_invalid() {
        for value in [
            json!([]),
            json!({"id":1,"method":"ping"}),
            json!({"jsonrpc":"2.0","id":1,"method":"ping","extra":true}),
            json!({"jsonrpc":"2.0","id":null,"method":"ping"}),
            json!({"jsonrpc":"2.0","id":1.5,"method":"ping"}),
            json!({"jsonrpc":"2.0","id":"x".repeat(MAX_ID_BYTES + 1),"method":"ping"}),
            json!({"jsonrpc":"2.0","id":1,"method":"ping","params":[]}),
            json!({"jsonrpc":"2.0","id":1,"method":7}),
        ] {
            assert_eq!(
                classify(value.clone()).unwrap_err().code,
                INVALID_REQUEST,
                "{value}"
            );
        }
    }

    #[test]
    fn encoded_success_matches_the_value_form() {
        let id = json!(7);
        let bytes = encode_success(&id, &json!({"ok":true}), 1024, &|| Ok(())).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            success(id, json!({"ok":true}))
        );
        let error = RpcError::new(SERVER_ERROR, "busy").with_data(json!({"retry":true}));
        assert_eq!(
            failure(json!(1), &error)["error"],
            json!({"code":-32000,"message":"busy","data":{"retry":true}})
        );
    }
}
