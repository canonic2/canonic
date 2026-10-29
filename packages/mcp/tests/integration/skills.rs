//! Bundled Skills and modern request handling through the real shared stdio driver.

use crate::fixture::{Events, Host, Latch, Script, arguments, text};
use canonic_mcp::{
    CachePolicy, CacheScope, ModernOptions, ServerInfo, SkillCatalog, SkillDefinition, SkillFile,
    modern, rpc,
    stdio::{Options, Outcome, Request},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn options() -> Options {
    let skills = SkillCatalog::new(vec![SkillDefinition::new("skill://acme/status", vec![SkillFile::text("SKILL.md", "---\nname: status\ndescription: Inspect Acme state.\n---\nUse acme_echo; see references/GUIDE.md.\n"), SkillFile::text("references/GUIDE.md", "Acme reference\n"), SkillFile::binary("assets/icon.bin", Arc::<[u8]>::from(&b"\0\xff"[..]))])]).expect("valid fixture skills");
    let mut modern = ModernOptions::new(ServerInfo::new("acme", "1"));
    modern.skills_cache = CachePolicy::new(1000, CacheScope::Public).expect("valid policy");
    Options {
        session: canonic_mcp::SessionOptions {
            modern: Some(modern),
            skills: Some(Arc::new(skills)),
            ..canonic_mcp::SessionOptions::default()
        },
        ..Options::default()
    }
}

fn send(host: &Host, id: u64, method: &str, mut params: Value, capabilities: Value) {
    params["_meta"] = json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION});
    params["_meta"][modern::CAPABILITIES_KEY] = capabilities;
    host.send(&rpc::request(json!(id), method, params));
}

#[test]
fn discovery_and_bundled_text_binary_reads_work_without_initialization() {
    let script = Script::new(|request, _| {
        assert!(
            !matches!(request, Request::Resource(_)),
            "library bytes must bypass consumer call"
        );
        Outcome::ResourceNotFound(
            arguments(&request)["uri"]
                .as_str()
                .unwrap_or_default()
                .into(),
        )
    });
    let host = Host::start(script, options());
    send(&host, 1, "server/discover", json!({}), json!({}));
    let result = host.recv();
    assert_eq!(
        result["result"]["capabilities"]["extensions"][canonic_mcp::skill::EXTENSION],
        json!({})
    );
    assert_eq!(result["result"]["capabilities"]["resources"], json!({}));
    send(&host, 2, "skills/list", json!({}), json!({}));
    let list = host.recv();
    assert_eq!(
        list["result"]["skills"][0]["resources"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(list["result"]["cacheScope"], "public");
    send(
        &host,
        3,
        "skills/get",
        json!({"uri":"skill://acme/status/SKILL.md"}),
        json!({}),
    );
    assert_eq!(host.recv()["result"]["skill"], list["result"]["skills"][0]);
    send(
        &host,
        4,
        "resources/read",
        json!({"uri":"skill://acme/status/references/GUIDE.md"}),
        json!({}),
    );
    let reply = host.recv();
    assert_eq!(reply["result"]["resultType"], "complete");
    assert_eq!(reply["result"]["contents"][0]["text"], "Acme reference\n");
    assert_eq!(reply["result"]["ttlMs"], 1000);
    send(
        &host,
        5,
        "resources/read",
        json!({"uri":"skill://acme/status/assets/icon.bin"}),
        json!({}),
    );
    assert_eq!(host.recv()["result"]["contents"][0]["blob"], "AP8=");
    send(&host, 6, "resources/list", json!({}), json!({}));
    let reply = host.recv();
    assert_eq!(reply["result"]["resources"].as_array().unwrap().len(), 1);
    assert_eq!(reply["result"]["cacheScope"], "private");
    for (id, method, params, code) in [
        (
            7,
            "resources/read",
            json!({"uri":"skill://acme/status/missing"}),
            rpc::INVALID_PARAMS,
        ),
        (
            8,
            "resources/read",
            json!({"uri":"acme://missing"}),
            rpc::INVALID_PARAMS,
        ),
        (
            9,
            "skills/get",
            json!({"uri":"skill://acme/missing/SKILL.md"}),
            rpc::INVALID_PARAMS,
        ),
        (
            10,
            "resources/directory/read",
            json!({"uri":"skill://acme/status"}),
            rpc::METHOD_NOT_FOUND,
        ),
    ] {
        send(&host, id, method, params, json!({}));
        assert_eq!(host.recv()["error"]["code"], code);
    }
    host.finish().0.unwrap();
}

#[test]
fn both_legacy_revisions_keep_resource_fallback_and_omit_skills() {
    for version in canonic_mcp::SUPPORTED_PROTOCOL_VERSIONS {
        let host = Host::start(
            Script::new(|_, _| panic!("bundled reads never call consumer")),
            options(),
        );
        host.send(&rpc::request(json!(1), "initialize", json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"acme-host","version":"1"}})));
        let reply = host.recv();
        assert_eq!(reply["result"]["protocolVersion"], *version);
        assert!(reply["result"]["capabilities"].get("extensions").is_none());
        host.send(&rpc::notification("notifications/initialized", json!({})));
        host.send(&rpc::request(json!(2), "skills/list", json!({})));
        assert_eq!(host.recv()["error"]["code"], rpc::METHOD_NOT_FOUND);
        host.send(&rpc::request(
            json!(3),
            "resources/read",
            json!({"uri":"skill://acme/status/references/GUIDE.md"}),
        ));
        let reply = host.recv();
        assert_eq!(reply["result"]["contents"][0]["text"], "Acme reference\n");
        assert!(reply["result"].get("resultType").is_none());
        host.finish().0.unwrap();
    }
}

