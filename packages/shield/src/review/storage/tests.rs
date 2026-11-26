//! Deterministic owned-process crashes at the actual store publication boundaries.
use super::*;
use crate::review::tests::enrolled;
use std::{
    cell::Cell,
    process::{Child, Command, Stdio},
    time::Instant,
};

#[test]
#[cfg(any(unix, windows))]
fn authority_lock_recovers_dead_legacy_writers_but_preserves_live_and_unknown_owners() {
    let (root, _) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let marker = store.directory.join("state.lock");
    for contents in [
        format!("{}\n", std::process::id()),
        String::new(),
        "unknown\n".into(),
    ] {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        let mut file = options.open(&marker).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        drop(file);
        assert!(matches!(
            locking::try_acquire(&store.directory),
            Err(Error::Locked)
        ));
        assert_eq!(fs::read_to_string(&marker).unwrap(), contents);
        fs::remove_file(&marker).unwrap();
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    let mut file = options.open(&marker).unwrap();
    writeln!(file, "{}", super::exited_pid()).unwrap();
    drop(file);
    store.with_interaction_lock(|| Ok(())).unwrap();
    assert!(!marker.exists());
    assert!(store.directory.join("writer.lock").is_file());
}

#[test]
fn authority_lock_serializes_contenders_and_excludes_legacy_writers() {
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    let (root, _) = enrolled();
    let store = Store::open(root.path()).unwrap();
    store
        .with_interaction_lock(|| {
            assert!(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(store.directory.join("state.lock"))
                    .is_err()
            );
            Ok(())
        })
        .unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let inside = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    thread::scope(|scope| {
        for _ in 0..4 {
            let (barrier, inside, completed) = (barrier.clone(), inside.clone(), completed.clone());
            let root = root.path();
            scope.spawn(move || {
                barrier.wait();
                let store = Store::open(root).unwrap();
                for _ in 0..4 {
                    store
                        .with_interaction_lock(|| {
                            assert_eq!(inside.fetch_add(1, Ordering::SeqCst), 0);
                            thread::sleep(Duration::from_millis(5));
                            assert_eq!(inside.fetch_sub(1, Ordering::SeqCst), 1);
                            completed.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        })
                        .unwrap();
                }
            });
        }
    });
    assert_eq!(completed.load(Ordering::SeqCst), 16);
    assert!(!store.directory.join("state.lock").exists());
}

#[test]
fn authority_lock_queues_in_arrival_order_and_skips_cancelled_waiters() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    let (root, _) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let cancelled = AtomicBool::new(false);
    let (finished, order) = mpsc::channel();
    thread::scope(|scope| {
        let holder = store.lock(&|| Ok(())).unwrap();
        for position in 0..4 {
            let finished = finished.clone();
            let cancelled = &cancelled;
            let directory = &store.directory;
            scope.spawn(move || {
                let lock = locking::acquire(directory, &|| {
                    if position == 1 && cancelled.load(Ordering::Acquire) {
                        return Err(crate::policy::Error::Interrupted);
                    }
                    Ok(())
                });
                if position == 1 {
                    assert!(matches!(
                        lock,
                        Err(Error::Policy(crate::policy::Error::Interrupted))
                    ));
                } else {
                    let lock = lock.unwrap();
                    finished.send(position).unwrap();
                    lock.release().unwrap();
                }
            });
            let ticket = store
                .directory
                .join("writer-queue")
                .join(position.to_string());
            let deadline = Instant::now() + Duration::from_secs(5);
            while !ticket.is_file() {
                assert!(Instant::now() < deadline, "writer {position} never queued");
                thread::sleep(Duration::from_millis(5));
            }
        }
        cancelled.store(true, Ordering::Release);
        holder.release().unwrap();
        for position in [0, 2, 3] {
            assert_eq!(
                order.recv_timeout(Duration::from_secs(5)).unwrap(),
                position
            );
        }
    });
    store.with_interaction_lock(|| Ok(())).unwrap();
    assert!(!store.directory.join("state.lock").exists());
}

