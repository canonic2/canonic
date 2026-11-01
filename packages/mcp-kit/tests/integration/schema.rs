//! Prepared input/output validation through stdio and independent runtimes.

use crate::fixture::{Host, Script};
use canonic_mcp_kit::{
    Catalog, Content, Inbound, ModernOptions, ServerInfo, Session, SessionOptions, Tool,
    ToolResult,
    control::Interrupted,
    json::EncodeError,
    modern, rpc,
    stdio::{Options, Outcome, Request},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn catalog() -> Arc<Catalog> {
    Arc::new(Catalog::new(vec![
        Tool::new("acme_checked", "Return checked Acme data.", json!({"type":"object","properties":{"n":{"type":"integer","minimum":0},"values":{"type":"array","not":{"uniqueItems":true}}},"additionalProperties":false}))
            .with_output_schema(json!({"type":"object","properties":{"n":{"type":"integer"}},"required":["n"],"additionalProperties":false})),
        Tool::new("acme_array", "Return an array.", json!({"type":"object"})).with_output_schema(json!({"type":"array","items":{"type":"integer"}})),
        Tool::new("acme_null", "Return null.", json!({"type":"object"})).with_output_schema(json!({"type":"null"})),
    ]).expect("valid schemas"))
}

fn params(modern_request: bool, name: &str, arguments: Value) -> Value {
    let mut params = json!({"name":name});
    params["arguments"] = arguments;
    if modern_request {
        params["_meta"] =
            json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{}});
    }
    params
}

fn initialize(host: &Host, version: &str) {
    host.send(&rpc::request(json!("init"), "initialize", json!({"protocolVersion":version,"clientInfo":{"name":"acme-host","version":"1"},"capabilities":{}})));
    assert_eq!(host.recv()["result"]["protocolVersion"], version);
    host.send(&rpc::notification("notifications/initialized", json!({})));
}

#[test]
fn schema_driver_checks_success_errors_and_refuses_before_effects() {
    for version in ["2025-06-18", "2025-11-25", modern::PROTOCOL_VERSION] {
        let modern_request = version == modern::PROTOCOL_VERSION;
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let handler = Script::new(move |request, _| {
            observed.fetch_add(1, Ordering::SeqCst);
            let Request::Tool(call) = request else {
                panic!("tool fixture");
            };
            let result = match call.name.as_str() {
                "acme_array" => ToolResult::structured(json!([1, 2]), false),
                "acme_null" => ToolResult::structured(Value::Null, false),
                _ => match call.arguments["n"].as_u64().unwrap_or(1) {
                    2 => ToolResult::success(vec![Content::text("missing structured result")]),
                    3 => ToolResult::structured(json!({"n":"wrong"}), false),
                    4 => ToolResult::error(vec![Content::text("Refused")]),
                    5 => ToolResult::structured(json!({"n":"wrong"}), true),
                    6 => {
                        return Outcome::Method(Ok(
                            json!({"content":[],"structuredContent":{"n":"wrong"}}),
                        ));
                    }
                    _ => ToolResult::structured(json!({"n":1}), false),
                },
            };
            Outcome::Tool(result)
        });
        let mut options = Options::default();
        options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
        let host = Host::start_with(Arc::new(handler), catalog(), options);
        if !modern_request {
            initialize(&host, version);
        }
        let mut discovery = json!({});
        if modern_request {
            discovery["_meta"] = params(true, "", json!({}))["_meta"].clone();
        }
        host.send(&rpc::request(json!(1), "tools/list", discovery));
        assert_eq!(
            host.recv()["result"]["tools"].as_array().unwrap().len(),
            if modern_request { 3 } else { 1 }
        );
        host.send(&rpc::request(
            json!(2),
            "tools/call",
            params(modern_request, "acme_checked", json!({"n":-1})),
        ));
        assert_eq!(host.recv()["result"]["isError"], true);
        host.send(&rpc::request(
            json!(3),
            "tools/call",
            params(
                modern_request,
                "acme_checked",
                // Logical negation must not turn a work-budget refusal into success.
                json!({"values":vec![0; 2100]}),
            ),
        ));
        assert_eq!(host.recv()["result"]["isError"], true);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        for n in 1..=6 {
            host.send(&rpc::request(
                json!(10 + n),
                "tools/call",
                params(modern_request, "acme_checked", json!({"n":n})),
            ));
            let reply = host.recv();
            match n {
                1 => assert_eq!(reply["result"]["structuredContent"], json!({"n":1})),
                4 => assert_eq!(reply["result"]["isError"], true),
                _ => {
                    assert_eq!(reply["error"]["code"], rpc::INTERNAL_ERROR);
                    assert!(reply.get("result").is_none());
                }
            }
        }
        host.send(&rpc::request(
            json!(30),
            "tools/call",
            params(modern_request, "acme_array", json!({})),
        ));
        let reply = host.recv();
        if modern_request {
            assert_eq!(reply["result"]["structuredContent"], json!([1, 2]));
            host.send(&rpc::request(
                json!(31),
                "tools/call",
                params(true, "acme_null", json!({})),
            ));
            let reply = host.recv();
            let typed: ToolResult = serde_json::from_value(reply["result"].clone()).unwrap();
            assert_eq!(typed.structured_content, Some(Value::Null));
        } else {
            assert_eq!(reply["error"]["code"], rpc::INVALID_PARAMS);
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            if modern_request { 8 } else { 6 }
        );
        host.finish().0.unwrap();
    }
}