#[test]
fn queued_bundled_reads_cancel_and_survive_eof_on_the_shared_lane() {
    let events = Events::new();
    let recorded = events.clone();
    let latch = Latch::default();
    let blocked = latch.clone();
    let mut host = Host::ready(
        Script::new(move |_, context| {
            recorded.record("running");
            blocked.wait(context);
            text("probe")
        }),
        options(),
        json!({}),
    );
    host.call(1, "acme_probe", json!({}));
    assert_eq!(events.next(), "running");
    send(
        &host,
        2,
        "resources/read",
        json!({"uri":"skill://acme/status/SKILL.md"}),
        json!({}),
    );
    host.cancel(2);
    send(
        &host,
        3,
        "resources/read",
        json!({"uri":"skill://acme/status/SKILL.md"}),
        json!({}),
    );
    host.call(4, "acme_change", json!({}));
    host.close();
    let refused = host.recv();
    assert_eq!(refused["id"], 4);
    assert_eq!(refused["error"]["code"], rpc::REQUEST_CANCELLED);
    latch.release();
    let (result, replies) = host.finish();
    result.unwrap();
    assert_eq!(
        replies
            .iter()
            .map(|reply| reply["id"].clone())
            .collect::<Vec<_>>(),
        vec![json!(1), json!(3)]
    );
    assert!(replies[1]["result"]["contents"][0]["text"].is_string());
}

#[test]
fn legacy_resource_helper_maps_only_recognizable_modern_read_errors() {
    let host = Host::ready(
        Script::new(|request, _| {
            let uri = arguments(&request)["uri"]
                .as_str()
                .unwrap_or("acme://missing");
            let error = match uri {
                "acme://missing" => canonic_mcp::resource::not_found(uri),
                "acme://invalid" => canonic_mcp::resource::not_found("relative"),
                "acme://legacy" => canonic_mcp::RpcError::new(-32042, "Legacy failure"),
                "acme://custom" => {
                    canonic_mcp::RpcError::new(rpc::RESOURCE_NOT_FOUND, "Custom failure")
                        .with_data(json!({"uri":uri}))
                }
                _ => canonic_mcp::RpcError::new(rpc::RESOURCE_NOT_FOUND, "Resource not found")
                    .with_data(json!({"uri":false})),
            };
            Outcome::Method(Err(error))
        }),
        options(),
        json!({}),
    );
    for (id, uri, code) in [
        (1, "acme://missing", rpc::INVALID_PARAMS),
        (2, "acme://legacy", rpc::INTERNAL_ERROR),
        (3, "acme://custom", rpc::INTERNAL_ERROR),
        (4, "acme://malformed", rpc::INTERNAL_ERROR),
        (7, "acme://invalid", rpc::INTERNAL_ERROR),
    ] {
        send(&host, id, "resources/read", json!({"uri":uri}), json!({}));
        let reply = host.recv();
        assert_eq!(reply["id"], id);
        assert_eq!(reply["error"]["code"], code);
        if uri == "acme://missing" {
            assert_eq!(reply["error"]["message"], "Resource not found");
            assert_eq!(reply["error"]["data"], json!({"uri":uri}));
        }
    }
    host.send(&rpc::request(
        json!(5),
        "resources/read",
        json!({"uri":"acme://missing"}),
    ));
    let legacy = host.recv();
    assert_eq!(legacy["error"]["code"], rpc::RESOURCE_NOT_FOUND);
    assert_eq!(legacy["error"]["data"], json!({"uri":"acme://missing"}));
    send(
        &host,
        6,
        "tools/call",
        json!({"name":"acme_echo"}),
        json!({}),
    );
    assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
    host.finish().0.unwrap();
}

