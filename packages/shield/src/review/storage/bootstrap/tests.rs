//! Fresh enrollment publishes authority only on an exact reviewed commit, and
//! cleanup keeps only recognized private evidence.
use super::*;
use std::cell::{Cell, RefCell};

const CONFIG: &str = "schema_version: 1\npacks: []\n";

/// A folder with only committed Shield files, awaiting its first enrollment.
fn unenrolled() -> (assert_fs::TempDir, std::path::PathBuf) {
    let parent = assert_fs::TempDir::new().expect("fixture parent");
    let root = parent.path().join("acme");
    fs::create_dir_all(root.join(".shield")).expect("fixture project");
    fs::write(root.join(".shield/config.yml"), CONFIG).expect("fixture config");
    (parent, root)
}

fn backup_count(root: &Path) -> usize {
    let Ok(entries) = fs::read_dir(root.join(".shield/review")) else {
        return 0;
    };
    entries
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

#[test]
fn bootstrap_refuses_a_folder_that_is_already_enrolled_wherever_it_is() {
    let (parent, root) = unenrolled();
    crate::review::bootstrap::prepare(&root)
        .unwrap()
        .commit()
        .unwrap();
    let moved = parent.path().join("moved");
    fs::rename(&root, &moved).unwrap();
    assert!(matches!(
        crate::review::bootstrap::prepare(&moved),
        Err(Error::Invalid(
            "project is already enrolled at this location"
        ))
    ));
}

#[cfg(unix)]
#[test]
fn bootstrap_cleanup_preserves_unverified_linked_nonprivate_and_live_files() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (_parent, root) = unenrolled();
    crate::review::bootstrap::prepare(&root)
        .unwrap()
        .commit()
        .unwrap();
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
    let (_parent, root) = unenrolled();
    let path = root.join(".shield/review/state.json");
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
    assert!(!path.exists());
    assert!(plan.committed().unwrap().is_none());
    assert!(backup_count(&root) <= 3);
    for _ in 0..3 {
        assert!(matches!(
            plan.commit_checked(&check),
            Err(Error::Policy(policy::Error::Interrupted))
        ));
        assert!(!path.exists());
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
    let (_parent, root) = unenrolled();
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
