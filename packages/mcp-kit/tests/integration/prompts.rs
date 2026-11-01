//! Prompt admission, mixed revisions, retries and shared scheduling lifetime.
use crate::fixture::{Events, Host, Latch, Script};
use canonic_mcp_kit::{
    Content, GetPromptResult, ModernOptions, Prompt, PromptArgument, PromptCatalog, PromptMessage,
    Role, ServerInfo, modern, rpc,
    stdio::{Options, Outcome, Request},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

fn options() -> Options {
    let mut options = Options::default();
    let mut prompt = Prompt::new("acme_review");
    prompt.arguments.push(PromptArgument::new("code", true));
    options.session.prompts = Some(Arc::new(
        PromptCatalog::new(vec![prompt]).expect("valid prompt catalog"),
    ));
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    options
}

fn result(text: impl Into<String>) -> Outcome {
    Outcome::Prompt(
        GetPromptResult::new(
            None,
            vec![PromptMessage {
                role: Role::User,
                content: Content::text(text),
            }],
        )
        .expect("valid prompt messages"),
    )
}

fn get(host: &Host, id: u64, code: &str) {
    host.send(&rpc::request(
        json!(id),
        "prompts/get",
        json!({"name":"acme_review","arguments":{"code":code}}),
    ));
}

fn tagged(mut params: Value) -> Value {
    params["_meta"] = json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION, modern::CAPABILITIES_KEY:{"elicitation":{"form":{}}}});
    params
}

#[test]
fn prompts_legacy_revisions_and_modern_publish_and_validate_before_dispatch() {
    for version in canonic_mcp_kit::SUPPORTED_PROTOCOL_VERSIONS {
        let host = Host::start(
            Script::new(|request, _| {
                let Request::Prompt(call) = request else {
                    panic!("prompt only")
                };
                result(call.arguments["code"].clone())
            }),
            options(),
        );
        get(&host, 0, "before");
        assert_eq!(host.recv()["error"]["code"], rpc::SERVER_ERROR);
        host.send(&rpc::request(json!(1),"initialize",json!({"protocolVersion":version,"clientInfo":{"name":"acme","version":"1"},"capabilities":{}})));
        assert_eq!(
            host.recv()["result"]["capabilities"]["prompts"],
            json!({"listChanged":false})
        );
        host.send(&rpc::notification("notifications/initialized", json!({})));
        for params in [
            json!({"name":"acme_review"}),
            json!({"name":"unknown"}),
            json!({"name":"acme_review","arguments":{"code":false}}),
        ] {
            host.send(&rpc::request(json!(2), "prompts/get", params));
            assert_eq!(host.recv()["error"]["code"], rpc::INVALID_PARAMS);
        }
        get(&host, 3, "exact\n");
        let legacy = host.recv();
        assert_eq!(
            legacy["result"]["messages"][0]["content"]["text"],
            "exact\n"
        );
        assert!(legacy["result"].get("resultType").is_none());
        host.send(&rpc::request(
            json!(4),
            "server/discover",
            tagged(json!({})),
        ));
        assert_eq!(host.recv()["result"]["capabilities"]["prompts"], json!({}));
        host.send(&rpc::request(json!(5), "prompts/list", tagged(json!({}))));
        let list = host.recv();
        assert_eq!(list["result"]["prompts"][0]["name"], "acme_review");
        assert_eq!(list["result"]["ttlMs"], 0);
        host.send(&rpc::request(
            json!(6),
            "prompts/get",
            tagged(json!({"name":"acme_review","arguments":{"code":"modern"}})),
        ));
        let modern = host.recv();
        assert_eq!(modern["result"]["resultType"], "complete");
        assert_eq!(modern["result"]["messages"][0]["content"]["text"], "modern");
        assert!(modern["result"].get("ttlMs").is_none());
        host.finish().0.unwrap();
    }
}

#[test]
fn prompts_disabled_catalog_cannot_be_enabled_by_registration() {
    let mut options = Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    options.session.extension_methods = vec!["prompts/get".into(), "prompts/list".into()];
    let host = Host::ready(
        Script::new(|_, _| panic!("disabled prompt dispatch")),
        options,
        json!({}),
    );
    for params in [json!({}), tagged(json!({}))] {
        for method in ["prompts/get", "prompts/list"] {
            host.send(&rpc::request(json!(1), method, params.clone()));
            assert_eq!(host.recv()["error"]["code"], rpc::METHOD_NOT_FOUND);
        }
    }
    host.finish().0.unwrap();
}