#[test]
fn authority_lock_cancellation_and_unwind_release_ownership() {
    let (root, _) = enrolled();
    let store = Store::open(root.path()).unwrap();
    store
        .with_interaction_lock(|| {
            assert!(matches!(
                store.lock(&|| Err(crate::policy::Error::Interrupted)),
                Err(Error::Policy(crate::policy::Error::Interrupted))
            ));
            assert!(matches!(
                store.with_interaction_lock(|| Ok(())),
                Err(Error::Locked)
            ));
            Ok(())
        })
        .unwrap();
    assert!(
        std::panic::catch_unwind(|| {
            store.with_interaction_lock(|| -> Result<(), Error> { panic!("owned writer panic") })
        })
        .is_err()
    );
    store.with_interaction_lock(|| Ok(())).unwrap();
    assert!(!store.directory.join("state.lock").exists());
}

#[cfg(unix)]
#[test]
fn authority_admission_contention_defers_migration_and_checks_cancellation() {
    let (root, _) = enrolled();
    let earlier = root.path().join(".shield/review");
    fs::rename(root.path().join(".shield/state"), &earlier).unwrap();
    locking::try_acquire(&earlier).unwrap().release().unwrap();
    let admission = OpenOptions::new()
        .read(true)
        .write(true)
        .open(earlier.join("writer-queue/admission.lock"))
        .unwrap();
    admission.lock().unwrap();
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    thread::scope(|scope| {
        let project = root.path();
        let earlier = &earlier;
        scope.spawn(move || {
            let store = Store::open(project).unwrap();
            let deferred = store.directory == earlier.canonicalize().unwrap();
            let checks = Cell::new(0);
            let waiting = locking::acquire(earlier, &|| {
                checks.set(checks.get() + 1);
                if checks.get() >= 3 {
                    Err(crate::policy::Error::Interrupted)
                } else {
                    Ok(())
                }
            });
            let cancelled = matches!(
                waiting,
                Err(Error::Policy(crate::policy::Error::Interrupted))
            );
            let _ = send.send((store, deferred, cancelled));
        });
        let outcome = receive.recv_timeout(Duration::from_secs(5));
        drop(admission);
        let (store, deferred, cancelled) = outcome.unwrap();
        assert!(deferred && cancelled);
        assert_eq!(Store::open(root.path()).unwrap().directory, store.directory);
        assert!(store.read_checkpoint().is_ok());
        drop(store);
    });
    assert!(
        Store::open(root.path())
            .unwrap()
            .directory
            .ends_with(".shield/state")
    );
}

#[cfg(unix)]
#[test]
fn authority_lock_refuses_linked_or_nonprivate_kernel_files() {
    use std::os::unix::fs::PermissionsExt;
    let (root, _) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let path = store.directory.join("writer.lock");
    let unrelated = root.path().join("unrelated");
    fs::write(&unrelated, "preserve me").unwrap();
    // Enrollment already created the persistent lock. This fixture has no
    // concurrent owner; replace its namespace only to exercise unsafe paths.
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&unrelated, &path).unwrap();
    assert!(store.with_interaction_lock(|| Ok(())).is_err());
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "preserve me");
    fs::remove_file(&path).unwrap();
    fs::hard_link(&unrelated, &path).unwrap();
    assert!(store.with_interaction_lock(|| Ok(())).is_err());
    fs::remove_file(&path).unwrap();
    fs::write(&path, "").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.with_interaction_lock(|| Ok(())).is_err());
    assert!(!store.directory.join("state.lock").exists());
}

#[test]
fn authority_lock_preserves_unknown_marker_contents() {
    let (root, _) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let marker = store.directory.join("state.lock");
    fs::create_dir(&marker).unwrap();
    fs::write(marker.join("unknown"), "preserve me").unwrap();
    assert!(store.with_interaction_lock(|| Ok(())).is_err());
    assert_eq!(
        fs::read_to_string(marker.join("unknown")).unwrap(),
        "preserve me"
    );
}