#[test]
fn schema_independent_runtime_preserves_shape_bounds_and_interruption() {
    let catalog = catalog();
    let mut session = Session::try_new(
        catalog.clone(),
        SessionOptions {
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    assert!(matches!(
        session.receive_value(rpc::request(
            json!(1),
            "tools/call",
            params(true, "acme_array", json!({}))
        )),
        Inbound::Tool { .. }
    ));
    let context = session.request_context(&json!(1)).unwrap();
    let result = ToolResult::structured(json!([1, 2]), false);
    let bytes = context
        .encode_success(&json!(1), &result, 4096, &|| Ok(()))
        .unwrap();
    assert_eq!(
        context.encode_success(
            &json!(1),
            &ToolResult::structured(json!(["bad"]), false),
            4096,
            &|| Ok(())
        ),
        Err(EncodeError::Invalid)
    );
    assert_eq!(
        context.encode_success(&json!(1), &result, bytes.len() - 1, &|| Ok(())),
        Err(EncodeError::TooLarge {
            limit: bytes.len() - 1
        })
    );
    assert_eq!(
        context.encode_success(&json!(1), &result, 4096, &|| Err(Interrupted)),
        Err(EncodeError::Interrupted)
    );
    assert_eq!(
        context.encode_success(
            &json!(1),
            &json!({"structuredContent":["bad"]}),
            4096,
            &|| Ok(())
        ),
        Err(EncodeError::Invalid)
    );
    assert!(session.finish(&json!(1)));
    let legacy = catalog.result_schema("acme_checked").unwrap();
    let good = ToolResult::structured(json!({"n":1}), false);
    assert!(
        legacy
            .encode_legacy(&json!(2), &good, 4096, &|| Ok(()))
            .is_ok()
    );
    assert_eq!(
        legacy.encode_legacy(&json!(2), &ToolResult::success(vec![]), 4096, &|| Ok(())),
        Err(EncodeError::Invalid)
    );
}

#[test]
fn schema_driver_accepts_large_pages_and_distinguishes_work_limits() {
    for version in ["2025-06-18", "2025-11-25", modern::PROTOCOL_VERSION] {
        let catalog = Arc::new(
            Catalog::new(vec![
                Tool::new(
                    "acme_page",
                    "Return a page.",
                    json!({"type":"object","properties":{"data":{}}}),
                )
                .with_output_schema(json!({"type":"object","properties":{"data":{}}})),
                Tool::new(
                    "acme_costly",
                    "Return checked unique data.",
                    json!({"type":"object","properties":{"data":{}}}),
                )
                .with_output_schema(
                    json!({"type":"object","properties":{"data":{"uniqueItems":true}}}),
                ),
            ])
            .unwrap(),
        );
        let handler = Script::new(|request, _| {
            let Request::Tool(call) = request else {
                panic!("tool fixture")
            };
            Outcome::Tool(ToolResult::structured(call.arguments, false))
        });
        let mut options = Options::default();
        options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
        let host = Host::start_with(Arc::new(handler), catalog, options);
        let modern_request = version == modern::PROTOCOL_VERSION;
        if !modern_request {
            initialize(&host, version);
        }
        let data = json!({"data":vec![0;4097]});
        host.send(&rpc::request(
            json!(1),
            "tools/call",
            params(modern_request, "acme_page", data.clone()),
        ));
        assert_eq!(host.recv()["result"]["structuredContent"], data);
        host.send(&rpc::request(
            json!(2),
            "tools/call",
            params(modern_request, "acme_costly", json!({"data":vec![0;2100]})),
        ));
        let reply = host.recv();
        assert_eq!(reply["error"]["code"], rpc::INTERNAL_ERROR);
        assert_eq!(
            reply["error"]["message"],
            "Tool result exceeds schema validation limits"
        );
        assert!(reply.get("result").is_none());
        host.finish().0.unwrap();
    }
}

#[test]
fn schema_custom_argument_errors_cannot_publish_invalid_structured_data() {
    struct Acme;
    impl canonic_mcp_kit::stdio::Handler for Acme {
        fn initialize(
            &self,
            _: &canonic_mcp_kit::Client,
        ) -> Result<ServerInfo, canonic_mcp_kit::RpcError> {
            Ok(ServerInfo::new("acme", "1"))
        }
        fn call(&self, _: Request, _: &canonic_mcp_kit::stdio::Context) -> Outcome {
            panic!("invalid arguments must not dispatch");
        }
        fn invalid_arguments(
            &self,
            _: &canonic_mcp_kit::ToolCall,
            _: &canonic_mcp_kit::schema::ArgumentError,
        ) -> ToolResult {
            ToolResult::structured(json!({"n":"wrong"}), true)
        }
    }
    for version in ["2025-06-18", "2025-11-25", modern::PROTOCOL_VERSION] {
        let mut options = Options::default();
        options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
        let host = Host::start_with(Arc::new(Acme), catalog(), options);
        let modern_request = version == modern::PROTOCOL_VERSION;
        if !modern_request {
            initialize(&host, version);
        }
        host.send(&rpc::request(
            json!(1),
            "tools/call",
            params(modern_request, "acme_checked", json!({"n":-1})),
        ));
        let reply = host.recv();
        assert_eq!(reply["error"]["code"], rpc::INTERNAL_ERROR);
        assert!(reply.get("result").is_none());
        host.finish().0.unwrap();
    }
}
