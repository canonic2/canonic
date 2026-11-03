//! Each validation step's status and error, both sides of every header rule,
//! response forms and encoding.
use super::*;
use crate::{Catalog, Inbound, ModernOptions, ServerInfo, Session, SessionOptions};
use std::sync::Arc;

const ACCEPT: &[u8] = b"application/json, text/event-stream";

fn endpoint() -> Endpoint {
    Endpoint::new(EndpointOptions {
        allowed_origins: vec!["https://app.example.com".to_owned()],
        ..EndpointOptions::default()
    })
    .unwrap()
}

/// A modern request body with `params` and the protocol version in `_meta`.
fn body(id: Value, method: &str, mut params: Value) -> Vec<u8> {
    params["_meta"][modern::VERSION_KEY] = json!(modern::PROTOCOL_VERSION);
    params["_meta"]["io.modelcontextprotocol/clientCapabilities"] = json!({});
    serde_json::to_vec(&rpc::request(id, method, params)).unwrap()
}

/// Valid headers for `method`, plus `extra`.
fn headers<'a>(method: &'a str, extra: &[(&'a str, &'a [u8])]) -> Vec<(&'a str, &'a [u8])> {
    let mut headers = vec![
        ("Content-Type", b"application/json".as_slice()),
        ("Accept", ACCEPT),
        (PROTOCOL_VERSION_HEADER, b"2026-07-28".as_slice()),
        (METHOD_HEADER, method.as_bytes()),
    ];
    headers.extend_from_slice(extra);
    headers
}

fn receive(headers: &[(&str, &[u8])], body: &[u8]) -> Exchange {
    endpoint().receive(
        &RequestHead {
            method: "POST",
            target: "/mcp",
            headers,
        },
        body,
    )
}

/// The status and JSON-RPC error code of a refusal.
type Refusal = (u16, Option<i64>);

fn refusal(exchange: &Exchange) -> Refusal {
    let Exchange::Respond(response) = exchange else {
        panic!("expected a response, got {exchange:?}");
    };
    let code = serde_json::from_slice::<Value>(&response.body)
        .ok()
        .and_then(|reply| reply["error"]["code"].as_i64());
    (response.status, code)
}

fn admitted(exchange: Exchange) -> Admitted {
    let Exchange::Request(admitted) = exchange else {
        panic!("expected an admitted request, got {exchange:?}");
    };
    admitted
}

#[test]
fn path_and_method_are_checked_before_anything_else() {
    let endpoint = endpoint();
    let head = |method, target| RequestHead {
        method,
        target,
        headers: &[],
    };
    assert_eq!(
        endpoint.check_head(&head("POST", "/other")).unwrap_err(),
        Response::empty(404)
    );
    for method in ["GET", "DELETE", "PUT", "post"] {
        let refused = endpoint.check_head(&head(method, "/mcp")).unwrap_err();
        assert_eq!(refused.status, 405, "{method}");
        assert_eq!(refused.headers, vec![("Allow", "POST")]);
    }
    let valid = headers("tools/list", &[]);
    for target in ["/mcp?trace=1", "http://localhost:8080/mcp", "/mcp#top"] {
        let head = RequestHead {
            method: "POST",
            target,
            headers: &valid,
        };
        assert_eq!(endpoint.check_head(&head), Ok(()), "{target}");
    }
}

#[test]
fn origin_is_refused_unless_absent_or_allowed() {
    let request = body(json!(1), "tools/list", json!({}));
    let allowed = headers("tools/list", &[("Origin", b"https://app.example.com")]);
    assert!(matches!(receive(&allowed, &request), Exchange::Request(_)));
    for origins in [
        vec![("origin", b"https://evil.example.com".as_slice())],
        vec![("Origin", b"null".as_slice())],
        vec![
            ("Origin", b"https://app.example.com".as_slice()),
            ("Origin", b"https://app.example.com".as_slice()),
        ],
    ] {
        let refused = receive(&headers("tools/list", &origins), &request);
        assert_eq!(refusal(&refused), (403, Some(-32000)), "{origins:?}");
    }
}

/// A tools/list request with this content type and these `Accept` fields.
fn negotiate(content_type: &[u8], accept: &[&[u8]]) -> Exchange {
    let mut fields: Vec<(&str, &[u8])> = vec![
        ("Content-Type", content_type),
        (PROTOCOL_VERSION_HEADER, b"2026-07-28"),
        (METHOD_HEADER, b"tools/list"),
    ];
    fields.extend(accept.iter().map(|value| ("accept", *value)));
    receive(&fields, &body(json!(1), "tools/list", json!({})))
}

