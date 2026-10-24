use super::*;
use crate::tool::Tool;

fn catalog() -> Arc<Catalog> {
    Arc::new(
        Catalog::new(vec![Tool::new(
            "acme_echo",
            "Return the Acme message.",
            json!({"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false}),
        )])
        .unwrap(),
    )
}

fn frame(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn initialize(version: &str, capabilities: &Value) -> Value {
    json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
        "protocolVersion":version,"capabilities":capabilities,
        "clientInfo":{"name":"acme-host","version":"2.1.0"}}})
}

fn ready(options: SessionOptions, capabilities: &Value) -> Session {
    let mut session = Session::new(catalog(), options);
    let Inbound::Initialize { id, .. } =
        session.receive(&frame(&initialize(LATEST_PROTOCOL_VERSION, capabilities)))
    else {
        panic!("initialize");
    };
    session.accept_initialize(id, &ServerInfo::new("acme", "1.0.0"));
    assert_eq!(
        session.receive(&frame(
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )),
        Inbound::Ignored
    );
    assert!(session.ready());
    session
}

fn call(id: u64, arguments: &Value) -> Vec<u8> {
    frame(
        &json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"acme_echo","arguments":arguments}}),
    )
}

fn error_code(inbound: &Inbound) -> i64 {
    match inbound {
        Inbound::Reply(reply) | Inbound::Fatal { reply, .. } => {
            reply["error"]["code"].as_i64().expect("error reply")
        }
        other => panic!("expected an error reply, got {other:?}"),
    }
}

#[test]
fn initialization_negotiates_versions_and_gates_tools_until_initialized() {
    let mut session = Session::new(catalog(), SessionOptions::default());
    let list = frame(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    assert_eq!(
        error_code(&session.receive(&list)),
        i64::from(rpc::SERVER_ERROR)
    );
    let ping = session.receive(&frame(&json!({"jsonrpc":"2.0","id":"p","method":"ping"})));
    assert_eq!(ping, Inbound::Reply(rpc::success(json!("p"), json!({}))));

    let Inbound::Initialize { id, client } =
        session.receive(&frame(&initialize("2024-01-01", &json!({}))))
    else {
        panic!("initialize");
    };
    assert_eq!(client.protocol_version, LATEST_PROTOCOL_VERSION);
    assert!(!client.form_elicitation);
    let reply = session.accept_initialize(
        id,
        &ServerInfo::new("acme", "1.0.0").with_instructions("Use acme_echo."),
    );
    assert_eq!(
        reply["result"],
        json!({"protocolVersion":LATEST_PROTOCOL_VERSION,"capabilities":{"tools":{"listChanged":false}},
               "serverInfo":{"name":"acme","version":"1.0.0"},"instructions":"Use acme_echo."})
    );
    assert_eq!(
        error_code(&session.receive(&frame(&initialize(LATEST_PROTOCOL_VERSION, &json!({}))))),
        i64::from(rpc::INVALID_REQUEST)
    );
    assert_eq!(
        error_code(&session.receive(&list)),
        i64::from(rpc::SERVER_ERROR)
    );
    session.receive(&frame(
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ));
    assert_eq!(
        session.receive(&list),
        Inbound::Reply(rpc::success(json!(1), catalog().listing()))
    );
}

#[test]
fn accepting_initialization_again_preserves_a_ready_session() {
    let mut session = ready(SessionOptions::default(), &json!({}));
    let client = session.client().cloned();
    let reply = session.accept_initialize(json!(99), &ServerInfo::new("acme", "2.0.0"));
    assert_eq!(reply["id"], 99);
    assert_eq!(reply["error"]["code"], rpc::INVALID_REQUEST);
    assert!(session.ready());
    assert_eq!(session.client(), client.as_ref());
    assert!(matches!(
        session.receive(&call(1, &json!({"message":"still ready"}))),
        Inbound::Tool { .. }
    ));
    assert_eq!(
        error_code(&session.receive(&frame(&initialize(LATEST_PROTOCOL_VERSION, &json!({}))))),
        i64::from(rpc::INVALID_REQUEST)
    );
}

#[test]
fn accepting_initialization_with_the_wrong_id_preserves_the_pending_request() {
    let mut session = Session::new(catalog(), SessionOptions::default());
    let mut request = initialize("2025-06-18", &json!({}));
    request["id"] = json!("acme-init");
    let Inbound::Initialize { id, .. } = session.receive(&frame(&request)) else {
        panic!("initialize");
    };
    let info = ServerInfo::new("acme", "1.0.0");
    let wrong = session.accept_initialize(json!(99), &info);
    assert_eq!(wrong["id"], 99);
    assert_eq!(wrong["error"]["code"], rpc::INVALID_REQUEST);
    assert!(session.client().is_none());
    let reply = session.accept_initialize(id.clone(), &info);
    assert_eq!(reply["id"], id);
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn accepting_initialization_again_preserves_the_negotiated_handshake() {
    let mut session = Session::new(catalog(), SessionOptions::default());
    let Inbound::Initialize { id, .. } =
        session.receive(&frame(&initialize(LATEST_PROTOCOL_VERSION, &json!({}))))
    else {
        panic!("initialize");
    };
    let info = ServerInfo::new("acme", "1.0.0");
    session.accept_initialize(id.clone(), &info);
    let reply = session.accept_initialize(id, &info);
    assert_eq!(reply["error"]["code"], rpc::INVALID_REQUEST);
    session.receive(&frame(
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ));
    assert!(session.ready());
}

#[test]
fn older_supported_versions_are_kept_and_bad_parameters_refused() {
    let mut session = Session::new(catalog(), SessionOptions::default());
    let Inbound::Initialize { client, .. } = session.receive(&frame(&initialize(
        "2025-06-18",
        &json!({"elicitation":{}}),
    ))) else {
        panic!("initialize");
    };
    assert_eq!(client.protocol_version, "2025-06-18");
    assert!(client.form_elicitation);

    for params in [
        json!({"protocolVersion":"","capabilities":{},"clientInfo":{"name":"a","version":"1"}}),
        json!({"protocolVersion":"2025-11-25","capabilities":[],"clientInfo":{"name":"a","version":"1"}}),
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"a"}}),
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"a","version":"1"},"extra":1}),
    ] {
        let mut session = Session::new(catalog(), SessionOptions::default());
        let inbound = session.receive(&frame(
            &json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":params}),
        ));
        assert_eq!(
            error_code(&inbound),
            i64::from(rpc::INVALID_PARAMS),
            "{params}"
        );
    }
}

