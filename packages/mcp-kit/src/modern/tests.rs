//! Mixed-version protocol admission and immutable request context.

use super::*;
use crate::{Catalog, Inbound, ServerInfo, Session, SessionOptions, Tool};

fn session() -> Session {
    Session::try_new(
        Arc::new(
            Catalog::new(vec![Tool::new(
                "acme_echo",
                "Echo",
                json!({"type":"object"}),
            )])
            .unwrap(),
        ),
        SessionOptions {
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            ..SessionOptions::default()
        },
    )
    .unwrap()
}

fn request(id: u64, method: &str, mut params: Value, capabilities: Value) -> Value {
    params["_meta"] = json!({VERSION_KEY:PROTOCOL_VERSION});
    params["_meta"][CAPABILITIES_KEY] = capabilities;
    rpc::request(json!(id), method, params)
}

fn reply(event: Inbound) -> Value {
    let Inbound::Reply(value) = event else {
        panic!("immediate reply expected")
    };
    value
}

#[test]
fn metadata_requests_share_prepared_identity_and_allow_empty_result_metadata() {
    let mut session = session();
    let mut contexts = vec![];
    for id in [1, 2] {
        assert!(matches!(
            session.receive_value(request(
                id,
                "tools/call",
                json!({"name":"acme_echo"}),
                json!({})
            )),
            Inbound::Tool { .. }
        ));
        contexts.push(session.request_context(&json!(id)).unwrap());
    }
    assert!(Arc::ptr_eq(&contexts[0].server, &contexts[1].server));
    let context = RequestContext::parse(
        &json!({"_meta":{CAPABILITIES_KEY:{}}}),
        &ModernOptions::new(ServerInfo::new("acme", "1")),
        "acme/custom",
        None,
        None,
    )
    .unwrap();
    let bytes = context
        .encode_success(&json!(1), &json!({"_meta":{},"value":true}), 4096, &|| {
            Ok(())
        })
        .unwrap();
    assert_eq!(
        crate::json::parse(&bytes, 4096).unwrap()["result"]["value"],
        true
    );
}

#[test]
fn metadata_client_display_fields_are_checked_without_dropping_unknown_fields() {
    let options = ModernOptions::new(ServerInfo::new("acme", "1"));
    let valid = json!({"name":"host","version":"1","description":"A host",
        "websiteUrl":"https://example.com","icons":[{"src":"https://example.com/i"}],"custom":{"opaque":null}});
    let params = json!({"_meta":{CAPABILITIES_KEY:{},CLIENT_KEY:valid}});
    let context = RequestContext::parse(&params, &options, "acme/custom", None, None).unwrap();
    assert_eq!(context.client_info.as_ref().unwrap(), &valid);
    for (field, value) in [
        ("description", json!(true)),
        ("websiteUrl", json!("file:///example")),
        ("icons", json!([{"src":"javascript:bad"}])),
        ("title", json!("x".repeat(16385))),
    ] {
        let mut params = params.clone();
        params["_meta"][CLIENT_KEY][field] = value;
        assert!(RequestContext::parse(&params, &options, "acme/custom", None, None).is_err());
    }
}

#[test]
fn metadata_each_typed_domain_result_uses_the_merged_envelope() {
    fn check(method: &str, result: &impl CompletePayload) {
        let context = RequestContext::parse(
            &json!({"_meta":{CAPABILITIES_KEY:{}}}),
            &ModernOptions::new(ServerInfo::new("acme", "1")),
            method,
            None,
            None,
        )
        .unwrap();
        let bytes = context
            .encode_success(&json!(1), result, 4096, &|| Ok(()))
            .unwrap();
        let value = crate::json::parse(&bytes, 4096).unwrap();
        assert_eq!(value["result"]["_meta"]["com.acme/answer"], "yes");
        assert_eq!(value["result"]["_meta"][SERVER_KEY]["name"], "acme");
    }
    let metadata = crate::Metadata::new(json!({"com.acme/answer":"yes"})).unwrap();
    check(
        "prompts/get",
        &crate::GetPromptResult::new(None, vec![])
            .unwrap()
            .with_metadata(metadata.clone())
            .unwrap(),
    );
    check(
        "resources/read",
        &crate::ReadResourceResult::new(vec![
            crate::ResourceContents::text("acme://report", None, "Report").unwrap(),
        ])
        .unwrap()
        .with_metadata(metadata.clone()),
    );
    check(
        "completion/complete",
        &crate::CompleteResult::new(vec![], None, None)
            .unwrap()
            .with_metadata(metadata)
            .unwrap(),
    );
}