#[test]
fn content_type_and_accept_must_allow_json_and_streams() {
    assert!(matches!(
        negotiate(b"Application/JSON; charset=utf-8", &[ACCEPT]),
        Exchange::Request(_)
    ));
    assert_eq!(refusal(&negotiate(b"text/plain", &[ACCEPT])), (415, None));
    let allowed: [&[&[u8]]; 3] = [
        &[b"*/*"],
        &[b"application/json", b"text/event-stream"],
        &[b"application/*;q=0.5, text/*"],
    ];
    for accept in allowed {
        assert!(
            matches!(negotiate(b"application/json", accept), Exchange::Request(_)),
            "{accept:?}"
        );
    }
    let refused: [&[&[u8]]; 3] = [
        &[],
        &[b"application/json"],
        &[b"application/json, text/event-stream;q=0"],
    ];
    for accept in refused {
        assert_eq!(
            refusal(&negotiate(b"application/json", accept)),
            (406, None),
            "{accept:?}"
        );
    }
}

#[test]
fn body_size_framing_and_message_shape_are_refused_without_an_id() {
    let endpoint = Endpoint::new(EndpointOptions {
        max_body_bytes: 64,
        ..EndpointOptions::default()
    })
    .unwrap();
    let fields = headers("tools/list", &[]);
    let head = RequestHead {
        method: "POST",
        target: "/mcp",
        headers: &fields,
    };
    let oversized = endpoint.receive(&head, &[b' '; 65]);
    assert_eq!(refusal(&oversized), (413, Some(-32700)));
    let cases: [(&[u8], Refusal); 4] = [
        (b"{", (400, Some(-32700))),
        (br#"{"a":1,"a":2}"#, (400, Some(-32700))),
        (
            br#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#,
            (400, Some(-32600)),
        ),
        (
            br#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
            (400, Some(-32600)),
        ),
    ];
    for (input, expected) in cases {
        let refused = receive(&fields, input);
        assert_eq!(refusal(&refused), expected);
        let Exchange::Respond(response) = refused else {
            unreachable!()
        };
        let reply: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(reply["id"], Value::Null);
    }
}

#[test]
fn notifications_are_accepted_without_mcp_headers() {
    let fields = [
        ("Content-Type", b"application/json".as_slice()),
        ("Accept", ACCEPT),
    ];
    let notification = br#"{"jsonrpc":"2.0","method":"acme/changed","params":{"n":1}}"#;
    assert_eq!(
        receive(&fields, notification),
        Exchange::Notification {
            method: "acme/changed".to_owned(),
            params: json!({"n": 1}),
        }
    );
    assert_eq!(Response::accepted().status, 202);
    assert_eq!(Response::accepted().body, Vec::<u8>::new());
}

#[test]
fn protocol_version_header_must_name_the_modern_revision_and_match_the_body() {
    let request = body(json!("a"), "tools/list", json!({}));
    let without = |name: &str| -> Vec<(&'static str, &'static [u8])> {
        headers("tools/list", &[])
            .into_iter()
            .filter(|(field, _)| !field.eq_ignore_ascii_case(name))
            .collect()
    };
    let missing = receive(&without(PROTOCOL_VERSION_HEADER), &request);
    assert_eq!(refusal(&missing), (400, Some(-32020)));
    let Exchange::Respond(response) = missing else {
        unreachable!()
    };
    let reply: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(reply["id"], "a", "the request ID is known once classified");

    let repeated = headers("tools/list", &[("mcp-protocol-version", b"2026-07-28")]);
    assert_eq!(refusal(&receive(&repeated, &request)), (400, Some(-32020)));

    let mut legacy = without(PROTOCOL_VERSION_HEADER);
    legacy.push((PROTOCOL_VERSION_HEADER, b"2025-11-25"));
    let refused = receive(&legacy, &request);
    assert_eq!(refusal(&refused), (400, Some(-32022)));
    let Exchange::Respond(response) = refused else {
        unreachable!()
    };
    let reply: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(
        reply["error"]["data"],
        json!({"requested": "2025-11-25", "supported": ["2026-07-28"]})
    );

    let initialize = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"acme","version":"1"}}}"#;
    let fields = headers("initialize", &[]);
    assert_eq!(
        refusal(&receive(&fields, initialize)),
        (400, Some(-32020)),
        "a legacy body has no modern version to match"
    );
    let mut other = serde_json::from_slice::<Value>(&request).unwrap();
    other["params"]["_meta"][modern::VERSION_KEY] = json!("2026-01-01");
    let other = serde_json::to_vec(&other).unwrap();
    assert_eq!(
        refusal(&receive(&headers("tools/list", &[]), &other)),
        (400, Some(-32020))
    );
}

