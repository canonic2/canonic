//! Local registry acquisition exercises real stage placement and unchanged activation.
use super::*;
use crate::{
    mcp::{
        Server,
        tests::{Transport, initialize},
    },
    review::{self, storage::Store},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[test]
fn operation_progress_rebinding_preserves_the_count_and_shared_emission_bound() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let first = observed.clone();
    let progress = Progress::new(move |number, _| first.lock().unwrap().push(number));
    progress.stage(Stage::Catalog);
    let clone = progress.clone();
    clone.stage(Stage::Artifact);
    let continued = observed.clone();
    let resumed = progress.rebind(move |number, _| continued.lock().unwrap().push(number));
    for _ in 0..100 {
        resumed.stage(Stage::ComparePolicy);
    }
    clone.stage(Stage::PrepareReview);
    assert_eq!(*observed.lock().unwrap(), (1..=64).collect::<Vec<_>>());
}

#[test]
fn operation_progress_reports_registry_acquisition_and_preserves_inactive_proposals() {
    let (root, mut context) = review::tests::enrolled();
    context.agent = Some("claude".into());
    let source = assert_fs::TempDir::new().unwrap();
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/progress","version":"1.0.0",
        "description":"Acme progress fixture","license":"MIT","maintainers":["Acme"],
        "engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[]}).to_string();
    std::fs::write(source.path().join("pack.json"), &pack).unwrap();
    std::fs::write(source.path().join("catalog.json"), json!({"schema_version":1,"packs":[{
        "id":"acme/progress","version":"1.0.0","path":"pack.json","sha256":crate::policy::fingerprint(pack.as_bytes())}]}).to_string()).unwrap();
    let registry =
        review::registries::enrollment(&context.fetcher, source.path().to_str().unwrap()).unwrap();
    let store = Store::open(root.path()).unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.operator["sources"]["acme"] = json!(registry);
            Ok(Value::Null)
        })
        .unwrap();
    let revision = store.read_checkpoint().unwrap().policy_revision;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let recorded = observed.clone();
    context.progress =
        Progress::new(move |_, stage| recorded.lock().unwrap().push(stage.message()));
    let input = serde_json::from_value(json!({"idempotency_key":"acme-core-acquire","change":{
        "kind":"pack_add","reference":"acme/acme/progress","profile":"recommended"}}))
    .unwrap();
    review::tests::propose(&context, &input, &|| Ok(())).unwrap();
    for expected in [
        "Reading and verifying registry catalog",
        "Reading and verifying pack artifact",
        "Comparing policy permissions and coverage",
    ] {
        assert!(observed.lock().unwrap().contains(&expected));
    }
    let transport = Transport::start(Server::for_review(context).unwrap());
    for message in initialize(&json!({}), "acme-progress") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    let request = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"policy_propose","arguments":{"idempotency_key":"acme-wire-acquire","change":{"kind":"pack_add","reference":"acme/acme/progress","profile":"recommended"}},"_meta":{"progressToken":"acme-acquire"}}});
    let (response, progress) = crate::mcp::tests::progress::collect(&transport, &request);
    crate::mcp::tests::progress::assert_progress(&progress, &json!("acme-acquire"));
    assert_eq!(response["result"]["isError"], false, "{response}");
    // Delivery is best effort: final queued stages may be dropped when the
    // operation settles. Exact placement is asserted by the core observer above.
    assert_eq!(store.read_checkpoint().unwrap().policy_revision, revision);
    assert_eq!(
        response["result"]["structuredContent"]["data"]["status"],
        "pending"
    );
}
