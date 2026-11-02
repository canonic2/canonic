//! Completion capability, checked targets and shared driver lifetime.
use crate::fixture::{Events, Host, Latch, Script};
use canonic_mcp_kit::{
    CompleteResult, CompletionReference, ModernOptions, Prompt, PromptArgument, PromptCatalog,
    ResourceCatalog, ResourceTemplate, ServerInfo, modern, rpc,
    stdio::{Confirm, Options, Outcome, Request},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

fn options() -> Options {
    let mut options = Options::default();
    let mut prompt = Prompt::new("acme_review");
    prompt.arguments = vec![
        PromptArgument::new("language", true),
        PromptArgument::new("framework", true),
    ];
    options.session.prompts = Some(Arc::new(
        PromptCatalog::new(vec![prompt]).expect("valid prompts"),
    ));
    options.session.resources = Some(Arc::new(
        ResourceCatalog::new(
            vec![],
            vec![ResourceTemplate::new(
                "acme://reports/{id}{?team}",
                "Reports",
            )],
        )
        .expect("valid templates"),
    ));
    options.session.completions = true;
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    options
}

fn params() -> Value {
    json!({"ref":{"type":"ref/prompt","name":"acme_review"},"argument":{"name":"framework","value":"fl"},"context":{"arguments":{"language":"python"}}})
}

fn tagged(mut params: Value) -> Value {
    params["_meta"] =
        json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{}});
    params
}

fn result(value: &str) -> Outcome {
    Outcome::Completion(
        CompleteResult::new(vec![value.into()], Some(1), Some(false)).expect("valid suggestions"),
    )
}

#[test]
fn completion_all_revisions_publish_capability_and_dispatch_both_target_kinds() {
    for version in canonic_mcp_kit::SUPPORTED_PROTOCOL_VERSIONS {
        let host = Host::start(
            Script::new(|request, _| {
                let Request::Completion(request) = request else {
                    panic!("completion")
                };
                match request.reference {
                    CompletionReference::Prompt { .. } => {
                        assert_eq!(request.context["language"], "python");
                        assert_eq!(request.argument.value, "fl");
                        result("flask")
                    }
                    CompletionReference::Resource { uri } => {
                        assert_eq!(uri, "acme://reports/{id}{?team}");
                        assert_eq!(request.context["team"], "acme");
                        result("report-1")
                    }
                }
            }),
            options(),
        );
        host.send(&rpc::request(json!(0), "completion/complete", params()));
        assert_eq!(host.recv()["error"]["code"], rpc::SERVER_ERROR);
        host.send(&rpc::request(json!(1),"initialize",json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"acme","version":"1"}})));
        assert_eq!(
            host.recv()["result"]["capabilities"]["completions"],
            json!({})
        );
        host.send(&rpc::notification("notifications/initialized", json!({})));
        host.send(&rpc::request(json!(2), "completion/complete", params()));
        let reply = host.recv();
        assert_eq!(reply["result"]["completion"]["values"], json!(["flask"]));
        assert!(reply["result"].get("resultType").is_none());
        host.send(&rpc::request(
            json!(3),
            "server/discover",
            tagged(json!({})),
        ));
        assert_eq!(
            host.recv()["result"]["capabilities"]["completions"],
            json!({})
        );
        host.send(&rpc::request(json!(4),"completion/complete",tagged(json!({"ref":{"type":"ref/resource","uri":"acme://reports/{id}{?team}"},"argument":{"name":"id","value":"report"},"context":{"arguments":{"team":"acme"}}}))));
        let reply = host.recv();
        assert_eq!(reply["result"]["completion"]["values"], json!(["report-1"]));
        assert_eq!(reply["result"]["resultType"], "complete");
        assert!(reply["result"].get("ttlMs").is_none());
        host.finish().0.unwrap();
    }
}

#[test]
fn completion_disabled_or_invalid_never_dispatches() {
    for enabled in [false, true] {
        let mut options = options();
        options.session.completions = enabled;
        options
            .session
            .extension_methods
            .push("completion/complete".into());
        let mut host = Host::start(Script::new(|_, _| panic!("no valid completion")), options);
        let init = host.initialize(json!({}));
        assert_eq!(
            init["result"]["capabilities"].get("completions").is_some(),
            enabled
        );
        let mut invalid = params();
        invalid["argument"]["name"] = json!("unknown");
        for params in [invalid.clone(), tagged(invalid)] {
            host.send(&rpc::request(json!(2), "completion/complete", params));
            assert_eq!(
                host.recv()["error"]["code"],
                if enabled {
                    rpc::INVALID_PARAMS
                } else {
                    rpc::METHOD_NOT_FOUND
                }
            );
        }
        host.finish().0.unwrap();
    }
}

