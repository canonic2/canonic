//! Metadata and propagation through core admission, wire encoding and stdio.
use super::fixture::{Host, Script};
use canonic_mcp_kit::{
    Catalog, CompleteResult, Content, GetPromptResult, Icon, Inbound, Metadata, ModernOptions,
    Prompt, PromptCatalog, ReadResourceResult, Resource, ResourceCatalog, ResourceContents,
    ResourceTemplate, ServerInfo, Session, SessionOptions, Tool, ToolResult,
    control::Interrupted,
    json::EncodeError,
    modern, rpc,
    stdio::{Options, Outcome},
};
use serde_json::{Value, json};
use std::sync::Arc;

const PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

fn metadata() -> Metadata {
    Metadata::new(json!({"com.acme/display":{"color":"blue","extra":null}}))
        .expect("valid metadata")
}

fn setup() -> (Arc<Catalog>, SessionOptions) {
    let icon = Icon::new(
        "https://example.com/icon.png",
        None,
        vec!["any".into()],
        None,
    )
    .expect("valid icon");
    let mut tool = Tool::new("acme_echo", "Echo", json!({"type":"object"}));
    tool.meta = Some(metadata());
    tool.icons = vec![icon.clone()];
    let mut resource = Resource::new("acme://report", "Report");
    resource.meta = Some(metadata());
    resource.icons = vec![icon.clone()];
    let mut template = ResourceTemplate::new("acme://report/{name}", "Reports");
    template.meta = Some(metadata());
    template.icons = vec![icon.clone()];
    let mut prompt = Prompt::new("acme_summary");
    prompt.meta = Some(metadata());
    prompt.icons = vec![icon.clone()];
    let mut server = ServerInfo::new("acme", "1");
    server.description = Some("Acme reports".into());
    server.website_url = Some("https://example.com".into());
    server.icons = vec![icon];
    (
        Arc::new(Catalog::new(vec![tool]).expect("valid tools")),
        SessionOptions {
            modern: Some(ModernOptions::new(server)),
            resources: Some(Arc::new(
                ResourceCatalog::new(vec![resource], vec![template]).expect("valid resources"),
            )),
            prompts: Some(Arc::new(
                PromptCatalog::new(vec![prompt]).expect("valid prompts"),
            )),
            ..SessionOptions::default()
        },
    )
}

fn params() -> Value {
    json!({"_meta":{
        modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{},
        "com.acme/request":{"opaque":null},"traceparent":PARENT,"tracestate":"vendor=abc","baggage":"key=value"
    }})
}

fn reply(inbound: Inbound) -> Value {
    let Inbound::Reply(value) = inbound else {
        panic!("reply expected")
    };
    value
}

#[test]
fn metadata_catalogs_and_identity_publish_in_all_revisions() {
    for version in ["2025-06-18", "2025-11-25", modern::PROTOCOL_VERSION] {
        let (catalog, options) = setup();
        let mut session = Session::try_new(catalog, options).unwrap();
        let modern_request = version == modern::PROTOCOL_VERSION;
        if modern_request {
            let result =
                reply(session.receive_value(rpc::request(json!(0), "server/discover", params())));
            let identity = &result["result"]["_meta"]["io.modelcontextprotocol/serverInfo"];
            assert_eq!(identity["description"], "Acme reports");
            assert_eq!(identity["websiteUrl"], "https://example.com");
            assert_eq!(identity["icons"][0]["sizes"], json!(["any"]));
        } else {
            let init = json!({"protocolVersion":version,"clientInfo":{"name":"acme-host","version":"1","description":"Host","custom":true},"capabilities":{}});
            let Inbound::Initialize { id, client } =
                session.receive_value(rpc::request(json!(0), "initialize", init))
            else {
                panic!("initialize")
            };
            assert_eq!(client.info["custom"], true);
            let mut server = ServerInfo::new("acme", "1");
            server.description = Some("Acme reports".into());
            let initialized = session.accept_initialize(id, &server);
            assert_eq!(
                initialized["result"]["serverInfo"]["description"],
                "Acme reports"
            );
            session.receive_value(rpc::notification("notifications/initialized", json!({})));
        }
        for (i, (method, key)) in [
            ("tools/list", "tools"),
            ("resources/list", "resources"),
            ("resources/templates/list", "resourceTemplates"),
            ("prompts/list", "prompts"),
        ]
        .into_iter()
        .enumerate()
        {
            let result = reply(session.receive_value(rpc::request(
                json!(i + 1),
                method,
                if modern_request { params() } else { json!({}) },
            )));
            let item = &result["result"][key][0];
            assert_eq!(
                item["_meta"],
                serde_json::to_value(metadata()).unwrap(),
                "{version} {method}: {result}"
            );
            assert_eq!(item["icons"][0]["src"], "https://example.com/icon.png");
            assert!(result["result"]["_meta"].get("com.acme/request").is_none());
        }
    }
}

