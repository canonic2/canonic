//! One human connection review publishes policy and registered host projections.
use super::*;

fn installed_fixture() -> (assert_fs::TempDir, Context, Connection, String) {
    let local = json!({"schema_version":1,"mcp":{"cloud":{
        "transport":{"type":"stdio","command":"acme-private-connector","args":["acme-before"]},
        "agents":{"claude":"acme_cloud","codex":"acme_cloud"}
    }}})
    .to_string();
    let bundle = Bundle::capture_layers(
        "schema_version: 1\npacks: []\n",
        Some(&local),
        &[],
        None,
        None,
        &[],
    )
    .unwrap();
    let (root, mut context, connection) = fixture_with(&bundle);
    context.agent = Some("claude".into());
    assert!(
        crate::test_support::git()
            .current_dir(root.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    let original = Store::open(root.path()).unwrap().read().unwrap();
    review::publication::projections::materialize(
        &context,
        &review::publication::projections::projections(
            &original.candidates["project"],
            &original.operator,
        )
        .unwrap(),
    )
    .unwrap();
    let setup = review::integrations::setup::prepare(
        root.path(),
        &original.operator,
        review::integrations::IntegrationChoice::Agents(
            review::integrations::IntegrationChoice::bits(&["claude", "codex"]).unwrap(),
        ),
    )
    .unwrap()
    .unwrap();
    review::integrations::apply(&context, &original.operator, &setup).unwrap();
    (root, context, connection, local)
}

#[test]
fn connection_publication_mcp_review_applies_local_transport_without_setup_or_new_connection() {
    let (root, context, connection, local) = installed_fixture();
    let registered = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let token =
        registered.operator["integration"]["connections"]["claude"]["launches"]["cloud"]["token"]
            .as_str()
            .unwrap()
            .to_owned();
    let host_before = std::fs::read(root.path().join(".mcp.json")).unwrap();
    let mut edited: Value = serde_json::from_str(&local).unwrap();
    edited["mcp"]["cloud"]["transport"]["args"] = json!(["acme-private-after"]);
    std::fs::write(
        root.path().join(".shield/config.local.yml"),
        edited.to_string(),
    )
    .unwrap();
    let proposed = invoke(
        &context,
        &connection,
        "policy_propose",
        json!({"change":{"kind":"project_files_adopt"},"idempotency_key":"approve-connection-and-hosts"}),
    );
    assert!(proposed["error"].is_null(), "{proposed}");
    assert!(
        !proposed.to_string().contains("acme-private-after"),
        "{proposed:#}"
    );
    let args = &proposed["data"]["apply_arguments"];
    assert_eq!(proposed["redacted"], true);
    let compared = invoke(
        &context,
        &connection,
        "policy_compare",
        json!({
            "before_candidate_id":proposed["data"]["before_candidate_id"],
            "after_candidate_id":proposed["data"]["after_candidate_id"],"domain":"literal_argv"
        }),
    );
    assert!(compared["error"].is_null(), "{compared}");
    assert!(!compared.to_string().contains("acme-private-after"));
    assert_eq!(compared["redacted"], true);
    let message = apply_confirmation(&context, &connection, args)
        .unwrap()
        .unwrap();
    assert!(message.contains("acme-private-after"));
    assert!(message.contains("publication of its owned connection entries"));
    let declined = invoke(&context, &connection, "policy_apply", args.clone());
    assert_eq!(declined["error"]["code"], "operator_review_required");
    assert_eq!(
        std::fs::read(root.path().join(".mcp.json")).unwrap(),
        host_before
    );
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .policy_revision,
        registered.policy_revision
    );
    let applied = application::apply(&context, &connection, args);
    assert_eq!(
        applied["data"]["connection_publication"], "published",
        "{applied}"
    );
    assert!(!applied.to_string().contains("acme-private-after"));
    let runtime = crate::runtime::Project::open(root.path())
        .unwrap()
        .snapshot()
        .unwrap();
    let configured = &runtime.mcp_connections()["cloud"];
    let (id, binding) = review::integrations::resolve_mcp_token(root.path(), &token).unwrap();
    assert_eq!(id, "cloud");
    assert!(binding.matches(configured));
    for agent in ["claude", "codex"] {
        let projected =
            review::integrations::mcp_projection(root.path(), agent, "cloud", configured).unwrap();
        review::integrations::verify_mcp_connection(root.path(), agent, "acme_cloud", &projected)
            .unwrap();
    }
    let committed = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert!(committed.operator["integration_pending"].is_null());
    let retry = application::apply(&context, &connection, args);
    assert_eq!(
        retry["data"]["receipt"]["activation_id"],
        applied["data"]["receipt"]["activation_id"]
    );
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .sequence,
        committed.sequence
    );
    assert!(invoke(&context, &connection, "status", json!({}))["error"].is_null());
}