#[test]
fn concurrent_modern_and_legacy_tool_results_use_their_own_context() {
    let host = Host::ready(
        Script::new(|_, context| {
            text(if context.request_context().is_some() {
                "modern"
            } else {
                "legacy"
            })
        }),
        options(),
        json!({}),
    );
    send(
        &host,
        1,
        "tools/call",
        json!({"name":"acme_echo"}),
        json!({"elicitation":{}}),
    );
    host.call(2, "acme_echo", json!({}));
    let first = host.recv();
    let second = host.recv();
    assert_eq!(first["result"]["resultType"], "complete");
    assert_eq!(first["result"]["content"][0]["text"], "modern");
    assert!(second["result"].get("resultType").is_none());
    assert_eq!(second["result"]["content"][0]["text"], "legacy");
    host.finish().0.unwrap();
}

#[test]
fn modern_confirmation_retries_validate_consumer_state_before_effects() {
    let consumed = Arc::new(AtomicBool::new(false));
    let used = consumed.clone();
    let valid = Arc::new(AtomicBool::new(true));
    let live = valid.clone();
    let script = Script::new(move |request, context| {
        let modern = context.request_context().expect("modern fixture");
        if modern.input_responses.is_none() {
            return match modern.confirmation("Publish?", "owned-state") {
                Ok(result) => Outcome::InputRequired(result),
                Err(error) => Outcome::Method(Err(error)),
            };
        }
        // A product owns these checks. The library never authenticates opaque state.
        if modern.request_state.as_deref() != Some("owned-state")
            || arguments(&request)["message"] != "scope-a"
            || !live.load(Ordering::SeqCst)
        {
            return Outcome::Method(Err(canonic_mcp::RpcError::new(
                rpc::INVALID_PARAMS,
                "Invalid workflow state",
            )));
        }
        match modern.confirmation_decision() {
            Ok(Some(canonic_mcp::Decision::Accepted)) if !used.swap(true, Ordering::SeqCst) => {
                text("accepted")
            }
            Ok(Some(canonic_mcp::Decision::Declined)) => text("declined"),
            Err(error) => Outcome::Method(Err(error)),
            _ => Outcome::Method(Err(canonic_mcp::RpcError::new(
                rpc::INVALID_PARAMS,
                "Consumed workflow state",
            ))),
        }
    });
    let host = Host::start(script, options());
    let caps = json!({"elicitation":{"form":{}}});
    send(
        &host,
        1,
        "tools/call",
        json!({"name":"acme_echo","arguments":{"message":"scope-a"}}),
        caps.clone(),
    );
    let interim = host.recv();
    assert_eq!(interim["result"]["resultType"], "input_required");
    assert!(interim["result"].get("ttlMs").is_none());
    let retry = |state: &str, scope: &str| json!({"name":"acme_echo","arguments":{"message":scope},"requestState":state,"inputResponses":{"confirmation":{"action":"accept"}}});
    for (id, state, scope) in [(2, "altered", "scope-a"), (3, "owned-state", "other-scope")] {
        send(&host, id, "tools/call", retry(state, scope), caps.clone());
        assert_eq!(host.recv()["error"]["code"], rpc::INVALID_PARAMS);
    }
    valid.store(false, Ordering::SeqCst);
    send(
        &host,
        4,
        "tools/call",
        retry("owned-state", "scope-a"),
        caps.clone(),
    );
    assert_eq!(host.recv()["error"]["code"], rpc::INVALID_PARAMS);
    valid.store(true, Ordering::SeqCst);
    send(
        &host,
        5,
        "tools/call",
        retry("owned-state", "scope-a"),
        json!({}),
    );
    assert_eq!(host.recv()["error"]["code"], -32021);
    assert!(!consumed.load(Ordering::SeqCst));
    send(
        &host,
        6,
        "tools/call",
        retry("owned-state", "scope-a"),
        caps.clone(),
    );
    assert_eq!(host.recv()["result"]["content"][0]["text"], "accepted");
    send(
        &host,
        7,
        "tools/call",
        retry("owned-state", "scope-a"),
        caps,
    );
    assert_eq!(host.recv()["error"]["code"], rpc::INVALID_PARAMS);
    host.finish().0.unwrap();
}