#[test]
fn tool_calls_are_checked_against_the_catalog_and_tracked_until_finished() {
    let mut session = ready(SessionOptions::default(), &json!({}));
    let unknown = frame(
        &json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"acme_other"}}),
    );
    assert_eq!(
        error_code(&session.receive(&unknown)),
        i64::from(rpc::INVALID_PARAMS)
    );

    let Inbound::InvalidArguments { id, error, .. } = session.receive(&call(1, &json!({}))) else {
        panic!("invalid arguments");
    };
    assert_eq!(error.message, "arguments.message is required");
    assert!(session.finish(&id));

    let Inbound::Tool { id, call: tool } = session.receive(&call(2, &json!({"message":"hi"})))
    else {
        panic!("tool");
    };
    assert_eq!(tool.arguments, json!({"message":"hi"}));
    assert_eq!(session.outstanding(), 1);
    let duplicate = session.receive(&call(2, &json!({"message":"again"})));
    assert!(matches!(duplicate, Inbound::Fatal { .. }));
    assert_eq!(error_code(&duplicate), i64::from(rpc::INVALID_REQUEST));
    assert!(session.finish(&id));
    assert!(!session.finish(&id));
}

#[test]
fn cancellation_applies_only_to_outstanding_requests() {
    let mut session = ready(SessionOptions::default(), &json!({}));
    let cancel = |id: u64| {
        frame(
            &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id,"reason":"user"}}),
        )
    };
    assert_eq!(session.receive(&cancel(5)), Inbound::Ignored);
    assert!(matches!(
        session.receive(&call(5, &json!({"message":"hi"}))),
        Inbound::Tool { .. }
    ));
    assert_eq!(
        session.receive(&cancel(5)),
        Inbound::Cancel { id: json!(5) }
    );
    assert!(!session.finish(&json!(5)));
    assert_eq!(session.receive(&cancel(5)), Inbound::Ignored);
}