#[test]
#[cfg(any(unix, windows))]
fn authority_lock_recovers_early_crashes_and_dead_legacy_processes() {
    for mode in ["legacy", "kernel"] {
        let (root, _) = enrolled();
        let store = Store::open(root.path()).unwrap();
        let marker = store.directory.join("state.lock");
        let before = fs::read(store.directory.join("state.json")).unwrap();
        let kernel = fs::File::open(store.directory.join("writer.lock")).unwrap();
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "review::storage::tests::authority_lock_writer_process",
                    "--ignored",
                    "--nocapture",
                ])
                .env("SHIELD_TEST_LOCK_ROOT", root.path())
                .env("SHIELD_TEST_LOCK_MODE", mode)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while fs::read_to_string(root.path().join("ready"))
            .ok()
            .as_deref()
            != Some(mode)
        {
            assert!(child.0.try_wait().unwrap().is_none(), "lock writer exited");
            assert!(Instant::now() < deadline, "lock writer never became ready");
            thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(
            locking::try_acquire(&store.directory),
            Err(Error::Locked)
        ));
        if mode == "legacy" {
            assert_eq!(
                fs::read_to_string(&marker).unwrap(),
                format!("{}\n", child.0.id())
            );
        } else {
            assert!(marker.is_dir());
        }
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        store.with_interaction_lock(|| Ok(())).unwrap();
        assert!(!marker.exists());
        assert_eq!(
            fs::read(store.directory.join("state.json")).unwrap(),
            before
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let held = kernel.metadata().unwrap();
            let current = fs::metadata(store.directory.join("writer.lock")).unwrap();
            assert_eq!((held.dev(), held.ino()), (current.dev(), current.ino()));
        }
        #[cfg(windows)]
        {
            let held = winapi_util::file::information(&kernel).unwrap();
            let current = fs::File::open(store.directory.join("writer.lock")).unwrap();
            let current = winapi_util::file::information(&current).unwrap();
            assert_eq!(
                (held.volume_serial_number(), held.file_index()),
                (current.volume_serial_number(), current.file_index())
            );
        }
    }
}

#[test]
#[ignore = "owned lock writer invoked by authority_lock_recovers_early_crashes_and_dead_legacy_processes"]
fn authority_lock_writer_process() {
    let root = std::env::var("SHIELD_TEST_LOCK_ROOT").unwrap();
    let mode = std::env::var("SHIELD_TEST_LOCK_MODE").unwrap();
    let store = Store::open(Path::new(&root)).unwrap();
    let _lock = if mode == "legacy" {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        let mut marker = options.open(store.directory.join("state.lock")).unwrap();
        writeln!(marker, "{}", std::process::id()).unwrap();
        drop(marker);
        None
    } else {
        Some(store.lock(&|| Ok(())).unwrap())
    };
    fs::write(Path::new(&root).join("ready"), mode).unwrap();
    loop {
        thread::park();
    }
}

#[test]
fn optional_session_capture_skips_unchanged_publication() {
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let before = fs::read(root.path().join(".shield/state/state.json")).unwrap();
    OBSERVER.with(|slot| slot.set(Some(|stage| panic!("unexpected publication: {stage:?}"))));
    let result = store.update_checkpoint_if_changed(&context, &|| Ok(()), |_| Ok(false));
    OBSERVER.with(|slot| slot.set(None));
    result.unwrap();
    assert_eq!(
        fs::read(root.path().join(".shield/state/state.json")).unwrap(),
        before
    );
}

#[test]
fn updates_restore_state_ignores_and_preserve_the_config_ignore_choice() {
    let (root, context) = enrolled();
    let path = root.path().join(".shield/.gitignore");
    // Keep the operator's choice to track the local configuration while adding state ignores.
    fs::write(&path, "review/").unwrap();
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |_| Ok(serde_json::Value::Null))
        .unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "review/\nstate/\ncache/\nrules.local.yml\nshield.local.lock\n"
    );
}