#[test]
fn discovery_is_optional_and_never_changes_legacy_readiness() {
    let mut session = session();
    let result = reply(session.receive_value(request(1, "server/discover", json!({}), json!({}))));
    assert_eq!(result["result"]["resultType"], "complete");
    assert_eq!(result["result"]["cacheScope"], "private");
    assert_eq!(result["result"]["_meta"][SERVER_KEY]["name"], "acme");
    assert!(!session.ready());
    assert!(matches!(
        session.receive_value(request(
            2,
            "tools/call",
            json!({"name":"acme_echo"}),
            json!({})
        )),
        Inbound::Tool { .. }
    ));
    assert_eq!(
        reply(session.receive_value(rpc::request(json!(3), "tools/list", json!({}))))["error"]["code"],
        rpc::SERVER_ERROR
    );
}

#[test]
fn missing_invalid_or_unsupported_metadata_dispatches_nothing() {
    let mut session = session();
    let mut unknown = request(1, "tools/call", json!({"name":"acme_echo"}), json!({}));
    unknown["params"]["_meta"][VERSION_KEY] = json!("2099-01-01");
    let result = reply(session.receive_value(unknown));
    assert_eq!(result["error"]["code"], -32022);
    assert!(
        result["error"]["data"]["supported"]
            .as_array()
            .unwrap()
            .contains(&json!(PROTOCOL_VERSION))
    );
    for meta in [
        json!({VERSION_KEY:PROTOCOL_VERSION}),
        json!({CAPABILITIES_KEY:{}}),
        json!({VERSION_KEY:PROTOCOL_VERSION,CAPABILITIES_KEY:null}),
        json!({VERSION_KEY:PROTOCOL_VERSION,CAPABILITIES_KEY:{},CLIENT_KEY:{"name":5,"version":"1"}}),
        json!({VERSION_KEY:PROTOCOL_VERSION,CAPABILITIES_KEY:{"extensions":[]}}),
    ] {
        let result = reply(session.receive_value(rpc::request(
            json!(1),
            "tools/call",
            json!({"name":"acme_echo","_meta":meta}),
        )));
        assert_eq!(result["error"]["code"], rpc::INVALID_PARAMS);
        assert_eq!(session.outstanding(), 0);
    }
    let options = SessionOptions {
        skills: Some(Arc::new(crate::SkillCatalog::new(vec![]).unwrap())),
        ..SessionOptions::default()
    };
    assert!(Session::try_new(Arc::new(Catalog::new(vec![]).unwrap()), options).is_err());
}

