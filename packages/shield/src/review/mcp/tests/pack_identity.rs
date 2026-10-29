//! Installed-instance identities survive changes and never return after removal.
use super::*;

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
