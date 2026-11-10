//! Durable admission, recovery, cancellation and retained evidence without activation.
use super::*;
use crate::review::preparation_tasks::TTL_MS;

fn fixture() -> (assert_fs::TempDir, Context, Runtime) {
    let (root, context) = crate::review::tests::enrolled();
    let runtime = Runtime::new(&context);
    (root, context, runtime)
}
pub(super) fn input(key: &str) -> Value {
    let mut value = serde_json::to_value(crate::review::tests::input(key)).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .retain(|_, value| !value.is_null());
    value
}

#[test]
fn preparation_tasks_admission_ignores_unknown_neighbors_and_reclaims_partial_records() {
    let (_root, _, runtime) = fixture();
    let store = runtime.store().unwrap();
    files::directory(&store.root, true).unwrap();
    std::fs::write(store.root.join(".DS_Store"), b"Acme unknown bytes").unwrap();
    let orphan = store.directory(&"a".repeat(64)).unwrap();
    files::directory(&orphan, true).unwrap();
    files::write(&orphan.join("input.json"), &input("acme-partial"), 524_288).unwrap();
    let (_record, _owner) = store
        .reserve(&input("acme-new"), &|| Ok(()))
        .unwrap()
        .unwrap();
    assert!(!orphan.exists());
    assert_eq!(store.entries().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(store.root.join(".DS_Store")).unwrap(),
        b"Acme unknown bytes"
    );
}

#[test]
fn preparation_tasks_concurrent_admission_never_exceeds_reserved_execution_slots() {
    let (_root, _, runtime) = fixture();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let (sent, received) = std::sync::mpsc::channel();
    let mut releases = Vec::new();
    let workers: Vec<_> = (0..3)
        .map(|index| {
            let store = runtime.store().unwrap().clone();
            let barrier = barrier.clone();
            let sent = sent.clone();
            let (release, waiting) = std::sync::mpsc::sync_channel(1);
            releases.push(release);
            std::thread::spawn(move || {
                barrier.wait();
                let reserved =
                    store.reserve(&input(&format!("acme-concurrent-{index}")), &|| Ok(()));
                // Every successful contender retains its lease until all attempts finish.
                sent.send(
                    reserved
                        .as_ref()
                        .map(Option::is_some)
                        .map_err(ToString::to_string),
                )
                .unwrap();
                waiting.recv_timeout(Duration::from_secs(5)).unwrap();
            })
        })
        .collect();
    barrier.wait();
    let results = (0..3)
        .map(|_| received.recv_timeout(Duration::from_secs(5)))
        .collect::<Vec<_>>();
    for release in releases {
        let _ = release.try_send(());
    }
    let joined = workers
        .into_iter()
        .map(std::thread::JoinHandle::join)
        .collect::<Vec<_>>();
    assert!(joined.into_iter().all(|result| result.is_ok()));
    let admitted = results
        .into_iter()
        .map(|result| usize::from(result.unwrap().unwrap()))
        .sum::<usize>();
    assert_eq!(admitted, 2);
}

#[test]
fn preparation_tasks_clock_regression_and_partial_publication_never_erase_live_results() {
    let (_root, context, runtime) = fixture();
    let store = runtime.store().unwrap();
    let (mut record, owner) = store
        .reserve(&input("acme-clock"), &|| Ok(()))
        .unwrap()
        .unwrap();
    drop(owner);
    record.created = clock::now_ms().unwrap() + 60_000;
    record.updated = record.created;
    store.save(&record).unwrap();
    assert!(store.get(&record.id).is_err());
    assert!(
        store
            .reserve(&input("acme-clock-next"), &|| Ok(()))
            .is_err()
    );
    assert!(store.directory(&record.id).unwrap().exists());
    let orphan = store.directory(&"a".repeat(64)).unwrap();
    files::directory(&orphan, true).unwrap();
    files::write(
        &orphan.join("input.json"),
        &input("acme-undispatched"),
        524_288,
    )
    .unwrap();
    let reopened = Runtime::new(&context);
    assert!(reopened.store().is_ok());
    assert!(!orphan.exists());
}

