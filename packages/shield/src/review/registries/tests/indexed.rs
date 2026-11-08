//! The product resolves pins and dependencies from signed history metadata.
use super::super::*;
use crate::review::{
    self,
    mcp::{Connection, call},
};
use canonic_registry_kit::testing::{Server, SignedRegistry};

fn history_registry() -> SignedRegistry {
    let mut registry = SignedRegistry::new("acme");
    for index in 0..70 {
        let version = format!("1.0.{index}");
        let bytes=json!({"schema_version":1,"kind":"pack","id":"acme/parent","version":version,
            "description":"Acme indexed fixture","license":"UNLICENSED","maintainers":["Acme"],
            "engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1","dependencies-v1"],
            "dependencies":[{"pack":"acme/child","requirement":"=1.0.0"}],"rules":[]}).to_string();
        registry.release("acme/parent", &version, bytes.as_bytes());
    }
    let bytes=json!({"schema_version":1,"kind":"pack","id":"acme/child","version":"1.0.0",
        "description":"Acme child","license":"UNLICENSED","maintainers":["Acme"],
        "engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[]}).to_string();
    registry.release("acme/child", "1.0.0", bytes.as_bytes());
    for major in 2..=130 {
        let mut document: Value = serde_json::from_str(&bytes).unwrap();
        let version = format!("{major}.0.0");
        document["version"] = json!(version);
        registry.release("acme/child", &version, document.to_string().as_bytes());
    }
    registry.publish_indexed(1, "9999-12-31T00:00:00Z");
    registry
}

#[test]
fn indexed_registry_installs_old_pin_with_transitive_dependencies() {
    let mut registry = history_registry();
    let (root, mut context) = review::tests::enrolled();
    context.agent = Some("acme-agent".into());
    context.session_id = Some("acme-session".into());
    context.surface = "mcp".into();
    context.connection_id = Some("indexed-registry".into());
    context.fetcher = Server::fetcher(Duration::from_secs(2));
    let source = enrollment(&context.fetcher, registry.server().url()).unwrap();
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator = review::project::metadata(state);
            state.operator["sources"]["hosted"] = json!(source);
            Ok(json!({}))
        })
        .unwrap();
    let result = review::packs::info(&context, "hosted/acme/parent@1.0.3").unwrap();
    assert_eq!(result["selected_release"]["version"], "1.0.3");
    let connection = Connection::enrolled(&context).unwrap();
    let proposal = call(
        &context,
        &connection,
        "policy_propose",
        &json!({"change":{"kind":"pack_add",
        "reference":"hosted/acme/parent@1.0.3","profile":"strict"},"idempotency_key":"indexed-pin"}),
        &|| Ok(()),
    );
    assert!(proposal["error"].is_null(), "{proposal}");
    review::operator::commit_proposal_fixture(
        &context,
        proposal["data"]["proposal_id"].as_str().unwrap(),
    )
    .unwrap();
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let rows = state.operator["installed"].as_array().unwrap();
    assert!(rows.iter().any(|row| row["id"] == "acme/parent"
        && row["version"] == "1.0.3"
        && row["pinned"] == true));
    assert!(
        rows.iter()
            .any(|row| row["id"] == "acme/child" && row["version"] == "1.0.0")
    );
    assert!(rows.iter().all(|row| row["profile"] == "strict"));
    let requests = registry.server().requests();
    assert_eq!(
        requests
            .iter()
            .filter(|path| path.contains("/packs/acme-child-"))
            .count(),
        1,
        "a direct pin's dependency downloads only its required artifact across 130 versions"
    );
    let unsupported =
        json!({"schema_version":1,"kind":"pack","id":"acme/parent","version":"1.0.70",
        "description":"Acme future engine","license":"UNLICENSED","maintainers":["Acme"],
        "engines":{"shield":">=99.0.0"},"capabilities":["structured-rules-v1"],"rules":[]})
        .to_string();
    registry.release("acme/parent", "1.0.70", unsupported.as_bytes());
    registry.publish_indexed(2, "9999-12-31T00:00:00Z");
    // A fresh project selects latest; the first project deliberately keeps its pin.
    let (root, mut context) = review::tests::enrolled();
    context.agent = Some("acme-agent".into());
    context.session_id = Some("acme-session".into());
    context.surface = "mcp".into();
    context.connection_id = Some("indexed-fallback".into());
    context.fetcher = Server::fetcher(Duration::from_secs(2));
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator = review::project::metadata(state);
            state.operator["sources"]["hosted"] = json!(source);
            Ok(json!({}))
        })
        .unwrap();
    let connection = Connection::enrolled(&context).unwrap();
    let proposal = call(
        &context,
        &connection,
        "policy_propose",
        &json!({"change":{"kind":"pack_add",
        "reference":"hosted/acme/parent","profile":"strict"},"idempotency_key":"indexed-fallback"}),
        &|| Ok(()),
    );
    assert!(proposal["error"].is_null(), "{proposal}");
    review::operator::commit_proposal_fixture(
        &context,
        proposal["data"]["proposal_id"]
            .as_str()
            .unwrap_or_else(|| panic!("{proposal}")),
    )
    .unwrap();
    let state = Store::open(root.path()).unwrap().read().unwrap();
    assert!(
        state.operator["installed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "acme/parent" && row["version"] == "1.0.69")
    );
}
