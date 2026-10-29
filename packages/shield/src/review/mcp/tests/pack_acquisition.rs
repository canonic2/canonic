//! Engine-compatible root selection through the shared CLI/MCP preparation path.
use super::*;

fn unchanged_authority(path: &Path, original: &[u8]) {
    let actual = std::fs::read(path).unwrap();
    let mut before: Value = serde_json::from_slice(original).unwrap();
    let mut after: Value = serde_json::from_slice(&actual).unwrap();
    // Verified catalog refresh retains anti-rollback evidence even if acquiring
    // a selected artifact fails. It must not change policy, grants or proposals.
    let previous = before
        .as_object_mut()
        .unwrap()
        .remove("registry_checkpoints")
        .unwrap();
    let checkpoints = after
        .as_object_mut()
        .unwrap()
        .remove("registry_checkpoints")
        .unwrap();
    assert!(!checkpoints.as_object().unwrap().is_empty());
    for (source, retained) in previous.as_object().unwrap() {
        assert_eq!(checkpoints.get(source), Some(retained));
    }
    assert_eq!(after, before);
}

fn engine_release(source: &assert_fs::TempDir, version: &str, engine: &str) {
    registry_release(source, "acme/child", version, None);
    rewrite_release(source, version, |pack| {
        pack["engines"]["shield"] = json!(engine);
    });
}

fn rewrite_release(source: &assert_fs::TempDir, version: &str, edit: impl FnOnce(&mut Value)) {
    let path = source.path().join(format!("acme-child-{version}.yml"));
    let mut pack: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    edit(&mut pack);
    let bytes = pack.to_string();
    std::fs::write(path, &bytes).unwrap();
    let path = source.path().join("catalog.json");
    let mut catalog: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let entry = catalog["packs"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == "acme/child" && entry["version"] == version)
        .unwrap();
    entry["sha256"] = json!(crate::policy::fingerprint(bytes.as_bytes()));
    std::fs::write(path, catalog.to_string()).unwrap();
}

#[test]
fn unversioned_add_selects_the_highest_engine_compatible_stable_root() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(&root, &context, &json!([]), false);
    engine_release(&source, "1.1.0", "*");
    engine_release(&source, "2.0.0", ">9999.0.0");
    engine_release(&source, "3.0.0-beta.1", "*");
    let store = Store::open(root.path()).unwrap();
    let before = store.read().unwrap();
    let state_path = root.path().join(".shield/review/state.json");
    let original = std::fs::read(&state_path).unwrap();
    let change = json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"});
    let parsed = serde_json::from_value(change.clone()).unwrap();
    let (_, prepared) = review::packs::prepare_change(&context, &before, &parsed).unwrap();
    assert_eq!(prepared["installed"][0]["version"], "1.1.0");
    assert_eq!(prepared["installed"][0]["pinned"], false);
    unchanged_authority(&state_path, &original);
    commit_pack_change(&context, &connection, change, "compatible-root");
    let installed = store.read().unwrap().operator["installed"].clone();
    assert_eq!(installed[0]["version"], "1.1.0");
    assert_eq!(installed[0]["profile"], "recommended");
    assert_eq!(installed[0]["pinned"], false);
    commit_pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child@3.0.0-beta.1"}),
        "exact-prerelease",
    );
    let installed = store.read().unwrap().operator["installed"].clone();
    assert_eq!(installed[0]["version"], "3.0.0-beta.1");
    assert_eq!(installed[0]["pinned"], true);
}

#[test]
fn unversioned_add_never_falls_back_after_integrity_failure_or_for_an_exact_pin() {
    let (root, context, connection) = fixture();
    let source = dependency_registry(&root, &context, &json!([]), false);
    engine_release(&source, "2.0.0", ">9999.0.0");
    let state_path = root.path().join(".shield/review/state.json");
    let original = std::fs::read(&state_path).unwrap();
    let exact = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child@2.0.0"}),
        "unsupported-pin",
    );
    assert_ne!(exact["error"], Value::Null);
    unchanged_authority(&state_path, &original);
    // The catalog's digest remains unchanged: engine incompatibility must not hide tampering.
    std::fs::write(source.path().join("acme-child-2.0.0.yml"), "tampered").unwrap();
    let invalid = pack_change(
        &context,
        &connection,
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
        "invalid-digest",
    );
    assert_ne!(invalid["error"], Value::Null);
    unchanged_authority(&state_path, &original);
}