#[cfg(windows)]
#[test]
fn windows_review_state_refuses_junction_directories() {
    for nested in [None, Some("state"), Some("review")] {
        let project = assert_fs::TempDir::new().unwrap();
        let outside = assert_fs::TempDir::new().unwrap();
        let shield = project.path().join(".shield");
        let path = if let Some(name) = nested {
            fs::create_dir(&shield).unwrap();
            shield.join(name)
        } else {
            shield
        };
        crate::test_support::junction(&path, outside.path()).unwrap();
        assert!(
            Store::location(project.path()).is_err(),
            "junction accepted: {path:?}"
        );
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }
}

fn names(directory: &Path) -> Vec<String> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect()
}

#[cfg(windows)]
#[test]
fn windows_replacement_refuses_nul_without_truncating_the_target() {
    let root = assert_fs::TempDir::new().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::write(&source, "Acme replacement").unwrap();
    fs::write(&target, "Acme original").unwrap();
    assert!(super::replace(&source, &root.path().join("target\0suffix")).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "Acme original");
    super::replace(&source, &target).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "Acme replacement");
    assert!(!source.exists());
}

#[test]
fn a_busy_history_lock_without_observations_is_not_a_history_failure() {
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let guard = history::lock(&store).unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |_| Ok(Value::Null))
        .unwrap();
    drop(guard);
    let review = root.path().join(".shield/state");
    assert!(!names(&review).iter().any(|name| {
        Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("error"))
    }));
    assert!(store.read().unwrap().history_available);
}

#[cfg(any(unix, windows))]
#[test]
fn writes_remove_leftover_temporaries_that_no_writer_owns() {
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let review = root.path().join(".shield/state");
    let dead = super::exited_pid();
    // The writer lock serializes state writers, so every state leftover goes.
    let removed = [
        review.join(format!(".state-{dead}-1.tmp")),
        review.join(".state-1-1.tmp"),
    ];
    let kept = [review.join(".state-acme.tmp"), review.join("acme.tmp")];
    for path in removed.iter().chain(&kept) {
        fs::write(path, b"partial").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        #[cfg(windows)]
        crate::windows_security::make_private(path).unwrap();
    }
    store
        .update_checkpoint(&context, &|| Ok(()), |_| Ok(Value::Null))
        .unwrap();
    for path in &removed {
        assert!(!path.exists(), "{}", path.display());
    }
    for path in &kept {
        assert!(path.exists(), "{}", path.display());
    }
}