#[test]
fn completion_wrong_successes_and_confirmation_steps_are_refused_without_resuming() {
    for modern in [false, true] {
        let events = Events::new();
        let resumed = events.clone();
        let host = Host::ready(
            Script::new(move |request, _| {
                let Request::Completion(request) = request else {
                    panic!("completion")
                };
                if request.argument.value == "raw" {
                    return Outcome::Method(Ok(json!({"completion":{"values":[]}})));
                }
                let resumed = resumed.clone();
                Outcome::Confirm(Confirm::new("Not a completion", move |_, _| {
                    resumed.record("resumed");
                    result("wrong")
                }))
            }),
            options(),
            json!({"elicitation":{"form":{}}}),
        );
        for value in ["raw", "question"] {
            let mut input = params();
            input["argument"]["value"] = json!(value);
            if modern {
                input = tagged(input);
            }
            host.send(&rpc::request(json!(1), "completion/complete", input));
            assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
        }
        assert_eq!(events.none_within(Duration::ZERO), None);
        host.assert_quiet();
        host.finish().0.unwrap();
    }
}

fn blocked(events: Events, latch: Latch) -> Script {
    Script::new(move |request, context| {
        let Request::Completion(request) = request else {
            panic!("completion")
        };
        events.record(format!("start {}", request.argument.value));
        latch.wait(context);
        events.record(format!("end {}", request.argument.value));
        result("flask")
    })
}

fn send(host: &Host, id: u64, value: &str) {
    let mut input = params();
    input["argument"]["value"] = json!(value);
    host.send(&rpc::request(json!(id), "completion/complete", input));
}

#[test]
fn completion_cancellation_capacity_and_deadlines_preserve_request_lifetime() {
    let events = Events::new();
    let latch = Latch::default();
    let mut opts = options();
    opts.lanes[0].capacity = 1;
    let host = Host::ready(blocked(events.clone(), latch.clone()), opts, json!({}));
    send(&host, 1, "running");
    assert_eq!(events.next(), "start running");
    send(&host, 2, "refused");
    assert_eq!(host.recv()["error"]["code"], rpc::SERVER_ERROR);
    host.cancel(1);
    assert_eq!(events.next(), "end running");
    latch.release();
    host.assert_quiet();
    host.finish().0.unwrap();
    let mut opts = options();
    opts.timeout = Duration::from_millis(30);
    let host = Host::ready(blocked(Events::new(), Latch::default()), opts, json!({}));
    send(&host, 1, "deadline");
    assert_eq!(host.recv()["error"]["code"], rpc::DEADLINE_EXCEEDED);
    send(&host, 1, "reused");
    assert_eq!(host.recv()["error"]["code"], rpc::DEADLINE_EXCEEDED);
    host.finish().0.unwrap();
}

#[test]
fn completion_eof_finishes_queued_and_running_read_only_requests() {
    let events = Events::new();
    let latch = Latch::default();
    let mut host = Host::ready(blocked(events.clone(), latch.clone()), options(), json!({}));
    send(&host, 1, "running");
    assert_eq!(events.next(), "start running");
    send(&host, 2, "queued");
    host.close();
    latch.release();
    let (status, replies) = host.finish();
    status.unwrap();
    assert_eq!(replies.len(), 2);
    assert!(
        replies
            .iter()
            .all(|reply| reply["result"]["completion"]["values"] == json!(["flask"]))
    );
}

#[test]
fn completion_progress_and_output_overflow_are_bounded() {
    let latch = Latch::default();
    let waiting = latch.clone();
    let mut opts = options();
    opts.max_output_bytes = 1024;
    let host = Host::ready(
        Script::new(move |_, context| {
            assert!(context.progress(1.0, None, Some("Finding matches")));
            waiting.wait(context);
            result(&"x".repeat(2000))
        }),
        opts,
        json!({}),
    );
    let mut input = params();
    input["_meta"] = json!({"progressToken":"acme-completion"});
    host.send(&rpc::request(json!(1), "completion/complete", input));
    let progress = host.recv();
    assert_eq!(progress["method"], "notifications/progress");
    assert_eq!(progress["params"]["progressToken"], "acme-completion");
    latch.release();
    assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
    host.finish().0.unwrap();
}
