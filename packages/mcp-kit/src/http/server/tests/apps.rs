//! Apps over the real HTTP adapter, including the shared request scheduler.
use super::*;
use crate::http::{
    BearerToken, Operation, Rejection, ResourceOptions, ResourceServer, ScopePolicy, TokenVerifier,
};
use crate::{AppsCatalog, UiResource, apps};

pub(super) const URI: &str = "ui://acme/status";
const HTML: &str =
    "<!doctype html><html lang=\"en\"><title>Acme status</title><p>Healthy</p></html>";

fn options(html: &str) -> Options {
    Options {
        session: SessionOptions {
            apps: Some(Arc::new(
                AppsCatalog::new(vec![UiResource::new(URI, "Status", html)]).unwrap(),
            )),
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            ..SessionOptions::default()
        },
        ..Options::default()
    }
}

fn params(mut params: Value, supported: bool) -> Value {
    params["_meta"][crate::modern::CAPABILITIES_KEY] = if supported {
        json!({"extensions":{apps::EXTENSION:{"mimeTypes":[apps::MIME_TYPE]}}})
    } else {
        json!({})
    };
    params
}

fn read(id: u64) -> Vec<u8> {
    post(
        "resources/read",
        &json!(id),
        params(json!({"uri":URI}), true),
        "Mcp-Name: ui://acme/status\r\n",
    )
}

#[test]
fn apps_http_discovery_views_calls_and_exact_html() {
    let server = Server::configured(http(), options(HTML));
    let discovery = server
        .exchange(&post("server/discover", &json!(0), json!({}), ""))
        .json();
    assert_eq!(
        discovery["result"]["capabilities"]["extensions"][apps::EXTENSION],
        json!({})
    );
    for supported in [true, false, true] {
        let listed = server
            .exchange(&post(
                "tools/list",
                &json!(1),
                params(json!({}), supported),
                "",
            ))
            .json();
        assert_eq!(
            listed["result"]["tools"].as_array().unwrap().len(),
            if supported { 6 } else { 5 }
        );
        let called = server
            .exchange(&post(
                "tools/call",
                &json!(2),
                params(json!({"name":"acme_whoami"}), supported),
                "",
            ))
            .json();
        if supported {
            assert_eq!(called["result"]["content"][0]["text"], "anonymous");
        } else {
            assert_eq!(called["error"]["code"], rpc::INVALID_PARAMS);
        }
    }
    let html = server.exchange(&read(3));
    assert_eq!(html.status, 200);
    assert_eq!(html.json()["result"]["contents"][0]["text"], HTML);
    assert_eq!(
        html.json()["result"]["contents"][0]["mimeType"],
        apps::MIME_TYPE
    );
    let missing = server
        .exchange(&post(
            "resources/read",
            &json!(4),
            json!({"uri":"ui://acme/missing"}),
            "Mcp-Name: ui://acme/missing\r\n",
        ))
        .json();
    assert_eq!(missing["error"]["code"], rpc::INVALID_PARAMS);
    assert_eq!(missing["error"]["data"]["uri"], "ui://acme/missing");
}

#[test]
fn apps_http_overflow_has_a_bounded_error_without_partial_html() {
    let mut options = options(&"x".repeat(4096));
    options.max_output_bytes = 1024;
    let server = Server::configured(http(), options);
    let reply = server.exchange(&read(1));
    assert!(reply.body.len() <= 1024);
    assert_eq!(reply.json()["error"]["code"], rpc::INTERNAL_ERROR);
    assert!(reply.json().get("result").is_none());
    let empty = server.exchange(&post(
        "tools/call",
        &json!(2),
        params(json!({"name":"acme_echo","arguments":{"empty":true}}), true),
        "",
    ));
    assert_eq!(empty.json()["error"]["code"], rpc::INTERNAL_ERROR);
}