#[test]
fn incomplete_state_and_proposal_records_are_refused_without_mutation() {
    let (root, context) = enrolled();
    crate::review::tests::propose(&context, &crate::review::tests::input("read"), &|| Ok(()))
        .unwrap();
    let store = Store::open(root.path()).unwrap();
    let path = root.path().join(".shield/state/state.json");
    let original = fs::read(&path).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    for (proposal, fields) in [
        (
            false,
            &[
                "operator",
                "registry_checkpoints",
                "read_requests",
                "run_requests",
                "history_generation",
            ][..],
        ),
        (
            true,
            &["base_metadata", "candidate_metadata", "receipt", "operator"][..],
        ),
    ] {
        for &field in fields {
            let mut incomplete = value.clone();
            let record = if proposal {
                &mut incomplete["proposals"][0]
            } else {
                &mut incomplete
            };
            record.as_object_mut().unwrap().remove(field);
            let bytes = serde_json::to_vec(&incomplete).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(store.read().is_err(), "missing {field}");
            assert_eq!(fs::read(&path).unwrap(), bytes, "missing {field}");
        }
    }
    fs::write(&path, &original).unwrap();
    assert!(store.read().is_ok());
    let mut missing_generation = value.clone();
    missing_generation["history_generation"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<State>(missing_generation).is_err());
    for field in ["files", "base_metadata", "candidate_metadata"] {
        let mut incomplete = value.clone();
        incomplete["proposals"][0][field] = serde_json::Value::Null;
        let bytes = serde_json::to_vec(&incomplete).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(store.read().is_err(), "null proposal {field}");
        assert_eq!(fs::read(&path).unwrap(), bytes, "null proposal {field}");
    }
    // The texts of a proposal always include the configuration it writes.
    let mut without_config = value.clone();
    without_config["proposals"][0]["files"]
        .as_object_mut()
        .unwrap()
        .remove("config.yml");
    let bytes = serde_json::to_vec(&without_config).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(store.read().is_err(), "proposal files without config.yml");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let mut null_operator = value;
    null_operator["operator"] = serde_json::Value::Null;
    let bytes = serde_json::to_vec(&null_operator).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(store.read().is_err(), "null operator metadata");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::write(&path, &original).unwrap();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    UpgradePrepared,
    Created,
    Written,
    Synced,
    Renamed,
    Complete,
}
impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::UpgradePrepared => "upgrade-prepared",
            Self::Created => "created",
            Self::Written => "written",
            Self::Synced => "synced",
            Self::Renamed => "renamed",
            Self::Complete => "complete",
        }
    }
}
thread_local! {
    static OBSERVER: Cell<Option<fn(Stage)>> = const { Cell::new(None) };
}
pub(super) fn observe(stage: Stage) {
    if let Some(observer) = OBSERVER.with(Cell::get) {
        observer(stage);
    }
}
pub(super) fn set_observer(observer: Option<fn(Stage)>) {
    OBSERVER.with(|slot| slot.set(observer));
}

fn barrier(stage: Stage) {
    if std::env::var("SHIELD_TEST_PUBLICATION_STAGE").unwrap() == stage.name() {
        let root = std::env::var("SHIELD_TEST_PUBLICATION_ROOT").unwrap();
        fs::write(Path::new(&root).join("ready"), stage.name()).unwrap();
        loop {
            thread::park();
        }
    }
}

#[test]
#[ignore = "subprocess fixture invoked explicitly by publication_crashes_keep_complete_checkpoints"]
fn crash_writer_process() {
    let root = std::env::var("SHIELD_TEST_PUBLICATION_ROOT").unwrap();
    let context = crate::review::open(Path::new(&root)).unwrap();
    #[cfg(target_os = "macos")]
    if std::env::var_os("SHIELD_TEST_CONSUMPTION").is_some() {
        crash_consumption(&context);
        return;
    }
    OBSERVER.with(|slot| slot.set(Some(barrier)));
    mark(&context).unwrap();
    panic!("writer did not reach the requested publication barrier");
}

/// One ordinary checkpoint change: a marker in the operator metadata.
fn mark(context: &Context) -> Result<Value, Error> {
    Store::open(Path::new(&context.project_root))?.update_checkpoint(context, &|| Ok(()), |state| {
        state.operator["acme_crash_marker"] = json!(true);
        Ok(Value::Null)
    })
}

/// The committed checkpoint's marker, read without the writer lock.
fn marked(root: &Path) -> bool {
    let bytes = fs::read(root.join(".shield/state/state.json")).unwrap();
    serde_json::from_slice::<Value>(&bytes).unwrap()["operator"]["acme_crash_marker"] == true
}

