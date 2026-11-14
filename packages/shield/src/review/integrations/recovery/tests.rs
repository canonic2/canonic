//! Human-reviewed resolution keeps policy authority separate from host files.
use super::*;
use crate::{
    configuration::Bundle,
    review::integrations::{self, IntegrationChoice, registration},
};
use std::fs;

fn fixture() -> (assert_fs::TempDir, Context, Plan) {
    fixture_source("schema_version: 1\npacks: []\n")
}

fn fixture_source(source: &str) -> (assert_fs::TempDir, Context, Plan) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle = Bundle::capture(source, &[], None).unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let metadata = metadata(&context);
    let plan = setup::prepare(root.path(), &metadata, IntegrationChoice::agent("claude"))
        .unwrap()
        .unwrap();
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = json!(plan);
            Ok(json!({}))
        })
        .unwrap();
    (root, context, plan)
}

fn metadata(context: &Context) -> Value {
    project::metadata(&project::checkpoint(context).unwrap())
}

fn authority(context: &Context) -> Value {
    let state = project::checkpoint(context).unwrap();
    json!({"read":state.read_requests,"run":state.run_requests,
        "hooks":state.hook_requests,"sessions":state.session_grants,"policy":state.policy_revision})
}

fn shared(root: &Path) -> Value {
    serde_json::from_str(read_file(root).unwrap().as_deref().unwrap_or("{}")).unwrap()
}

#[test]
fn reviewed_cancel_reverses_partial_setup_and_preserves_unrelated_entries() {
    for written in [false, true] {
        let (root, context, plan) = fixture();
        let before = authority(&context);
        if written {
            integrations::publish(root.path(), &plan).unwrap();
            files::publish(root.path(), &plan.files).unwrap();
        }
        let mut config = shared(root.path());
        if config["mcpServers"].is_null() {
            config["mcpServers"] = json!({});
        }
        config["mcpServers"]["acme"] = json!({"command":"acme-server"});
        fs::write(root.path().join(".mcp.json"), config.to_string()).unwrap();
        let prepared = prepare(root.path(), &metadata(&context), Action::Cancel).unwrap();
        assert_eq!(prepared.review(root.path())["pending"], "cancel");
        assert_eq!(authority(&context), before);
        let receipt = prepared.commit(&context, &|| Ok(())).unwrap();
        assert_eq!(receipt["pending_resolution"], "cancel");
        let config = shared(root.path());
        assert!(config["mcpServers"]["shield"].is_null());
        assert_eq!(config["mcpServers"]["acme"]["command"], "acme-server");
        assert!(pending(&metadata(&context)).unwrap().is_none());
        assert!(registration(&metadata(&context)).unwrap().is_none());
        assert_eq!(authority(&context), before);
    }
}

#[test]
fn reviewed_cancel_of_an_originally_absent_mcp_file_leaves_valid_configuration() {
    let (root, context, plan) = fixture();
    integrations::publish(root.path(), &plan).unwrap();
    let prepared = prepare(root.path(), &metadata(&context), Action::Cancel).unwrap();
    prepared.commit(&context, &|| Ok(())).unwrap();
    assert!(shared(root.path()).is_object());
    assert!(pending(&metadata(&context)).unwrap().is_none());
}

#[test]
fn owned_conflict_review_replaces_only_reviewed_fragments_and_refuses_stale_edits() {
    for stale in [false, true] {
        let (root, context, plan) = fixture();
        let actual = json!({"mcpServers":{"shield":{"command":"manual"},"acme":{"command":"acme-server"}},"theme":"light"});
        fs::write(root.path().join(".mcp.json"), actual.to_string()).unwrap();
        let original_metadata = metadata(&context);
        let before = authority(&context);
        let prepared = prepare(root.path(), &original_metadata, Action::Review).unwrap();
        assert_eq!(metadata(&context), original_metadata);
        if stale {
            let mut changed = actual.clone();
            changed["mcpServers"]["shield"]["command"] = json!("another-manual");
            fs::write(root.path().join(".mcp.json"), changed.to_string()).unwrap();
            let result = prepared.commit(&context, &|| Ok(()));
            assert!(
                result.is_err() || result.is_ok_and(|receipt| receipt["setup_error"].is_string())
            );
            assert_eq!(shared(root.path()), changed);
            assert_eq!(metadata(&context), original_metadata);
        } else {
            prepared.commit(&context, &|| Ok(())).unwrap();
            let result = shared(root.path());
            assert_eq!(result["mcpServers"]["shield"], plan.desired.unwrap().entry);
            assert_eq!(result["mcpServers"]["acme"], actual["mcpServers"]["acme"]);
            assert_eq!(result["theme"], "light");
            assert!(pending(&metadata(&context)).unwrap().is_none());
        }
        assert_eq!(authority(&context), before);
    }
}