#[test]
fn apps_http_queued_read_deadline_releases_request_ids() {
    let mut options = options(HTML);
    options.timeout = Duration::from_millis(200);
    let server = Server::configured(http(), options);
    // One default worker: the queued UI read cannot finish until the blocking tool ends.
    let mut blocking = server.connect();
    blocking
        .write_all(&call(
            &json!(1),
            "acme_block",
            &json!({"timeoutMs":5000}),
            "",
        ))
        .unwrap();
    assert_eq!(server.next_event(), Ok("started"));
    let mut queued = server.connect();
    queued.write_all(&read(2)).unwrap();
    let expired = Reply::read(&mut BufReader::new(queued));
    assert_eq!(expired.json()["error"]["code"], rpc::DEADLINE_EXCEEDED);
    drop(blocking);
    assert_eq!(server.next_event(), Ok("cancelled"));
    // Each POST has its own ID namespace. Reuse succeeds after the timeout.
    assert_eq!(
        server.exchange(&read(2)).json()["result"]["contents"][0]["text"],
        HTML
    );
}

#[test]
fn apps_http_progress_and_connection_closure_do_not_capture_neighbor_reads() {
    let server = Server::configured(http(), options(HTML));
    let mut progress = BufReader::new(server.connect());
    progress
        .get_mut()
        .write_all(&post(
            "tools/call",
            &json!(1),
            params(
                json!({"name":"acme_progress","_meta":{"progressToken":"tool"}}),
                true,
            ),
            "",
        ))
        .unwrap();
    assert_eq!(
        Reply::read(&mut progress).header("content-type"),
        Some("text/event-stream")
    );
    assert_eq!(
        event(&mut progress).unwrap()["params"]["progressToken"],
        "tool"
    );
    // Closing one response does not close the shared session or another POST's ID.
    drop(progress);
    let mut blocking = server.connect();
    blocking
        .write_all(&post(
            "tools/call",
            &json!(1),
            params(json!({"name":"acme_block"}), true),
            "",
        ))
        .unwrap();
    assert_eq!(server.next_event(), Ok("started"));
    blocking.shutdown(std::net::Shutdown::Write).unwrap();
    assert_eq!(server.next_event(), Ok("cancelled"));
    let mut reading = server.connect();
    reading.write_all(&read(1)).unwrap();
    assert_eq!(
        Reply::read(&mut BufReader::new(reading)).json()["result"]["contents"][0]["text"],
        HTML
    );
}

struct Access;
impl TokenVerifier for Access {
    fn verify(&self, token: BearerToken<'_>, resource: &str) -> Result<Grant, Rejection> {
        match token.as_str() {
            "acme-reader" => Ok(Grant::new(vec![resource.into()])
                .with_scopes(["acme:read"])
                .with_subject("reader")),
            "acme-narrow" => Ok(Grant::new(vec![resource.into()])),
            _ => Err(Rejection::invalid()),
        }
    }
}
impl ScopePolicy for Access {
    fn required(&self, operation: &Operation<'_>) -> Vec<String> {
        if matches!(operation.method, "resources/read" | "tools/call") {
            vec!["acme:read".into()]
        } else {
            vec![]
        }
    }
}

#[test]
fn apps_http_reads_and_frame_calls_keep_the_resource_server_authorization_boundary() {
    let resource = ResourceServer::new(ResourceOptions::new(
        "http://127.0.0.1/mcp",
        vec!["https://auth.example.com".into()],
    ))
    .unwrap();
    let http = HttpOptions {
        authorization: Some(
            HttpAuthorization::new(resource, Arc::new(Access)).with_policy(Arc::new(Access)),
        ),
        ..http()
    };
    let server = Server::configured(http, options(HTML));
    for (token, status) in [
        ("", 401),
        ("acme-invalid", 401),
        ("acme-narrow", 403),
        ("acme-reader", 200),
    ] {
        let header = if token.is_empty() {
            String::new()
        } else {
            format!("Authorization: Bearer {token}\r\n")
        };
        let read = server.exchange(&post(
            "resources/read",
            &json!(1),
            params(json!({"uri":URI}), true),
            &format!("Mcp-Name: {URI}\r\n{header}"),
        ));
        let call = server.exchange(&post(
            "tools/call",
            &json!(2),
            params(json!({"name":"acme_whoami"}), true),
            &header,
        ));
        assert_eq!(read.status, status);
        assert_eq!(call.status, status);
        if status == 200 {
            assert_eq!(read.json()["result"]["contents"][0]["text"], HTML);
            assert_eq!(call.json()["result"]["content"][0]["text"], "reader");
        }
    }
}
