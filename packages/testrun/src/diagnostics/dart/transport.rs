//! One owned, sequential VM Service client. No reconnect, redirects, DNS or RPC forwarding.
use super::*;
use futures_util::{FutureExt, SinkExt, StreamExt};
use std::{
    net::{IpAddr, SocketAddr},
    path::Path,
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::WebSocketConfig},
};

pub(super) struct Endpoint {
    pub address: SocketAddr,
    pub uri: String,
    secret: String,
}
impl Endpoint {
    pub fn read(path: &Path, unauthenticated: bool) -> Result<Self> {
        let bytes = crate::artifacts::read_private_file(path, 4096).map_err(|e| {
            if e.code == "PRIVATE_FILE_UNAVAILABLE" {
                Error::new(
                    "DART_ENDPOINT_FILE",
                    "Cannot read selected private VM Service URI file",
                )
            } else {
                e
            }
        })?;
        let uri = std::str::from_utf8(&bytes)
            .map_err(|_| Error::invalid("VM Service URI file must be UTF-8"))?;
        Self::parse(uri.trim(), unauthenticated)
    }
    fn parse(uri: &str, unauthenticated: bool) -> Result<Self> {
        let invalid = || {
            Error::invalid(
                "Select ws://numeric-loopback:explicit-port/TOKEN/ws in endpointFile; forwarding is external, credentials/query/fragment/DNS are refused",
            )
        };
        let url = url::Url::parse(uri).map_err(|_| invalid())?;
        let ip: IpAddr = url
            .host_str()
            .ok_or_else(invalid)?
            .trim_matches(['[', ']'])
            .parse()
            .map_err(|_| invalid())?;
        if uri.len() > 4096
            || uri.chars().any(char::is_control)
            || url.scheme() != "ws"
            || !ip.is_loopback()
            || url.port().is_none_or(|p| p == 0)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        let parts: Vec<_> = url.path().split('/').collect();
        let secret = match parts.as_slice() {
            ["", token, "ws"]
                if !token.is_empty()
                    && token.len() <= 512
                    && token
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-=+".contains(&b)) =>
            {
                token.to_string()
            }
            ["", "ws"] if unauthenticated => String::new(),
            _ => return Err(invalid()),
        };
        Ok(Self {
            address: SocketAddr::new(ip, url.port().unwrap()),
            uri: url.to_string(),
            secret,
        })
    }
    pub fn redact(&self, value: &mut Value) {
        match value {
            Value::String(s) => {
                *s = s.replace(&self.uri, "[redacted-vm-service]");
                if !self.secret.is_empty() {
                    *s = s.replace(&self.secret, "[redacted-service-token]");
                }
            }
            Value::Array(a) => {
                for v in a {
                    self.redact(v)
                }
            }
            Value::Object(o) => {
                // A producer can also put a URI in a key.
                let fields = std::mem::take(o);
                for (key, mut v) in fields {
                    self.redact(&mut v);
                    let mut key = Value::String(key);
                    self.redact(&mut key);
                    o.insert(key.as_str().unwrap().into(), v);
                }
            }
            _ => {}
        }
    }
}
pub(super) struct Client {
    socket: WebSocketStream<tokio::net::TcpStream>,
    next: u64,
    closed: bool,
}
fn admitted(method: &str, params: &Value) -> bool {
    let Some(fields) = params.as_object() else {
        return false;
    };
    let keys = |names: &[&str]| {
        fields.len() == names.len() && fields.keys().all(|k| names.contains(&k.as_str()))
    };
    let isolate = params["isolateId"]
        .as_str()
        .is_some_and(|s| !s.is_empty() && s.len() <= 256);
    let object = |key: &str| {
        params[key]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= 256)
    };
    let window = || {
        params["timeOriginMicros"].as_u64().is_some()
            && params["timeExtentMicros"]
                .as_u64()
                .is_some_and(|n| n <= 300_000_000)
    };
    match method {
        "getVersion" | "getVM" | "getVMTimelineMicros" | "getProcessMemoryUsage" => {
            fields.is_empty()
        }
        "getIsolate"
        | "getMemoryUsage"
        | "getAllocationProfile"
        | "ext.dart.io.getHttpProfile"
        | "ext.dart.io.httpEnableTimelineLogging"
        | "ext.flutter.inspector.isWidgetCreationTracked"
        | "ext.flutter.inspector.isWidgetTreeReady"
        | "ext.flutter.inspector.trackRebuildDirtyWidgets"
        | "ext.flutter.inspector.trackRepaintWidgets" => keys(&["isolateId"]) && isolate,
        "streamListen" => {
            keys(&["streamId"])
                && params["streamId"].as_str().is_some_and(|s| {
                    [
                        "Isolate",
                        "Debug",
                        "Logging",
                        "GC",
                        "Extension",
                        "Stdout",
                        "Stderr",
                    ]
                    .contains(&s)
                })
        }
        "getStack" => keys(&["isolateId", "limit"]) && isolate && params["limit"] == 64,
        "getCpuSamples" => {
            keys(&["isolateId", "timeOriginMicros", "timeExtentMicros"]) && isolate && window()
        }
        "getVMTimeline" => keys(&["timeOriginMicros", "timeExtentMicros"]) && window(),
        "getInstances" => {
            keys(&[
                "isolateId",
                "objectId",
                "limit",
                "includeSubclasses",
                "includeImplementers",
            ]) && isolate
                && object("objectId")
                && params["limit"] == 16
                && params["includeSubclasses"] == false
                && params["includeImplementers"] == false
        }
        "getObject" => {
            keys(&["isolateId", "objectId", "offset", "count"])
                && isolate
                && object("objectId")
                && params["offset"] == 0
                && params["count"] == 32
        }
        "getRetainingPath" | "getInboundReferences" => {
            keys(&["isolateId", "targetId", "limit"])
                && isolate
                && object("targetId")
                && params["limit"] == 32
        }
        "ext.flutter.inspector.getRootWidget" | "ext.flutter.inspector.disposeGroup" => {
            keys(&["isolateId", "objectGroup"])
                && isolate
                && params["objectGroup"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("testrun-") && s.len() <= 64)
        }
        "ext.flutter.inspector.getDetailsSubtree" => {
            keys(&["isolateId", "objectGroup", "arg", "subtreeDepth"])
                && isolate
                && object("arg")
                && params["subtreeDepth"] == "1"
                && params["objectGroup"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("testrun-") && s.len() <= 64)
        }
        _ => false,
    }
}
impl Client {
    pub async fn open(endpoint: &Endpoint, deadline: tokio::time::Instant) -> Result<Self> {
        let stream =
            tokio::time::timeout_at(deadline, tokio::net::TcpStream::connect(endpoint.address))
                .await
                .map_err(|_| {
                    Error::new(
                        "DART_CONNECT_TIMEOUT",
                        "Selected VM Service connection timed out",
                    )
                })?
                .map_err(|_| {
                    Error::new(
                        "DART_ENDPOINT_UNAVAILABLE",
                        "Selected VM Service is unavailable; no alternative endpoint was tried",
                    )
                })?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(super::super::analysis::MAX_VM_REPLY))
            .max_frame_size(Some(super::super::analysis::MAX_VM_REPLY));
        let (socket, _) = tokio::time::timeout_at(
            deadline,
            tokio_tungstenite::client_async_with_config(
                endpoint.uri.as_str(),
                stream,
                Some(config),
            ),
        )
        .await
        .map_err(|_| Error::new("DART_CONNECT_TIMEOUT", "VM Service upgrade timed out"))?
        .map_err(|_| {
            Error::new(
                "DART_AUTH_OR_UPGRADE",
                "VM Service refused the selected URI; credentials were not printed or retried",
            )
        })?;
        Ok(Self {
            socket,
            next: 0,
            closed: false,
        })
    }
    pub async fn send(
        &mut self,
        method: &str,
        params: Value,
        deadline: tokio::time::Instant,
    ) -> Result<u64> {
        if !admitted(method, &params) {
            return Err(Error::invalid(
                "VM Service method/parameters are outside the fixed bounded read contract",
            ));
        }
        if self.closed {
            return Err(Error::new(
                "DART_CONNECTION_LOST",
                "VM Service connection is closed",
            ));
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| Error::invalid("VM request IDs exhausted"))?;
        tokio::time::timeout_at(
            deadline,
            self.socket.send(Message::Text(
                json!({"jsonrpc":"2.0","id":self.next.to_string(),"method":method,"params":params})
                    .to_string()
                    .into(),
            )),
        )
        .await
        .map_err(|_| {
            Error::new(
                "DART_REQUEST_TIMEOUT",
                "VM request write timed out; not retried",
            )
        })?
        .map_err(|_| {
            Error::new(
                "DART_CONNECTION_LOST",
                "VM request write failed; not retried",
            )
        })?;
        Ok(self.next)
    }
    pub async fn receive(&mut self, deadline: tokio::time::Instant) -> Result<Value> {
        loop {
            let message = tokio::time::timeout_at(deadline, self.socket.next())
                .await
                .map_err(|_| {
                    Error::new(
                        "DART_REQUEST_TIMEOUT",
                        "VM Service read timed out; not retried",
                    )
                })?;
            match message {
                Some(Ok(Message::Text(text))) => {
                    let value = super::super::analysis::vm_service_json(text.as_bytes())?;
                    if value["jsonrpc"] != "2.0" {
                        return Err(Error::new(
                            "DART_PROTOCOL",
                            "Expected VM Service JSON-RPC 2.0",
                        ));
                    }
                    return Ok(value);
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                // The WebSocket stream ends after a refused message; report its size bound.
                Some(Err(tokio_tungstenite::tungstenite::Error::Capacity(capacity))) => {
                    self.closed = true;
                    let size = match capacity {
                        tokio_tungstenite::tungstenite::error::CapacityError::MessageTooLong {
                            size,
                            ..
                        } => Some(size),
                        _ => None,
                    };
                    return Err(Error::new(
                        "DART_RESULT_BOUNDS",
                        "VM reply exceeds the 32 MiB message limit; the connection is not reused",
                    )
                    .details(json!({ "replyBytes": size })));
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                    self.closed = true;
                    return Err(Error::new(
                        "DART_CONNECTION_LOST",
                        "VM Service disconnected; select a fresh generation without replay",
                    ));
                }
                _ => {
                    return Err(Error::new(
                        "DART_PROTOCOL",
                        "Unexpected VM Service frame; binary heap streams require their own decoder",
                    ));
                }
            }
        }
    }
    pub fn try_receive(&mut self) -> Option<Result<Value>> {
        for _ in 0..8 {
            let message = self.socket.next().now_or_never()?;
            match message {
                Some(Ok(Message::Text(text))) => {
                    return Some(
                        super::super::analysis::vm_service_json(text.as_bytes()).and_then(|v| {
                            if v["jsonrpc"] == "2.0" {
                                Ok(v)
                            } else {
                                Err(Error::new(
                                    "DART_PROTOCOL",
                                    "Expected VM Service JSON-RPC 2.0",
                                ))
                            }
                        }),
                    );
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                    self.closed = true;
                    return Some(Err(Error::new(
                        "DART_CONNECTION_LOST",
                        "VM Service disconnected; no reconnect was attempted",
                    )));
                }
                _ => {
                    return Some(Err(Error::new(
                        "DART_PROTOCOL",
                        "Unexpected VM Service frame",
                    )));
                }
            }
        }
        Some(Err(Error::new(
            "DART_EVENT_BOUNDS",
            "Control-frame flood exhausted drain budget",
        )))
    }
    pub async fn close(&mut self) -> bool {
        if self.closed {
            return true;
        }
        self.closed = true;
        tokio::time::timeout(Duration::from_secs(1), self.socket.close(None))
            .await
            .is_ok_and(|r| r.is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_dart_endpoint_scope_and_recursive_credential_redaction() {
        for (method, params) in [
            ("resume", json!({"isolateId":"isolates/1"})),
            (
                "evaluate",
                json!({"isolateId":"isolates/1","expression":"arbitrary()"}),
            ),
            (
                "getAllocationProfile",
                json!({"isolateId":"isolates/1","gc":true}),
            ),
            (
                "ext.dart.io.httpEnableTimelineLogging",
                json!({"isolateId":"isolates/1","enabled":true}),
            ),
            (
                "getObject",
                json!({"isolateId":"isolates/1","objectId":"objects/1","count":999999,"offset":0}),
            ),
        ] {
            assert!(!admitted(method, &params));
        }
        assert!(admitted(
            "getRetainingPath",
            &json!({"isolateId":"isolates/1","targetId":"objects/1","limit":32})
        ));
        for uri in [
            "ws://localhost:8081/a=/ws",
            "ws://192.0.2.1:8081/a=/ws",
            "wss://127.0.0.1:8081/a=/ws",
            "ws://u:p@127.0.0.1:8081/a=/ws",
            "ws://127.0.0.1:8081/a=/ws?auth=x",
            "ws://127.0.0.1:8081/ws",
        ] {
            assert!(Endpoint::parse(uri, false).is_err());
        }
        let endpoint = Endpoint::parse("ws://127.0.0.1:18081/acme-secret=/ws", false).unwrap();
        let mut payload = json!({"ws://127.0.0.1:18081/acme-secret=/ws":["token acme-secret=",{"message":"ws://127.0.0.1:18081/acme-secret=/ws"}]});
        endpoint.redact(&mut payload);
        assert!(!payload.to_string().contains("acme-secret="));
        assert!(Endpoint::parse("ws://127.0.0.1:18081/ws", true).is_ok());
    }
}