#[test]
fn confirmations_need_form_elicitation_and_match_their_reply() {
    let mut plain = ready(SessionOptions::default(), &json!({}));
    assert_eq!(
        plain.request_confirmation("Remove Acme data?"),
        Err(Decision::Unavailable)
    );

    let mut session = ready(
        SessionOptions::default(),
        &json!({"elicitation":{"form":{}}}),
    );
    let (id, request) = session.request_confirmation("Remove Acme data?").unwrap();
    assert_eq!(request["method"], "elicitation/create");
    assert_eq!(request["params"]["mode"], "form");
    assert_eq!(request["params"]["message"], "Remove Acme data?");
    assert_eq!(
        session.request_confirmation("Another change?"),
        Err(Decision::Unavailable)
    );
    let unrelated = frame(&json!({"jsonrpc":"2.0","id":"other","result":{"action":"accept"}}));
    assert_eq!(session.receive(&unrelated), Inbound::Ignored);
    let accept = frame(&json!({"jsonrpc":"2.0","id":id,"result":{"action":"accept","content":{}}}));
    assert_eq!(
        session.receive(&accept),
        Inbound::Confirmation {
            id: id.clone(),
            decision: Decision::Accepted
        }
    );
    assert_eq!(session.receive(&accept), Inbound::Ignored);

    for (reply, decision) in [
        (json!({"result":{"action":"decline"}}), Decision::Declined),
        (json!({"result":{"action":"cancel"}}), Decision::Declined),
        (
            json!({"error":{"code":-32601,"message":"Unsupported"}}),
            Decision::Unavailable,
        ),
    ] {
        let (id, _) = session.request_confirmation("Remove Acme data?").unwrap();
        let mut message = json!({"jsonrpc":"2.0","id":id});
        for (key, value) in reply.as_object().unwrap() {
            message[key] = value.clone();
        }
        assert_eq!(
            session.receive(&frame(&message)),
            Inbound::Confirmation { id, decision }
        );
    }

    let (id, _) = session.request_confirmation("Remove Acme data?").unwrap();
    let notice = session.cancel_confirmation(&id).unwrap();
    assert_eq!(notice["method"], "notifications/cancelled");
    assert_eq!(notice["params"]["requestId"], id);
    assert!(session.cancel_confirmation(&id).is_none());

    session.request_confirmation("Remove Acme data?").unwrap();
    assert!(session.close().is_some());
    assert_eq!(
        session.request_confirmation("Remove Acme data?"),
        Err(Decision::Unavailable)
    );
}

#[test]
fn older_protocol_confirmations_omit_the_mode_field() {
    let mut session = Session::new(catalog(), SessionOptions::default());
    let Inbound::Initialize { id, .. } = session.receive(&frame(&initialize(
        "2025-06-18",
        &json!({"elicitation":{}}),
    ))) else {
        panic!("initialize");
    };
    session.accept_initialize(id, &ServerInfo::new("acme", "1.0.0"));
    let (_, request) = session.request_confirmation("Remove Acme data?").unwrap();
    assert!(request["params"].get("mode").is_none());
}

#[test]
fn frames_are_bounded_and_invalid_json_is_answered() {
    let options = SessionOptions {
        max_message_bytes: 64,
        max_messages: Some(3),
        ..SessionOptions::default()
    };
    let mut session = Session::new(catalog(), options);
    assert_eq!(
        error_code(&session.receive(b"{oops}\n")),
        i64::from(rpc::PARSE_ERROR)
    );
    let duplicate_key = br#"{"jsonrpc":"2.0","jsonrpc":"2.0","id":1,"method":"ping"}"#;
    assert_eq!(
        error_code(&session.receive(duplicate_key)),
        i64::from(rpc::PARSE_ERROR)
    );
    let oversized = session.receive(&[b' '; 65]);
    assert!(matches!(
        oversized,
        Inbound::Fatal {
            reason: "message exceeds size limit",
            ..
        }
    ));
    let limit = session.receive(br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
    assert!(matches!(
        limit,
        Inbound::Fatal {
            reason: "session message limit reached",
            ..
        }
    ));
}

#[test]
fn extension_methods_and_notifications_route_only_when_listed_and_ready() {
    let options = SessionOptions {
        extension_methods: vec!["acme/task_end".into()],
        extension_notifications: vec!["notifications/acme/task_end".into()],
        ..SessionOptions::default()
    };
    let method = frame(&json!({"jsonrpc":"2.0","id":4,"method":"acme/task_end"}));
    let notice = frame(&json!({"jsonrpc":"2.0","method":"notifications/acme/task_end"}));
    let mut early = Session::new(catalog(), options.clone());
    assert_eq!(
        error_code(&early.receive(&method)),
        i64::from(rpc::SERVER_ERROR)
    );
    assert_eq!(early.receive(&notice), Inbound::Ignored);

    let mut session = ready(options, &json!({}));
    assert_eq!(
        session.receive(&method),
        Inbound::Method {
            id: json!(4),
            method: "acme/task_end".into(),
            params: json!({})
        }
    );
    assert!(session.finish(&json!(4)));
    assert_eq!(
        session.receive(&notice),
        Inbound::Notification {
            method: "notifications/acme/task_end".into(),
            params: json!({})
        }
    );
    let other = frame(&json!({"jsonrpc":"2.0","id":5,"method":"acme/other"}));
    assert_eq!(
        error_code(&session.receive(&other)),
        i64::from(rpc::METHOD_NOT_FOUND)
    );
}
