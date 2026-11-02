//! Installed-instance identities survive changes and never return after removal.
use super::*;

#[test]
fn mixed_layer_pack_changes_keep_artifacts_and_installed_metadata_aligned() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(&root, &context, &json!([]), false);
    registry_release(&source, "acme/extra", "1.0.0", None);
    registry_release(&source, "acme/another", "1.0.0", None);
    for (id, layer) in [
        ("parent", "project"),
        ("child", "local"),
        ("extra", "local"),
        ("another", "project"),
    ] {
        commit_pack_change(
            &context,
            &connection,
            json!({"kind":"pack_add","reference":format!("acme-dependencies/acme/{id}"),"layer":layer}),
            id,
        );
        let state = Store::open(root.path()).unwrap().read().unwrap();
        let candidate = &state.candidates["project"];
        let merged = crate::configuration::layers::merged(candidate).unwrap();
        for (index, row) in state.operator["installed"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let pack = crate::policy::Pack::from_yaml(candidate["packs"][index].as_str().unwrap())
                .unwrap();
            assert_eq!(row["id"], pack.id());
            assert_eq!(row["version"], pack.version());
            assert_eq!(merged["packs"][index]["id"], row["id"]);
        }
    }
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let candidate = &state.candidates["project"];
    let shared: Value = crate::policy::parse(candidate["config"].as_str().unwrap()).unwrap();
    let local: Value = crate::policy::parse(candidate["local_config"].as_str().unwrap()).unwrap();
    assert_eq!(
        shared["packs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["acme/parent", "acme/another"]
    );
    assert_eq!(
        local["packs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["acme/child", "acme/extra"]
    );
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_remove","reference":"acme/parent"}),
        "remove-shared",
    );
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_configure","reference":"acme/extra","override":{"read":"deny"},"layer":"local"}),
        "configure-local",
    );
}

fn selectors(root: &assert_fs::TempDir) -> BTreeMap<String, Value> {
    Store::open(root.path()).unwrap().read().unwrap().operator["installed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["id"].as_str().unwrap().to_owned(),
                row["selector"].clone(),
            )
        })
        .collect()
}

#[test]
fn mixed_layer_dependency_resolution_preserves_existing_local_order() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1"}]),
        false,
    );
    registry_release(&source, "acme/extra", "1.0.0", None);
    for (id, layer) in [
        ("extra", "project"),
        ("child", "local"),
        ("parent", "local"),
    ] {
        commit_pack_change(
            &context,
            &connection,
            json!({"kind":"pack_add","reference":format!("acme-dependencies/acme/{id}"),"layer":layer}),
            id,
        );
    }
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let candidate = &state.candidates["project"];
    let merged = crate::configuration::layers::merged(candidate).unwrap();
    let rows = state.operator["installed"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["acme/extra", "acme/child", "acme/parent"]
    );
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(merged["packs"][index]["id"], row["id"]);
        let pack =
            crate::policy::Pack::from_yaml(candidate["packs"][index].as_str().unwrap()).unwrap();
        assert_eq!(row["id"], pack.id());
    }
}

#[test]
fn installed_pack_selectors_survive_changes_but_are_not_reused_after_removal() {
    let (root, context, connection) = fixture();
    let _source = dependency_registry(
        &root,
        &context,
        &json!([{"pack":"acme/child","requirement":"^1"}]),
        false,
    );
    let add_parent = json!({"kind":"pack_add","reference":"acme-dependencies/acme/parent"});
    commit_pack_change(&context, &connection, add_parent.clone(), "first-install");
    let original = selectors(&root);
    assert_eq!(original.len(), 2);
    // Promoting a dependency and changing its profile keep the installed instance.
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child","profile":"strict"}),
        "promote-child",
    );
    assert_eq!(selectors(&root), original);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_remove","reference":original["acme/parent"]}),
        "remove-parent",
    );
    assert_eq!(selectors(&root)["acme/child"], original["acme/child"]);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_remove","reference":original["acme/child"]}),
        "remove-child",
    );
    assert!(selectors(&root).is_empty());
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let change = serde_json::from_value(add_parent.clone()).unwrap();
    let first = review::packs::prepare_change(&context, &before, &change).unwrap();
    let repeated = review::packs::prepare_change(&context, &before, &change).unwrap();
    assert_eq!(
        first, repeated,
        "preparation at one captured base is deterministic"
    );
    commit_pack_change(&context, &connection, add_parent, "second-install");
    let installed = selectors(&root);
    assert_eq!(installed.len(), 2);
    for (id, old) in original {
        assert_ne!(installed[&id], old);
        let before = Store::open(root.path()).unwrap().read().unwrap();
        let remove = serde_json::from_value(json!({"kind":"pack_remove","reference":old})).unwrap();
        assert!(matches!(
            review::packs::prepare_change(&context, &before, &remove),
            Err(review::Error::Missing)
        ));
    }
}
