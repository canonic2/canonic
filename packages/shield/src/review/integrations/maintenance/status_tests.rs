//! Diagnostics and pending fallback stay local to the affected agent.
use super::super::*;

fn installed() -> (assert_fs::TempDir, Context, State) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let state = review::project::checkpoint(&context).unwrap();
    let meta = review::project::metadata(&state);
    let plan = setup::prepare(
        root.path(),
        &meta,
        IntegrationChoice::Agents(IntegrationChoice::bits(&["claude", "codex"]).unwrap()),
    )
    .unwrap()
    .unwrap();
    apply(&context, &meta, &plan).unwrap();
    let state = review::project::checkpoint(&context).unwrap();
    (root, context, state)
}

fn assert_agent(report: &Value, agent: &str, native: &str, mcp: &str) {
    for (key, expected) in [
        ("installed_integrations", native),
        ("registered_integrations", mcp),
    ] {
        let row = report[key]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["agent"] == agent)
            .unwrap();
        assert_eq!(row["status"], expected, "{report}");
    }
}

#[test]
fn status_keeps_healthy_agent_rows_when_another_configuration_is_unreadable() {
    for broken in ["claude", "codex"] {
        let (root, context, state) = installed();
        let path = if broken == "claude" {
            ".mcp.json"
        } else {
            ".codex/config.toml"
        };
        fs::write(root.path().join(path), "{ malformed host configuration").unwrap();
        let report = status(&context, &state);
        assert_eq!(report["repair_needed"], true, "{report}");
        assert_agent(&report, broken, "degraded", "degraded");
        let healthy = if broken == "claude" {
            "codex"
        } else {
            "claude"
        };
        assert_agent(&report, healthy, "installed", "registered");
    }
}

#[test]
fn pending_setup_blocks_only_affected_agent_fallback_and_status() {
    let (root, context, mut state) = installed();
    state.operator["integration"]["ask_fallback"]["codex"] = json!("allow");
    let metadata = review::project::metadata(&state);
    let plan = setup::prepare(root.path(), &metadata, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    state.operator["integration_pending"] = json!(plan);
    let metadata = review::project::metadata(&state);
    assert!(matches!(
        fallback(&metadata, "codex").unwrap(),
        AskFallback::Allow
    ));
    assert!(matches!(
        fallback(&metadata, "claude").unwrap(),
        AskFallback::Block
    ));
    let report = status(&context, &state);
    assert_eq!(report["repair_needed"], true, "{report}");
    assert_agent(&report, "claude", "degraded", "degraded");
    assert_agent(&report, "codex", "installed", "registered");
    state.operator["integration_pending"]["targets"] = Value::Null;
    assert!(matches!(
        fallback(&review::project::metadata(&state), "codex").unwrap(),
        AskFallback::Block
    ));
}

#[cfg(not(windows))]
fn with_box(handover: bool) -> (assert_fs::TempDir, Context, State) {
    let (root, context, _) = installed();
    fs::create_dir(root.path().join(".box")).unwrap();
    fs::write(root.path().join(".box/config.yml"), "schema_version: 1\n").unwrap();
    let metadata = review::project::metadata(&review::project::checkpoint(&context).unwrap());
    let plan = setup::prepare_with(
        root.path(),
        &metadata,
        IntegrationChoice::agent("box"),
        false,
        None,
        handover,
        false,
    )
    .unwrap()
    .unwrap();
    apply(&context, &metadata, &plan).unwrap();
    let state = review::project::checkpoint(&context).unwrap();
    (root, context, state)
}

#[cfg(not(windows))]
#[test]
fn box_only_prefix_and_binary_drift_are_attributed_to_box() {
    let (root, context, mut state) = with_box(true);
    assert!(
        state.operator["integration"]["agents"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let prefix = root.path().join(".codex/rules/shield.rules");
    fs::create_dir_all(prefix.parent().unwrap()).unwrap();
    fs::write(&prefix, "foreign Codex rules").unwrap();
    let report = status(&context, &state);
    assert_eq!(report["repair_needed"], true, "{report}");
    assert_eq!(report["box_integration"]["status"], "degraded", "{report}");
    assert_eq!(report["box_integration"]["definition_status"], "installed");
    fs::remove_file(prefix).unwrap();
    state.operator["integration"]["binary"] = json!(root.path().join("missing-shield"));
    let report = status(&context, &state);
    assert_eq!(report["box_integration"]["status"], "degraded", "{report}");
    assert_eq!(report["box_integration"]["definition_status"], "installed");
}

#[cfg(not(windows))]
#[test]
fn healthy_box_is_not_blamed_for_damage_to_a_retained_direct_agent() {
    let (root, context, state) = with_box(false);
    fs::write(
        root.path().join(".codex/config.toml"),
        "{ malformed direct Codex configuration",
    )
    .unwrap();
    let report = status(&context, &state);
    assert_eq!(report["repair_needed"], true, "{report}");
    assert_eq!(report["box_integration"]["status"], "installed", "{report}");
    assert_agent(&report, "codex", "degraded", "degraded");
    assert_agent(&report, "claude", "installed", "registered");
}
