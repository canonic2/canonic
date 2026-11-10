use super::*;
use serde_json::json;

fn configured(root: &Path) {
    fs::create_dir_all(root.join(".shield/review")).expect("fixture review directory");
    fs::write(
        root.join(".shield/config.yml"),
        "schema_version: 1\npacks: []\n",
    )
    .expect("fixture config");
}

#[test]
fn action_request_kinds_sdk_uses_only_the_registered_claude_form_channel() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let input = ReviewContext {
        project: root.path().into(),
        agent: Some("claude-sdk".into()),
        session_id: Some("acme-session".into()),
        tool_use_id: None,
    };
    let before = review_context(&input).unwrap();
    assert!(before.agent.is_none() && !before.confirmed_mcp_forms);
    crate::review::operator::integrations::configure_fixture(
        root.path(),
        crate::review::integrations::IntegrationChoice::agent("claude"),
        crate::review::integrations::AskFallback::Block,
    );
    let unknown = review_context(&input).unwrap();
    assert!(unknown.agent.is_none() && !unknown.confirmed_mcp_forms);
    let native = review_context(&ReviewContext {
        agent: Some("claude".into()),
        ..input
    })
    .unwrap();
    assert_eq!(native.agent.as_deref(), Some("claude"));
    assert_eq!(native.session_id.as_deref(), Some("acme-session"));
}

#[test]
fn bootstrap_healthy_launcher_remapping_requires_its_confirmed_receipt() {
    let root = assert_fs::TempDir::new().unwrap();
    let original = root.path().join("original");
    let destination = root.path().join("destination");
    let config = "schema_version: 1\npacks: []\n";
    let bundle = crate::configuration::Bundle::capture(config, &[], None).unwrap();
    for project in [&original, &destination] {
        fs::create_dir(project).unwrap();
        crate::review::mcp::initialize_fixture(project, &bundle);
        fs::write(project.join(".shield/config.yml"), config).unwrap();
    }
    let original = original.canonicalize().unwrap();
    let destination = destination.canonicalize().unwrap();
    let checkpoint = destination.join(".shield/review/state.json");
    let mut healthy: serde_json::Value =
        serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    healthy["operator"]["bootstrap"] = json!({"launch_roots":[original]});
    fs::write(&checkpoint, healthy.to_string()).unwrap();
    assert!(!crate::review::bootstrap::accepts_launch(
        &destination,
        &original
    ));
    assert_eq!(
        project_path(Some(&original), &destination).unwrap(),
        original
    );
    // A real, explicitly accepted enrollment creates the independent proof.
    fs::copy(original.join(".shield/review/state.json"), &checkpoint).unwrap();
    crate::review::bootstrap::prepare_launch(&destination, &original)
        .unwrap()
        .commit()
        .unwrap();
    assert!(crate::review::bootstrap::accepts_launch(
        &destination,
        &original
    ));
    assert_eq!(
        project_path(Some(&original), &destination).unwrap(),
        destination
    );
}

#[test]
fn bootstrap_pinned_old_launch_selects_nearest_copy_or_move_for_review() {
    let root = assert_fs::TempDir::new().unwrap();
    let original = root.path().join("original");
    configured(&original);
    let original = original.canonicalize().unwrap();
    let destination = root.path().join("destination");
    configured(&destination);
    let destination = destination.canonicalize().unwrap();
    let nested = destination.join("src/child");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        destination.join(".shield/review/state.json"),
        json!({"schema_version":1,"project_root":original}).to_string(),
    )
    .unwrap();
    assert_eq!(project_path(Some(&original), &nested).unwrap(), destination);
    fs::rename(&original, root.path().join("original-moved")).unwrap();
    assert_eq!(project_path(Some(&original), &nested).unwrap(), destination);
    assert_eq!(project_path(None, &nested).unwrap(), destination);
}

#[test]
fn bootstrap_other_enrollments_cannot_replace_explicit_authority() {
    let root = assert_fs::TempDir::new().unwrap();
    let selected = root.path().join("selected");
    let current = root.path().join("current");
    configured(&selected);
    configured(&current);
    let selected = selected.canonicalize().unwrap();
    let current = current.canonicalize().unwrap();
    fs::write(
        current.join(".shield/review/state.json"),
        json!({"schema_version":1,"project_root":current}).to_string(),
    )
    .unwrap();
    assert_eq!(project_path(Some(&selected), &current).unwrap(), selected);
    let nested = current.join("nested");
    configured(&nested);
    assert_eq!(project_path(None, &nested).unwrap(), nested);
    // The nested config has no copied state naming selected; do not cross it.
    assert_eq!(project_path(Some(&selected), &nested).unwrap(), selected);
    assert_eq!(project_path(Some(&current), &nested).unwrap(), current);
    let pending = current.join("pending");
    fs::create_dir_all(pending.join(".shield/review")).unwrap();
    assert_eq!(project_path(None, &pending).unwrap(), pending);
    assert_eq!(project_path(Some(&current), &pending).unwrap(), current);
    // An unconfigured working directory leaves an explicit selection intact.
    assert_eq!(
        project_path(Some(&selected), root.path()).unwrap(),
        selected
    );
}

#[test]
fn bootstrap_corrupt_or_linked_claims_cannot_redirect_explicit_authority() {
    let root = assert_fs::TempDir::new().unwrap();
    let selected = root.path().join("selected");
    let current = root.path().join("current");
    configured(&selected);
    configured(&current);
    let selected = selected.canonicalize().unwrap();
    let current = current.canonicalize().unwrap();
    let state = current.join(".shield/review/state.json");
    fs::write(&state, "broken").unwrap();
    assert_eq!(project_path(Some(&selected), &current).unwrap(), selected);
    // A future format can name the old root for diagnostics, but only the
    // copied destination may then be opened; its strict codec refuses it.
    fs::write(
        &state,
        json!({"schema_version":999,"project_root":selected}).to_string(),
    )
    .unwrap();
    assert_eq!(project_path(Some(&selected), &current).unwrap(), current);
    #[cfg(unix)]
    {
        let other = root.path().join("linked-state.json");
        fs::write(
            &other,
            json!({"schema_version":1,"project_root":selected}).to_string(),
        )
        .unwrap();
        fs::remove_file(&state).unwrap();
        std::os::unix::fs::symlink(other, &state).unwrap();
        assert_eq!(project_path(Some(&selected), &current).unwrap(), selected);
    }
}

#[cfg(unix)]
#[test]
fn bootstrap_discovery_canonicalizes_cwd_without_changing_explicit_authority() {
    let root = assert_fs::TempDir::new().unwrap();
    let selected = root.path().join("selected");
    let current = root.path().join("current");
    configured(&selected);
    configured(&current);
    let selected = selected.canonicalize().unwrap();
    let current = current.canonicalize().unwrap();
    let alias = root.path().join("current-alias");
    std::os::unix::fs::symlink(&current, &alias).unwrap();
    fs::write(
        current.join(".shield/review/state.json"),
        json!({
            "schema_version":1,"project_root":current,
            "operator":{"bootstrap":{"launch_roots":[selected]}}
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(project_path(Some(&selected), &alias).unwrap(), selected);
    assert_eq!(project_path(None, &alias).unwrap(), current);
    fs::remove_file(current.join(".shield/config.yml")).unwrap();
    fs::remove_file(current.join(".shield/review/state.json")).unwrap();
    std::os::unix::fs::symlink("missing-config", current.join(".shield/config.yml")).unwrap();
    assert_eq!(project_path(Some(&selected), &current).unwrap(), selected);
}