#[cfg(target_os = "macos")]
fn crash_consumption(operator: &Context) {
    let row = json!(
        Store::open(Path::new(&operator.project_root))
            .unwrap()
            .read()
            .unwrap()
            .run_requests
    )[0]
    .clone();
    let mut context = operator.clone();
    context.surface = "run".into();
    let request = serde_json::from_value(row["request"].clone()).unwrap();
    let snapshot = crate::runtime::Project::open(Path::new(&context.project_root))
        .unwrap()
        .snapshot()
        .unwrap();
    let evidence =
        crate::execution::evaluate_approval(&snapshot, &request, &row["resource"]).unwrap();
    OBSERVER.with(|slot| slot.set(Some(barrier)));
    crate::review::run_approval::gate(
        &context,
        &evidence,
        &row["resource"],
        row["request_id"].as_str(),
        || true,
        |decision| {
            assert_eq!(decision["allowed"], true);
            fs::write(Path::new(&context.project_root).join("launched"), b"effect").unwrap();
        },
    )
    .unwrap();
    panic!("consumption did not reach publication barrier");
}

#[cfg(target_os = "macos")]
#[test]
fn publication_crashes_never_launch_before_durable_consumption() {
    for stage in [
        Stage::Created,
        Stage::Written,
        Stage::Synced,
        Stage::Renamed,
        Stage::Complete,
    ] {
        let (root, operator) = enrolled();
        let mut context = operator.clone();
        context.surface = "run".into();
        // Match the child harness's intentionally absent origin attribution.
        context.agent = None;
        context.session_id = None;
        let request = crate::policy::Request {
            executable: "/bin/echo".into(),
            arguments: vec!["Acme".into()],
            cwd: context.project_root.clone(),
            project: context.project_root.clone(),
        };
        let resource = crate::execution::approval_fixture(&request, 5000).unwrap();
        let snapshot = crate::runtime::Project::open(root.path())
            .unwrap()
            .snapshot()
            .unwrap();
        let evidence = crate::execution::evaluate_approval(&snapshot, &request, &resource).unwrap();
        let decision = crate::review::run_approval::gate(
            &context,
            &evidence,
            &resource,
            None,
            || true,
            |_| (),
        )
        .unwrap()
        .0;
        let id = decision["approval"]["request_id"].as_str().unwrap();
        crate::review::run_approval::authorize(&operator, id, 300, None, false).unwrap();
        let mut child = consumption_writer_at(root.path(), stage);
        assert!(matches!(
            locking::try_acquire(&Store::open(root.path()).unwrap().directory),
            Err(Error::Locked)
        ));
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(!root.path().join("launched").exists());
        let store = Store::open(root.path()).unwrap();
        let state = store.read().unwrap();
        let committed = matches!(stage, Stage::Renamed | Stage::Complete);
        assert_eq!(
            json!(state.run_requests)[0]["status"],
            if committed { "consumed" } else { "approved" }
        );
        assert_eq!(
            state
                .authorization_events
                .iter()
                .filter(|e| e["kind"] == "run_grant_consumed")
                .count(),
            usize::from(committed)
        );
        // Retrying after the owned writer exits needs no lock-file cleanup.
        // The grant was claimed in the per-user ledger before the crash, so it
        // stays spent even where the checkpoint did not record it: a crash
        // never makes a one-time grant usable twice.
        let retry = crate::review::run_approval::gate(
            &context,
            &evidence,
            &resource,
            Some(id),
            || true,
            |_| (),
        )
        .unwrap()
        .0;
        assert_eq!(retry["allowed"], false);
    }
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[cfg(target_os = "macos")]
fn consumption_writer_at(root: &Path, stage: Stage) -> OwnedChild {
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "review::storage::tests::crash_writer_process",
                "--ignored",
                "--nocapture",
            ])
            .env("SHIELD_TEST_PUBLICATION_STAGE", stage.name())
            .env("SHIELD_TEST_PUBLICATION_ROOT", root)
            .env("SHIELD_TEST_CONSUMPTION", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::read_to_string(root.join("ready")).ok().as_deref() != Some(stage.name()) {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "child exited before {stage:?}"
        );
        assert!(Instant::now() < deadline, "barrier deadline at {stage:?}");
        thread::sleep(Duration::from_millis(10));
    }
    child
}