#[test]
fn unversioned_add_refuses_a_verified_artifact_with_the_wrong_identity() {
    let (root, context, _connection) = fixture();
    let source = dependency_registry(&root, &context, &json!([]), false);
    engine_release(&source, "2.0.0", "*");
    rewrite_release(&source, "2.0.0", |pack| {
        pack["id"] = json!("acme/other");
    });
    let state_path = root.path().join(".shield/review/state.json");
    let original = std::fs::read(&state_path).unwrap();
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let change = serde_json::from_value(
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
    )
    .unwrap();
    assert!(matches!(
        review::packs::prepare_change(&context, &before, &change),
        Err(review::Error::Invalid(
            "registry artifact identity mismatch"
        ))
    ));
    unchanged_authority(&state_path, &original);
}

/// A named edit that makes a release manifest malformed.
type ReleaseEdit = (&'static str, fn(&mut Value));

#[test]
fn unversioned_add_refuses_a_malformed_newest_release_instead_of_an_older_one() {
    let edits: [ReleaseEdit; 3] = [
        ("pack id", |pack| pack["id"] = json!("acme")),
        ("maintainers", |pack| {
            pack["maintainers"] = json!(["Acme", "Acme"]);
        }),
        ("unknown field", |pack| pack["unexpected"] = json!(true)),
    ];
    for (case, edit) in edits {
        let (root, context, _connection) = fixture();
        let source = dependency_registry(&root, &context, &json!([]), false);
        engine_release(&source, "1.1.0", "*");
        engine_release(&source, "2.0.0", "*");
        rewrite_release(&source, "2.0.0", edit);
        let state_path = root.path().join(".shield/review/state.json");
        let original = std::fs::read(&state_path).unwrap();
        let before = Store::open(root.path()).unwrap().read().unwrap();
        let change = serde_json::from_value(
            json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
        )
        .unwrap();
        let result = review::packs::prepare_change(&context, &before, &change);
        assert!(
            matches!(result, Err(review::Error::Policy(_))),
            "{case}: {result:?}"
        );
        unchanged_authority(&state_path, &original);
    }
}

#[test]
fn unversioned_add_skips_a_newest_release_in_an_unsupported_schema() {
    let (root, context, _connection) = fixture();
    let source = dependency_registry(&root, &context, &json!([]), false);
    engine_release(&source, "1.1.0", "*");
    engine_release(&source, "2.0.0", "*");
    rewrite_release(&source, "2.0.0", |pack| {
        pack["schema_version"] = json!(2);
        pack["future"] = json!({"rules":"v2"});
    });
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let change = serde_json::from_value(
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
    )
    .unwrap();
    let (_, prepared) = review::packs::prepare_change(&context, &before, &change).unwrap();
    assert_eq!(prepared["installed"][0]["version"], "1.1.0");
}

#[test]
fn unversioned_add_skips_a_newest_release_declaring_an_unknown_capability() {
    let (root, context, _connection) = fixture();
    let source = dependency_registry(&root, &context, &json!([]), false);
    engine_release(&source, "1.1.0", "*");
    engine_release(&source, "2.0.0", "*");
    // A capability from a later Shield, used by a field this engine rejects.
    rewrite_release(&source, "2.0.0", |pack| {
        pack["capabilities"]
            .as_array_mut()
            .unwrap()
            .push(json!("acme-future-v1"));
        pack["future"] = json!({"rules":"v2"});
    });
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let change = serde_json::from_value(
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
    )
    .unwrap();
    let (_, prepared) = review::packs::prepare_change(&context, &before, &change).unwrap();
    assert_eq!(prepared["installed"][0]["version"], "1.1.0");
}

#[test]
fn unversioned_add_reports_when_every_stable_root_is_unsupported() {
    let (root, context, _connection) = fixture();
    let source = dependency_registry(&root, &context, &json!([]), false);
    let path = source.path().join("catalog.json");
    let mut catalog: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    catalog["packs"] = json!([]);
    std::fs::write(path, catalog.to_string()).unwrap();
    engine_release(&source, "2.0.0", ">9999.0.0");
    engine_release(&source, "3.0.0-beta.1", "*");
    let before = Store::open(root.path()).unwrap().read().unwrap();
    let change = serde_json::from_value(
        json!({"kind":"pack_add","reference":"acme-dependencies/acme/child"}),
    )
    .unwrap();
    assert!(matches!(
        review::packs::prepare_change(&context, &before, &change),
        Err(review::Error::UnsupportedReleases(id)) if id == "acme/child"
    ));
}
