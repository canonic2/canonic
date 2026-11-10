//! Deterministic owned-process crashes at the actual store publication boundaries.
use super::*;
use crate::review::{runtime, tests::enrolled};
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
            store.with_interaction_lock(|| Ok(())),
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
            store.with_interaction_lock(|| Ok(())),
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
fn current_policy_cache_reuse_preserves_source_integrity_and_read_only_fallback() {
    let (root, context) = enrolled();
    let status = runtime::status(&context).unwrap();
    runtime::publish_fixture(
        &context,
        status["review"]["policy_revision"].as_str().unwrap(),
        status["review"]["candidate_digest"].as_str().unwrap(),
        None,
        &|| Ok(()),
    )
    .unwrap();
    let status = runtime::status(&context).unwrap();
    let cache = &status["compiled_cache"];
    let path = std::path::PathBuf::from(cache["directory"].as_str().unwrap())
        .join("compiled")
        .join(cache["project_key"].as_str().unwrap())
        .join(format!("{}.bin", cache["plan_key"].as_str().unwrap()));
    let compiled = fs::read(&path).unwrap();
    let store = Store::open(root.path()).unwrap();
    let checkpoint = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    fs::write(&path, b"invalid derived plan").unwrap();
    let (_, evaluator) = store.read_current_evaluated().unwrap();
    assert_eq!(
        evaluator.revision(),
        status["review"]["policy_revision"].as_str().unwrap()
    );
    assert_eq!(fs::read(&path).unwrap(), b"invalid derived plan");
    assert_eq!(
        fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        checkpoint
    );
    fs::write(&path, compiled).unwrap();
    let state = store.read_checkpoint().unwrap();
    let source = &state.candidates["project"];
    fs::write(
        root.path().join(".shield/review/sources").join(format!(
            "{}.manifest.json",
            source["manifest_digest"].as_str().unwrap()
        )),
        b"changed source",
    )
    .unwrap();
    assert!(
        store.read_current_evaluated().is_err(),
        "a valid compiled artifact must not hide changed source bytes"
    );
}

#[test]
fn optional_session_capture_skips_unchanged_publication() {
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let before = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    OBSERVER.with(|slot| slot.set(Some(|stage| panic!("unexpected publication: {stage:?}"))));
    let result = store.update_checkpoint_if_changed(&context, &|| Ok(()), |_| Ok(false));
    OBSERVER.with(|slot| slot.set(None));
    result.unwrap();
    assert_eq!(
        fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        before
    );
}

#[test]
fn updates_append_missing_local_state_ignores_to_an_existing_file() {
    let (root, context) = enrolled();
    let path = root.path().join(".shield/.gitignore");
    fs::write(&path, "review/").unwrap();
    Store::open(root.path())
        .unwrap()
        .update(&context, &|| Ok(()), |_| Ok(serde_json::Value::Null))
        .unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "review/\ncache/\nconfig.local.yml\nrules.local.yml\nshield.local.lock\n"
    );
}