#[test]
fn method_header_must_match_the_body_exactly() {
    let request = body(json!(1), "tools/list", json!({}));
    let names_match = headers("tools/list", &[]);
    let renamed: Vec<_> = names_match
        .iter()
        .map(|(name, value)| {
            let name = if *name == METHOD_HEADER {
                "mcp-method"
            } else {
                *name
            };
            (name, *value)
        })
        .collect();
    assert!(matches!(receive(&renamed, &request), Exchange::Request(_)));
    for method in ["Tools/List", "tools/call"] {
        assert_eq!(
            refusal(&receive(&headers(method, &[]), &request)),
            (400, Some(-32020)),
            "{method}"
        );
    }
}

#[test]
fn name_header_follows_the_method_and_decodes_base64_sentinels() {
    let call = |name: &str| {
        body(
            json!(1),
            "tools/call",
            json!({"name": name, "arguments": {}}),
        )
    };
    let accepted = |name: &str, header: &'static [u8]| {
        matches!(
            receive(
                &headers("tools/call", &[(NAME_HEADER, header)]),
                &call(name)
            ),
            Exchange::Request(_)
        )
    };
    assert!(accepted("acme_status", b"acme_status"));
    assert!(accepted("acme_status", b"=?base64?YWNtZV9zdGF0dXM=?="));
    assert!(accepted("Hello, 世界", b"=?base64?SGVsbG8sIOS4lueVjA==?="));
    assert!(accepted(" padded ", b"=?base64?IHBhZGRlZCA=?="));
    assert!(accepted(
        "=?base64?literal?=",
        b"=?base64?PT9iYXNlNjQ/bGl0ZXJhbD89?="
    ));
    for header in [
        b"acme_other".as_slice(),
        b" acme_status",
        b"acme_status ",
        "acme_sta\u{e9}tus".as_bytes(),
        b"=?base64?YWNtZV9zdGF0dXM?=",
        b"=?base64?/w==?=",
    ] {
        let fields = headers("tools/call", &[(NAME_HEADER, header)]);
        assert_eq!(
            refusal(&receive(&fields, &call("acme_status"))),
            (400, Some(-32020)),
            "{header:?}"
        );
    }
    let missing = headers("tools/call", &[]);
    assert_eq!(
        refusal(&receive(&missing, &call("acme_status"))),
        (400, Some(-32020))
    );

    let read = body(
        json!(2),
        "resources/read",
        json!({"uri": "acme://report/1"}),
    );
    let fields = headers("resources/read", &[(NAME_HEADER, b"acme://report/1")]);
    assert!(matches!(receive(&fields, &read), Exchange::Request(_)));
    let prompt = body(json!(3), "prompts/get", json!({"name": "acme_brief"}));
    let fields = headers("prompts/get", &[(NAME_HEADER, b"acme_brief")]);
    assert!(matches!(receive(&fields, &prompt), Exchange::Request(_)));

    let list = body(json!(4), "tools/list", json!({}));
    let fields = headers("tools/list", &[(NAME_HEADER, b"acme_status")]);
    assert_eq!(
        refusal(&receive(&fields, &list)),
        (400, Some(-32020)),
        "a method without a name source refuses Mcp-Name"
    );
}

/// Extra header fields for a test request.
type Fields = &'static [(&'static str, &'static [u8])];

fn deploy_catalog() -> Catalog {
    Catalog::new(vec![crate::Tool::new(
        "acme_deploy",
        "Deploy an Acme service.",
        json!({"type":"object","properties":{
            "region": {"type":"string","x-mcp-header":"Region"},
            "spec": {"type":"object","properties":{
                "replicas": {"type":"integer","x-mcp-header":"Replicas"}
            }},
            "dry": {"type":"boolean","x-mcp-header":"Dry-Run"}
        }}),
    )])
    .unwrap()
}