#[test]
fn prompts_modern_input_round_and_typed_result_refusal() {
    let host = Host::start(
        Script::new(|request, context| {
            let Request::Prompt(call) = request else {
                panic!("prompt")
            };
            if call.arguments["code"] == "raw" {
                return Outcome::Method(Ok(json!({"messages":[]})));
            }
            let protocol = context.request_context().unwrap();
            if protocol.request_state.is_none() {
                return Outcome::InputRequired(
                    protocol
                        .confirmation("Include private context?", "acme-state")
                        .unwrap(),
                );
            }
            assert_eq!(protocol.request_state.as_deref(), Some("acme-state"));
            assert_eq!(
                protocol.confirmation_decision().unwrap(),
                Some(canonic_mcp_kit::Decision::Declined)
            );
            result("Review public context.")
        }),
        options(),
    );
    for (id, params) in [
        (1, json!({"name":"acme_review","arguments":{"code":"x"}})),
        (
            2,
            json!({"name":"acme_review","arguments":{"code":"x"},"requestState":"acme-state","inputResponses":{"confirmation":{"action":"decline"}}}),
        ),
        (3, json!({"name":"acme_review","arguments":{"code":"raw"}})),
    ] {
        host.send(&rpc::request(json!(id), "prompts/get", tagged(params)));
        let reply = host.recv();
        match id {
            1 => assert_eq!(reply["result"]["resultType"], "input_required"),
            2 => assert_eq!(
                reply["result"]["messages"][0]["content"]["text"],
                "Review public context."
            ),
            _ => assert_eq!(reply["error"]["code"], rpc::INTERNAL_ERROR),
        }
    }
    host.finish().0.unwrap();
}

#[test]
fn prompts_legacy_continuation_keeps_typed_result_contract() {
    let host = Host::ready(
        Script::new(|_, _| {
            Outcome::Confirm(canonic_mcp_kit::stdio::Confirm::new(
                "Include context?",
                |_, _| Outcome::Method(Ok(json!({"messages":[]}))),
            ))
        }),
        options(),
        json!({"elicitation":{"form":{}}}),
    );
    get(&host, 1, "code");
    let question = host.recv();
    assert_eq!(question["method"], "elicitation/create");
    host.send(&rpc::success(
        question["id"].clone(),
        json!({"action":"accept"}),
    ));
    assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
    host.finish().0.unwrap();
}

#[test]
fn prompts_progress_and_frame_limit_are_enforced() {
    let latch = Latch::default();
    let waiting = latch.clone();
    let mut opts = options();
    opts.max_output_bytes = 1024;
    let host = Host::ready(
        Script::new(move |_, context| {
            assert!(context.progress(1.0, Some(2.0), Some("Preparing messages")));
            waiting.wait(context);
            result("x".repeat(2000))
        }),
        opts,
        json!({}),
    );
    host.send(&rpc::request(json!(1),"prompts/get",json!({"name":"acme_review","arguments":{"code":"x"},"_meta":{"progressToken":"prompt-progress"}})));
    let progress = host.recv();
    assert_eq!(progress["method"], "notifications/progress");
    assert_eq!(progress["params"]["progressToken"], "prompt-progress");
    latch.release();
    assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
    host.finish().0.unwrap();
}

fn blocked(events: Events, latch: Latch) -> Script {
    Script::new(move |request, context| {
        let Request::Prompt(call) = request else {
            panic!("prompt")
        };
        events.record(format!("start {}", call.arguments["code"]));
        latch.wait(context);
        events.record(format!("end {}", call.arguments["code"]));
        result("ready")
    })
}

#[test]
fn prompts_cancellation_does_not_run_queued_renderers() {
    let events = Events::new();
    let latch = Latch::default();
    let host = Host::ready(blocked(events.clone(), latch.clone()), options(), json!({}));
    get(&host, 1, "running");
    assert_eq!(events.next(), "start running");
    get(&host, 2, "queued");
    host.cancel(2);
    host.cancel(1);
    assert_eq!(events.next(), "end running");
    latch.release();
    host.assert_quiet();
    assert_eq!(events.none_within(Duration::ZERO), None);
    host.finish().0.unwrap();
}

#[test]
fn prompts_eof_completes_read_only_work_and_deadlines_release_ids() {
    let events = Events::new();
    let latch = Latch::default();
    let mut host = Host::ready(blocked(events.clone(), latch.clone()), options(), json!({}));
    get(&host, 1, "running");
    assert_eq!(events.next(), "start running");
    get(&host, 2, "queued");
    host.close();
    latch.release();
    let (status, replies) = host.finish();
    status.unwrap();
    assert_eq!(replies.len(), 2);
    assert!(
        replies
            .iter()
            .all(|reply| reply["result"]["messages"].is_array())
    );
    let mut options = options();
    options.timeout = Duration::from_millis(30);
    let host = Host::ready(blocked(Events::new(), Latch::default()), options, json!({}));
    get(&host, 1, "timed");
    assert_eq!(host.recv()["error"]["code"], rpc::DEADLINE_EXCEEDED);
    get(&host, 1, "reused after deadline");
    assert_eq!(host.recv()["error"]["code"], rpc::DEADLINE_EXCEEDED);
    host.finish().0.unwrap();
}
