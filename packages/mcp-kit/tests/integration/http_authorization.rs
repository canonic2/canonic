//! An independent runtime protects the exchange with its own verifier: it
//! checks the head, verifies the token off its request thread (as an
//! asynchronous runtime would) between the head check and admission, then
//! applies the grant and scope checks before the session sees the request.

use canonic_mcp_kit::{
    Catalog, Inbound, ModernOptions, ServerInfo, Session, SessionOptions, Tool,
    http::{
        Endpoint, EndpointOptions, Exchange, Grant, METHOD_HEADER, NAME_HEADER, Operation,
        PROTOCOL_VERSION_HEADER, Rejection, RequestHead, ResourceOptions, ResourceServer, Response,
        ScopePolicy,
    },
    rpc,
};
use serde_json::{Value, json};
use std::{sync::Arc, thread};

const RESOURCE: &str = "https://mcp.example.com/mcp";

/// Introspection stand-in: knows one reader token and one writer token.
fn introspect(token: &str) -> Result<Grant, Rejection> {
    let grant = Grant::new(vec![RESOURCE.to_owned()]);
    match token {
        "acme-reader" => Ok(grant.with_scopes(["acme:read"]).with_subject("reader")),
        "acme-writer" => Ok(grant
            .with_scopes(["acme:read", "acme:write"])
            .with_subject("writer")),
        _ => Err(Rejection::invalid()),
    }
}

/// `acme_write` needs `acme:write`.
struct Policy;

impl ScopePolicy for Policy {
    fn required(&self, operation: &Operation<'_>) -> Vec<String> {
        if operation.name == Some("acme_write") {
            vec!["acme:write".to_owned()]
        } else {
            Vec::new()
        }
    }
}

struct Runtime {
    endpoint: Endpoint,
    session: Session,
    next: u64,
}

impl Runtime {
    fn new() -> Self {
        let catalog = Catalog::new(vec![Tool::new(
            "acme_write",
            "Write an Acme record.",
            json!({"type":"object"}),
        )])
        .expect("valid Acme catalog");
        let mut resource =
            ResourceOptions::new(RESOURCE, vec!["https://auth.example.com".to_owned()]);
        resource.required_scopes = vec!["acme:read".to_owned()];
        Self {
            endpoint: Endpoint::new(EndpointOptions {
                authorization: Some(ResourceServer::new(resource).expect("valid resource")),
                ..EndpointOptions::default()
            })
            .expect("protected endpoint"),
            session: Session::try_new(
                Arc::new(catalog),
                SessionOptions {
                    modern: Some(ModernOptions::new(ServerInfo::new("acme", "1.0.0"))),
                    ..SessionOptions::default()
                },
            )
            .expect("modern session"),
            next: 0,
        }
    }

    /// Answers a POST; a tool call that passes every check answers with the
    /// grant's subject.
    fn post(&mut self, headers: &[(&str, &[u8])], body: &[u8]) -> Response {
        let head = RequestHead {
            method: "POST",
            target: "/mcp",
            headers,
        };
        if let Err(response) = self.endpoint.check_head(&head) {
            return response;
        }
        let server = self.endpoint.authorization().expect("protected");
        let token = server
            .bearer(&head)
            .expect("checked with the head")
            .as_str()
            .to_owned();
        // Verified elsewhere before the body is used, as an async runtime would.
        let outcome = thread::spawn(move || introspect(&token))
            .join()
            .expect("verification finishes");
        let grant = match server.check_grant(outcome, Some(&Policy as &dyn ScopePolicy)) {
            Ok(grant) => grant,
            Err(response) => return response,
        };
        let admitted = match self.endpoint.receive(&head, body) {
            Exchange::Respond(response) => return response,
            Exchange::Notification { .. } => return Response::accepted(),
            Exchange::Request(admitted) => admitted,
        };
        let checked = admitted
            .check_parameters(self.session.catalog())
            .and_then(|()| server.check_scopes(&admitted, &grant, &Policy));
        if let Err(response) = checked {
            return response;
        }
        self.next += 1;
        let mut message = admitted.message.clone();
        message["id"] = json!(self.next);
        let reply = match self.session.receive_value(message) {
            Inbound::Tool { id, .. } => {
                assert!(self.session.finish(&id));
                rpc::success(admitted.id, json!({"subject": grant.subject()}))
            }
            Inbound::Reply(mut reply) => {
                reply["id"] = admitted.id;
                reply
            }
            other => panic!("unexpected inbound {other:?}"),
        };
        Response::reply_value(&reply, 64 * 1024).expect("reply fits")
    }
}

fn post(runtime: &mut Runtime, token: Option<&str>) -> Response {
    let mut params = json!({"name": "acme_write", "arguments": {}});
    params["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("2026-07-28");
    params["_meta"]["io.modelcontextprotocol/clientCapabilities"] = json!({});
    let body = serde_json::to_vec(&rpc::request(json!(1), "tools/call", params)).expect("encodes");
    let authorization = token.map(|token| format!("Bearer {token}"));
    let mut headers = vec![
        ("Content-Type", b"application/json".as_slice()),
        ("Accept", b"application/json, text/event-stream".as_slice()),
        (PROTOCOL_VERSION_HEADER, b"2026-07-28".as_slice()),
        (METHOD_HEADER, b"tools/call".as_slice()),
        (NAME_HEADER, b"acme_write".as_slice()),
    ];
    if let Some(authorization) = &authorization {
        headers.push(("Authorization", authorization.as_bytes()));
    }
    runtime.post(&headers, &body)
}

fn challenge(response: &Response) -> &str {
    response
        .headers
        .iter()
        .find(|(name, _)| *name == "WWW-Authenticate")
        .map(|(_, value)| value.as_ref())
        .expect("a challenge")
}

#[test]
fn an_independent_runtime_verifies_tokens_between_the_head_and_admission() {
    let mut runtime = Runtime::new();
    let missing = post(&mut runtime, None);
    assert_eq!(missing.status, 401);
    assert!(challenge(&missing).contains("scope=\"acme:read\""));
    let invalid = post(&mut runtime, Some("acme-unknown"));
    assert_eq!(invalid.status, 401);
    assert!(challenge(&invalid).starts_with("Bearer error=\"invalid_token\""));
    let reader = post(&mut runtime, Some("acme-reader"));
    assert_eq!(reader.status, 403);
    assert!(challenge(&reader).contains("scope=\"acme:write\""));
    let writer = post(&mut runtime, Some("acme-writer"));
    assert_eq!(writer.status, 200);
    let reply: Value = serde_json::from_slice(&writer.body).expect("JSON reply");
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["subject"], "writer");
}