#[cfg(windows)]
#[test]
fn windows_review_state_refuses_junction_directories() {
    for nested in [false, true] {
        let project = assert_fs::TempDir::new().unwrap();
        let outside = assert_fs::TempDir::new().unwrap();
        let shield = project.path().join(".shield");
        let path = if nested {
            fs::create_dir(&shield).unwrap();
            shield.join("review")
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
    let review = root.path().join(".shield/review");
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
    let review = root.path().join(".shield/review");
    let sources = review.join("sources");
    fs::create_dir_all(&sources).unwrap();
    let dead = super::exited_pid();
    let digest = "0".repeat(64);
    // Windows' reserved PID zero cannot establish the exit of a writer;
    // Unix's init process remains live. Both entries must be retained.
    let unverified = i32::from(!cfg!(windows));
    // The writer lock serializes state writers, so every state leftover goes.
    let removed = [
        review.join(format!(".state-{dead}-1.tmp")),
        review.join(".state-1-1.tmp"),
        sources.join(format!("{digest}.{dead}-1.tmp")),
    ];
    let kept = [
        review.join(".state-acme.tmp"),
        review.join("acme.tmp"),
        sources.join(format!("{digest}.{unverified}-1.tmp")),
        sources.join(format!("{digest}.{}-1.tmp", std::process::id())),
        sources.join(format!("acme.{dead}-1.tmp")),
    ];
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
    sources::compact(
        &store,
        &mut json!(
            crate::configuration::Bundle::capture(
                "# Acme new source\nschema_version: 1\npacks: []\n",
                &[],
                None
            )
            .unwrap()
        ),
    )
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
    let path = root.path().join(".shield/review/state.json");
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
                "active",
            ][..],
        ),
        (
            true,
            &[
                "base_candidate",
                "base_metadata",
                "candidate_metadata",
                "receipt",
                "operator",
            ][..],
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
    for field in [
        "base_candidate",
        "candidate",
        "base_metadata",
        "candidate_metadata",
    ] {
        let mut incomplete = value.clone();
        incomplete["proposals"][0][field] = serde_json::Value::Null;
        let bytes = serde_json::to_vec(&incomplete).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(store.read().is_err(), "null proposal {field}");
        assert_eq!(fs::read(&path).unwrap(), bytes, "null proposal {field}");
    }
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
    RelocationPrepared,
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
            Self::RelocationPrepared => "relocation-prepared",
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
#[ignore = "subprocess fixture invoked explicitly by publication_crashes_keep_complete_authorized_snapshots"]
fn crash_writer_process() {
    let root = std::env::var("SHIELD_TEST_PUBLICATION_ROOT").unwrap();
    let context = crate::review::open(Path::new(&root)).unwrap();
    #[cfg(target_os = "macos")]
    if std::env::var_os("SHIELD_TEST_CONSUMPTION").is_some() {
        crash_consumption(&context);
        return;
    }
    let status = runtime::status(&context).unwrap();
    OBSERVER.with(|slot| slot.set(Some(barrier)));
    runtime::publish_fixture(
        &context,
        status["review"]["policy_revision"].as_str().unwrap(),
        status["review"]["candidate_digest"].as_str().unwrap(),
        status["active"]["activation_id"].as_str(),
        &|| Ok(()),
    )
    .unwrap();
    panic!("writer did not reach the requested publication barrier");
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
        let status = runtime::status(&operator).unwrap();
        runtime::publish_fixture(
            &operator,
            status["review"]["policy_revision"].as_str().unwrap(),
            status["review"]["candidate_digest"].as_str().unwrap(),
            None,
            &|| Ok(()),
        )
        .unwrap();
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
            Store::open(root.path())
                .unwrap()
                .with_interaction_lock(|| Ok(())),
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
        assert_eq!(retry["allowed"], !committed);
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
fn publication_crashes_keep_complete_authorized_snapshots() {
    for stage in [
        Stage::Created,
        Stage::Written,
        Stage::Synced,
        Stage::Renamed,
        Stage::Complete,
    ] {
        let (root, context) = enrolled();
        let status = runtime::status(&context).unwrap();
        let revision = status["review"]["policy_revision"].as_str().unwrap();
        let digest = status["review"]["candidate_digest"].as_str().unwrap();
        let initial =
            runtime::publish_fixture(&context, revision, digest, None, &|| Ok(())).unwrap();
        let previous = initial["activation_id"].as_str().unwrap();
        let state_path = root.path().join(".shield/review/state.json");
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
        let visible = runtime::status(&context).unwrap();
        let committed = matches!(stage, Stage::Renamed | Stage::Complete);
        assert_eq!(
            visible["active"] == initial,
            !committed,
            "visibility at {stage:?}"
        );
        assert!(matches!(
            runtime::publish_fixture(&context, revision, digest, Some(previous), &|| Ok(())),
            Err(Error::Locked)
        ));
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        assert_eq!(runtime::status(&context).unwrap(), visible);
        let store = Store::open(root.path()).unwrap();
        assert_eq!(store.read().unwrap().authorization_events.len(), 1);
        if !committed {
            assert_eq!(fs::read(&state_path).unwrap(), before);
        }
        // The kernel releases the writer's ownership. Recovery must be an
        // ordinary retry, preserving the committed outcome and consumed grants.
        let recovered =
            runtime::publish_fixture(&context, revision, digest, Some(previous), &|| Ok(()))
                .unwrap();
        if committed {
            assert_eq!(recovered, visible["active"]);
        }
        assert!(
            !names(&root.path().join(".shield/review"))
                .iter()
                .any(|name| name.starts_with(".state-")),
            "killed writer's temporary remains after {stage:?}"
        );
        let state = store.read().unwrap();
        assert_eq!(state.authorization_events.len(), 1);
        assert_eq!(
            state.authorization_events[0]["links"]["activation_id"],
            recovered["activation_id"]
        );
        assert_eq!(
            runtime::publish_fixture(&context, revision, digest, Some(previous), &|| Ok(()))
                .unwrap(),
            recovered
        );
        assert_eq!(store.read().unwrap().authorization_events.len(), 1);
        reconnect_and_prepare_cache(&context, stage);
        assert_eq!(runtime::status(&context).unwrap()["active"], recovered);
    }
}

/// A fresh MCP connection must start after the crash and prepare the committed
/// cache through its normal tool, without manual lock repair.
fn reconnect_and_prepare_cache(context: &Context, stage: Stage) {
    let mut context = context.clone();
    context.surface = "mcp".into();
    context.connection_id = Some(format!("crash-reconnect-{stage:?}"));
    let connection = crate::review::mcp::Connection::enrolled(&context).unwrap();
    let prepared =
        crate::review::mcp::call(&context, &connection, "cache_prepare", &json!({}), &|| {
            Ok(())
        });
    assert!(prepared["error"].is_null(), "{prepared}");
    assert_eq!(prepared["data"]["prepared"], true, "{prepared}");
}
