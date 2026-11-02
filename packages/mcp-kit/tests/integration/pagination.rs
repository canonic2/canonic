//! Tool listing pages through the same core replies as ordinary discovery.
use crate::fixture::{Events, Host, Latch, Script};
use canonic_mcp_kit::{
    Annotations, CachePolicy, CacheScope, Catalog, ModernOptions, ServerInfo, Tool, ToolResult,
    modern, rpc,
    stdio::{Options, Outcome},
};
use serde_json::{Value, json};
use std::sync::Arc;

fn catalog() -> Arc<Catalog> {
    Arc::new(
        Catalog::new(
            (0..150)
                .map(|i| {
                    let tool = Tool::new(
                        format!("acme_{i}"),
                        format!("Read item {i}."),
                        json!({"type":"object"}),
                    );
                    if i % 2 == 0 {
                        tool.with_output_schema(json!({"type":"array"}))
                    } else {
                        tool.with_annotations(Annotations {
                            read_only_hint: Some(true),
                            ..Annotations::default()
                        })
                    }
                })
                .collect(),
        )
        .expect("valid catalog"),
    )
}

fn options() -> Options {
    let mut options = Options::default();
    let mut modern = ModernOptions::new(ServerInfo::new("acme", "1"));
    modern.tools_cache = CachePolicy::new(5000, CacheScope::Private).expect("valid cache policy");
    options.session.modern = Some(modern);
    options
}

fn params(mut value: Value, version: &str) -> Value {
    if version == modern::PROTOCOL_VERSION {
        value["_meta"] =
            json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{}});
    }
    value
}

fn initialize(host: &Host, version: &str) {
    if version != modern::PROTOCOL_VERSION {
        host.send(&rpc::request(json!(0),"initialize",json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"acme","version":"1"}})));
        assert_eq!(host.recv()["result"]["protocolVersion"], version);
        host.send(&rpc::notification("notifications/initialized", json!({})));
    }
}

#[test]
fn pagination_tools_driver_preserves_each_revision_and_legacy_visibility() {
    let catalog = catalog();
    for version in canonic_mcp_kit::SUPPORTED_PROTOCOL_VERSIONS
        .iter()
        .copied()
        .chain([modern::PROTOCOL_VERSION])
    {
        let host = Host::start_with(
            Arc::new(Script::new(|_, _| panic!("listing never dispatches"))),
            catalog.clone(),
            options(),
        );
        initialize(&host, version);
        let mut input = json!({});
        let mut all = vec![];
        loop {
            host.send(&rpc::request(
                json!(1),
                "tools/list",
                params(input, version),
            ));
            let reply = host.recv();
            let result = &reply["result"];
            let entries = result["tools"].as_array().unwrap();
            assert_ne!(entries.as_slice(), &[] as &[Value]);
            assert!(entries.len() <= 64);
            all.extend(entries.iter().cloned());
            if version == modern::PROTOCOL_VERSION {
                assert_eq!(result["resultType"], "complete");
                assert_eq!(result["ttlMs"], 5000);
                assert_eq!(result["cacheScope"], "private");
                assert_eq!(
                    result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
                    "acme"
                );
            } else {
                assert!(result.get("resultType").is_none());
                assert!(result.get("ttlMs").is_none());
            }
            let Some(cursor) = result.get("nextCursor") else {
                break;
            };
            input = json!({"cursor":cursor});
        }
        let expected: Vec<Value> = catalog
            .tools()
            .iter()
            .enumerate()
            .filter(|(i, _)| version == modern::PROTOCOL_VERSION || i % 2 == 1)
            .map(|(_, tool)| serde_json::to_value(tool).unwrap())
            .collect();
        assert_eq!(all, expected);
        host.finish().0.unwrap();
    }
}

#[test]
fn pagination_tools_driver_refuses_invalid_cursors_without_retaining_ids() {
    let host = Host::start_with(
        Arc::new(Script::new(|_, _| panic!("listing never dispatches"))),
        catalog(),
        options(),
    );
    host.send(&rpc::request(
        json!(1),
        "tools/list",
        params(json!({}), modern::PROTOCOL_VERSION),
    ));
    let cursor = host.recv()["result"]["nextCursor"].clone();
    for input in [
        json!({"cursor":null}),
        json!({"cursor":""}),
        json!({"cursor":"resources:64"}),
        json!({"requestState":"retry"}),
        json!({"inputResponses":{}}),
    ] {
        host.send(&rpc::request(
            json!(1),
            "tools/list",
            params(input, modern::PROTOCOL_VERSION),
        ));
        assert_eq!(host.recv()["error"]["code"], rpc::INVALID_PARAMS);
    }
    initialize(&host, canonic_mcp_kit::LATEST_PROTOCOL_VERSION);
    host.send(&rpc::request(
        json!(1),
        "tools/list",
        json!({"cursor":cursor}),
    ));
    assert_eq!(host.recv()["error"]["code"], rpc::INVALID_PARAMS);
    host.send(&rpc::request(
        json!(1),
        "tools/list",
        params(json!({"cursor":cursor}), modern::PROTOCOL_VERSION),
    ));
    assert_eq!(host.recv()["result"]["tools"][0]["name"], "acme_64");
    host.finish().0.unwrap();
}

#[test]
fn pagination_tools_remain_responsive_while_a_tool_handler_is_busy() {
    let events = Events::new();
    let started = events.clone();
    let latch = Latch::default();
    let waiting = latch.clone();
    let host = Host::start_with(
        Arc::new(Script::new(move |_, context| {
            started.record("started");
            waiting.wait(context);
            Outcome::Tool(ToolResult::success(vec![]))
        })),
        catalog(),
        options(),
    );
    initialize(&host, canonic_mcp_kit::LATEST_PROTOCOL_VERSION);
    host.call(1, "acme_1", json!({}));
    assert_eq!(events.next(), "started");
    host.send(&rpc::request(json!(2), "tools/list", json!({})));
    let first = host.recv();
    assert_eq!(first["id"], 2);
    host.send(&rpc::request(
        json!(3),
        "tools/list",
        json!({"cursor":first["result"]["nextCursor"]}),
    ));
    let second = host.recv();
    assert_eq!(second["id"], 3);
    assert_eq!(second["result"]["tools"].as_array().unwrap().len(), 11);
    latch.release();
    assert_eq!(host.recv()["id"], 1);
    host.finish().0.unwrap();
}

#[test]
fn pagination_tools_page_overflow_is_an_error_not_a_partial_catalog() {
    let mut options = options();
    options.max_output_bytes = 1024;
    let host = Host::start_with(
        Arc::new(Script::new(|_, _| panic!("listing never dispatches"))),
        catalog(),
        options,
    );
    host.send(&rpc::request(
        json!(1),
        "tools/list",
        params(json!({}), modern::PROTOCOL_VERSION),
    ));
    let reply = host.recv();
    assert_eq!(reply["error"]["code"], rpc::INTERNAL_ERROR);
    assert!(reply.get("result").is_none());
    host.finish().0.unwrap();
}