#[test]
fn unreadable_or_manual_conflicts_can_be_cancelled_without_editing_any_host_file() {
    for contents in [
        "{broken",
        r#"{"mcpServers":{"shield":{"command":"manual"}}}"#,
    ] {
        let (root, mut context, plan) = fixture();
        context.surface = "mcp".into();
        integrations::publish(root.path(), &plan).unwrap();
        files::publish(root.path(), &plan.files).unwrap();
        fs::write(root.path().join(".mcp.json"), contents).unwrap();
        let before = authority(&context);
        let hooks = fs::read(root.path().join(".claude/settings.local.json")).unwrap();
        let prepared = prepare(root.path(), &metadata(&context), Action::Cancel).unwrap();
        assert!(
            !prepared.review(root.path())["preserved_paths"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let receipt = prepared.commit(&context, &|| Ok(())).unwrap();
        assert_eq!(receipt["host_files_changed"], false);
        assert_eq!(
            fs::read_to_string(root.path().join(".mcp.json")).unwrap(),
            contents
        );
        assert_eq!(
            fs::read(root.path().join(".claude/settings.local.json")).unwrap(),
            hooks
        );
        assert!(pending(&metadata(&context)).unwrap().is_none());
        assert!(registration(&metadata(&context)).unwrap().is_none());
        assert!(crate::review::bootstrap::forms_reviewed_at(root.path()));
        assert_eq!(authority(&context), before);
    }
}

#[cfg(unix)]
#[test]
fn linked_host_file_can_be_preserved_by_explicit_cancellation() {
    let (root, context, _) = fixture();
    let target = root.path().join("manual.json");
    fs::write(&target, "Acme unrelated data").unwrap();
    std::os::unix::fs::symlink(&target, root.path().join(".mcp.json")).unwrap();
    let before = authority(&context);
    let prepared = prepare(root.path(), &metadata(&context), Action::Cancel).unwrap();
    let receipt = prepared.commit(&context, &|| Ok(())).unwrap();
    assert_eq!(receipt["host_files_changed"], false);
    assert!(
        fs::symlink_metadata(root.path().join(".mcp.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(target).unwrap(), "Acme unrelated data");
    assert!(pending(&metadata(&context)).unwrap().is_none());
    assert_eq!(authority(&context), before);
}

#[test]
fn pending_setup_cannot_replay_a_different_committed_predecessor() {
    let (root, context, plan) = fixture();
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration"] = json!(plan.desired);
            Ok(json!({}))
        })
        .unwrap();
    let current = metadata(&context);
    assert!(setup::resume_confirmed(&context).is_err());
    assert!(prepare(root.path(), &current, Action::Review).is_err());
    assert!(!root.path().join(".mcp.json").exists());
    assert_eq!(metadata(&context), current);
    let cancelled = prepare(root.path(), &current, Action::Cancel).unwrap();
    cancelled.commit(&context, &|| Ok(())).unwrap();
    assert!(pending(&metadata(&context)).unwrap().is_none());
    assert!(!root.path().join(".mcp.json").exists());
}

#[test]
fn initial_provisional_adoption_remains_resumable_without_a_committed_registration() {
    let (root, context, mut plan) = fixture();
    integrations::publish(root.path(), &plan).unwrap();
    files::publish(root.path(), &plan.files).unwrap();
    plan.previous = plan.desired.clone();
    plan.policy_revision = None;
    plan.before = Some(plan.after.clone());
    for file in &mut plan.files {
        file.before = file.after.clone();
    }
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration_pending"] = json!(plan);
            Ok(json!({}))
        })
        .unwrap();
    assert!(registration(&metadata(&context)).unwrap().is_none());
    assert!(setup::resume_confirmed(&context).unwrap());
    assert!(pending(&metadata(&context)).unwrap().is_none());
    assert!(registration(&metadata(&context)).unwrap().is_some());
}

#[test]
fn cancelled_conflict_confirmation_cannot_replace_the_durable_pending_plan() {
    let (root, context, _) = fixture();
    let manual = r#"{"mcpServers":{"shield":{"command":"manual"}}}"#;
    fs::write(root.path().join(".mcp.json"), manual).unwrap();
    let current = metadata(&context);
    let prepared = prepare(root.path(), &current, Action::Review).unwrap();
    let checks = std::cell::Cell::new(0);
    let result = prepared.commit(&context, &|| {
        checks.set(checks.get() + 1);
        if checks.get() > 1 {
            Err(crate::policy::Error::Interrupted)
        } else {
            Ok(())
        }
    });
    assert!(checks.get() > 1);
    assert!(result.is_err());
    assert_eq!(metadata(&context), current);
    assert_eq!(
        fs::read_to_string(root.path().join(".mcp.json")).unwrap(),
        manual
    );
    assert!(!root.path().join(".claude/settings.local.json").exists());
}

#[test]
fn large_unrelated_settings_do_not_make_cancellation_exceed_the_human_form_bound() {
    let (root, context, plan) = fixture();
    integrations::publish(root.path(), &plan).unwrap();
    let mut config = shared(root.path());
    config["acme-unrelated"] = json!("x".repeat(30 * 1024));
    let source = config.to_string();
    fs::write(root.path().join(".mcp.json"), &source).unwrap();
    let before = authority(&context);
    let prepared = prepare(root.path(), &metadata(&context), Action::Cancel).unwrap();
    assert!(prepared.review(root.path()).to_string().len() < 24 * 1024);
    let receipt = prepared.commit(&context, &|| Ok(())).unwrap();
    assert_eq!(receipt["host_files_changed"], false);
    assert_eq!(
        fs::read_to_string(root.path().join(".mcp.json")).unwrap(),
        source
    );
    assert!(pending(&metadata(&context)).unwrap().is_none());
    assert_eq!(authority(&context), before);
}

#[test]
fn state_only_box_cancellation_preserves_independent_direct_connection() {
    let source = r#"{"schema_version":1,"packs":[],"mcp":{"cloud":{"transport":{"type":"http","url":"https://cloud.example.com/mcp"},"agents":{"claude":"acme","codex":"acme"}}}}"#;
    let (root, context, mut plan) = fixture_source(source);
    integrations::publish(root.path(), &plan).unwrap();
    files::publish(root.path(), &plan.files).unwrap();
    let mut committed = plan.desired.clone().unwrap();
    let direct = committed.connections["claude"].clone();
    committed.connections.insert("codex".into(), direct.clone());
    committed.box_definition = Some(super::super::box_definition::Definition {
        shared: "approved shared definition".into(),
        local: "approved local definition".into(),
    });
    let direct_agents = json!(committed.agents);
    plan.targets = Some(vec![super::super::box_definition::ID.into()]);
    plan.before = Some(plan.after.clone());
    plan.previous = Some(committed.clone());
    plan.desired = Some(committed.clone());
    plan.files = vec![files::Change {
        path: super::super::box_definition::PATH.into(),
        before: Some("approved shared definition".into()),
        after: Some("replacement shared definition".into()),
    }];
    fs::create_dir_all(root.path().join(".box/mcp")).unwrap();
    let definition = root.path().join(super::super::box_definition::PATH);
    fs::write(&definition, "manual definition").unwrap();
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            state.operator["integration"] = json!(committed);
            state.operator["integration_pending"] = json!(plan);
            Ok(json!({}))
        })
        .unwrap();
    let before = authority(&context);
    let prepared = prepare(root.path(), &metadata(&context), Action::Cancel).unwrap();
    let receipt = prepared.commit(&context, &|| Ok(())).unwrap();
    assert_eq!(receipt["host_files_changed"], false);
    let retained = registration(&metadata(&context)).unwrap().unwrap();
    assert_eq!(json!(retained.agents), direct_agents);
    assert_eq!(retained.connections["claude"], direct);
    assert!(!retained.connections.contains_key("codex"));
    assert!(retained.box_definition.is_none());
    assert!(pending(&metadata(&context)).unwrap().is_none());
    assert_eq!(fs::read_to_string(definition).unwrap(), "manual definition");
    assert_eq!(authority(&context), before);
}
