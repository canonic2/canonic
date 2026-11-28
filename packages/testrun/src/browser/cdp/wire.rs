//! CDP JSON envelopes and flattened-session validation; no public authority.

use super::{MAX_COMMAND_BYTES, MAX_MESSAGE_BYTES};
use serde_json::{Value, json};
use std::io::{self, Write};

pub(super) const MAX_ID: u64 = 9_007_199_254_740_991;

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error("CDP command failed (code {code}); inspect private diagnostics")]
    Command {
        code: i64,
        message: String,
        data: Option<Value>,
    },
}

pub(super) enum Incoming {
    Reply {
        id: u64,
        session: Option<String>,
        result: Result<Value, RemoteError>,
    },
    Event(super::Event),
}

pub(super) fn method_valid(method: &str) -> bool {
    let Some((domain, name)) = method.split_once('.') else {
        return false;
    };
    [domain, name].iter().all(|part| {
        !part.is_empty()
            && part.len() <= 128
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    })
}

pub(super) fn session_valid(session: &str) -> bool {
    !session.is_empty() && session.len() <= 1024 && !session.chars().any(char::is_control)
}

pub(super) fn command(
    id: u64,
    session: Option<&str>,
    method: &str,
    params: Value,
    fixture_payload: bool,
) -> Result<String, super::Error> {
    if !method_valid(method)
        || !params.is_object()
        || session.is_some_and(|session| !session_valid(session))
    {
        return Err(super::Error::InvalidCommand);
    }
    let mut message = json!({"id":id,"method":method});
    message["params"] = params;
    if let Some(session) = session {
        message["sessionId"] = json!(session);
    }
    // A bounded 1 MiB evidence body expands by 4/3 in its base64 envelope.
    let limit = if fixture_payload {
        crate::browser::transfer::FIXTURE_FRAME_BYTES
    } else if method == "Fetch.fulfillRequest" {
        2 * MAX_COMMAND_BYTES
    } else {
        MAX_COMMAND_BYTES
    };
    let mut output = Limited(Vec::new(), limit);
    serde_json::to_writer(&mut output, &message).map_err(|_| super::Error::CommandBounds)?;
    String::from_utf8(output.0).map_err(|_| super::Error::InvalidCommand)
}

struct Limited(Vec<u8>, usize);
impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.1 - self.0.len() {
            return Err(io::Error::other("CDP command exceeds its byte budget"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn incoming(text: &str) -> Result<Incoming, super::Loss> {
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(super::Loss::Protocol);
    }
    let value: Value = serde_json::from_str(text).map_err(|_| super::Loss::Protocol)?;
    let object = value.as_object().ok_or(super::Loss::Protocol)?;
    let session = object
        .get("sessionId")
        .map(|value| {
            value
                .as_str()
                .filter(|value| session_valid(value))
                .map(str::to_owned)
                .ok_or(super::Loss::Protocol)
        })
        .transpose()?;
    if let Some(id) = object.get("id") {
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "id" | "sessionId" | "result" | "error"))
        {
            return Err(super::Loss::Protocol);
        }
        let id = id
            .as_u64()
            .filter(|id| *id > 0 && *id <= MAX_ID)
            .ok_or(super::Loss::Protocol)?;
        let result = match (object.get("result"), object.get("error")) {
            (Some(result), None) if result.is_object() => Ok(result.clone()),
            (None, Some(error)) => {
                let error = error.as_object().ok_or(super::Loss::Protocol)?;
                if error
                    .keys()
                    .any(|key| !matches!(key.as_str(), "code" | "message" | "data"))
                {
                    return Err(super::Loss::Protocol);
                }
                Err(RemoteError::Command {
                    code: error
                        .get("code")
                        .and_then(Value::as_i64)
                        .ok_or(super::Loss::Protocol)?,
                    message: error
                        .get("message")
                        .and_then(Value::as_str)
                        .ok_or(super::Loss::Protocol)?
                        .into(),
                    data: error.get("data").cloned(),
                })
            }
            _ => return Err(super::Loss::Protocol),
        };
        Ok(Incoming::Reply {
            id,
            session,
            result,
        })
    } else {
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "method" | "params" | "sessionId"))
        {
            return Err(super::Loss::Protocol);
        }
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .filter(|method| method_valid(method))
            .ok_or(super::Loss::Protocol)?;
        let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
        if !params.is_object() {
            return Err(super::Loss::Protocol);
        }
        Ok(Incoming::Event(super::Event {
            method: method.into(),
            session_id: session,
            params,
        }))
    }
}