#[test]
fn preparation_tasks_reject_oversize_publication_and_changed_capture_without_authority() {
    let (root, context, runtime) = fixture();
    let record = runtime
        .start(
            &context,
            &input("acme-bounded"),
            &|| Ok(()),
            Box::new(|_, _| Ok(json!({"oversize":"x".repeat(RESULT_BYTES)}))),
        )
        .unwrap()
        .unwrap();
    runtime.join();
    assert_eq!(
        runtime.store().unwrap().get(&record.id).unwrap().status,
        "failed"
    );
    assert!(
        !runtime
            .store()
            .unwrap()
            .directory(&record.id)
            .unwrap()
            .join("result.json")
            .exists()
    );
    let good = runtime
        .start(
            &context,
            &input("acme-capture"),
            &|| Ok(()),
            Box::new(|_, data| Ok(data)),
        )
        .unwrap()
        .unwrap();
    runtime.join();
    let store = runtime.store().unwrap();
    let path = store.directory(&good.id).unwrap().join("capture.json");
    files::write(&path, &json!({"changed":true}), CAPTURE_BYTES).unwrap();
    assert!(store.capture(&good.id).is_err());
    assert!(
        storage::Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .proposals
            .is_empty()
    );
}

#[test]
fn preparation_tasks_cancellation_remains_responsive_while_a_worker_is_finishing() {
    let (_root, context, runtime) = fixture();
    let (started, reached) = std::sync::mpsc::sync_channel(1);
    let (release, waiting) = std::sync::mpsc::sync_channel(1);
    let waiting = std::sync::Mutex::new(waiting);
    let record = runtime
        .start(
            &context,
            &input("acme-finishing"),
            &|| Ok(()),
            Box::new(move |_, data| {
                started.send(()).unwrap();
                waiting
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                Ok(data)
            }),
        )
        .unwrap()
        .unwrap();
    reached
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let store = runtime.store().unwrap();
    assert_eq!(store.get(&record.id).unwrap().status, "working");
    store.cancel(&record.id).unwrap();
    assert!(store.get(&record.id).unwrap().cancel_requested);
    release.send(()).unwrap();
    runtime.join();
    assert_eq!(store.get(&record.id).unwrap().status, "cancelled");
}

#[test]
fn preparation_tasks_recover_interrupted_owners_without_replaying_or_creating_proposals() {
    let (root, context, runtime) = fixture();
    let store = runtime.store().unwrap();
    let (record, owner) = store
        .reserve(&input("acme-interrupted"), &|| Ok(()))
        .unwrap()
        .unwrap();
    let observer = Runtime::new(&context);
    assert_eq!(
        Runtime::new(&context)
            .store()
            .unwrap()
            .get(&record.id)
            .unwrap()
            .status,
        "working"
    );
    drop(owner);
    let record_path = store.directory(&record.id).unwrap().join("record.json");
    let recorded = std::fs::read(&record_path).unwrap();
    assert_eq!(
        observer
            .observation_store()
            .unwrap()
            .observe(&record.id)
            .unwrap()
            .status,
        "failed"
    );
    assert_eq!(std::fs::read(&record_path).unwrap(), recorded);
    assert_eq!(store.get(&record.id).unwrap().status, "working");
    // Actual mutating access still persists recovery after prior observation.
    assert_eq!(
        observer.store().unwrap().get(&record.id).unwrap().status,
        "failed"
    );
    let reopened = Runtime::new(&context);
    let failed = reopened.store().unwrap().get(&record.id).unwrap();
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.id, record.id);
    assert!(
        storage::Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .proposals
            .is_empty()
    );
    let (retry, owner) = reopened
        .store()
        .unwrap()
        .reserve(&input("acme-interrupted"), &|| Ok(()))
        .unwrap()
        .unwrap();
    assert_eq!(retry.id, record.id);
    assert!(owner.is_none());
}