#[test]
fn metadata_independent_runtime_merges_once_and_checks_owned_fields_and_limits() {
    let (catalog, options) = setup();
    let mut session = Session::try_new(catalog, options).unwrap();
    let mut p = params();
    p["name"] = json!("acme_echo");
    assert!(matches!(
        session.receive_value(rpc::request(json!(1), "tools/call", p)),
        Inbound::Tool { .. }
    ));
    let context = session.request_context(&json!(1)).unwrap();
    let result = ToolResult::success(vec![Content::text("hello").with_metadata(metadata())])
        .with_metadata(metadata());
    let bytes = context
        .encode_success(&json!(1), &result, 4096, &|| Ok(()))
        .unwrap();
    // The strict parser rejects duplicate keys, including duplicate _meta objects.
    let value = canonic_mcp_kit::json::parse(&bytes, 4096).unwrap();
    assert_eq!(
        value["result"]["_meta"]["com.acme/display"],
        json!({"color":"blue","extra":null})
    );
    assert_eq!(
        value["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "acme"
    );
    assert!(value["result"]["_meta"].get("traceparent").is_none());
    assert_eq!(
        context.encode_success(&json!(1), &result, bytes.len(), &|| Ok(())),
        Ok(bytes.clone())
    );
    assert!(matches!(
        context.encode_success(&json!(1), &result, bytes.len() - 1, &|| Ok(())),
        Err(EncodeError::TooLarge { .. })
    ));
    assert_eq!(
        context.encode_success(&json!(1), &result, 4096, &|| Err(Interrupted)),
        Err(EncodeError::Interrupted)
    );
    for key in [
        "io.modelcontextprotocol/serverInfo",
        modern::VERSION_KEY,
        "progressToken",
    ] {
        let result =
            ToolResult::success(vec![]).with_metadata(Metadata::new(json!({key:{}})).unwrap());
        assert_eq!(
            context.encode_success(&json!(1), &result, 4096, &|| Ok(())),
            Err(EncodeError::Invalid)
        );
    }
    assert!(session.finish(&json!(1)));
    let mut p = params();
    p["name"] = json!("acme_echo");
    p["_meta"].as_object_mut().unwrap().remove("traceparent");
    assert!(matches!(
        session.receive_value(rpc::request(json!(1), "tools/call", p)),
        Inbound::Tool { .. }
    ));
    assert_eq!(
        session
            .request_context(&json!(1))
            .unwrap()
            .trace_context()
            .traceparent(),
        None
    );
    assert_eq!(context.trace_context().traceparent(), Some(PARENT));
}

#[test]
fn metadata_stdio_keeps_context_per_request_and_refuses_bad_trace_before_work() {
    let (catalog, session) = setup();
    let options = Options {
        session,
        ..Options::default()
    };
    let host = Host::start_with(
        Arc::new(Script::new(|_, context| {
            let context = context.request_context().unwrap();
            let trace = context.trace_context();
            assert_eq!(
                context.metadata().as_map()["com.acme/request"],
                json!({"opaque":null})
            );
            Outcome::Tool(
                ToolResult::success(vec![Content::text(trace.traceparent().unwrap_or("absent"))])
                    .with_metadata(metadata()),
            )
        })),
        catalog,
        options,
    );
    for (id, parent) in [(1, Some(PARENT)), (2, None)] {
        let mut p = params();
        p["name"] = json!("acme_echo");
        if parent.is_none() {
            p["_meta"].as_object_mut().unwrap().remove("traceparent");
        }
        host.send(&rpc::request(json!(id), "tools/call", p));
    }
    let mut results = [host.recv(), host.recv()];
    results.sort_by_key(|value| value["id"].as_u64());
    assert_eq!(results[0]["result"]["content"][0]["text"], PARENT);
    assert_eq!(results[1]["result"]["content"][0]["text"], "absent");
    let mut p = params();
    p["name"] = json!("acme_echo");
    p["_meta"]["traceparent"] = json!("bad");
    host.send(&rpc::request(json!(3), "tools/call", p));
    assert_eq!(host.recv()["error"]["code"], rpc::INVALID_PARAMS);
    let (result, remaining) = host.finish();
    assert!(result.is_ok());
    assert_eq!(remaining, Vec::<Value>::new());
}

#[test]
fn metadata_other_typed_results_preserve_custom_fields() {
    let resource = ReadResourceResult::new(vec![
        ResourceContents::text("acme://report", None, "Report").unwrap(),
    ])
    .unwrap()
    .with_metadata(metadata());
    let prompt = GetPromptResult::new(None, vec![])
        .unwrap()
        .with_metadata(metadata())
        .unwrap();
    let completion = CompleteResult::new(vec!["report".into()], None, None)
        .unwrap()
        .with_metadata(metadata())
        .unwrap();
    for value in [
        serde_json::to_value(resource).unwrap(),
        serde_json::to_value(prompt).unwrap(),
        serde_json::to_value(completion).unwrap(),
    ] {
        assert_eq!(value["_meta"], serde_json::to_value(metadata()).unwrap());
    }
}

#[test]
fn metadata_custom_method_results_use_the_same_checked_merge() {
    let (catalog, mut session) = setup();
    session.extension_methods.push("acme/custom".into());
    let host = Host::start_with(
        Arc::new(Script::new(|_, _| {
            Outcome::Method(Ok(json!({"value":true,"_meta":{"com.acme/answer":null}})))
        })),
        catalog,
        Options {
            session,
            ..Options::default()
        },
    );
    host.send(&rpc::request(json!(1), "acme/custom", params()));
    let value = host.recv();
    assert_eq!(value["result"]["value"], true);
    assert_eq!(value["result"]["_meta"]["com.acme/answer"], Value::Null);
    assert_eq!(
        value["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "acme"
    );
    assert!(host.finish().0.is_ok());
}

#[test]
fn metadata_request_cost_observation() {
    use std::time::Instant;
    for (size, custom) in [
        (32, false),
        (32, true),
        (512 * 1024, false),
        (512 * 1024, true),
    ] {
        let start = Instant::now();
        let (catalog, session) = setup();
        let result = ToolResult::success(vec![Content::text("a".repeat(size))]);
        let result = if custom {
            result.with_metadata(metadata())
        } else {
            result
        };
        let host = Host::start_with(
            Arc::new(Script::new(move |_, _| Outcome::Tool(result.clone()))),
            catalog,
            Options {
                session,
                ..Options::default()
            },
        );
        let mut p = params();
        p["name"] = json!("acme_echo");
        host.send(&rpc::request(json!(0), "tools/call", p.clone()));
        assert_eq!(
            host.recv()["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .len(),
            size
        );
        let first = start.elapsed();
        let retained = Instant::now();
        for id in 1..=8 {
            host.send(&rpc::request(json!(id), "tools/call", p.clone()));
            let value = host.recv();
            assert_eq!(
                value["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .len(),
                size
            );
            assert_eq!(
                value["result"]["_meta"].get("com.acme/display").is_some(),
                custom
            );
        }
        let retained = retained.elapsed();
        let closing = Instant::now();
        assert!(host.finish().0.is_ok());
        eprintln!(
            "metadata-cost size={size} custom={custom} first={first:?} retained-eight={retained:?} cleanup={:?}",
            closing.elapsed()
        );
    }
}
