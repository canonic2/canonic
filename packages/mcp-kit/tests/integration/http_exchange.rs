//! An independent runtime drives the Streamable HTTP exchange without the
//! shared driver: it owns the session, scopes request IDs to each POST and
//! encodes JSON and SSE responses through the exchange's encoders.

use canonic_mcp_kit::{
    Catalog, Content, Inbound, ModernOptions, ServerInfo, Session, SessionOptions, Tool,
    ToolResult,
    http::{
        Endpoint, EndpointOptions, Exchange, Form, METHOD_HEADER, NAME_HEADER,
        PROTOCOL_VERSION_HEADER, RequestHead, Response, sse_event,
    },
    rpc,
};
use serde_json::{Value, json};
use std::sync::Arc;

const LIMIT: usize = 64 * 1024;

/// One session for every POST; each admitted request gets a fresh session ID,
/// and replies are encoded with the client's own ID.
struct Runtime {
    endpoint: Endpoint,
    session: Session,
    next: u64,
    /// Calls admitted but not yet answered: session ID, client ID and form.
    running: Vec<(Value, Value, Form, String)>,
}

impl Runtime {
    fn new() -> Self {
        let catalog = Catalog::new(vec![Tool::new(
            "acme_echo",
            "Return the given Acme text.",
            json!({"type":"object","properties":{"text":{"type":"string","x-mcp-header":"Text"}}}),
        )])
        .expect("valid Acme catalog");
        let options = SessionOptions {
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1.0.0"))),
            ..SessionOptions::default()
        };
        Self {
            endpoint: Endpoint::new(EndpointOptions::default()).expect("default endpoint"),
            session: Session::try_new(Arc::new(catalog), options).expect("modern session"),
            next: 0,
            running: Vec::new(),
        }
    }

    /// Answers at once, or admits a tool call to [`Self::finish`] later.
    fn post(&mut self, headers: &[(&str, &[u8])], body: &[u8]) -> Option<Response> {
        let head = RequestHead {
            method: "POST",
            target: "/mcp",
            headers,
        };
        let admitted = match self.endpoint.receive(&head, body) {
            Exchange::Respond(response) => return Some(response),
            Exchange::Notification { .. } => return Some(Response::accepted()),
            Exchange::Request(admitted) => admitted,
        };
        if let Err(response) = admitted.check_parameters(self.session.catalog()) {
            return Some(response);
        }
        self.next += 1;
        let mut message = admitted.message.clone();
        message["id"] = json!(self.next);
        match self.session.receive_value(message) {
            Inbound::Tool { id, call } => {
                let text = call.arguments["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                self.running.push((id, admitted.id, admitted.form, text));
                None
            }
            Inbound::Reply(mut reply) => {
                reply["id"] = admitted.id;
                Some(Response::reply_value(&reply, LIMIT).expect("reply fits"))
            }
            other => panic!("unexpected inbound {other:?}"),
        }
    }

    /// Runs the oldest admitted call and encodes its reply in its form.
    fn finish(&mut self) -> Response {
        let (id, client, form, text) = self.running.remove(0);
        let context = self
            .session
            .request_context(&id)
            .expect("modern request context");
        let result = ToolResult::success(vec![Content::text(text)]);
        let bytes = context
            .encode_success(&client, &result, LIMIT, &|| Ok(()))
            .expect("result fits");
        assert!(self.session.finish(&id));
        match form {
            Form::Json => Response::reply(bytes, None),
            Form::Stream => Response {
                body: sse_event(&bytes),
                ..Response::stream()
            },
        }
    }
}

fn body(id: &Value, method: &str, mut params: Value) -> Vec<u8> {
    params["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("2026-07-28");
    params["_meta"]["io.modelcontextprotocol/clientCapabilities"] = json!({});
    serde_json::to_vec(&rpc::request(id.clone(), method, params)).expect("request encodes")
}

fn headers<'a>(method: &'a str, extra: &[(&'a str, &'a [u8])]) -> Vec<(&'a str, &'a [u8])> {
    let mut headers = vec![
        ("Content-Type", b"application/json".as_slice()),
        ("Accept", b"application/json, text/event-stream".as_slice()),
        (PROTOCOL_VERSION_HEADER, b"2026-07-28".as_slice()),
        (METHOD_HEADER, method.as_bytes()),
    ];
    headers.extend_from_slice(extra);
    headers
}

fn reply(response: &Response) -> Value {
    serde_json::from_slice(&response.body).expect("JSON reply body")
}

#[test]
fn an_independent_runtime_serves_the_exchange_without_the_driver() {
    let mut runtime = Runtime::new();
    let listed = runtime
        .post(
            &headers("tools/list", &[]),
            &body(&json!("l"), "tools/list", json!({})),
        )
        .unwrap();
    assert_eq!(
        (listed.status, reply(&listed)["id"].clone()),
        (200, json!("l"))
    );

    let call = |id: &Value, text: &str| {
        body(
            id,
            "tools/call",
            json!({"name":"acme_echo","arguments":{"text":text}}),
        )
    };
    let fields = |text: &'static [u8]| {
        headers(
            "tools/call",
            &[(NAME_HEADER, b"acme_echo"), ("Mcp-Param-Text", text)],
        )
    };
    // Two POSTs reuse one client ID while both are outstanding.
    assert!(
        runtime
            .post(&fields(b"one"), &call(&json!(1), "one"))
            .is_none()
    );
    assert!(
        runtime
            .post(&fields(b"two"), &call(&json!(1), "two"))
            .is_none()
    );
    for text in ["one", "two"] {
        let answered = runtime.finish();
        let answered = reply(&answered);
        assert_eq!(answered["id"], 1);
        assert_eq!(answered["result"]["content"][0]["text"], text);
    }

    let mismatch = runtime
        .post(&fields(b"other"), &call(&json!(2), "one"))
        .unwrap();
    assert_eq!(
        (mismatch.status, reply(&mismatch)["error"]["code"].clone()),
        (400, json!(-32020))
    );
    let missing = runtime
        .post(
            &headers("acme/missing", &[]),
            &body(&json!(3), "acme/missing", json!({})),
        )
        .unwrap();
    assert_eq!(missing.status, 404);

    let progress = body(
        &json!(4),
        "tools/call",
        json!({"name":"acme_echo","arguments":{"text":"p"},"_meta":{"progressToken":"t"}}),
    );
    assert!(runtime.post(&fields(b"p"), &progress).is_none());
    let streamed = runtime.finish();
    assert_eq!(streamed.headers, Response::stream().headers);
    let event = String::from_utf8(streamed.body).unwrap();
    let data = event
        .strip_prefix("event: message\ndata: ")
        .and_then(|rest| rest.strip_suffix("\n\n"))
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(data).unwrap()["id"], 4);
}
