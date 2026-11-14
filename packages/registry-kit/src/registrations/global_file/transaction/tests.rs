//! Real owner crashes, immutable publication, ABA refusal and projection conflicts.
use super::*;
use crate::source::Source;
use assert_fs::{TempDir, prelude::*};
use serde_json::json;

pub(super) fn point(stage: &str) {
    if std::env::var("REGISTRY_TEST_CRASH_STAGE").ok().as_deref() == Some(stage) {
        std::process::exit(77);
    }
}
fn source() -> Source {
    Source::signed(
        "https://registry.example.com/",
        crate::test_support::EXAMPLE_KEY,
        "acme",
    )
    .unwrap()
}
fn selected() -> Registrations {
    let mut value = Registrations::default();
    value.add("acme", source(), "main").unwrap();
    value
}
fn operation() -> String {
    digest::sha256_hex(b"add-acme")
}
fn record() -> Value {
    json!({"committed":true,"change":"add","alias":"acme"})
}

#[test]
fn generation_snapshots_reject_aba_and_replay_only_the_exact_frozen_publication() {
    let root = TempDir::new().unwrap();
    let file = GlobalFile::new(root.path().join("registries.json"), "main");
    let empty = file.snapshot().unwrap();
    let first = file
        .publish(&empty, &selected(), &operation(), &record())
        .unwrap();
    assert!(first.durable && !first.repair_needed && !first.replayed);
    let replay = file
        .publish(&empty, &selected(), &operation(), &record())
        .unwrap();
    assert_eq!(first.generation, replay.generation);
    assert_eq!(first.receipt, replay.receipt);
    assert!(replay.replayed);
    let selected_snapshot = file.snapshot().unwrap();
    file.publish(
        &selected_snapshot,
        &Registrations::default(),
        &digest::sha256_hex(b"clear"),
        &json!({}),
    )
    .unwrap();
    assert_eq!(file.read().unwrap(), Registrations::default());
    assert!(matches!(
        file.publish(&empty, &selected(), &operation(), &record()),
        Err(Error::Changed)
    ));
    assert_ne!(empty.generation(), file.snapshot().unwrap().generation());
    assert!(file.receipt(&operation()).unwrap().is_none());
}

#[test]
fn committed_authority_preserves_a_manual_projection_until_a_reviewed_publication_replaces_it() {
    let root = TempDir::new().unwrap();
    let file = GlobalFile::new(root.path().join("registries.json"), "main");
    file.publish(
        &file.snapshot().unwrap(),
        &selected(),
        &operation(),
        &record(),
    )
    .unwrap();
    root.child("registries.json")
        .write_str("manual bytes to preserve\n")
        .unwrap();
    assert_eq!(file.read().unwrap(), selected());
    let recovered = file.recover().unwrap().unwrap();
    assert!(recovered.repair_needed);
    assert_eq!(
        fs::read_to_string(file.path()).unwrap(),
        "manual bytes to preserve\n"
    );
    assert!(matches!(
        file.replace(&selected(), &Registrations::default()),
        Err(Error::ProjectionConflict(_))
    ));
    assert!(matches!(file.read_projection(), Err(Error::Malformed)));
    assert_eq!(
        fs::read_to_string(file.path()).unwrap(),
        "manual bytes to preserve\n"
    );
    // A caller that reviewed the conflicting bytes may replace exactly those.
    let reviewed = file.snapshot().unwrap();
    assert!(!reviewed.projection_ready());
    let overwritten = file
        .publish(
            &reviewed,
            reviewed.registrations(),
            &digest::sha256_hex(b"overwrite"),
            &json!({}),
        )
        .unwrap();
    assert!(!overwritten.repair_needed && overwritten.durable);
    assert_eq!(file.read_projection().unwrap(), Some(selected()));
}

#[test]
fn a_reviewed_manual_edit_is_adopted_and_a_stale_review_of_it_refuses() {
    let root = TempDir::new().unwrap();
    let file = GlobalFile::new(root.path().join("registries.json"), "main");
    file.replace(&Registrations::default(), &selected())
        .unwrap();
    root.child("registries.json").write_str("{}").unwrap();
    let reviewed = file.snapshot().unwrap();
    let edit = file.read_projection().unwrap().unwrap();
    assert_eq!(edit, Registrations::default());
    root.child("registries.json")
        .write_str("{\"sources\":{}}")
        .unwrap();
    assert!(matches!(
        file.publish(&reviewed, &edit, &digest::sha256_hex(b"adopt"), &json!({})),
        Err(Error::Changed)
    ));
    assert!(!reference_path(&file, "pending").exists());
    assert_eq!(
        fs::read_to_string(file.path()).unwrap(),
        "{\"sources\":{}}",
        "an edit the caller never saw is preserved"
    );
    let reviewed = file.snapshot().unwrap();
    let adopted = file
        .publish(&reviewed, &edit, &digest::sha256_hex(b"adopt"), &json!({}))
        .unwrap();
    assert!(!adopted.repair_needed);
    assert_eq!(file.read().unwrap(), Registrations::default());
    assert!(file.snapshot().unwrap().projection_ready());
}

#[test]
fn leases_refuse_live_and_unverifiable_legacy_owners_without_unlinking_markers() {
    let root = TempDir::new().unwrap();
    let file = GlobalFile::new(root.path().join("registries.json"), "main");
    root.child("registries.lock")
        .write_str("old owner without a lease")
        .unwrap();
    assert!(matches!(
        file.publish(
            &file.snapshot().unwrap(),
            &selected(),
            &operation(),
            &record()
        ),
        Err(Error::Locked(_))
    ));
    assert_eq!(
        fs::read_to_string(root.path().join("registries.lock")).unwrap(),
        "old owner without a lease"
    );
    fs::remove_file(root.path().join("registries.lock")).unwrap();
    let held = lease(&file).unwrap();
    assert!(matches!(
        file.publish(
            &file.snapshot().unwrap(),
            &selected(),
            &operation(),
            &record()
        ),
        Err(Error::Locked(_))
    ));
    drop(held);
    assert!(
        file.publish(
            &file.snapshot().unwrap(),
            &selected(),
            &operation(),
            &record()
        )
        .is_ok()
    );
}

