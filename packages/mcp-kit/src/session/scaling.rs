//! Prepared work and live ownership must not grow with completed request history.
use super::*;
use crate::{
    Tool, ToolResult,
    test_work::{self, Counts},
};

fn catalog(count: usize) -> Arc<Catalog> {
    Arc::new(Catalog::new((0..count).map(|index| {
        Tool::new(if index + 1 == count { "acme_target".into() } else { format!("acme_{index:04}") },
            "Return Acme data.", json!({"type":"object"}))
            .with_output_schema(json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"]}))
    }).collect()).unwrap())
}

fn request(id: Value, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({crate::modern::VERSION_KEY:crate::modern::PROTOCOL_VERSION,
        crate::modern::CAPABILITIES_KEY:{}});
    rpc::request(id, method, params)
}

fn call(session: &mut Session, id: &Value, result: &ToolResult) -> (Vec<u8>, Counts) {
    let message = request(id.clone(), "tools/call", json!({"name":"acme_target"}));
    test_work::reset();
    assert!(matches!(
        session.receive_value(message),
        Inbound::Tool { .. }
    ));
    let context = session.request_context(id).unwrap();
    let weak = Arc::downgrade(&context);
    let encoded = context
        .encode_success(id, result, 1024 * 1024, &|| Ok(()))
        .unwrap();
    assert!(session.finish(id));
    drop(context);
    assert!(
        weak.upgrade().is_none(),
        "completed request retained its context"
    );
    assert_idle(session);
    (encoded, test_work::counts())
}

fn assert_idle(session: &Session) {
    assert_eq!(session.outstanding(), 0);
    assert!(session.contexts.is_empty());
    assert!(session.subscriptions.is_empty());
    assert!(session.confirmation.is_none());
    assert!(session.rich_question.is_none());
    assert_eq!(Arc::strong_count(&session.identity), 1);
}

fn capacities(session: &Session) -> (usize, usize) {
    (session.outstanding.capacity(), session.contexts.capacity())
}

fn age(session: &mut Session) {
    let result = ToolResult::structured(json!({"value":"Acme"}), false);
    for index in 0..2048 {
        let id = if index % 2 == 0 {
            json!("reused")
        } else {
            json!(format!("history-{index}"))
        };
        call(session, &id, &result);
    }
}

#[test]
fn scaling_retained_calls_reuse_preparation_and_encoding_work_after_history_growth() {
    let mut reference = None;
    for count in [2, 1024] {
        test_work::reset();
        let catalog = catalog(count);
        assert_eq!(test_work::counts().preparations, 2 * count);
        let mut session = Session::new(
            catalog.clone(),
            SessionOptions {
                modern: Some(crate::ModernOptions::new(ServerInfo::new("acme", "1"))),
                ..SessionOptions::default()
            },
        );
        let result = ToolResult::structured(json!({"value":"x".repeat(256 * 1024)}), false);
        let fresh = call(&mut session, &json!(1), &result);
        assert_eq!(fresh.1.preparations, 0);
        assert_eq!(fresh.1.encodings, 1);
        assert_eq!(fresh.1.page_definitions, 0);
        let capacity = capacities(&session);
        age(&mut session);
        let aged = call(&mut session, &json!(1), &result);
        assert_eq!(aged, fresh, "request work changed with completed history");
        assert_eq!(
            capacities(&session),
            capacity,
            "history expanded live-state tables"
        );
        assert_eq!(Arc::strong_count(&catalog), 2);
        if let Some(reference) = &reference {
            assert_eq!(&aged, reference);
        }
        reference = Some(aged);
    }
}

fn page(session: &mut Session) -> (Value, Counts) {
    let message = request(json!(1), "tools/list", json!({}));
    test_work::reset();
    let Inbound::Reply(reply) = session.receive_value(message) else {
        panic!("page reply")
    };
    let bytes = crate::json::encode(&reply, 1024 * 1024).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), reply);
    assert_idle(session);
    let mut payload = reply["result"].clone();
    assert!(
        payload
            .as_object_mut()
            .unwrap()
            .remove("nextCursor")
            .is_some()
    );
    (payload, test_work::counts())
}

#[test]
fn scaling_page_materialization_depends_on_selected_entries_not_catalog_or_history() {
    let mut reference = None;
    for count in [65, 1024] {
        let mut session = Session::new(
            catalog(count),
            SessionOptions {
                modern: Some(crate::ModernOptions::new(ServerInfo::new("acme", "1"))),
                ..SessionOptions::default()
            },
        );
        let fresh = page(&mut session);
        assert_eq!(fresh.1.preparations, 0);
        assert_eq!(fresh.1.page_definitions, crate::tool::PAGE_SIZE);
        assert_eq!(fresh.1.encodings, 1);
        age(&mut session);
        let aged = page(&mut session);
        assert_eq!(aged, fresh);
        if let Some(reference) = &reference {
            assert_eq!(&aged, reference);
        }
        reference = Some(aged);
    }
}
