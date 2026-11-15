use super::*;

#[test]
fn handles_charge_their_capture_and_prepare_only_on_evaluation() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let state = Store::open(root.path())
        .unwrap()
        .read_following_files()
        .unwrap();
    let candidate = &state.candidates["project"];
    let mut views = Views::new("capture-admission".into());
    let id = views
        .retain_prepared(
            &context,
            candidate,
            &state.operator,
            None,
            Some(&state.policy_revision),
            None,
        )
        .unwrap();
    assert!(views.bytes >= crate::json::encoded_len(&[candidate, &state.operator]).unwrap());
    let captured = views.get(&id).unwrap();
    assert!(captured.evaluator.get().is_none());
    assert_eq!(
        views
            .prepare(&context, &id)
            .unwrap()
            .evaluator()
            .unwrap()
            .revision(),
        state.policy_revision
    );
}

#[test]
fn a_handle_keeps_its_capture_after_the_files_change() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let mut context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    context.surface = "cli".into();
    let mut views = Views::new("acme-lazy-reader".into());
    let id = views.active_readonly(&context).unwrap();
    let captured = views.get(&id).unwrap();
    let original = captured.candidate.clone();
    let revision = captured.revision().to_owned();
    std::fs::write(
        root.path().join(".shield/config.yml"),
        format!(
            "schema_version: 1\npacks: []\naccess_paths:\n  read:\n  - {}\n",
            crate::test_support::absolute("/srv/acme")
        ),
    )
    .unwrap();
    assert_eq!(captured.candidate, original);
    assert_eq!(
        views
            .prepare(&context, &id)
            .unwrap()
            .evaluator()
            .unwrap()
            .revision(),
        revision
    );
    let next = views.active_readonly(&context).unwrap();
    assert_ne!(next, id);
}
