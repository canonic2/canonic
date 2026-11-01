//! Independent-runtime encoding through the library's public modern API.

use canonic_mcp_kit::{
    Catalog, Inbound, ModernOptions, RequestContext, ServerInfo, Session, SessionOptions,
    control::Interrupted, json::EncodeError, modern, rpc,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn admitted(method: &str, capabilities: &Value) -> (Session, Arc<RequestContext>) {
    let options = SessionOptions {
        modern: Some(ModernOptions::new(
            ServerInfo::new("acme", "1").with_title("Acme service"),
        )),
        resources: Some(Arc::new(
            canonic_mcp_kit::ResourceCatalog::new(vec![], vec![]).expect("empty resources"),
        )),
        extension_methods: vec!["acme/inspect".into()],
        ..SessionOptions::default()
    };
    let mut session = Session::try_new(
        Arc::new(Catalog::new(vec![]).expect("empty tools")),
        options,
    )
    .expect("valid modern options");
    let request = rpc::request(
        json!(1),
        method,
        json!({
            "uri":"acme://report",
            "_meta":{
                modern::VERSION_KEY:modern::PROTOCOL_VERSION,
                modern::CAPABILITIES_KEY:capabilities
            }
        }),
    );
    assert!(matches!(
        session.receive_value(request),
        Inbound::Method { .. }
    ));
    let context = session
        .request_context(&json!(1))
        .expect("admitted context");
    (session, context)
}

#[test]
fn independent_runtime_encodes_bounded_interim_with_captured_identity() {
    let (mut session, context) = admitted("resources/read", &json!({"elicitation":{"form":{}}}));
    let interim = context
        .confirmation("Read report?", "opaque-state")
        .unwrap();
    let bytes = context
        .encode_interim(&json!(1), &interim, 4096, &|| Ok(()))
        .unwrap();
    let reply: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["jsonrpc"], "2.0");
    assert_eq!(reply["result"]["resultType"], "input_required");
    assert_eq!(reply["result"]["requestState"], "opaque-state");
    assert_eq!(
        reply["result"]["inputRequests"],
        interim.as_value()["inputRequests"]
    );
    assert_eq!(
        reply["result"]["_meta"]["io.modelcontextprotocol/serverInfo"],
        json!({"name":"acme","version":"1","title":"Acme service"})
    );
    assert!(reply["result"].get("ttlMs").is_none());
    assert!(reply["result"].get("cacheScope").is_none());
    assert!(session.finish(&json!(1)));

    assert_eq!(
        context.encode_interim(&json!(1), &interim, bytes.len(), &|| Ok(())),
        Ok(bytes.clone())
    );
    assert_eq!(
        context.encode_interim(&json!(1), &interim, bytes.len() - 1, &|| Ok(())),
        Err(EncodeError::TooLarge {
            limit: bytes.len() - 1
        })
    );
    assert_eq!(
        context.encode_interim(&json!(1), &interim, 4096, &|| Err(Interrupted)),
        Err(EncodeError::Interrupted)
    );
    let (_, unsupported) = admitted("resources/read", &json!({}));
    assert_eq!(
        unsupported.encode_interim(&json!(1), &interim, 4096, &|| Ok(())),
        Err(EncodeError::Invalid)
    );
    let (_, ineligible) = admitted("acme/inspect", &json!({"elicitation":{"form":{}}}));
    assert_eq!(
        ineligible.encode_interim(&json!(1), &interim, 4096, &|| Ok(())),
        Err(EncodeError::Invalid)
    );
}

fn protocol_errors() -> Vec<(canonic_mcp_kit::RpcError, i32)> {
    use canonic_mcp_kit::RpcError;
    let mut cases = Vec::new();
    for code in [
        -32700, -32600, -32601, -32602, -32603, -32000, -32001, -32010, -32800, -40000, 7,
    ] {
        cases.push((
            RpcError::new(code, "Acme refusal").with_data(json!({"detail":"retained"})),
            code,
        ));
    }
    for code in [
        -32768, -32701, -32604, -32100, -32099, -32042, -32023, -32020, -32002,
    ] {
        cases.push((
            RpcError::new(code, "Invalid allocation").with_data(json!({"detail":"not published"})),
            rpc::INTERNAL_ERROR,
        ));
    }
    for (code, data) in [
        (
            -32021,
            json!({"requiredCapabilities":{"elicitation":{"form":{}}}}),
        ),
        (-32021, json!({"requiredCapabilities":{"roots":{}}})),
        (
            -32021,
            json!({"requiredCapabilities":{"extensions":{canonic_mcp_kit::task::EXTENSION:{}}}}),
        ),
        (
            -32021,
            json!({"requiredCapabilities":{"sampling":{"context":{},"tools":{}}}}),
        ),
        (
            -32021,
            json!({"requiredCapabilities":{"experimental":{"acme/feature":{}}}}),
        ),
        (
            -32021,
            json!({"requiredCapabilities":{"acme/future":false}}),
        ),
        (
            -32022,
            json!({"requested":"2099-01-01","supported":[modern::PROTOCOL_VERSION]}),
        ),
    ] {
        cases.push((
            RpcError::new(code, "Required protocol data").with_data(data),
            code,
        ));
    }
    for (code, data) in [
        (-32021, json!({})),
        (-32021, json!({"requiredCapabilities":{}})),
        (-32021, json!({"requiredCapabilities":{"elicitation":null}})),
        (-32021, json!({"requiredCapabilities":{"extensions":[]}})),
        (-32021, json!({"requiredCapabilities":{"roots":false}})),
        (-32021, json!({"requiredCapabilities":{"sampling":false}})),
        (
            -32021,
            json!({"requiredCapabilities":{"sampling":{"tools":true}}}),
        ),
        (
            -32021,
            json!({"requiredCapabilities":{"sampling":{"context":null}}}),
        ),
        (-32021, json!({"requiredCapabilities":{"experimental":[]}})),
        (
            -32021,
            json!({"requiredCapabilities":{"experimental":{"acme/feature":true}}}),
        ),
        (-32022, json!({"requested":true,"supported":[]})),
        (-32022, json!({"requested":"future","supported":[null]})),
    ] {
        cases.push((
            RpcError::new(code, "Malformed protocol data").with_data(data),
            rpc::INTERNAL_ERROR,
        ));
    }
    cases
}