#[test]
fn preparation_task_observation_never_repairs_missing_mutex_or_stale_epoch() {
    let (root, context, runtime) = fixture();
    let store = runtime.store().unwrap();
    let (record, owner) = store
        .reserve(&input("acme-observe"), &|| Ok(()))
        .unwrap()
        .unwrap();
    drop(owner);
    let owner_mutex = store.directory(&record.id).unwrap().join(".objects.lock");
    std::fs::remove_file(&owner_mutex).unwrap();
    let record_path = store.directory(&record.id).unwrap().join("record.json");
    let recorded = std::fs::read(&record_path).unwrap();
    let observer = Runtime::new(&context).observation_store().unwrap();
    let recorded_status = observer.observe(&record.id).unwrap();
    assert_eq!(recorded_status.status, "working");
    assert!(recorded_status.message.contains("unchecked"));
    assert!(!owner_mutex.exists());
    assert_eq!(std::fs::read(&record_path).unwrap(), recorded);
    let checkpoint_path = root.path().join(".shield/review/state.json");
    let mut checkpoint: Value =
        serde_json::from_slice(&std::fs::read(&checkpoint_path).unwrap()).unwrap();
    checkpoint["operator"]
        .as_object_mut()
        .unwrap()
        .remove("runtime_incarnation");
    let bytes = serde_json::to_vec(&checkpoint).unwrap();
    std::fs::write(&checkpoint_path, &bytes).unwrap();
    assert!(observer.observe(&record.id).is_err());
    assert_eq!(std::fs::read(&checkpoint_path).unwrap(), bytes);
    assert_eq!(std::fs::read(&record_path).unwrap(), recorded);
    assert!(!owner_mutex.exists());
}

#[test]
fn preparation_tasks_cancel_intent_wins_late_completion_and_terminal_rewrites_refuse() {
    let (root, _, runtime) = fixture();
    let store = runtime.store().unwrap();
    let (record, _owner) = store
        .reserve(&input("acme-cancel"), &|| Ok(()))
        .unwrap()
        .unwrap();
    store.cancel(&record.id).unwrap();
    assert!(store.get(&record.id).unwrap().cancel_requested);
    store
        .settle(
            &record.id,
            "completed",
            "late completion",
            Some(("a".repeat(64), "b".repeat(64))),
        )
        .unwrap();
    assert_eq!(store.get(&record.id).unwrap().status, "cancelled");
    let path = store.directory(&record.id).unwrap().join("record.json");
    let before = std::fs::read(&path).unwrap();
    store
        .settle(&record.id, "failed", "late failure", None)
        .unwrap();
    store.cancel(&record.id).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(
        storage::Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .proposals
            .is_empty()
    );
}

#[test]
fn preparation_tasks_reserve_execution_and_record_capacity_before_acceptance() {
    let (_root, _, runtime) = fixture();
    let store = runtime.store().unwrap();
    let (first, one) = store
        .reserve(&input("acme-one"), &|| Ok(()))
        .unwrap()
        .unwrap();
    let (second, two) = store
        .reserve(&input("acme-two"), &|| Ok(()))
        .unwrap()
        .unwrap();
    assert!(
        store
            .reserve(&input("acme-third"), &|| Ok(()))
            .unwrap()
            .is_none()
    );
    assert_eq!(store.entries().unwrap().len(), 2);
    store
        .settle(&first.id, "failed", "fixture completion", None)
        .unwrap();
    store
        .settle(&second.id, "failed", "fixture completion", None)
        .unwrap();
    drop((one, two));
    for index in 2..MAX_RECORDS {
        let (record, owner) = store
            .reserve(&input(&format!("acme-{index}")), &|| Ok(()))
            .unwrap()
            .unwrap();
        store
            .settle(&record.id, "failed", "fixture completion", None)
            .unwrap();
        drop(owner);
    }
    assert!(
        store
            .reserve(&input("acme-full"), &|| Ok(()))
            .unwrap()
            .is_none()
    );
    assert_eq!(store.entries().unwrap().len(), MAX_RECORDS);
}

#[test]
fn preparation_tasks_idempotency_expiry_and_cancelled_creation_keep_exact_records() {
    let (_root, _, runtime) = fixture();
    let store = runtime.store().unwrap();
    assert!(
        store
            .reserve(&input("acme-no-commit"), &|| Err(
                crate::policy::Error::Interrupted
            ))
            .is_err()
    );
    let args = input("acme-dedup");
    let (record, owner) = store.reserve(&args, &|| Ok(())).unwrap().unwrap();
    let (same, duplicate) = store.reserve(&args, &|| Ok(())).unwrap().unwrap();
    assert_eq!(same.id, record.id);
    assert!(duplicate.is_none());
    let mut changed = args;
    changed["agent_reason"] = json!("Changed intent");
    assert!(store.reserve(&changed, &|| Ok(())).is_err());
    drop(owner);
    let mut expired = record;
    expired.created = clock::now_ms().unwrap() - TTL_MS - 1;
    expired.updated = expired.created;
    store.save(&expired).unwrap();
    assert!(matches!(store.get(&expired.id), Err(Error::Missing)));
    store.reserve(&input("acme-next"), &|| Ok(())).unwrap();
    assert!(!store.directory(&expired.id).unwrap().exists());
}

