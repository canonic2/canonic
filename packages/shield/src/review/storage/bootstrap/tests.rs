//! A crash during recovery preparation leaves bounded evidence and retryable ownership.
use super::*;
use std::{
    cell::{Cell, RefCell},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const CONFIG: &str = "schema_version: 1\npacks: []\n";

struct OwnedChild(std::process::Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn moved() -> (assert_fs::TempDir, std::path::PathBuf) {
    let parent = assert_fs::TempDir::new().expect("fixture parent");
    let original = assert_fs::TempDir::new().expect("original project");
    let bundle = configuration::Bundle::capture(CONFIG, &[], None).expect("fixture policy");
    crate::review::mcp::initialize_fixture(original.path(), &bundle);
    fs::write(original.path().join(".shield/config.yml"), CONFIG).expect("fixture config");
    let root = parent.path().join("moved");
    fs::rename(original.path(), &root).expect("move fixture");
    (parent, root)
}

fn backup_count(root: &Path) -> usize {
    fs::read_dir(root.join(".shield/review"))
        .expect("recovery directory")
        .flatten()
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with("bootstrap-")
                    && Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
            })
        })
        .count()
}

fn name(stage: super::super::tests::Stage) -> &'static str {
    use super::super::tests::Stage;
    match stage {
        Stage::UpgradePrepared => "upgrade-prepared",
        Stage::RelocationPrepared => "relocation-prepared",
        Stage::Created => "created",
        Stage::Written => "written",
        Stage::Synced => "synced",
        Stage::Renamed => "renamed",
        Stage::Complete => "complete",
    }
}

fn backup_barrier(stage: super::super::tests::Stage) {
    if std::env::var("SHIELD_TEST_BOOTSTRAP_BACKUP_STAGE").expect("backup stage") == name(stage) {
        let root = std::env::var("SHIELD_TEST_BOOTSTRAP_ROOT").expect("backup project");
        fs::write(Path::new(&root).join("ready"), name(stage)).expect("backup readiness");
        loop {
            std::thread::park();
        }
    }
}

#[test]
#[ignore = "owned child invoked by bootstrap_backup_crashes_release_ownership_and_reclaim_temporaries"]
fn bootstrap_backup_writer() {
    let root = std::env::var("SHIELD_TEST_BOOTSTRAP_ROOT").unwrap();
    let plan = crate::review::bootstrap::prepare(Path::new(&root)).unwrap();
    super::super::tests::set_observer(Some(backup_barrier));
    plan.commit().unwrap();
}

#[test]
fn bootstrap_backup_crashes_release_ownership_and_reclaim_temporaries() {
    for stage in ["created", "written", "synced", "renamed", "complete"] {
        let (_parent, root) = moved();
        let checkpoint = root.join(".shield/review/state.json");
        let original = fs::read(&checkpoint).unwrap();
        let plan = crate::review::bootstrap::prepare(&root).unwrap();
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "review::storage::bootstrap::tests::bootstrap_backup_writer",
                    "--ignored",
                    "--nocapture",
                ])
                .env("SHIELD_TEST_BOOTSTRAP_ROOT", &root)
                .env("SHIELD_TEST_BOOTSTRAP_BACKUP_STAGE", stage)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !root.join("ready").exists() && Instant::now() < deadline {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "backup child exited before {stage}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let ready = root.join("ready").exists();
        if ready && matches!(stage, "created" | "written" | "synced") {
            let temporary = root.join(format!(".shield/review/.bootstrap-{}.tmp", child.0.id()));
            clear_temporaries(&Store::location(&root).unwrap());
            assert!(
                temporary.exists(),
                "a live writer's temporary must remain protected"
            );
        }
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(ready, "backup child did not reach {stage}");
        assert_eq!(fs::read(&checkpoint).unwrap(), original);
        let context = plan.commit().unwrap();
        assert_eq!(
            crate::review::open(&root).unwrap().project_id,
            context.project_id
        );
        assert_eq!(backup_count(&root), 1);
        assert!(
            !fs::read_dir(root.join(".shield/review"))
                .unwrap()
                .flatten()
                .any(|entry| {
                    entry.file_name().to_str().is_some_and(|name| {
                        name.starts_with(".bootstrap-")
                            && Path::new(name)
                                .extension()
                                .is_some_and(|extension| extension.eq_ignore_ascii_case("tmp"))
                    })
                })
        );
    }
}

thread_local! { static CREATED: Cell<u8> = const { Cell::new(0) }; }

fn fail_authority_write(stage: super::super::tests::Stage) {
    if stage == super::super::tests::Stage::Created {
        CREATED.with(|count| {
            count.set(count.get() + 1);
            assert_ne!(count.get(), 2, "injected authority publication failure");
        });
    }
}

