use super::*;

fn enrolled() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().expect("SDK project directory");
    let bundle = crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
        .expect("SDK fixture policy");
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    (root, context)
}

#[test]
fn reviewed_sdk_contexts_bind_real_private_issuance_epoch_and_retry_owner() {
    let (root, _) = enrolled();
    let fixture = testing::Fixture::new(root.path()).unwrap();
    let single = fixture.consumer().unwrap();
    assert_eq!(
        single.context().project_root,
        root.path().canonicalize().unwrap().to_str().unwrap()
    );
    drop(single);
    let mut consumers = fixture.consumers(2).unwrap();
    let first = consumers.pop().unwrap();
    let second = consumers.pop().unwrap();
    assert_ne!(first.consumer_id(), second.consumer_id());
    assert_ne!(first.binding_digest(), second.binding_digest());
    let request_binding = "a".repeat(64);
    assert_ne!(
        first
            .retry_reference("acme-read", &request_binding)
            .unwrap(),
        second
            .retry_reference("acme-read", &request_binding)
            .unwrap()
    );
    let mut state = current(root.path()).unwrap();
    first.check_state(&state).unwrap();
    state.project_id = "b".repeat(64);
    assert!(first.check_state(&state).is_err());
}

#[test]
fn sdk_admission_does_not_read_policy_sources_or_rewrite_checkpoint() {
    let (root, _) = enrolled();
    let fixture = testing::Fixture::new(root.path()).unwrap();
    fixture
        .scoped(|| {
            let issued = mint(root.path(), mapping::digest())?;
            let path = root.path().join(".shield/state/state.json");
            let checkpoint = std::fs::read(&path)?;
            std::fs::write(root.path().join(".shield/config.yml"), b"invalid: [")?;
            let consumer = RegisteredReadConsumer::admit_secret(root.path(), issued.0)?;
            consumer.check_state(&current(root.path())?)?;
            assert_eq!(std::fs::read(path)?, checkpoint);
            Ok(())
        })
        .unwrap();
}

