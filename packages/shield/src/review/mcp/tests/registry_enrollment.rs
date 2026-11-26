//! Registry discovery cannot substitute for an exact retained human review.
use super::*;

fn local() -> (assert_fs::TempDir, Context, Connection, Value) {
    let (root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    std::fs::create_dir(root.path().join("registry")).unwrap();
    std::fs::write(
        root.path().join("registry/catalog.json"),
        "{\"schema_version\":1,\"packs\":[]}",
    )
    .unwrap();
    (
        root,
        context,
        connection,
        json!({"name":"acme","location":"registry"}),
    )
}

fn confirmed(context: &Context, connection: &Connection, args: &Value, binding: u64) -> Value {
    call_with_bound_confirmation(
        context,
        connection,
        "registry_add",
        args,
        &|| Ok(()),
        true,
        Some(binding),
    )
}

#[test]
fn registry_enrollment_requires_its_own_connection_bound_confirmation_and_consumes_it_once() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::registry_enrollment_requires_its_own_connection_bound_confirmation_and_consumes_it_once"
    )) {
        return;
    }
    let (_root, context, connection, args) = local();
    let before = review::project::checkpoint(&context)
        .unwrap()
        .policy_revision;
    assert_eq!(
        call(&context, &connection, "registry_add", &args, &|| Ok(()))["error"]["code"],
        "operator_review_required"
    );
    assert_eq!(
        call_with_confirmation(
            &context,
            &connection,
            "registry_add",
            &args,
            &|| Ok(()),
            true
        )["error"]["code"],
        "stale_revision"
    );
    let (message, binding) =
        admin_confirmation(&context, &connection, "registry_add", &args, &|| Ok(()))
            .unwrap()
            .unwrap();
    assert!(message.contains("unsigned catalog"));
    assert!(
        !review::project::checkpoint(&context)
            .unwrap()
            .settings
            .registries
            .sources
            .contains_key("acme")
    );
    let foreign = Connection::new("foreign".into());
    assert!(confirmed(&context, &foreign, &args, binding)["error"].is_object());
    let result = confirmed(&context, &connection, &args, binding);
    assert!(result["error"].is_null(), "{result}");
    assert_eq!(result["data"]["scope"], "project");
    assert_eq!(
        review::project::checkpoint(&context)
            .unwrap()
            .policy_revision,
        before
    );
    assert_eq!(
        confirmed(&context, &connection, &args, binding)["error"]["code"],
        "stale_revision"
    );
}

#[test]
fn registry_enrollment_changed_and_discarded_reviews_do_not_enroll() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::registry_enrollment_changed_and_discarded_reviews_do_not_enroll"
    )) {
        return;
    }
    for mutation in ["name", "location", "scope", "discarded"] {
        let (_root, context, connection, args) = local();
        let (_, binding) =
            admin_confirmation(&context, &connection, "registry_add", &args, &|| Ok(()))
                .unwrap()
                .unwrap();
        let mut changed = args.clone();
        match mutation {
            "name" => changed["name"] = json!("other"),
            "location" => changed["location"] = json!("other"),
            "scope" => changed["scope"] = json!("user"),
            "discarded" => discard_admin(&connection, binding),
            _ => unreachable!(),
        }
        assert_eq!(
            confirmed(&context, &connection, &changed, binding)["error"]["code"],
            "stale_revision"
        );
        assert_eq!(
            confirmed(&context, &connection, &args, binding)["error"]["code"],
            "stale_revision"
        );
        assert!(
            !review::project::checkpoint(&context)
                .unwrap()
                .settings
                .registries
                .sources
                .contains_key("acme")
        );
    }
}

#[test]
fn registry_enrollment_full_review_capacity_refuses_before_source_discovery_until_one_is_discarded()
{
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::registry_enrollment_full_review_capacity_refuses_before_source_discovery_until_one_is_discarded"
    )) {
        return;
    }
    let (_root, context, connection, args) = local();
    for _ in 0..8 {
        admin_confirmation(&context, &connection, "registry_add", &args, &|| Ok(()))
            .unwrap()
            .unwrap();
    }
    let missing = json!({"name":"other","location":"missing-registry"});
    assert!(matches!(
        admin_confirmation(&context, &connection, "registry_add", &missing, &|| Ok(())),
        Err(refusal) if matches!(refusal.error.as_ref(), Error::Overloaded)
    ));
    let binding = *connection.admin.lock().unwrap().keys().next().unwrap();
    discard_admin(&connection, binding);
    let refusal = admin_confirmation(&context, &connection, "registry_add", &missing, &|| Ok(()))
        .unwrap_err();
    assert!(!matches!(refusal.error.as_ref(), Error::Overloaded));
    admin_confirmation(&context, &connection, "registry_add", &args, &|| Ok(()))
        .unwrap()
        .unwrap();
    assert_eq!(connection.admin.lock().unwrap().len(), 8);
    assert!(
        !review::project::checkpoint(&context)
            .unwrap()
            .settings
            .registries
            .sources
            .contains_key("acme")
    );
}

#[test]
fn registry_enrollment_stale_snapshot_cannot_overwrite_another_confirmed_source() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::registry_enrollment_stale_snapshot_cannot_overwrite_another_confirmed_source"
    )) {
        return;
    }
    let (_root, context, connection, args) = local();
    let (_, earlier) = admin_confirmation(&context, &connection, "registry_add", &args, &|| Ok(()))
        .unwrap()
        .unwrap();
    let mut other = args.clone();
    other["name"] = json!("other");
    let (_, later) = admin_confirmation(&context, &connection, "registry_add", &other, &|| Ok(()))
        .unwrap()
        .unwrap();
    assert!(confirmed(&context, &connection, &other, later)["error"].is_null());
    assert_eq!(
        confirmed(&context, &connection, &args, earlier)["error"]["code"],
        "stale_revision"
    );
    let state = review::project::checkpoint(&context).unwrap();
    assert!(!state.settings.registries.sources.contains_key("acme"));
    assert!(state.settings.registries.sources.contains_key("other"));
    let (_, fresh) = admin_confirmation(&context, &connection, "registry_add", &args, &|| Ok(()))
        .unwrap()
        .unwrap();
    assert!(confirmed(&context, &connection, &args, fresh)["error"].is_null());
    assert!(
        review::project::checkpoint(&context)
            .unwrap()
            .settings
            .registries
            .sources
            .contains_key("other")
    );
}
