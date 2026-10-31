//! Tool artifacts across protocol revisions and independent runtimes.

use crate::fixture::{Host, Script, catalog};
use canonic_mcp_kit::{
    Content, Inbound, ModernOptions, ReadResourceResult, Resource, ResourceCatalog,
    ResourceContents, ServerInfo, Session, SessionOptions, ToolResult,
    control::Interrupted,
    json::EncodeError,
    modern, rpc,
    stdio::{Options, Outcome, Request},
};
use serde_json::{Value, json};
use std::sync::Arc;

fn result() -> ToolResult {
    ToolResult::structured(json!({"passed":true}), false)
        .with_content(Content::image(&[0, 255], "image/png"))
        .with_content(
            Content::resource_link(Resource::new("acme://reports/a%2Fb", "Report"))
                .expect("valid report link"),
        )
        .with_content(Content::resource(
            ResourceContents::text("acme://summary", Some("text/plain".into()), "Passed")
                .expect("valid summary"),
        ))
        .with_content(Content::resource(
            ResourceContents::blob("acme://capture", Some("image/png".into()), &[0, 255, 42])
                .expect("valid capture"),
        ))
}

fn params(modern_request: bool) -> Value {
    if modern_request {
        json!({"_meta": {modern::VERSION_KEY:modern::PROTOCOL_VERSION, modern::CAPABILITIES_KEY:{}}})
    } else {
        json!({})
    }
}

fn tool_params(modern_request: bool) -> Value {
    let mut params = params(modern_request);
    params["name"] = json!("acme_echo");
    params["arguments"] = json!({});
    params
}

#[test]
fn resource_content_driver_all_revisions_can_read_unlisted_links() {
    for version in ["2025-06-18", "2025-11-25", modern::PROTOCOL_VERSION] {
        let modern_request = version == modern::PROTOCOL_VERSION;
        let mut options = Options::default();
        options.session.resources = Some(Arc::new(ResourceCatalog::new(vec![], vec![]).unwrap()));
        options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
        let host = Host::start(
            Script::new(|request, _| match request {
                Request::Tool(_) => Outcome::Tool(result()),
                Request::Method { params, .. } => {
                    let uri = params["uri"].as_str().unwrap();
                    if uri != "acme://reports/a%2Fb" {
                        return Outcome::ResourceNotFound(uri.into());
                    }
                    Outcome::Resource(
                        ReadResourceResult::new(vec![
                            ResourceContents::text(uri, None, "Full report").unwrap(),
                        ])
                        .unwrap(),
                    )
                }
                _ => panic!("unexpected request"),
            }),
            options,
        );
        if !modern_request {
            host.send(&rpc::request(json!("init"), "initialize", json!({"protocolVersion":version,"clientInfo":{"name":"acme-host","version":"1"},"capabilities":{}})));
            assert_eq!(host.recv()["result"]["protocolVersion"], version);
            host.send(&rpc::notification("notifications/initialized", json!({})));
        }
        host.send(&rpc::request(
            json!(1),
            "tools/call",
            tool_params(modern_request),
        ));
        let reply = host.recv();
        let expected = serde_json::to_value(result()).unwrap();
        assert_eq!(reply["result"]["content"], expected["content"], "{version}");
        assert_eq!(
            reply["result"]["structuredContent"],
            expected["structuredContent"]
        );
        assert_eq!(reply["result"]["isError"], false);
        if modern_request {
            assert_eq!(reply["result"]["resultType"], "complete");
        }
        host.send(&rpc::request(
            json!(2),
            "resources/list",
            params(modern_request),
        ));
        assert_eq!(host.recv()["result"]["resources"], json!([]));
        let mut read = params(modern_request);
        read["uri"] = json!("acme://reports/a%2Fb");
        host.send(&rpc::request(json!(3), "resources/read", read.clone()));
        assert_eq!(host.recv()["result"]["contents"][0]["text"], "Full report");
        read["uri"] = json!("acme://denied");
        host.send(&rpc::request(json!(4), "resources/read", read));
        assert_eq!(
            host.recv()["error"]["code"],
            if modern_request {
                rpc::INVALID_PARAMS
            } else {
                rpc::RESOURCE_NOT_FOUND
            }
        );
        host.finish().0.unwrap();
    }
}

#[test]
fn resource_content_does_not_enable_resource_methods() {
    let host = Host::ready(
        Script::new(|_, _| Outcome::Tool(result())),
        Options::default(),
        json!({}),
    );
    host.call(1, "acme_echo", json!({}));
    assert_eq!(host.recv()["result"]["content"][2]["type"], "resource_link");
    host.send(&rpc::request(
        json!(2),
        "resources/read",
        json!({"uri":"acme://reports/a%2Fb"}),
    ));
    assert_eq!(host.recv()["error"]["code"], rpc::METHOD_NOT_FOUND);
    host.finish().0.unwrap();
}

#[test]
fn resource_content_output_overflow_returns_error_without_partial_data() {
    let options = Options {
        max_output_bytes: 1024,
        ..Options::default()
    };
    let host = Host::ready(
        Script::new(|_, _| {
            Outcome::Tool(ToolResult::success(vec![Content::resource(
                ResourceContents::text("acme://report", None, "x".repeat(2000)).unwrap(),
            )]))
        }),
        options,
        json!({}),
    );
    host.call(1, "acme_echo", json!({}));
    let reply = host.recv();
    assert_eq!(reply["error"]["code"], rpc::INTERNAL_ERROR);
    assert!(reply.get("result").is_none());
    host.assert_quiet();
    host.finish().0.unwrap();
}

#[test]
fn resource_content_independent_runtime_encodes_with_bound_and_cancellation() {
    let mut session = Session::try_new(
        catalog(),
        SessionOptions {
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    assert!(matches!(
        session.receive_value(rpc::request(json!(1), "tools/call", tool_params(true))),
        Inbound::Tool { .. }
    ));
    let context = session.request_context(&json!(1)).unwrap();
    let result = result();
    let bytes = context
        .encode_success(&json!(1), &result, 4096, &|| Ok(()))
        .unwrap();
    let reply: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["result"]["content"], json!(&result)["content"]);
    assert_eq!(reply["result"]["resultType"], "complete");
    assert_eq!(
        context.encode_success(&json!(1), &result, bytes.len(), &|| Ok(())),
        Ok(bytes.clone())
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
    assert!(session.finish(&json!(1)));
}