/// Checks an `acme_deploy` call's parameter headers against its arguments.
fn check(arguments: &Value, extra: &[(&'static str, &'static [u8])]) -> Result<(), Refusal> {
    let mut fields = vec![(NAME_HEADER, b"acme_deploy".as_slice())];
    fields.extend_from_slice(extra);
    let request = body(
        json!(9),
        "tools/call",
        json!({"name": "acme_deploy", "arguments": arguments}),
    );
    admitted(receive(&headers("tools/call", &fields), &request))
        .check_parameters(&deploy_catalog())
        .map_err(|response| {
            let reply: Value = serde_json::from_slice(&response.body).unwrap();
            assert_eq!(reply["id"], 9);
            (response.status, reply["error"]["code"].as_i64())
        })
}

#[test]
fn parameter_headers_refuse_every_mismatch_in_the_annotation_table() {
    let refused: [(Value, Fields); 9] = [
        (json!({"region":"eu-west"}), &[]),
        (json!({}), &[("Mcp-Param-Region", b"eu-west")]),
        (json!({"region": null}), &[("Mcp-Param-Region", b"eu-west")]),
        (
            json!({"region":"eu-west"}),
            &[("Mcp-Param-Region", b"EU-WEST")],
        ),
        (
            json!({"region":"eu-west"}),
            &[
                ("Mcp-Param-Region", b"eu-west"),
                ("mcp-param-region", b"eu-west"),
            ],
        ),
        (json!({"dry":true}), &[("Mcp-Param-Dry-Run", b"True")]),
        (
            json!({"spec":{"replicas":3}}),
            &[("Mcp-Param-Replicas", b"3.5")],
        ),
        (
            json!({"spec":{"replicas":9_007_199_254_740_992_u64}}),
            &[("Mcp-Param-Replicas", b"9007199254740992")],
        ),
        (
            json!({"spec":{"replicas":3}}),
            &[("Mcp-Param-Replicas", b"three")],
        ),
    ];
    for (arguments, fields) in refused {
        assert_eq!(
            check(&arguments, fields),
            Err((400, Some(-32020))),
            "{arguments} {fields:?}"
        );
    }
}

#[test]
fn parameter_headers_accept_matching_values_and_ignore_unknown_fields() {
    assert_eq!(check(&json!({}), &[]), Ok(()), "absent and missing");
    assert_eq!(
        check(&json!({"region": null}), &[]),
        Ok(()),
        "null and missing"
    );
    let all: Fields = &[
        ("Mcp-Param-Region", b"eu-west"),
        ("mcp-param-replicas", b"3"),
        ("MCP-PARAM-DRY-RUN", b"true"),
        ("Mcp-Param-Unknown", b"ignored"),
    ];
    let arguments = json!({"region":"eu-west","spec":{"replicas":3},"dry":true});
    assert_eq!(check(&arguments, all), Ok(()));
    assert_eq!(
        check(
            &json!({"region":"東京"}),
            &[("Mcp-Param-Region", b"=?base64?5p2x5Lqs?=")]
        ),
        Ok(())
    );
    for replicas in [b"42.0".as_slice(), b"4.2e1"] {
        assert_eq!(
            check(
                &json!({"spec":{"replicas":42}}),
                &[("Mcp-Param-Replicas", replicas)]
            ),
            Ok(()),
            "{replicas:?}"
        );
    }
    assert_eq!(
        check(&json!({"region": 7}), &[("Mcp-Param-Region", b"seven")]),
        Ok(()),
        "a body type mismatch is left to argument validation"
    );
    let other = body(
        json!(1),
        "tools/call",
        json!({"name": "acme_other", "arguments": {}}),
    );
    let fields = headers("tools/call", &[(NAME_HEADER, b"acme_other")]);
    assert_eq!(
        admitted(receive(&fields, &other)).check_parameters(&deploy_catalog()),
        Ok(()),
        "an unknown tool is left to the core"
    );
}

#[test]
fn listen_and_progress_requests_stream_while_others_reply_once() {
    let form = |method: &str, params: Value, extra: &[(&'static str, &'static [u8])]| {
        admitted(receive(
            &headers(method, extra),
            &body(json!(1), method, params),
        ))
        .form
    };
    assert_eq!(form("tools/list", json!({}), &[]), Form::Json);
    assert_eq!(
        form("subscriptions/listen", json!({"notifications": {}}), &[]),
        Form::Stream
    );
    assert_eq!(
        form(
            "tools/call",
            json!({"name": "acme_status", "arguments": {}, "_meta": {"progressToken": "p"}}),
            &[(NAME_HEADER, b"acme_status")]
        ),
        Form::Stream
    );
}