#[cfg(unix)]
#[test]
fn settled_recovery_reconfirms_projection_durability_before_claiming_it() {
    use std::os::unix::fs::PermissionsExt;
    let root = TempDir::new().unwrap();
    let parent = root.path().join("config");
    let file = GlobalFile::new(parent.join("registries.json"), "main");
    file.publish(
        &file.snapshot().unwrap(),
        &selected(),
        &operation(),
        &record(),
    )
    .unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o300)).unwrap();
    if File::open(&parent).is_ok() {
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let outcome = file.recover();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    let publication = outcome.unwrap().unwrap();
    assert!(!publication.durable);
    assert!(!publication.repair_needed);
    assert_eq!(file.read().unwrap(), selected());
    assert!(file.recover().unwrap().unwrap().durable);
}

#[test]
fn settled_mutations_reclaim_superseded_generations_and_bound_receipts() {
    let root = TempDir::new().unwrap();
    let file = GlobalFile::new(root.path().join("registries.json"), "main");
    for index in 0..20 {
        let next = if index % 2 == 0 {
            selected()
        } else {
            Registrations::default()
        };
        file.publish(
            &file.snapshot().unwrap(),
            &next,
            &digest::sha256_hex(index.to_string().as_bytes()),
            &json!({"index":index}),
        )
        .unwrap();
    }
    assert!(fs::read_dir(directory(&file)).unwrap().count() <= 3);
    assert!(matches!(
        file.publish(
            &file.snapshot().unwrap(),
            &selected(),
            &operation(),
            &json!("x".repeat(
                usize::try_from(MAX_RECEIPT_BYTES).expect("receipt limit fits supported targets")
            ))
        ),
        Err(Error::TooLarge)
    ));
}

#[test]
fn a_deleted_projection_is_rewritten_from_the_active_generation_before_writes() {
    let root = TempDir::new().unwrap();
    let file = GlobalFile::new(root.path().join("registries.json"), "main");
    file.replace(&Registrations::default(), &selected())
        .unwrap();
    fs::remove_file(file.path()).unwrap();
    assert!(!file.snapshot().unwrap().projection_ready());
    assert_eq!(file.read().unwrap(), selected(), "reads never repair");
    assert!(!file.path().exists());
    file.replace(&selected(), &Registrations::default())
        .unwrap();
    assert_eq!(file.read().unwrap(), Registrations::default());
    fs::remove_file(file.path()).unwrap();
    let recovered = file.recover().unwrap().unwrap();
    assert!(!recovered.repair_needed && recovered.durable);
    assert_eq!(
        serde_json::from_slice::<Registrations>(&fs::read(file.path()).unwrap()).unwrap(),
        Registrations::default()
    );
    assert!(file.snapshot().unwrap().projection_ready());
}

#[test]
fn an_uncommitted_intent_whose_base_changed_is_discarded_instead_of_blocking_writes() {
    let root = crash("intent");
    let file = GlobalFile::new(root.path().join("registries.json"), "main");
    root.child("registries.json").write_str("{}").unwrap();
    assert!(file.recover().unwrap().is_none());
    assert!(!reference_path(&file, "pending").exists());
    assert!(file.receipt(&operation()).unwrap().is_none());
    assert_eq!(file.read().unwrap(), Registrations::default());
    file.replace(&Registrations::default(), &selected())
        .unwrap();
    assert_eq!(file.read().unwrap(), selected());
}

fn crash(stage: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "registrations::global_file::transaction::tests::crash_child",
            "--ignored",
            "--nocapture",
        ])
        .env("REGISTRY_TEST_CRASH_STAGE", stage)
        .env("REGISTRY_TEST_CRASH_ROOT", root.path())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(77),
        "{stage}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    root
}

#[test]
fn crash_recovery_releases_ownership_and_reconciles_exact_saved_intent() {
    for stage in ["lease", "intent", "reference", "projection"] {
        let root = crash(stage);
        let file = GlobalFile::new(root.path().join("registries.json"), "main");
        if stage == "lease" {
            assert_eq!(file.read().unwrap(), Registrations::default());
            file.publish(
                &file.snapshot().unwrap(),
                &selected(),
                &operation(),
                &record(),
            )
            .unwrap();
        } else {
            let recovered = file.recover().unwrap().unwrap();
            assert_eq!(recovered.receipt, record());
            assert!(recovered.replayed && recovered.durable && !recovered.repair_needed);
            assert_eq!(
                file.receipt(&operation()).unwrap().unwrap().generation,
                recovered.generation
            );
        }
        assert_eq!(file.read().unwrap(), selected());
        assert_eq!(
            serde_json::from_slice::<Registrations>(&fs::read(file.path()).unwrap()).unwrap(),
            selected()
        );
    }
}

#[test]
#[ignore = "subprocess fixture for owned publication crash stages"]
fn crash_child() {
    let root = std::env::var_os("REGISTRY_TEST_CRASH_ROOT").unwrap();
    let file = GlobalFile::new(PathBuf::from(root).join("registries.json"), "main");
    file.publish(
        &file.snapshot().unwrap(),
        &selected(),
        &operation(),
        &record(),
    )
    .unwrap();
    panic!("crash stage was not reached");
}
