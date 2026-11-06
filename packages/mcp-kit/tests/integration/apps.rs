//! Apps use the same bounded scheduler and result boundary as other requests.
use crate::fixture::{Events, Host, Latch, Script};
use canonic_mcp_kit::{
    AppsCatalog, Catalog, Content, ModernOptions, ServerInfo, Tool, ToolResult, ToolUi, UiResource,
    UiVisibility, apps, modern, rpc,
    stdio::{Confirm, Lane, Options, Outcome, Request},
    tool::Annotations,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const URI: &str = "ui://acme/status";
const HTML: &str =
    "<!doctype html><html lang=\"en\"><title>Acme status</title><p>Healthy</p></html>";

fn options(html: &str) -> Options {
    let mut options = Options::default();
    options.session.apps = Some(Arc::new(
        AppsCatalog::new(vec![UiResource::new(URI, "Acme status", html)]).expect("valid Acme HTML"),
    ));
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    options
}

fn catalog() -> Arc<Catalog> {
    let mut refresh = ToolUi::new(URI);
    refresh.visibility = Some(vec![UiVisibility::App]);
    Arc::new(
        Catalog::new(vec![
            Tool::new("acme_show", "Show status.", json!({"type":"object"}))
                .with_ui(ToolUi::new(URI))
                .with_annotations(Annotations {
                    read_only_hint: Some(true),
                    ..Annotations::default()
                }),
            Tool::new("acme_refresh", "Refresh status.", json!({"type":"object"})).with_ui(refresh),
        ])
        .expect("valid Acme tools"),
    )
}

fn send(host: &Host, id: u64, method: &str, mut params: Value, supported: bool) {
    let capabilities = if supported {
        json!({"extensions":{apps::EXTENSION:{"mimeTypes":[apps::MIME_TYPE]}}})
    } else {
        json!({})
    };
    params["_meta"] =
        json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:capabilities});
    host.send(&rpc::request(json!(id), method, params));
}

