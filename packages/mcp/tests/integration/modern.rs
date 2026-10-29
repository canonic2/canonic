//! Independent-runtime encoding through the library's public modern API.

use canonic_mcp::{
    Catalog, Inbound, ModernOptions, RequestContext, ServerInfo, Session, SessionOptions,
    control::Interrupted, json::EncodeError, modern, rpc,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn admitted(method: &str, capabilities: &Value) -> (Session, Arc<RequestContext>) {
    let options = SessionOptions {
        modern: Some(ModernOptions::new(
            ServerInfo::new("acme", "1").with_title("Acme service"),
        )),
        resources: Some(Arc::new(
            canonic_mcp::ResourceCatalog::new(vec![], vec![]).expect("empty resources"),
        )),
        extension_methods: vec!["acme/inspect".into()],
        ..SessionOptions::default()
    };
    let mut session = Session::try_new(
        Arc::new(Catalog::new(vec![]).expect("empty tools")),
        options,
    )
    .expect("valid modern options");
    let request = rpc::request(
        json!(1),
        method,
        json!({
            "uri":"acme://report",
            "_meta":{
                modern::VERSION_KEY:modern::PROTOCOL_VERSION,
                modern::CAPABILITIES_KEY:capabilities
            }
        }),
    );
    assert!(matches!(
        session.receive_value(request),
        Inbound::Method { .. }
    ));
    let context = session
        .request_context(&json!(1))
        .expect("admitted context");
    (session, context)
}

#[test]
fn independent_runtime_encodes_bounded_interim_with_captured_identity() {
    let (mut session, context) = admitted("resources/read", &json!({"elicitation":{"form":{}}}));
    let interim = context
        .confirmation("Read report?", "opaque-state")
        .unwrap();
    let bytes = context
        .encode_interim(&json!(1), &interim, 4096, &|| Ok(()))
        .unwrap();
    let reply: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["jsonrpc"], "2.0");
    assert_eq!(reply["result"]["resultType"], "input_required");
    assert_eq!(reply["result"]["requestState"], "opaque-state");
    assert_eq!(
        reply["result"]["inputRequests"],
        interim.as_value()["inputRequests"]
    );
    assert_eq!(
        reply["result"]["_meta"]["io.modelcontextprotocol/serverInfo"],
        json!({"name":"acme","version":"1","title":"Acme service"})
    );
    assert!(reply["result"].get("ttlMs").is_none());
    assert!(reply["result"].get("cacheScope").is_none());
    assert!(session.finish(&json!(1)));

    assert_eq!(
        context.encode_interim(&json!(1), &interim, bytes.len(), &|| Ok(())),
        Ok(bytes.clone())
    );
    assert_eq!(
        context.encode_interim(&json!(1), &interim, bytes.len() - 1, &|| Ok(())),
        Err(EncodeError::TooLarge {
            limit: bytes.len() - 1
        })
    );
    assert_eq!(
        context.encode_interim(&json!(1), &interim, 4096, &|| Err(Interrupted)),
        Err(EncodeError::Interrupted)
    );
    let (_, unsupported) = admitted("resources/read", &json!({}));
    assert_eq!(
        unsupported.encode_interim(&json!(1), &interim, 4096, &|| Ok(())),
        Err(EncodeError::Invalid)
    );
    let (_, ineligible) = admitted("acme/inspect", &json!({"elicitation":{"form":{}}}));
    assert_eq!(
        ineligible.encode_interim(&json!(1), &interim, 4096, &|| Ok(())),
        Err(EncodeError::Invalid)
    );
}
