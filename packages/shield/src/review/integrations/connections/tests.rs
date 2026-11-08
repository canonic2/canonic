//! An unsettled registration affects only its changed private launcher bindings.
use super::*;
use crate::{
    configuration::Bundle,
    review::{self, Context, storage::Store},
};
use std::collections::BTreeMap;

fn installation() -> super::super::native::Installation {
    serde_json::from_value(json!({"groups":{},"mcp":null,"settings":null}))
        .expect("fixture installation")
}

fn fixture() -> (
    assert_fs::TempDir,
    Context,
    Registration,
    BTreeMap<String, Connection>,
) {
    let root = assert_fs::TempDir::new().expect("fixture project");
    let local = json!({"schema_version":1,"mcp":{
        "cloud":{"transport":{"type":"stdio","command":"acme-connector","args":["cloud"]},"agents":{"claude":"cloud"}},
        "neighbor":{"transport":{"type":"stdio","command":"acme-connector","args":["neighbor"]},"agents":{"claude":"neighbor"}}
    }}).to_string();
    let bundle = Bundle::capture_layers(
        "schema_version: 1\npacks: []\n",
        Some(&local),
        &[],
        None,
        None,
        &[],
    )
    .expect("fixture policy");
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let snapshot = crate::runtime::Project::open(root.path())
        .expect("runtime project")
        .snapshot()
        .expect("active policy");
    let connections = snapshot.mcp_connections().clone();
    let mut registration = super::super::setup::desired(root.path()).expect("registration");
    registration.agents.insert("claude".into(), installation());
    let mut entries = BTreeMap::new();
    let mut launches = BTreeMap::new();
    let mut rows = Vec::new();
    for (id, connection) in &connections {
        let binding = Binding::new(connection).expect("fixture launcher binding");
        let entry = connection
            .projected(&registration.binary, root.path(), Some(&binding.token))
            .expect("private projection");
        entries.insert(id.clone(), entry);
        launches.insert(id.clone(), binding);
        rows.push(json!({"id":id,"alias":id,"enabled":true}));
    }
    registration.connections.insert("claude".into(), json!({"entries":entries,"launches":launches,"review":rows,"settings":["cloud","neighbor"]}));
    Store::open(root.path())
        .expect("store")
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration"] = json!(registration);
            Ok(json!({}))
        })
        .expect("committed registration");
    (root, context, registration, connections)
}

fn pending(context: &Context, previous: &Registration, desired: &Registration) {
    let plan = super::super::Plan {
        targets: None,
        scope_reason: None,
        policy_revision: None,
        before: None,
        after: String::new(),
        previous: Some(previous.clone()),
        desired: Some(desired.clone()),
        files: vec![],
        handed_over: vec![],
    };
    Store::open(Path::new(&context.project_root))
        .expect("store")
        .update(context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = json!(plan);
            Ok(json!({}))
        })
        .expect("pending fixture setup");
}

fn token(registration: &Registration, id: &str) -> String {
    registration.connections["claude"]["launches"][id]["token"]
        .as_str()
        .expect("fixture token")
        .to_owned()
}

#[test]
fn pending_setup_keeps_unchanged_committed_private_connections_usable() {
    let (root, context, registered, connections) = fixture();
    let mut desired = registered.clone();
    desired.agents.insert("codex".into(), installation());
    desired.connections.insert(
        "codex".into(),
        json!({"entries":{},"launches":{},"review":[]}),
    );
    pending(&context, &registered, &desired);
    for (id, connection) in &connections {
        let (resolved, binding) = resolve_mcp_token(root.path(), &token(&registered, id)).unwrap();
        assert_eq!(&resolved, id);
        assert!(binding.matches(connection));
        assert_eq!(
            mcp_projection(root.path(), "claude", id, connection).unwrap(),
            projected(root.path(), &registered, "claude", id, connection).unwrap()
        );
    }
}

#[test]
fn pending_setup_blocks_changed_aliases_without_blocking_their_neighbor() {
    let (root, context, registered, connections) = fixture();
    let mut desired = registered.clone();
    let mut changed = connections["neighbor"].clone();
    if let Transport::Stdio { args, .. } = &mut changed.transport {
        args.push("changed".into());
    }
    let binding = Binding::new(&changed).unwrap();
    let entry = changed
        .projected(&desired.binary, root.path(), Some(&binding.token))
        .unwrap();
    desired.connections.get_mut("claude").unwrap()["launches"]["neighbor"] = json!(binding);
    desired.connections.get_mut("claude").unwrap()["entries"]["neighbor"] = json!(entry);
    pending(&context, &registered, &desired);
    assert!(resolve_mcp_token(root.path(), &token(&registered, "cloud")).is_ok());
    assert!(mcp_projection(root.path(), "claude", "cloud", &connections["cloud"]).is_ok());
    assert!(resolve_mcp_token(root.path(), &token(&registered, "neighbor")).is_err());
    assert!(resolve_mcp_token(root.path(), &binding.token).is_err());
    assert!(mcp_projection(root.path(), "claude", "neighbor", &connections["neighbor"]).is_err());
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = Value::Null;
            Ok(json!({}))
        })
        .unwrap();
    assert!(resolve_mcp_token(root.path(), &token(&registered, "neighbor")).is_ok());
}

#[test]
fn pending_setup_requires_matching_committed_ownership_and_launcher_binary() {
    for mutation in ["alias", "removal", "binary", "previous"] {
        let (root, context, registered, connections) = fixture();
        let mut previous = registered.clone();
        let mut desired = registered.clone();
        match mutation {
            "alias" => {
                desired.connections.get_mut("claude").unwrap()["review"][0]["alias"] =
                    json!("another");
            }
            "removal" => {
                desired.agents.remove("claude");
            }
            "binary" => desired.binary = root.path().join("another-shield"),
            "previous" => {
                previous.connections.get_mut("claude").unwrap()["launches"]["cloud"]["token"] =
                    json!("f".repeat(64));
            }
            _ => unreachable!(),
        }
        pending(&context, &previous, &desired);
        assert!(
            resolve_mcp_token(root.path(), &token(&registered, "cloud")).is_err(),
            "{mutation}"
        );
        assert!(
            mcp_projection(root.path(), "claude", "cloud", &connections["cloud"]).is_err(),
            "{mutation}"
        );
    }
}