#[test]
fn sdk_admission_waits_for_profile_and_context_writers() {
    use std::{sync::mpsc, thread, time::Duration};

    let (root, _) = enrolled();
    let fixture = testing::Fixture::new(root.path()).unwrap();
    let root = root.path();
    for namespace in ["sdk-read-profiles", "sdk-read-contexts"] {
        let issued = fixture.scoped(|| mint(root, mapping::digest())).unwrap();
        let directory = fixture
            .scoped(|| files::directory(namespace, false))
            .unwrap();
        thread::scope(|scope| {
            let writer = objects::Directory::enter(&directory).unwrap();
            let (started, ready) = mpsc::channel();
            let (reply, result) = mpsc::channel();
            let fixture = &fixture;
            scope.spawn(move || {
                started.send(()).unwrap();
                reply
                    .send(fixture.scoped(|| {
                        RegisteredReadConsumer::admit_secret(root, issued.0)?;
                        Ok(())
                    }))
                    .unwrap();
            });
            ready.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(matches!(
                result.recv_timeout(Duration::from_millis(100)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
            drop(writer);
            result
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
        });
    }
}

#[test]
fn sdk_admission_cancellation_releases_the_profile_lease() {
    let (root, _) = enrolled();
    let fixture = testing::Fixture::new(root.path()).unwrap();
    fixture
        .scoped(|| {
            let issued = mint(root.path(), mapping::digest())?;
            let profiles = files::directory("sdk-read-profiles", false)?;
            let contexts = files::directory("sdk-read-contexts", false)?;
            let writer = objects::Directory::enter(&contexts)?;
            let checks = std::cell::Cell::new(0);
            let checkpoint = || {
                checks.set(checks.get() + 1);
                if checks.get() < 3 {
                    return Ok(());
                }
                assert!(matches!(
                    objects::Directory::try_enter(&profiles),
                    Err(Error::Locked)
                ));
                Err(crate::policy::Error::Interrupted)
            };
            assert!(matches!(
                RegisteredReadConsumer::admit_with_secret(root.path(), issued.0, &checkpoint),
                Err(Error::Policy(crate::policy::Error::Interrupted))
            ));
            objects::Directory::try_enter(&profiles)?;
            drop(writer);
            Ok(())
        })
        .unwrap();
}

#[test]
fn an_aged_context_stays_live_only_while_its_owner_holds_the_lease() {
    let (root, _) = enrolled();
    let fixture = testing::Fixture::new(root.path()).unwrap();
    fixture
        .scoped(|| {
            let issued = mint(root.path(), mapping::digest())?;
            let checkpoint = std::fs::read(root.path().join(".shield/state/state.json"))?;
            let directory = files::directory("sdk-read-contexts", false)?;
            {
                let _lease = objects::Directory::enter(&directory)?;
                let mut index = files::contexts(&directory)?;
                let row = index.contexts.get_mut(&issued.0.hash()).unwrap();
                row.created_at_ms = 0;
                files::write(&directory, &index)?;
            }
            let retry = Secret::parse(issued.launch_value().into())?;
            let consumer = RegisteredReadConsumer::admit_secret(root.path(), issued.0)?;
            let state = current(root.path())?;
            consumer.check_state(&state)?;
            drop(issued.1);
            assert!(consumer.check_state(&state).is_err());
            drop(consumer);
            assert!(RegisteredReadConsumer::admit_secret(root.path(), retry).is_err());
            let issued = mint(root.path(), mapping::digest())?;
            std::fs::remove_file(directory.join(".objects.lock"))?;
            assert!(RegisteredReadConsumer::admit_secret(root.path(), issued.0).is_err());
            assert!(!directory.join(".objects.lock").exists());
            assert_eq!(
                std::fs::read(root.path().join(".shield/state/state.json"))?,
                checkpoint
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn owner_shutdown_keeps_its_lease_while_waiting_to_claim_context() {
    use std::{sync::mpsc, thread, time::Duration};

    let (root, _) = enrolled();
    let root = root.path();
    let fixture = testing::Fixture::new(root).unwrap();
    fixture
        .scoped(|| {
            let issued = mint(root, mapping::digest())?;
            let key = issued.0.hash();
            let directory = files::directory("sdk-read-contexts", false)?;
            let writer = objects::Directory::enter(&directory)?;
            let consumer_id = files::contexts(&directory)?.contexts[&key]
                .consumer_id
                .clone();
            thread::scope(|scope| {
                let (started, ready) = mpsc::channel();
                let (reply, result) = mpsc::channel();
                let fixture = &fixture;
                scope.spawn(move || {
                    started.send(()).unwrap();
                    reply.send(fixture.scoped(|| issued.close(root))).unwrap();
                });
                ready.recv_timeout(Duration::from_secs(5)).unwrap();
                assert!(matches!(
                    result.recv_timeout(Duration::from_millis(100)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ));
                // A factory inspecting the row while it owns the namespace can
                // reclaim it only after the owner has stopped holding this lease.
                assert!(files::context_alive(&directory, &consumer_id).unwrap());
                drop(writer);
                result.recv_timeout(Duration::from_secs(5)).unwrap()
            })?;
            let _next = mint(root, mapping::digest())?;
            assert!(!files::contexts(&directory)?.contexts.contains_key(&key));
            assert!(!files::context_alive(&directory, &consumer_id)?);
            Ok(())
        })
        .unwrap();
}

#[test]
fn context_close_cannot_race_a_live_admission_and_closes_after_handoff() {
    let (root, _) = enrolled();
    let fixture = testing::Fixture::new(root.path()).unwrap();
    fixture
        .scoped(|| {
            let issued = mint(root.path(), mapping::digest())?;
            let close = Secret::parse(issued.launch_value().into())?;
            let retry = Secret::parse(issued.launch_value().into())?;
            let consumer = RegisteredReadConsumer::admit_secret(root.path(), issued.0)?;
            assert!(close_secret(root.path(), &close, None).is_err());
            drop(consumer);
            close_secret(
                root.path(),
                &Secret::parse(retry.launch_value().into())?,
                None,
            )?;
            assert!(RegisteredReadConsumer::admit_secret(root.path(), retry).is_err());
            Ok(())
        })
        .unwrap();
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir(to).expect("copied SDK directory");
    for entry in std::fs::read_dir(from).expect("SDK directory entries") {
        let entry = entry.expect("SDK copy entry");
        if entry.file_type().expect("SDK entry type").is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()));
        } else {
            assert!(entry.file_type().expect("SDK entry type").is_file());
            std::fs::copy(entry.path(), to.join(entry.file_name())).expect("SDK copied file");
        }
    }
    std::fs::set_permissions(
        to,
        std::fs::metadata(from)
            .expect("SDK source metadata")
            .permissions(),
    )
    .expect("SDK copied private permissions");
}

#[test]
fn copied_checkout_reuses_profile_but_never_original_context_while_original_remains() {
    let (root, original) = enrolled();
    let fixture = testing::Fixture::new(root.path()).unwrap();
    let destination = assert_fs::TempDir::new().unwrap();
    let copied = destination.path().join("copied");
    fixture
        .scoped(|| {
            let original_receipt =
                Prepared::new(&original, mapping::ID)?.commit(&original, &|| Ok(()))?;
            for field in ["binary_digest", "mapping_digest", "policy_lineage"] {
                assert!(model::digest(original_receipt[field].as_str().unwrap()));
            }
            assert!(original_receipt.get("project_root").is_none());
            assert!(original_receipt.get("binary").is_none());
            let old = mint(root.path(), mapping::digest())?;
            let probe = Secret::parse(old.launch_value().into())?;
            copy_tree(root.path(), &copied);
            assert!(RegisteredReadConsumer::admit_secret(&copied, probe).is_err());
            // The copy is the same project, but an SDK read context is a
            // handle for the folder it was minted for.
            let moved = super::super::open(&copied)?;
            assert_eq!(moved.project_id, original.project_id);
            // The independently retained profile is already reviewed. A copy
            // obtains a new local context without registering the profile again.
            let fresh = mint(&copied, mapping::digest())?;
            assert_eq!(
                files::profiles(&files::directory("sdk-read-profiles", false)?)?
                    .profiles
                    .len(),
                1
            );
            let copied_consumer = RegisteredReadConsumer::admit_secret(&copied, fresh.0)?;
            copied_consumer.check_state(&current(&copied)?)?;
            let original_consumer = RegisteredReadConsumer::admit_secret(root.path(), old.0)?;
            original_consumer.check_state(&current(root.path())?)?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn context_capacity_is_global_bounded_and_abandoned_rows_are_reclaimed_by_mint() {
    let (root, _) = enrolled();
    let fixture = testing::Fixture::new(root.path()).unwrap();
    fixture
        .scoped(|| {
            let mut owners = (0..CONTEXT_LIMIT)
                .map(|_| mint(root.path(), mapping::digest()))
                .collect::<Result<Vec<_>, _>>()?;
            assert!(matches!(
                mint(root.path(), mapping::digest()),
                Err(Error::Overloaded)
            ));
            let directory = files::directory("sdk-read-contexts", false)?;
            owners.truncate(1);
            let fresh = mint(root.path(), mapping::digest())?;
            assert_eq!(files::contexts(&directory)?.contexts.len(), 2);
            RegisteredReadConsumer::admit_secret(root.path(), fresh.0)?;
            Ok(())
        })
        .unwrap();
}