#[test]
fn modern_conformance_independent_runtime_error_allocation_bounds_and_cancellation() {
    let (_, context) = admitted("acme/inspect", &json!({}));
    for (error, expected) in protocol_errors() {
        let bytes = context
            .encode_error(&json!(1), &error, 4096, &|| Ok(()))
            .unwrap();
        let reply: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reply["error"]["code"], expected, "{error:?}");
        assert!(reply.get("result").is_none());
        if expected == error.code {
            assert_eq!(reply["error"]["data"], error.data.unwrap());
        } else {
            assert!(reply["error"].get("data").is_none());
        }
    }
    let error =
        canonic_mcp_kit::RpcError::new(7, "Acme failure").with_data(json!({"detail":"bounded"}));
    let bytes = context
        .encode_error(&json!(1), &error, 4096, &|| Ok(()))
        .unwrap();
    assert_eq!(
        context.encode_error(&json!(1), &error, bytes.len(), &|| Ok(())),
        Ok(bytes.clone())
    );
    assert_eq!(
        context.encode_error(&json!(1), &error, bytes.len() - 1, &|| Ok(())),
        Err(EncodeError::TooLarge {
            limit: bytes.len() - 1
        })
    );
    assert_eq!(
        context.encode_error(&json!(1), &error, 4096, &|| Err(Interrupted)),
        Err(EncodeError::Interrupted)
    );
    let (_, read) = admitted("resources/read", &json!({}));
    let bytes = read
        .encode_error(
            &json!(1),
            &canonic_mcp_kit::resource::not_found("acme://missing"),
            4096,
            &|| Ok(()),
        )
        .unwrap();
    let reply: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["error"]["code"], rpc::INVALID_PARAMS);
    assert_eq!(reply["error"]["data"]["uri"], "acme://missing");
}

#[test]
fn modern_conformance_driver_errors_and_unavailable_methods_match_core() {
    use crate::fixture::{Host, Script};
    use canonic_mcp_kit::stdio::{Options, Outcome, Request};
    let script = Script::new(|request, _| {
        let Request::Method { method, params } = request else {
            panic!("custom method")
        };
        if method == "resources/read" {
            return if params["uri"] == "acme://wrong" {
                Outcome::Tool(canonic_mcp_kit::ToolResult::success(vec![]))
            } else {
                Outcome::Resource(
                    canonic_mcp_kit::ReadResourceResult::new(vec![
                        canonic_mcp_kit::ResourceContents::text("acme://report", None, "report")
                            .unwrap(),
                    ])
                    .unwrap(),
                )
            };
        }
        let mut error = canonic_mcp_kit::RpcError::new(
            i32::try_from(params["code"].as_i64().unwrap()).unwrap(),
            "Acme error",
        );
        error.data = params.get("data").cloned();
        Outcome::Method(Err(error))
    });
    let unavailable = [
        "ping",
        "initialize",
        "logging/setLevel",
        "resources/subscribe",
        "resources/unsubscribe",
        "subscriptions/listen",
        "prompts/list",
        "prompts/get",
        "completion/complete",
        "roots/list",
        "sampling/createMessage",
        "elicitation/create",
    ];
    let mut options = Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    options.session.resources = Some(Arc::new(
        canonic_mcp_kit::ResourceCatalog::new(vec![], vec![]).unwrap(),
    ));
    options.session.extension_methods = unavailable
        .iter()
        .map(|method| (*method).into())
        .chain(std::iter::once("acme/inspect".into()))
        .collect();
    let host = Host::start_with(
        Arc::new(script),
        Arc::new(Catalog::new(vec![]).unwrap()),
        options,
    );
    let metadata =
        json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{}});
    for method in unavailable {
        host.send(&rpc::request(json!(1), method, json!({"_meta":metadata})));
        assert_eq!(
            host.recv()["error"]["code"],
            rpc::METHOD_NOT_FOUND,
            "{method}"
        );
    }
    for (error, expected) in protocol_errors() {
        host.send(&rpc::request(
            json!(1),
            "acme/inspect",
            json!({"code":error.code,"data":error.data,"_meta":metadata}),
        ));
        let response = host.recv();
        assert_eq!(response["error"]["code"], expected);
        assert!(response.get("result").is_none());
        if expected != error.code {
            assert!(response["error"].get("data").is_none());
        }
    }
    host.send(&rpc::request(
        json!(1),
        "resources/read",
        json!({"uri":"acme://wrong","_meta":metadata}),
    ));
    let invalid = host.recv();
    assert_eq!(invalid["error"]["code"], rpc::INTERNAL_ERROR);
    assert!(invalid.get("result").is_none());
    host.send(&rpc::request(
        json!(1),
        "resources/read",
        json!({"uri":"acme://report","_meta":metadata}),
    ));
    let result = host.recv();
    assert_eq!(result["result"]["resultType"], "complete");
    assert_eq!(result["result"]["contents"][0]["text"], "report");
    assert_eq!(result["result"]["cacheScope"], "private");
    host.finish().0.unwrap();
}