#[test]
fn preparation_tasks_worker_finishes_private_capture_without_creating_live_authority() {
    let (root, context, runtime) = fixture();
    let before = storage::Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap()
        .policy_revision;
    let record = runtime
        .start(
            &context,
            &input("acme-worker"),
            &|| Ok(()),
            Box::new(|_, value| Ok(value)),
        )
        .unwrap()
        .unwrap();
    runtime.join();
    let store = runtime.store().unwrap();
    let completed = store.get(&record.id).unwrap();
    assert_eq!(completed.status, "completed", "{}", completed.message);
    assert_eq!(
        store.result(&completed).unwrap()["data"]["preparation_task_id"],
        record.id
    );
    assert!(store.capture(&record.id).is_ok());
    let state = storage::Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    assert_eq!(state.policy_revision, before);
    assert!(state.proposals.is_empty());
    let reopened = Runtime::new(&context);
    let checkpoint_path = root.path().join(".shield/review/state.json");
    let checkpoint = std::fs::read(&checkpoint_path).unwrap();
    let record_path = store.directory(&record.id).unwrap().join("record.json");
    let recorded = std::fs::read(&record_path).unwrap();
    let observed = reopened.observation_store().unwrap();
    let observed_record = observed.observe(&record.id).unwrap();
    assert!(observed.result(&observed_record).is_ok());
    assert_eq!(std::fs::read(&checkpoint_path).unwrap(), checkpoint);
    assert_eq!(std::fs::read(&record_path).unwrap(), recorded);
    assert_eq!(
        reopened.store().unwrap().get(&record.id).unwrap().status,
        "completed"
    );
}

#[cfg(unix)]
#[test]
fn copied_task_directories_are_repaired_only_by_owned_writers() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let (_original, _, original) = fixture();
    let source = original.store().unwrap();
    let (old, owner) = source
        .reserve(&input("acme-before-copy"), &|| Ok(()))
        .unwrap()
        .unwrap();
    drop(owner);
    let (_copied, current, runtime) = fixture();
    let target = runtime.observation_store().unwrap();
    copy(&source.root, &target.root);
    let old_directory = target.directory(&old.id).unwrap();
    let neighbor = target.root.join("acme-user-neighbor");
    std::fs::create_dir(&neighbor).unwrap();
    std::fs::write(neighbor.join("keep.txt"), b"Acme untouched").unwrap();
    let outside = assert_fs::TempDir::new().unwrap();
    std::fs::set_permissions(outside.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let link = target.root.join("e".repeat(64));
    symlink(outside.path(), &link).unwrap();
    let record_path = old_directory.join("record.json");
    let recorded = std::fs::read(&record_path).unwrap();
    for path in [&target.root, &old_directory, &neighbor] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert!(target.observe(&old.id).is_err());
    assert_eq!(std::fs::read(&record_path).unwrap(), recorded);
    for path in [&target.root, &old_directory, &neighbor] {
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    let writer = runtime.store().unwrap();
    assert_eq!(
        std::fs::metadata(&writer.root)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&old_directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let (fresh, _owner) = writer
        .reserve(&input("acme-after-copy"), &|| Ok(()))
        .unwrap()
        .unwrap();
    assert_eq!(fresh.project, current.project_id);
    assert!(writer.observe(&old.id).is_err());
    assert!(!old_directory.exists());
    assert_eq!(
        std::fs::metadata(&neighbor).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(
        std::fs::read(neighbor.join("keep.txt")).unwrap(),
        b"Acme untouched"
    );
    assert!(
        std::fs::symlink_metadata(link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::metadata(outside.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}

#[cfg(unix)]
#[test]
fn preparation_tasks_refuse_linked_records_without_touching_foreign_data() {
    let (_root, _, runtime) = fixture();
    let store = runtime.store().unwrap();
    let (record, owner) = store
        .reserve(&input("acme-link"), &|| Ok(()))
        .unwrap()
        .unwrap();
    drop(owner);
    let foreign = assert_fs::TempDir::new().unwrap();
    let target = foreign.path().join("foreign.json");
    std::fs::write(&target, b"Acme private bytes").unwrap();
    let path = store.directory(&record.id).unwrap().join("record.json");
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(store.get(&record.id).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"Acme private bytes");
}
