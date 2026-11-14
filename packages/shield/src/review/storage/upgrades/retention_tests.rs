use super::*;

fn historical_fixture() -> (assert_fs::TempDir, Value) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let mut raw = super::tests::earlier_checkpoint(root.path());
    raw["project_root"] = json!(context.project_root);
    let object = raw.as_object_mut().unwrap();
    for key in [
        "hook_requests",
        "session_grants",
        "policy_drafts",
        "dismissed_reviews",
        "enforcement",
        "followed_files",
    ] {
        object.remove(key);
    }
    super::tests::legacy_activation(&mut raw);
    raw["history_generation"] = Value::Null;
    (root, raw)
}

fn backup(store: &Store, bytes: &[u8]) -> String {
    let digest = crate::policy::fingerprint(bytes);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    let mut file = options
        .open(store.directory.join(source_name(bytes)))
        .unwrap();
    file.write_all(bytes).unwrap();
    digest
}

fn backup_count(store: &Store) -> usize {
    fs::read_dir(&store.directory)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_str()
                .is_some_and(|name| {
                    name.starts_with("upgrade-")
                        && std::path::Path::new(name)
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
                })
        })
        .count()
}

#[test]
fn upgrade_backup_retention_bounds_failed_attempts_and_preserves_committed_sources() {
    let (root, mut raw) = historical_fixture();
    let store = Store::open(root.path()).unwrap();
    let first_bytes = b"{\"committed-source\":1}";
    let second_bytes = b"{\"committed-source\":2}";
    let first = backup(&store, first_bytes);
    let second = backup(&store, second_bytes);
    raw["operator"]["upgrade_recovery"] = json!([first, second]);
    // An unreadable integration record fails each conversion attempt.
    raw["operator"]["integration"] = json!("invalid");
    let checkpoint = store.directory.join("state.json");
    for sequence in 100..112 {
        raw["sequence"] = json!(sequence);
        let original = serde_json::to_vec(&raw).unwrap();
        fs::write(&checkpoint, &original).unwrap();
        assert!(store.read_checkpoint().is_err());
        assert_eq!(fs::read(&checkpoint).unwrap(), original);
        assert!(backup_count(&store) <= 3);
        assert_eq!(
            retained(&store, &source_name(first_bytes)).unwrap(),
            first_bytes
        );
        assert_eq!(
            retained(&store, &source_name(second_bytes)).unwrap(),
            second_bytes
        );
    }
    raw["operator"]
        .as_object_mut()
        .unwrap()
        .remove("integration");
    let original = serde_json::to_vec(&raw).unwrap();
    fs::write(&checkpoint, &original).unwrap();
    let state = store.read_checkpoint().unwrap();
    assert_eq!(backup_count(&store), 2);
    assert_eq!(
        state.operator["upgrade_recovery"],
        json!([crate::policy::fingerprint(&original), first])
    );
    assert_eq!(retained(&store, &source_name(&original)).unwrap(), original);
    assert_eq!(
        retained(&store, &source_name(first_bytes)).unwrap(),
        first_bytes
    );
    assert!(!store.directory.join(source_name(second_bytes)).exists());
    let committed = fs::read(&checkpoint).unwrap();
    store.read_checkpoint().unwrap();
    assert_eq!(fs::read(checkpoint).unwrap(), committed);
    assert_eq!(backup_count(&store), 2);
}

#[test]
fn upgrade_backup_retention_ignores_large_unrelated_directory_population() {
    let (root, raw) = historical_fixture();
    let store = Store::open(root.path()).unwrap();
    for index in 0..1030 {
        fs::write(
            store.directory.join(format!("unrelated-{index}")),
            b"preserve",
        )
        .unwrap();
    }
    fs::write(store.directory.join("state.json"), raw.to_string()).unwrap();
    store.read_checkpoint().unwrap();
    assert_eq!(backup_count(&store), 1);
    for index in 0..1030 {
        assert_eq!(
            fs::read(store.directory.join(format!("unrelated-{index}"))).unwrap(),
            b"preserve"
        );
    }
}

#[test]
fn upgrade_backup_retention_preserves_unverified_files_and_rejects_invalid_pins() {
    let (root, raw) = historical_fixture();
    let store = Store::open(root.path()).unwrap();
    let unrelated_name = format!("upgrade-{}.json", "a".repeat(64));
    let unrelated = store.directory.join(&unrelated_name);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    let mut file = options.open(&unrelated).unwrap();
    file.write_all(b"unverified recovery evidence").unwrap();
    drop(file);
    let pins = Vec::new();
    cleanup(&store, &pins).unwrap();
    assert_eq!(
        fs::read(&unrelated).unwrap(),
        b"unverified recovery evidence"
    );
    for invalid in [
        Value::Null,
        json!(true),
        json!(["bad"]),
        json!(["a".repeat(64)]),
    ] {
        let mut raw = raw.clone();
        raw["operator"]["upgrade_recovery"] = invalid;
        let bytes = serde_json::to_vec(&raw).unwrap();
        fs::write(store.directory.join("state.json"), &bytes).unwrap();
        assert!(store.read_checkpoint().is_err());
        assert_eq!(fs::read(store.directory.join("state.json")).unwrap(), bytes);
        assert_eq!(
            fs::read(&unrelated).unwrap(),
            b"unverified recovery evidence"
        );
    }
}

#[test]
#[cfg(unix)]
fn upgrade_backup_retention_never_removes_linked_or_nonprivate_sources() {
    use std::os::unix::fs::PermissionsExt;
    let (root, _) = historical_fixture();
    let store = Store::open(root.path()).unwrap();
    let public_bytes = b"{\"public-source\":true}";
    let public = store.directory.join(source_name(public_bytes));
    fs::write(&public, public_bytes).unwrap();
    fs::set_permissions(&public, fs::Permissions::from_mode(0o644)).unwrap();
    let linked_bytes = b"{\"linked-source\":true}";
    let original = root.path().join("original-source.json");
    fs::write(&original, linked_bytes).unwrap();
    let linked = store.directory.join(source_name(linked_bytes));
    std::os::unix::fs::symlink(&original, &linked).unwrap();
    cleanup(&store, &[]).unwrap();
    assert_eq!(fs::read(public).unwrap(), public_bytes);
    assert!(
        fs::symlink_metadata(linked)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(original).unwrap(), linked_bytes);
}