#[test]
fn publication_crashes_keep_complete_checkpoints() {
    for stage in [
        Stage::Created,
        Stage::Written,
        Stage::Synced,
        Stage::Renamed,
        Stage::Complete,
    ] {
        let (root, context) = enrolled();
        let state_path = root.path().join(".shield/state/state.json");
        let before = fs::read(&state_path).unwrap();
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "review::storage::tests::crash_writer_process",
                    "--ignored",
                    "--nocapture",
                ])
                .env("SHIELD_TEST_PUBLICATION_STAGE", stage.name())
                .env("SHIELD_TEST_PUBLICATION_ROOT", root.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if fs::read_to_string(root.path().join("ready"))
                .ok()
                .as_deref()
                == Some(stage.name())
            {
                break;
            }
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "writer exited before {stage:?}"
            );
            assert!(Instant::now() < deadline, "writer did not reach {stage:?}");
            thread::sleep(Duration::from_millis(10));
        }
        let committed = matches!(stage, Stage::Renamed | Stage::Complete);
        assert_eq!(marked(root.path()), committed, "visibility at {stage:?}");
        assert!(matches!(
            locking::try_acquire(&Store::open(root.path()).unwrap().directory),
            Err(Error::Locked)
        ));
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        assert_eq!(marked(root.path()), committed);
        if !committed {
            assert_eq!(fs::read(&state_path).unwrap(), before);
        }
        // The kernel releases the writer's ownership. Recovery is an ordinary
        // retry, preserving the committed outcome.
        mark(&context).unwrap();
        assert!(marked(root.path()));
        assert!(
            !names(&root.path().join(".shield/state"))
                .iter()
                .any(|name| name.starts_with(".state-")),
            "killed writer's temporary remains after {stage:?}"
        );
        reconnect(&context, stage);
    }
}

/// A fresh MCP connection must start after the crash and read the policy
/// through its normal tools, without manual lock repair.
fn reconnect(context: &Context, stage: Stage) {
    let mut context = context.clone();
    context.surface = "mcp".into();
    context.connection_id = Some(format!("crash-reconnect-{stage:?}"));
    let connection = crate::review::mcp::Connection::enrolled(&context).unwrap();
    let status = crate::review::mcp::call(&context, &connection, "status", &json!({}), &|| Ok(()));
    assert!(status["error"].is_null(), "{status}");
}

/// The earlier state folder is renamed only by the holder of its writer lock:
/// while another writer holds it the folder keeps serving under its earlier
/// name. Once the writer and stores using the earlier path finish, the next
/// open converts it.
#[cfg(unix)]
#[test]
fn earlier_state_is_converted_only_under_its_writer_lock() {
    let (root, context) = enrolled();
    let earlier = root.path().join(".shield/review");
    fs::rename(root.path().join(".shield/state"), &earlier).unwrap();
    let held = locking::acquire(&earlier, &|| Ok(())).unwrap();
    let busy = Store::open(root.path()).unwrap();
    assert_eq!(busy.directory, earlier.canonicalize().unwrap());
    assert!(!root.path().join(".shield/state").exists());
    held.release().unwrap();
    busy.read_checkpoint().unwrap().check(&context).unwrap();
    drop(busy);

    let converted = Store::open(root.path()).unwrap();
    assert!(converted.directory.ends_with(".shield/state"));
    assert!(!earlier.exists());
    // The lock's marker left with the converting writer.
    assert!(!root.path().join(".shield/state/state.lock").exists());
    converted
        .read_checkpoint()
        .unwrap()
        .check(&context)
        .unwrap();

    // A link at the earlier name is refused, never renamed into place.
    let linked = assert_fs::TempDir::new().unwrap();
    let other = assert_fs::TempDir::new().unwrap();
    fs::create_dir(linked.path().join(".shield")).unwrap();
    std::os::unix::fs::symlink(other.path(), linked.path().join(".shield/review")).unwrap();
    assert!(Store::open(linked.path()).is_err());
    assert!(!linked.path().join(".shield/state").exists());
}
