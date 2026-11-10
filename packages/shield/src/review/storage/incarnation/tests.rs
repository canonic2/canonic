use super::*;
use crate::review;

struct Host;
impl Drop for Host {
    fn drop(&mut self) {
        TEST_DIRECTORY.with(|v| *v.borrow_mut() = None);
    }
}
fn host(path: &Path) -> Host {
    TEST_DIRECTORY.with(|v| *v.borrow_mut() = Some(path.into()));
    Host
}
fn copy(from: &Path, to: &Path) {
    fs::create_dir(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &to.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

#[test]
fn same_absolute_path_replacement_closes_handles_and_keeps_reviewed_policy() {
    let home = assert_fs::TempDir::new().unwrap();
    let _host = host(&home.path().join("runtime"));
    let (root, context, evidence, digest) = review::read_approval::tests::fixture();
    let request =
        review::read_approval::gate(&context, "acme-factory", &evidence, &digest, None, || true)
            .unwrap();
    let id = request["approval"]["request_id"].as_str().unwrap();
    review::read_approval::authorize(&context, id, 300, None, false).unwrap();
    let store = Store::open(root.path()).unwrap();
    let before = store.read_checkpoint().unwrap();
    let current = root.path().join(".shield/review");
    let saved = root.path().join(".shield/previous-review");
    fs::rename(&current, &saved).unwrap();
    copy(&saved, &current);
    let next = review::open(root.path()).unwrap();
    assert_ne!(next.project_id, context.project_id);
    let after = store.read_checkpoint().unwrap();
    assert_eq!(after.policy_lineage(), before.policy_lineage());
    assert_eq!(
        after.history_owner.as_deref(),
        Some(before.project_id.as_str())
    );
    assert!(after.read_requests.is_empty());
    assert!(
        json!(after.session_grants)["records"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    after.check_operations().unwrap();
    crate::runtime::Project::open(root.path())
        .unwrap()
        .snapshot()
        .unwrap();
    assert_eq!(
        review::open(root.path()).unwrap().project_id,
        next.project_id
    );
}

#[test]
fn different_host_nonce_and_legacy_adoption_rotate_only_operation_epoch() {
    let home = assert_fs::TempDir::new().unwrap();
    let _host = host(&home.path().join("runtime"));
    let (root, context, _, _) = review::read_approval::tests::fixture();
    let store = Store::open(root.path()).unwrap();
    let before = store.read_checkpoint().unwrap();
    // Another private user/host nonce simulates the copied runtime on a new host.
    TEST_DIRECTORY.with(|v| *v.borrow_mut() = Some(home.path().join("another-host")));
    let next = review::open(root.path()).unwrap();
    assert_ne!(next.project_id, context.project_id);
    let after = store.read_checkpoint().unwrap();
    assert_eq!(after.policy_lineage(), before.policy_lineage());
    after.check_operations().unwrap();
    let mut raw = serde_json::to_value(&after).unwrap();
    raw["operator"]
        .as_object_mut()
        .unwrap()
        .remove("runtime_incarnation");
    fs::write(
        root.path().join(".shield/review/state.json"),
        raw.to_string(),
    )
    .unwrap();
    let migrated = review::open(root.path()).unwrap();
    assert_ne!(migrated.project_id, next.project_id);
    assert_eq!(
        review::open(root.path()).unwrap().project_id,
        migrated.project_id
    );
}

#[test]
fn unavailable_host_storage_preserves_checkpoint_and_allows_readonly_diagnosis() {
    let home = assert_fs::TempDir::new().unwrap();
    let _host = host(&home.path().join("runtime"));
    let (root, context, _, _) = review::read_approval::tests::fixture();
    let before = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let checkpoint = root.path().join(".shield/review/state.json");
    let bytes = fs::read(&checkpoint).unwrap();
    fs::write(home.path().join("blocked"), "Acme unavailable directory").unwrap();
    TEST_DIRECTORY.with(|v| *v.borrow_mut() = Some(home.path().join("blocked")));
    for _ in 0..3 {
        assert!(matches!(
            Store::open(root.path()).unwrap().read_checkpoint(),
            Err(Error::Invalid(
                "operation continuity unavailable; retained checkpoint unchanged"
            ))
        ));
        assert_eq!(fs::read(&checkpoint).unwrap(), bytes);
    }
    assert!(before.check_operations().is_err());
    assert_eq!(readiness(&before, root.path())["available"], false);
    let diagnostic = review::open_diagnostics(root.path()).unwrap();
    review::project::status(&diagnostic).unwrap();
    review::project::doctor(&diagnostic).unwrap();
    assert!(review::runtime::consumer::load(&context, &|| Ok(())).is_err());
    assert_eq!(fs::read(&checkpoint).unwrap(), bytes);
}

#[test]
fn inactive_legacy_and_moved_previews_reset_runtime_without_inventing_activation() {
    let home = assert_fs::TempDir::new().unwrap();
    let _host = host(&home.path().join("runtime"));
    for moved in [false, true] {
        let (root, context) = review::tests::enrolled();
        let store = Store::open(root.path()).unwrap();
        let mut state = store.read_checkpoint().unwrap();
        assert!(state.active.is_none());
        let before = state.candidates.clone();
        state
            .operator
            .as_object_mut()
            .unwrap()
            .remove("runtime_incarnation");
        fs::write(
            root.path().join(".shield/review/state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        let parent = assert_fs::TempDir::new().unwrap();
        let destination = parent.path().join("moved");
        let target = if moved {
            fs::rename(root.path(), &destination).unwrap();
            destination.as_path()
        } else {
            root.path()
        };
        let next = review::open(target).unwrap();
        assert_ne!(next.project_id, context.project_id);
        let state = Store::open(target).unwrap().read_checkpoint().unwrap();
        assert!(state.active.is_none());
        assert_eq!(state.candidates, before);
        assert!(
            state
                .authorization_events
                .iter()
                .all(|event| event["kind"] != "runtime_activated")
        );
        state.check_operations().unwrap();
        assert_eq!(review::open(target).unwrap().project_id, next.project_id);
    }
}
