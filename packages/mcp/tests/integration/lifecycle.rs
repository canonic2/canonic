//! Initialization, catalog replies, tool results and connection-ending violations.

use crate::fixture::{Host, Script, arguments, catalog, result_text, text};
use canonic_mcp::{
    SessionOptions,
    stdio::{self, Lane, Options, Outcome, Request, ServeError},
};
use serde_json::{Value, json};
use std::{
    io::{BufReader, Cursor},
    sync::Arc,
};

fn echo() -> Script {
    Script::new(|request, _| match request {
        Request::Tool(call) if call.name == "acme_panic" => panic!("Acme handler failure"),
        Request::Tool(call) => text(call.arguments["message"].as_str().unwrap_or("")),
        Request::Method { method, params } => {
            Outcome::Method(Ok(json!({"method":method,"params":params})))
        }
        Request::Notification { .. } => Outcome::Done,
    })
}

#[test]
fn finite_input_still_receives_initialization_and_catalog_replies() {
    let mut input = Vec::new();
    for message in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"acme-host","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    ] {
        input.extend(serde_json::to_vec(&message).unwrap());
        input.push(b'\n');
    }
    let mut output = Vec::new();
    stdio::serve(
        Arc::new(echo()),
        catalog(),
        BufReader::new(Cursor::new(input)),
        &mut output,
        &Options::default(),
    )
    .unwrap();
    let replies: Vec<Value> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "acme");
    assert_eq!(replies[0]["result"]["instructions"], "Use acme_echo.");
    assert_eq!(replies[1]["result"], catalog().listing());
}

#[test]
fn a_refused_initialization_can_be_retried() {
    let mut host = Host::start(echo(), Options::default());
    host.send(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"refused-host","version":"1"}}}));
    assert_eq!(host.recv()["error"]["message"], "Acme refuses this host");
    let reply = host.initialize(json!({}));
    assert_eq!(reply["result"]["protocolVersion"], "2025-11-25");
    host.assert_quiet();
}

#[test]
fn tool_calls_return_results_and_argument_errors_as_tool_results() {
    let host = Host::ready(echo(), Options::default(), json!({}));
    host.call(1, "acme_echo", json!({"message":"hello"}));
    let reply = host.recv();
    assert_eq!(reply["id"], 1);
    assert_eq!(result_text(&reply), "hello");
    assert_eq!(reply["result"]["isError"], false);

    host.call(2, "acme_echo", json!({"message":5}));
    let reply = host.recv();
    assert_eq!(reply["result"]["isError"], true);
    assert_eq!(result_text(&reply), "arguments.message must be string");

    host.call(3, "acme_missing", json!({}));
    assert_eq!(host.recv()["error"]["code"], -32602);
}

#[test]
fn handler_panics_become_internal_errors_and_the_worker_survives() {
    let catalog = Arc::new(
        canonic_mcp::Catalog::new(vec![
            canonic_mcp::Tool::new("acme_panic", "Fail.", json!({"type":"object"})),
            canonic_mcp::Tool::new("acme_echo", "Echo.", json!({"type":"object"})),
        ])
        .unwrap(),
    );
    let mut host = Host::start_with(Arc::new(echo()), catalog, Options::default());
    host.initialize(json!({}));
    host.call(1, "acme_panic", json!({}));
    assert_eq!(host.recv()["error"]["code"], -32603);
    host.call(2, "acme_echo", json!({"message":"still here"}));
    assert_eq!(result_text(&host.recv()), "still here");
}

#[test]
fn extension_methods_reply_and_notifications_run_without_replies() {
    let notified = crate::fixture::Events::new();
    let events = notified.clone();
    let script = Script::new(move |request, _| match request {
        Request::Notification { method, .. } => {
            events.record(method);
            Outcome::Done
        }
        Request::Method { params, .. } => Outcome::Method(Ok(params)),
        Request::Tool(_) => text(""),
    });
    let options = Options {
        session: SessionOptions {
            extension_methods: vec!["acme/task_end".into()],
            extension_notifications: vec!["notifications/acme/task_end".into()],
            ..SessionOptions::default()
        },
        ..Options::default()
    };
    let host = Host::ready(script, options, json!({}));
    host.send(&json!({"jsonrpc":"2.0","id":7,"method":"acme/task_end","params":{"reason":"done"}}));
    assert_eq!(
        host.recv(),
        json!({"jsonrpc":"2.0","id":7,"result":{"reason":"done"}})
    );
    host.send(&json!({"jsonrpc":"2.0","method":"notifications/acme/task_end"}));
    assert_eq!(notified.next(), "notifications/acme/task_end");
    host.assert_quiet();
}

#[test]
fn oversized_frames_end_the_connection_after_an_error_reply() {
    let options = Options {
        session: SessionOptions {
            max_message_bytes: 512,
            ..SessionOptions::default()
        },
        ..Options::default()
    };
    let host = Host::ready(echo(), options, json!({}));
    let mut frame = vec![b' '; 513];
    frame.push(b'\n');
    host.send_bytes(frame);
    assert_eq!(host.recv()["error"]["code"], -32700);
    let (result, _) = host.finish();
    assert!(matches!(result, Err(ServeError::Protocol(_))));
}

#[test]
fn a_duplicate_outstanding_id_ends_the_connection() {
    let latch = crate::fixture::Latch::default();
    let blocked = latch.clone();
    let script = Script::new(move |_, context| {
        blocked.wait(context);
        text("done")
    });
    let host = Host::ready(script, Options::default(), json!({}));
    host.call(1, "acme_change", json!({}));
    host.call(1, "acme_change", json!({}));
    let reply = host.recv();
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["error"]["code"], -32600);
    latch.release();
    let (result, _) = host.finish();
    assert!(matches!(result, Err(ServeError::Protocol(_))));
}

#[test]
fn oversized_results_become_internal_errors() {
    let script = Script::new(|request, _| {
        text(&"x".repeat(arguments(&request)["message"].as_str().unwrap_or("").len() * 1000))
    });
    let options = Options {
        max_output_bytes: 2048,
        ..Options::default()
    };
    let host = Host::ready(script, options, json!({}));
    host.call(1, "acme_echo", json!({"message":"abc"}));
    let reply = host.recv();
    assert_eq!(reply["error"]["code"], -32603);
    assert_eq!(reply["error"]["message"], "Response exceeds output limit");
}

#[test]
fn invalid_options_are_refused_before_serving() {
    for lanes in [
        vec![],
        vec![Lane::new("tools", 0, 1)],
        vec![Lane::new("tools", 2, 1)],
    ] {
        let result = stdio::serve(
            Arc::new(echo()),
            catalog(),
            BufReader::new(Cursor::new(Vec::new())),
            Vec::new(),
            &Options {
                lanes,
                ..Options::default()
            },
        );
        assert!(matches!(result, Err(ServeError::Options(_))));
    }
}