#[test]
fn apps_stdio_reads_skip_handlers_and_calls_capture_request_support() {
    let host = Host::start_with(
        Arc::new(Script::new(|request, context| {
            assert!(
                matches!(request, Request::Tool(_)),
                "HTML must bypass handlers"
            );
            Outcome::Tool(
                ToolResult::structured(json!({"supportsApps":context.supports_apps()}), false)
                    .with_content(Content::text("Acme is healthy.")),
            )
        })),
        catalog(),
        options(HTML),
    );
    for (id, supported) in [(1, true), (2, false), (3, true)] {
        send(&host, id, "tools/list", json!({}), supported);
        assert_eq!(
            host.recv()["result"]["tools"].as_array().unwrap().len(),
            if supported { 2 } else { 1 }
        );
        send(
            &host,
            id + 10,
            "tools/call",
            json!({"name":"acme_show"}),
            supported,
        );
        let result = host.recv();
        assert_eq!(
            result["result"]["structuredContent"]["supportsApps"],
            supported
        );
        assert!(
            result["result"]["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|content| content["text"] == "Acme is healthy.")
        );
        send(
            &host,
            id + 20,
            "tools/call",
            json!({"name":"acme_refresh"}),
            supported,
        );
        let result = host.recv();
        if supported {
            assert!(result["result"]["content"].is_array());
        } else {
            assert_eq!(result["error"]["code"], rpc::INVALID_PARAMS);
        }
    }
    send(&host, 40, "resources/read", json!({"uri":URI}), true);
    let result = host.recv();
    assert_eq!(result["result"]["contents"][0]["text"], HTML);
    assert_eq!(result["result"]["contents"][0]["mimeType"], apps::MIME_TYPE);
    host.finish().0.unwrap();
}

#[test]
fn apps_stdio_linked_empty_results_and_output_overflow_are_internal_errors() {
    let mut opts = options(&"x".repeat(4096));
    opts.max_output_bytes = 1024;
    let host = Host::start_with(
        Arc::new(Script::new(|_, _| {
            Outcome::Tool(ToolResult::success(vec![]))
        })),
        catalog(),
        opts,
    );
    for (id, method, params) in [
        (1, "tools/call", json!({"name":"acme_show"})),
        (2, "resources/read", json!({"uri":URI})),
    ] {
        send(&host, id, method, params, true);
        let reply = host.recv();
        assert_eq!(reply["error"]["code"], rpc::INTERNAL_ERROR);
        assert!(reply.get("result").is_none());
    }
    host.finish().0.unwrap();
}

#[test]
fn apps_stdio_queued_read_cancellation_and_read_only_eof() {
    let events = Events::new();
    let started = events.clone();
    let latch = Latch::default();
    let blocked = latch.clone();
    let mut host = Host::start_with(
        Arc::new(Script::new(move |_, context| {
            started.record("started");
            blocked.wait(context);
            Outcome::Tool(ToolResult::success(vec![Content::text("Healthy")]))
        })),
        catalog(),
        options(HTML),
    );
    host.initialize(json!({}));
    host.call(1, "acme_show", json!({}));
    assert_eq!(events.next(), "started");
    host.send(&rpc::request(
        json!(2),
        "resources/read",
        json!({"uri":URI}),
    ));
    host.cancel(2);
    host.assert_quiet(); // The dispatcher has consumed cancellation while the worker stays blocked.
    host.send(&rpc::request(
        json!(3),
        "resources/read",
        json!({"uri":URI}),
    ));
    host.close();
    latch.release();
    let (result, replies) = host.finish();
    result.unwrap();
    assert_eq!(replies.len(), 2);
    assert!(replies.iter().all(|reply| reply["id"] != 2));
    assert_eq!(
        replies.iter().find(|reply| reply["id"] == 3).unwrap()["result"]["contents"][0]["text"],
        HTML
    );
}

#[test]
fn apps_stdio_queued_deadline_and_progress_keep_request_identity() {
    let latch = Latch::default();
    let blocked = latch.clone();
    let mut opts = options(HTML);
    opts.timeout = Duration::from_millis(50);
    let mut host = Host::start_with(
        Arc::new(Script::new(move |_, context| {
            assert!(context.progress(1.0, Some(2.0), Some("Working")));
            blocked.wait(context);
            Outcome::Tool(ToolResult::success(vec![Content::text("Healthy")]))
        })),
        catalog(),
        opts,
    );
    host.initialize(json!({}));
    host.send(&rpc::request(json!(1), "tools/call", json!({"name":"acme_show","arguments":{"timeoutMs":3000},"_meta":{"progressToken":"acme-tool"}})));
    assert_eq!(host.recv()["params"]["progressToken"], "acme-tool");
    host.send(&rpc::request(
        json!(2),
        "resources/read",
        json!({"uri":URI}),
    ));
    let expired = host.recv();
    assert_eq!(expired["id"], 2);
    assert_eq!(expired["error"]["code"], rpc::DEADLINE_EXCEEDED);
    latch.release();
    assert_eq!(host.recv()["id"], 1);
    host.assert_quiet();
    host.finish().0.unwrap();
}

#[test]
fn apps_stdio_interleaved_clients_and_continuations_keep_their_captured_support() {
    let events = Events::new();
    let started = events.clone();
    let latch = Latch::default();
    let blocked = latch.clone();
    let mut opts = options(HTML);
    opts.lanes = vec![Lane::new("tools", 2, 4)];
    let host = Host::start_with(
        Arc::new(Script::new(move |request, context| {
            let Request::Tool(call) = request else {
                panic!("tool")
            };
            if call.arguments["block"] == true {
                assert!(context.supports_apps());
                started.record("started");
                blocked.wait(context);
                // Modern Confirm falls back through the same request's continuation.
                Outcome::Confirm(Confirm::new("Continue?", |_, context| {
                    assert!(context.supports_apps());
                    Outcome::Tool(ToolResult::success(vec![Content::text("Supported")]))
                }))
            } else {
                assert!(!context.supports_apps());
                Outcome::Tool(ToolResult::success(vec![Content::text("Text fallback")]))
            }
        })),
        catalog(),
        opts,
    );
    send(
        &host,
        1,
        "tools/call",
        json!({"name":"acme_show","arguments":{"block":true}}),
        true,
    );
    assert_eq!(events.next(), "started");
    send(&host, 2, "tools/call", json!({"name":"acme_show"}), false);
    assert_eq!(host.recv()["result"]["content"][0]["text"], "Text fallback");
    latch.release();
    assert_eq!(host.recv()["result"]["content"][0]["text"], "Supported");
    host.finish().0.unwrap();
}
