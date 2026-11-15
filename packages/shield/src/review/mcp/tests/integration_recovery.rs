//! Recovery targets damaged integrations even when another agent asks for status.
use super::*;

#[test]
fn integration_recovery_targets_degraded_agents_instead_of_current_agent() {
    let (root, mut context, _connection) = fixture();
    context.agent = Some("claude".into());
    context.confirmed_mcp_forms = true;
    let meta = review::project::metadata(&review::project::checkpoint(&context).unwrap());
    let plan = review::integrations::setup::prepare(
        root.path(),
        &meta,
        review::integrations::IntegrationChoice::Agents(
            review::integrations::IntegrationChoice::bits(&["claude", "codex"]).unwrap(),
        ),
    )
    .unwrap()
    .unwrap();
    review::integrations::apply(&context, &meta, &plan).unwrap();
    std::fs::write(
        root.path().join(".codex/config.toml"),
        "{ malformed Codex settings",
    )
    .unwrap();
    let mut data = review::project::status(&context).unwrap();
    integration_recovery(&context, &mut data);
    assert_eq!(data["repair_route"], "mcp_setup", "{data}");
    assert_eq!(
        data["repair_arguments"],
        json!({"agents":["codex"]}),
        "{data}"
    );

    // A healthy Hub or Box integration must not redirect repair of a direct
    // agent; a changed one is repaired through its own host.
    for host in ["hub", "box"] {
        data["repair_route"] = json!("shield mcp setup");
        data["coverage"]["host_integration"] = json!({"agent":host,"status":"installed"});
        integration_recovery(&context, &mut data);
        assert_eq!(data["repair_arguments"], json!({"agents":["codex"]}));

        data["repair_route"] = json!("shield mcp setup");
        data["coverage"]["host_integration"]["status"] = json!("changed");
        integration_recovery(&context, &mut data);
        let mut expected = vec![host, "codex"];
        expected.sort_unstable();
        assert_eq!(data["repair_arguments"], json!({"agents":expected}));
    }

    data["repair_route"] = json!("shield mcp setup");
    data["coverage"]["diagnostics"] = json!(["confirmed setup requires repair"]);
    integration_recovery(&context, &mut data);
    assert_eq!(data["repair_arguments"], json!({"pending":"review"}));
    assert!(data["pending_setup"]["cancel"].is_object());
}