#[test]
fn admitted_requests_drive_a_modern_session_and_map_reply_status() {
    let options = SessionOptions {
        modern: Some(ModernOptions::new(ServerInfo::new("acme", "1.0.0"))),
        ..SessionOptions::default()
    };
    let mut session = Session::try_new(Arc::new(Catalog::new(vec![]).unwrap()), options).unwrap();
    let reply = |session: &mut Session, method: &str| {
        let admitted = admitted(receive(
            &headers(method, &[]),
            &body(json!(7), method, json!({})),
        ));
        assert_eq!(admitted.id, json!(7));
        let Inbound::Reply(reply) = session.receive_value(admitted.message) else {
            panic!("{method} should be answered by the core");
        };
        Response::reply_value(&reply, 4096).unwrap()
    };
    let listed = reply(&mut session, "tools/list");
    assert_eq!(listed.status, 200);
    let listed: Value = serde_json::from_slice(&listed.body).unwrap();
    assert_eq!(listed["result"]["tools"], json!([]));
    assert_eq!(reply(&mut session, "acme/missing").status, 404);
    assert_eq!(reply(&mut session, "ping").status, 404);
}

#[test]
fn responses_encode_status_headers_and_sse_events() {
    for (code, status) in [
        (-32020, 400),
        (-32021, 400),
        (-32022, 400),
        (-32600, 400),
        (-32700, 400),
        (-32601, 404),
        (-32602, 200),
        (-32001, 200),
        (-32603, 200),
    ] {
        assert_eq!(status_for(code), status, "{code}");
        assert_eq!(Response::reply(Vec::new(), Some(code)).status, status);
    }
    assert_eq!(Response::reply(b"{}".to_vec(), None).status, 200);
    assert!(Response::reply_value(&json!({"result": "a".repeat(64)}), 16).is_err());
    let stream = Response::stream();
    assert_eq!(stream.status, 200);
    assert!(
        stream
            .headers
            .contains(&("Content-Type", "text/event-stream"))
    );
    assert!(stream.headers.contains(&("X-Accel-Buffering", "no")));
    assert_eq!(
        sse_event(br#"{"jsonrpc":"2.0"}"#),
        b"event: message\ndata: {\"jsonrpc\":\"2.0\"}\n\n"
    );
    assert_eq!(SSE_KEEP_ALIVE, b":\n");
}

#[test]
fn endpoint_options_are_validated_once() {
    let with = |options: EndpointOptions| Endpoint::new(options).map(|_| ());
    let path = |path: &str| EndpointOptions {
        path: path.to_owned(),
        ..EndpointOptions::default()
    };
    let origins = |origins: Vec<String>| EndpointOptions {
        allowed_origins: origins,
        ..EndpointOptions::default()
    };
    assert_eq!(with(path("/acme/mcp")), Ok(()));
    for invalid in ["", "mcp", "/mcp?x", "/mcp#x", "/m cp", "/\u{e9}"] {
        assert_eq!(with(path(invalid)), Err(EndpointError::Path), "{invalid:?}");
    }
    assert_eq!(
        with(path(&format!("/{}", "a".repeat(MAX_PATH_BYTES)))),
        Err(EndpointError::Path)
    );
    assert_eq!(
        with(origins(vec!["http://localhost:3000".to_owned()])),
        Ok(())
    );
    for invalid in [
        "null",
        "ftp://example.com",
        "https://",
        "https://example.com/",
        "https://user@example.com",
    ] {
        assert_eq!(
            with(origins(vec![
                "https://example.com".to_owned(),
                invalid.to_owned()
            ])),
            Err(EndpointError::Origin(1)),
            "{invalid}"
        );
    }
    assert_eq!(
        with(origins(vec![
            "https://example.com".to_owned();
            MAX_ORIGINS + 1
        ])),
        Err(EndpointError::TooManyOrigins)
    );
    assert_eq!(
        with(EndpointOptions {
            max_body_bytes: 0,
            ..EndpointOptions::default()
        }),
        Err(EndpointError::BodyLimit)
    );
}