#[test]
fn bootstrap_failed_attempts_keep_two_committed_backups_and_one_pending_source() {
    let (parent, mut root) = moved();
    for index in 0..2 {
        crate::review::bootstrap::prepare(&root)
            .unwrap()
            .commit()
            .unwrap();
        let destination = parent.path().join(format!("next-{index}"));
        fs::rename(&root, &destination).unwrap();
        root = destination;
    }
    assert_eq!(backup_count(&root), 2);
    let checkpoint = root.join(".shield/review/state.json");
    let mut raw: Value = serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    let pinned: Vec<_> = pins(&raw["operator"]["bootstrap"]["checkpoint_recovery"])
        .into_iter()
        .map(|pin| {
            let path = root.join(format!(".shield/review/bootstrap-{pin}.json"));
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    for sequence in 100..112 {
        raw["sequence"] = json!(sequence);
        let bytes = serde_json::to_vec(&raw).unwrap();
        fs::write(&checkpoint, &bytes).unwrap();
        let plan = crate::review::bootstrap::prepare(&root).unwrap();
        CREATED.with(|count| count.set(0));
        super::super::tests::set_observer(Some(fail_authority_write));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plan.commit()));
        super::super::tests::set_observer(None);
        assert!(
            result.is_err(),
            "fixture must interrupt publication after backup completion"
        );
        assert_eq!(fs::read(&checkpoint).unwrap(), bytes);
        assert!(backup_count(&root) <= 3);
        for (path, bytes) in &pinned {
            assert_eq!(fs::read(path).unwrap(), *bytes);
        }
    }
    crate::review::bootstrap::prepare(&root)
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(backup_count(&root), 2);
}

#[cfg(unix)]
#[test]
fn bootstrap_cleanup_preserves_unverified_linked_nonprivate_and_live_files() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (_parent, root) = moved();
    let store = Store::location(&root).unwrap();
    let pid = super::super::exited_pid();
    let dead = store.directory.join(format!(".bootstrap-{pid}.tmp"));
    fs::write(&dead, "partial").unwrap();
    fs::set_permissions(&dead, fs::Permissions::from_mode(0o600)).unwrap();
    let nonprivate = store.directory.join(format!(
        "bootstrap-{}.json",
        policy::fingerprint(b"nonprivate")
    ));
    fs::write(&nonprivate, "nonprivate").unwrap();
    fs::set_permissions(&nonprivate, fs::Permissions::from_mode(0o644)).unwrap();
    let linked = store
        .directory
        .join(format!("bootstrap-{}.json", policy::fingerprint(b"linked")));
    fs::write(&linked, "linked").unwrap();
    fs::set_permissions(&linked, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&linked, store.directory.join("other-link")).unwrap();
    let alias = store
        .directory
        .join(format!("bootstrap-{}.json", policy::fingerprint(b"alias")));
    symlink(&nonprivate, &alias).unwrap();
    let unknown = store
        .directory
        .join(format!("bootstrap-{}.json", "0".repeat(64)));
    fs::write(&unknown, "unknown").unwrap();
    fs::set_permissions(&unknown, fs::Permissions::from_mode(0o600)).unwrap();
    let lock = store.lock(&|| Ok(())).unwrap();
    clear_temporaries(&store);
    cleanup(&store, &[]);
    lock.release().unwrap();
    assert!(!dead.exists());
    for path in [&nonprivate, &linked, &alias, &unknown] {
        assert!(fs::symlink_metadata(path).is_ok());
    }
}

#[test]
fn bootstrap_cancellation_after_cache_preparation_never_publishes_authority() {
    let (_parent, root) = moved();
    let path = root.join(".shield/review/state.json");
    let original = fs::read(&path).unwrap();
    let cache = root.canonicalize().unwrap().join(".shield/cache");
    let plan = crate::review::bootstrap::prepare_options(&root, None, Some(&cache)).unwrap();
    let check = || {
        if cache.join("compiled").exists() {
            Err(policy::Error::Interrupted)
        } else {
            Ok(())
        }
    };
    assert!(matches!(
        plan.commit_checked(&check),
        Err(Error::Policy(policy::Error::Interrupted))
    ));
    assert!(
        cache.join("compiled").exists(),
        "fixture must reach the final prepublication cancellation check"
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(plan.committed().unwrap().is_none());
    assert!(backup_count(&root) <= 3);
    for _ in 0..3 {
        assert!(matches!(
            plan.commit_checked(&check),
            Err(Error::Policy(policy::Error::Interrupted))
        ));
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    let fresh = crate::review::bootstrap::prepare_options(&root, None, Some(&cache)).unwrap();
    fresh.commit_checked(&|| Ok(())).unwrap();
}

thread_local! {
    static CANCELED_AFTER_COMMIT: Cell<bool> = const { Cell::new(false) };
    static COMMIT_ROOT: RefCell<Option<std::path::PathBuf>> = const { RefCell::new(None) };
}

fn cancel_after_durable_publication(stage: super::super::tests::Stage) {
    if stage == super::super::tests::Stage::Complete {
        COMMIT_ROOT.with(|root| {
            if root
                .borrow()
                .as_ref()
                .is_some_and(|root| super::current_enrollment(root).is_ok())
            {
                CANCELED_AFTER_COMMIT.with(|canceled| canceled.set(true));
            }
        });
    }
}

#[test]
fn bootstrap_cancellation_after_durable_publication_reports_the_committed_result() {
    let (_parent, root) = moved();
    let plan = crate::review::bootstrap::prepare(&root).unwrap();
    CANCELED_AFTER_COMMIT.with(|canceled| canceled.set(false));
    COMMIT_ROOT.with(|target| *target.borrow_mut() = Some(root.clone()));
    super::super::tests::set_observer(Some(cancel_after_durable_publication));
    let result = plan.commit_checked(&|| {
        if CANCELED_AFTER_COMMIT.with(Cell::get) {
            Err(policy::Error::Interrupted)
        } else {
            Ok(())
        }
    });
    super::super::tests::set_observer(None);
    COMMIT_ROOT.with(|target| *target.borrow_mut() = None);
    assert!(
        CANCELED_AFTER_COMMIT.with(Cell::get),
        "fixture must cancel only after durable authority publication"
    );
    let context = result.unwrap();
    assert_eq!(
        plan.committed().unwrap().unwrap().project_id,
        context.project_id
    );
}
