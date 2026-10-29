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
    let bytes = second
        .encode_success(&json!(2), &json!({"content":[]}), 4096, &|| Ok(()))
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
            "tools/list"
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
