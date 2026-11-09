//! Confirmed controls restore the existing owned lock, without weakening observations.
use super::*;

#[cfg(unix)]
#[test]
fn history_control_permission_drift_preserves_lock_inode_records_and_authority() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    for (mode, args) in [
        (0o644, json!({"enabled":false})),
        (0o660, json!({"purge":true})),
        (0o640, json!({"limit":1})),
        (0o400, json!({"limit":20})),
        (0o200, json!({"enabled":false})),
    ] {
        let (root, context, evidence, resource) = review::read_approval::tests::fixture();
        let store = Store::open(root.path()).unwrap();
        let pending = review::read_approval::gate(
            &context,
            "acme-pending",
            &evidence,
            &resource,
            None,
            || true,
        )
        .unwrap();
        let granted = review::read_approval::gate(
            &context,
            "acme-granted",
            &evidence,
            &resource,
            None,
            || true,
        )
        .unwrap();
        let grant_id = granted["approval"]["request_id"].as_str().unwrap();
        review::read_approval::authorize(&context, grant_id, 300, None, false).unwrap();
        decision(&context, "first");
        decision(&context, "second");
        let before = store.read_checkpoint().unwrap();
        let path = store.directory.join("history.lock");
        let inode = fs::metadata(&path).unwrap().ino();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        if mode & 0o077 != 0 {
            assert!(lock(&store).is_err());
        }
        let plan =
            review::admin::prepare(&context, "history_configure", &args, &|| Ok(())).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            mode
        );
        plan.commit(&context, &|| Ok(())).unwrap();
        let after = store.read_history().unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(before.policy_revision, after.policy_revision);
        assert_eq!(json!(before.active), json!(after.active));
        assert_eq!(json!(before.read_requests), json!(after.read_requests));
        assert_eq!(json!(before.run_requests), json!(after.run_requests));
        assert!(after.history_available);
        assert_eq!(
            distinct_decisions(&after.events),
            if args["purge"] == true {
                0
            } else if args["limit"] == 1 {
                1
            } else {
                2
            }
        );
        if args["enabled"] == false {
            assert!(!after.history_enabled);
        }
        assert!(lock(&store).is_ok());
        assert_eq!(
            review::approvals::request_view(
                &context,
                pending["approval"]["request_id"].as_str().unwrap()
            )
            .unwrap()
            .1["status"],
            "pending"
        );
        let consume = || {
            review::read_approval::gate(
                &context,
                "acme-granted",
                &evidence,
                &resource,
                Some(grant_id),
                || true,
            )
            .unwrap()
        };
        assert_eq!(consume()["code"], "allowed-once");
        assert_ne!(consume()["code"], "allowed-once");
    }
}

#[cfg(unix)]
#[test]
fn history_control_rejects_a_lock_replaced_while_waiting_without_chmoding_either_inode() {
    use clap::Parser as _;
    use std::os::unix::fs::PermissionsExt;
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let path = store.directory.join("history.lock");
    let held = lock(&store).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let trace = log
        .request("command", "history_configure", None, None)
        .unwrap();
    let plan = review::admin::prepare(
        &context,
        "history_configure",
        &json!({"purge":true}),
        &|| Ok(()),
    )
    .unwrap();
    let before = fs::read(store.directory.join("state.json")).unwrap();
    thread::scope(|scope| {
        let trace = &trace;
        let context = &context;
        let worker = scope.spawn(move || {
            let _scope = trace.enter();
            plan.commit(context, &|| Ok(()))
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while trace.recorded_count("history.lock_waits") == 0 {
            assert!(
                Instant::now() < deadline,
                "control never reached the held inode"
            );
            thread::yield_now();
        }
        let displaced = store.directory.join("acme-displaced.lock");
        fs::rename(&path, &displaced).unwrap();
        fs::write(&path, "Acme replacement lock").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        drop(held);
        assert!(matches!(worker.join().unwrap(), Err(Error::Invalid(_))));
        for path in [&path, &displaced] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o644
            );
        }
    });
    trace.finish("refused");
    assert_eq!(
        fs::read(store.directory.join("state.json")).unwrap(),
        before
    );
}

#[cfg(unix)]
#[test]
fn history_control_refuses_links_and_keeps_the_target_untouched() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for hard in [false, true] {
        let (root, context) = enrolled();
        let store = Store::open(root.path()).unwrap();
        let path = store.directory.join("history.lock");
        fs::remove_file(&path).unwrap();
        let target = root.path().join("acme-lock.txt");
        fs::write(&target, "Acme original text").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        if hard {
            fs::hard_link(&target, &path).unwrap();
        } else {
            symlink(&target, &path).unwrap();
        }
        let before = fs::read(store.directory.join("state.json")).unwrap();
        assert!(lock(&store).is_err());
        let plan = review::admin::prepare(
            &context,
            "history_configure",
            &json!({"purge":true}),
            &|| Ok(()),
        )
        .unwrap();
        assert!(plan.commit(&context, &|| Ok(())).is_err());
        assert_eq!(
            fs::read(store.directory.join("state.json")).unwrap(),
            before
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "Acme original text");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }
}

#[cfg(unix)]
#[test]
fn history_control_never_replaces_or_normalizes_a_busy_lock() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let (root, context) = enrolled();
    let store = Store::open(root.path()).unwrap();
    let path = store.directory.join("history.lock");
    let held = lock(&store).unwrap();
    let inode = held.metadata().unwrap().ino();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let plan = review::admin::prepare(
        &context,
        "history_configure",
        &json!({"enabled":false}),
        &|| Ok(()),
    )
    .unwrap();
    let before = fs::read(store.directory.join("state.json")).unwrap();
    assert!(matches!(
        plan.commit(&context, &|| Ok(())),
        Err(Error::Locked)
    ));
    assert_eq!(
        fs::read(store.directory.join("state.json")).unwrap(),
        before
    );
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    drop(held);
    let plan = review::admin::prepare(
        &context,
        "history_configure",
        &json!({"enabled":false}),
        &|| Ok(()),
    )
    .unwrap();
    plan.commit(&context, &|| Ok(())).unwrap();
    assert!(!store.read_checkpoint().unwrap().history_enabled);
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
}