#[test]
fn interleaved_calls_retain_context_through_cancel_and_id_reuse() {
    let mut session = session();
    session.receive_value(request(
        1,
        "tools/call",
        json!({"name":"acme_echo"}),
        json!({"elicitation":{"form":{}}}),
    ));
    let first = session.request_context(&json!(1)).unwrap();
    session.receive_value(request(
        2,
        "tools/call",
        json!({"name":"acme_echo"}),
        json!({}),
    ));
    let second = session.request_context(&json!(2)).unwrap();
    assert!(first.confirmation("Proceed?", "signed-state").is_ok());
    assert_eq!(
        second
            .confirmation("Proceed?", "signed-state")
            .unwrap_err()
            .code,
        -32021
    );
    assert!(matches!(
        session.receive_value(rpc::notification(
            "notifications/cancelled",
            json!({"requestId":1})
        )),
        Inbound::Cancel { .. }
    ));
    assert!(session.request_context(&json!(1)).is_none());
    session.receive_value(request(
        1,
        "tools/call",
        json!({"name":"acme_echo"}),
        json!({}),
    ));
    assert!(
        session
            .request_context(&json!(1))
            .unwrap()
            .confirmation("Proceed?", "x")
            .is_err()
    );
    assert!(first.confirmation("Proceed?", "x").is_ok());
    assert!(session.finish(&json!(1)));
    assert!(
        second
            .encode_success(&json!(2), &json!({"content":[]}), 4096, &|| Ok(()))
            .is_err()
    );
    let bytes = second
        .encode_success(
            &json!(2),
            &crate::ToolResult::success(vec![]),
            4096,
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(
        crate::json::parse(&bytes, 4096).unwrap()["result"]["resultType"],
        "complete"
    );
}

#[test]
fn metadata_and_confirmation_bounds_are_enforced() {
    let mut session = session();
    let oversized = request(
        1,
        "tools/call",
        json!({"name":"acme_echo"}),
        json!({"x":"a".repeat(65536)}),
    );
    assert_eq!(
        reply(session.receive_value(oversized))["error"]["code"],
        rpc::INVALID_PARAMS
    );
    let params = json!({"_meta":{CAPABILITIES_KEY:{"elicitation":{}}},"requestState":"signed","inputResponses":{"confirmation":{"action":"decline"}}});
    let context = RequestContext::parse(
        &params,
        &ModernOptions::new(ServerInfo::new("acme", "1")),
        "tools/call",
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        context.confirmation_decision().unwrap(),
        Some(Decision::Declined)
    );
    assert!(
        context
            .confirmation("question", &"x".repeat(65537))
            .is_err()
    );
    assert!(CachePolicy::new(1_u64 << 53, CacheScope::Public).is_err());
    assert!(CachePolicy::new((1_u64 << 53) - 1, CacheScope::Public).is_ok());
    assert!(
        context
            .encode_success(&json!(1), &json!({"resultType":"complete"}), 4096, &|| Ok(
                ()
            ))
            .is_err()
    );
    assert!(
        context
            .encode_success(&json!(1), &json!({"_meta":{}}), 4096, &|| Ok(()))
            .is_err()
    );
    let params = json!({"_meta":{CAPABILITIES_KEY:{"x":"a".repeat(65528)}}});
    assert!(
        RequestContext::parse(
            &params,
            &ModernOptions::new(ServerInfo::new("acme", "1")),
            "tools/list",
            None,
            None,
        )
        .is_ok()
    );
}

#[test]
fn default_off_discovery_retains_the_legacy_probe_error() {
    let mut session = Session::new(
        Arc::new(Catalog::new(vec![]).unwrap()),
        SessionOptions::default(),
    );
    let result = reply(session.receive_value(request(1, "server/discover", json!({}), json!({}))));
    assert_eq!(result["error"]["code"], rpc::SERVER_ERROR);
    assert!(result.get("result").is_none());
}

#[test]
fn ordinary_confirmation_retries_validate_answer_metadata_before_dispatch() {
    let mut session = session();
    for meta in [None, Some(json!({})), Some(json!({"acme/tag":"retained"}))] {
        let mut params = json!({"name":"acme_echo","requestState":"signed-state",
            "inputResponses":{"confirmation":{"action":"accept"}}});
        if let Some(meta) = meta {
            params["inputResponses"]["confirmation"]["_meta"] = meta;
        }
        let Inbound::Tool { id, .. } = session.receive_value(request(
            1,
            "tools/call",
            params,
            json!({"elicitation":{"form":{}}}),
        )) else {
            panic!("valid ordinary retry")
        };
        assert_eq!(
            session
                .request_context(&id)
                .unwrap()
                .confirmation_decision(),
            Ok(Some(Decision::Accepted))
        );
        assert!(session.finish(&id));
    }
    for meta in [
        Value::Null,
        json!([]),
        json!(true),
        json!(1),
        json!("invalid"),
    ] {
        let params = json!({"name":"acme_echo","requestState":"signed-state",
            "inputResponses":{"confirmation":{"action":"accept","_meta":meta}}});
        let refused = reply(session.receive_value(request(
            1,
            "tools/call",
            params,
            json!({"elicitation":{"form":{}}}),
        )));
        assert_eq!(refused["error"]["code"], rpc::INVALID_PARAMS);
        assert_eq!(session.outstanding(), 0);
    }
}

#[test]
fn modern_conformance_capabilities_validate_known_shapes_and_preserve_unknown_fields() {
    let mut session = session();
    for (capabilities, valid) in [
        (json!({"roots":{}}), true),
        (json!({"roots":false}), false),
        (json!({"roots":null}), false),
        (json!({"sampling":{"context":{},"tools":{}}}), true),
        (json!({"sampling":{"tools":true}}), false),
        (json!({"sampling":{"context":null}}), false),
        (json!({"experimental":{"acme/feature":{}}}), true),
        (json!({"experimental":{"acme/feature":true}}), false),
        (json!({"experimental":[]}), false),
        (json!({"extensions":{"acme/feature":{}}}), true),
        (json!({"elicitation":{"url":[]}}), false),
        (
            json!({"acme/future":false,"sampling":{"acme/future":true}}),
            true,
        ),
    ] {
        let event = session.receive_value(request(
            1,
            "tools/call",
            json!({"name":"acme_echo"}),
            capabilities.clone(),
        ));
        if valid {
            assert!(matches!(event, Inbound::Tool { .. }), "{capabilities}");
            assert_eq!(
                session.request_context(&json!(1)).unwrap().capabilities,
                capabilities
            );
            assert!(session.finish(&json!(1)));
        } else {
            assert_eq!(
                reply(event)["error"]["code"],
                rpc::INVALID_PARAMS,
                "{capabilities}"
            );
            assert_eq!(session.outstanding(), 0);
        }
    }
}

#[test]
fn modern_conformance_removed_and_unavailable_methods_never_dispatch() {
    let methods = [
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
    let mut unavailable = Session::try_new(
        Arc::new(Catalog::new(vec![]).unwrap()),
        SessionOptions {
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            extension_methods: methods.iter().map(|method| (*method).into()).collect(),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    for method in methods {
        let response = reply(unavailable.receive_value(request(1, method, json!({}), json!({}))));
        assert_eq!(response["error"]["code"], rpc::METHOD_NOT_FOUND, "{method}");
        assert_eq!(unavailable.outstanding(), 0);
        assert!(unavailable.request_context(&json!(1)).is_none());
        assert!(!unavailable.ready());
    }
    for version in crate::SUPPORTED_PROTOCOL_VERSIONS {
        let mut session = session();
        assert!(
            reply(session.receive_value(rpc::request(json!(1), "ping", json!({}))))["result"]
                .is_object()
        );
        let Inbound::Initialize { id, .. } = session.receive_value(rpc::request(json!(2), "initialize", json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"acme-host","version":"1"}}))) else { panic!("legacy initialization") };
        session.accept_initialize(id, &ServerInfo::new("acme", "1"));
        session.receive_value(rpc::notification(
            "notifications/initialized",
            json!({"_meta":{VERSION_KEY:PROTOCOL_VERSION}}),
        ));
        assert!(!session.ready());
        session.receive_value(rpc::notification("notifications/initialized", json!({})));
        assert!(session.ready());
        let mut params = json!({"_meta":{VERSION_KEY:version}});
        assert_eq!(
            reply(session.receive_value(rpc::request(json!(3), "ping", params.clone())))["result"],
            json!({})
        );
        params["_meta"][VERSION_KEY] = json!(PROTOCOL_VERSION);
        params["_meta"][CAPABILITIES_KEY] = json!({});
        assert_eq!(
            reply(session.receive_value(rpc::request(json!(4), "ping", params)))["error"]["code"],
            rpc::METHOD_NOT_FOUND
        );
        assert!(session.ready());
    }
}

#[test]
fn modern_conformance_catalog_envelopes_and_resource_payloads_are_typed() {
    let mut options = SessionOptions {
        modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
        resources: Some(Arc::new(
            crate::ResourceCatalog::new(vec![], vec![]).unwrap(),
        )),
        skills: Some(Arc::new(crate::SkillCatalog::new(vec![]).unwrap())),
        ..SessionOptions::default()
    };
    options.extension_methods.push("acme/inspect".into());
    let mut session = Session::try_new(Arc::new(Catalog::new(vec![]).unwrap()), options).unwrap();
    for (method, field) in [
        ("server/discover", "supportedVersions"),
        ("tools/list", "tools"),
        ("resources/list", "resources"),
        ("resources/templates/list", "resourceTemplates"),
        ("skills/list", "skills"),
    ] {
        let result =
            reply(session.receive_value(request(1, method, json!({}), json!({}))))["result"]
                .clone();
        assert_eq!(result["resultType"], "complete", "{method}");
        assert_eq!(result["ttlMs"], 0);
        assert_eq!(result["cacheScope"], "private");
        assert_eq!(
            result["_meta"][SERVER_KEY],
            json!({"name":"acme","version":"1"})
        );
        assert!(result[field].is_array(), "{method}: {result}");
        assert_eq!(session.outstanding(), 0);
    }
    let Inbound::Method { id, .. } = session.receive_value(request(
        1,
        "resources/read",
        json!({"uri":"acme://report"}),
        json!({"elicitation":{"form":{}}}),
    )) else {
        panic!("consumer read")
    };
    let context = session.request_context(&id).unwrap();
    for invalid in [json!({"contents":[]}), json!({"content":[]})] {
        assert_eq!(
            context.encode_success(&id, &invalid, 4096, &|| Ok(())),
            Err(EncodeError::Invalid)
        );
    }
    assert_eq!(
        context.encode_success(&id, &crate::ToolResult::success(vec![]), 4096, &|| Ok(())),
        Err(EncodeError::Invalid)
    );
    let read = crate::ReadResourceResult::new(vec![
        crate::ResourceContents::text("acme://report", None, "report").unwrap(),
    ])
    .unwrap();
    let encoded = context
        .encode_success(&id, &read, 4096, &|| Ok(()))
        .unwrap();
    let result = crate::json::parse(&encoded, 4096).unwrap();
    assert_eq!(result["result"]["contents"][0]["text"], "report");
    assert_eq!(result["result"]["cacheScope"], "private");
    assert!(session.finish(&id));
    assert!(session.request_context(&id).is_none());
    session.receive_value(request(
        1,
        "acme/inspect",
        json!({}),
        json!({"elicitation":{"form":{}}}),
    ));
    assert!(
        session
            .request_context(&json!(1))
            .unwrap()
            .confirmation("Proceed?", "state")
            .is_err()
    );
}

#[test]
fn modern_conformance_mrtr_retries_keep_mode_and_state_request_local() {
    let mut session = session();
    let questions = crate::elicitation::Requests::new(vec![(
        "authorize".into(),
        crate::elicitation::Request::url(
            "Authorize Acme access?",
            "https://example.com/authorize",
            Some("legacy-only-id"),
        )
        .unwrap(),
    )])
    .unwrap();
    session.receive_value(request(
        1,
        "tools/call",
        json!({"name":"acme_echo"}),
        json!({"elicitation":{"url":{}}}),
    ));
    let original = session.request_context(&json!(1)).unwrap();
    let round = original
        .elicitation(questions.clone(), Some("opaque-acme-state"))
        .unwrap();
    let bytes = original
        .encode_interim(&json!(1), &round, 4096, &|| Ok(()))
        .unwrap();
    let value = crate::json::parse(&bytes, 4096).unwrap();
    assert_eq!(value["result"]["resultType"], "input_required");
    assert_eq!(value["result"]["requestState"], "opaque-acme-state");
    assert!(
        value["result"]["inputRequests"]["authorize"]["params"]
            .get("elicitationId")
            .is_none()
    );
    assert!(value["result"].get("ttlMs").is_none());
    assert!(session.finish(&json!(1)));
    let retry = json!({"name":"acme_echo","requestState":"opaque-acme-state","inputResponses":{"authorize":{"action":"accept"},"unrelated":{"opaque":true}}});
    session.receive_value(request(
        2,
        "tools/call",
        retry.clone(),
        json!({"elicitation":{"url":{}}}),
    ));
    let context = session.request_context(&json!(2)).unwrap();
    assert_eq!(context.request_state.as_deref(), Some("opaque-acme-state"));
    assert_eq!(context.elicitation_answers(&questions).unwrap().len(), 1);
    assert!(session.finish(&json!(2)));
    session.receive_value(request(3, "tools/call", retry, json!({})));
    let context = session.request_context(&json!(3)).unwrap();
    assert_eq!(
        context.elicitation_answers(&questions).unwrap_err().code,
        -32021
    );
    assert!(original.elicitation(questions, None).is_ok());
    assert!(session.finish(&json!(3)));
    for method in ["server/discover", "tools/list"] {
        assert_eq!(
            reply(session.receive_value(request(
                4,
                method,
                json!({"requestState":"not-a-retry"}),
                json!({})
            )))["error"]["code"],
            rpc::INVALID_PARAMS
        );
        assert!(session.request_context(&json!(4)).is_none());
    }
}

#[test]
fn modern_conformance_removed_notifications_cannot_dispatch_legacy_handlers() {
    let methods = [
        "notifications/roots/list_changed",
        "notifications/elicitation/complete",
    ];
    for enabled in [false, true] {
        let mut session = Session::try_new(
            Arc::new(Catalog::new(vec![]).unwrap()),
            SessionOptions {
                modern: enabled.then(|| ModernOptions::new(ServerInfo::new("acme", "1"))),
                extension_notifications: methods.iter().map(|name| (*name).into()).collect(),
                ..SessionOptions::default()
            },
        )
        .unwrap();
        let Inbound::Initialize { id, .. } = session.receive_value(rpc::request(json!(1), "initialize", json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"acme-host","version":"1"}}))) else { panic!("legacy initialize") };
        session.accept_initialize(id, &ServerInfo::new("acme", "1"));
        session.receive_value(rpc::notification(
            "notifications/initialized",
            json!({"_meta":{VERSION_KEY:PROTOCOL_VERSION}}),
        ));
        assert_eq!(session.ready(), !enabled);
        session.receive_value(rpc::notification("notifications/initialized", json!({})));
        for method in methods {
            let inbound = session.receive_value(rpc::notification(
                method,
                json!({"_meta":{VERSION_KEY:PROTOCOL_VERSION}}),
            ));
            if enabled {
                assert!(matches!(inbound, Inbound::Ignored));
            } else {
                assert!(matches!(inbound, Inbound::Notification { .. }));
            }
            assert!(matches!(
                session.receive_value(rpc::notification(method, json!({}))),
                Inbound::Notification { .. }
            ));
            assert!(session.ready());
            assert_eq!(session.outstanding(), 0);
        }
    }
}
