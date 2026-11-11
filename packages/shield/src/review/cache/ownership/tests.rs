use super::super::{atomic_write, cache_owner};
use super::*;
use crate::{
    policy,
    review::{Context, storage::Store},
};
use serde_json::json;

fn private_cache() -> assert_fs::TempDir {
    let cache = assert_fs::TempDir::new().unwrap();
    // An explicitly selected existing cache must already be private. Test
    // temporary roots otherwise inherit the host's ordinary directory mode.
    storage::private_directory(cache.path()).unwrap();
    cache
}

fn publication(cache: &Path) -> (assert_fs::TempDir, Context, Artifact) {
    let (root, context) = super::super::tests::fixture();
    super::super::relocate_committed(&context, cache, &|| Ok(())).unwrap();
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let evaluator = state
        .active
        .as_ref()
        .unwrap()
        .compiled
        .load(&context.project_root, &context.project_id)
        .unwrap();
    let artifact = super::super::build_candidate(
        &context,
        &state.candidates,
        &state.policy_revision,
        &state.operator,
        &evaluator,
    )
    .unwrap();
    (root, context, artifact)
}
fn sweep(cache: &Path, context: &Context, current: &str) {
    let cache = cache.canonicalize().unwrap();
    for _ in 0..8 {
        maintain(
            &cache,
            current,
            cache_owner(&context.project_root).unwrap(),
            &context.project_root,
        )
        .unwrap();
    }
}
#[test]
fn registered_missing_checkout_payloads_reclaim_progressively_while_legacy_and_current_stay() {
    let cache = private_cache();
    let (old, _, artifact) = publication(cache.path());
    let old_key = artifact.project_key.clone();
    let old_path = artifact.path();
    let old_partition = old_path.parent().unwrap().to_path_buf();
    let (current, context, current_artifact) = publication(cache.path());
    let bytes = fs::read(current_artifact.path()).unwrap();
    let legacy = cache.path().join("compiled").join("e".repeat(64));
    prepare_directory(&legacy, cache_owner(&context.project_root).unwrap()).unwrap();
    let legacy_file = legacy.join(format!("{}.bin", "f".repeat(64)));
    atomic_write(&legacy_file, b"Acme unregistered cache").unwrap();
    drop(artifact);
    old.close().unwrap();
    sweep(cache.path(), &context, &current_artifact.project_key);
    assert!(!old_path.exists());
    assert!(
        old_partition.join(".objects.lock").is_file(),
        "stable shells are preserved"
    );
    assert!(
        legacy_file.exists(),
        "a digest-shaped legacy namespace is not provenance"
    );
    assert_eq!(fs::read(current_artifact.path()).unwrap(), bytes);
    assert!(
        !read(&cache.path().join("ownership"))
            .unwrap()
            .owners
            .contains_key(&old_key)
    );
    assert!(current.path().join(".shield/review/state.json").is_file());
}
#[test]
fn orphan_pins_partition_writers_and_unknown_neighbors_are_preserved() {
    let cache = private_cache();
    let (old, _, artifact) = publication(cache.path());
    let old_path = artifact.path();
    let partition = old_path.parent().unwrap().to_path_buf();
    let (_current, context, current) = publication(cache.path());
    old.close().unwrap();
    sweep(cache.path(), &context, &current.project_key);
    assert!(
        old_path.exists(),
        "in-flight binary and manifest reader pins protect payloads"
    );
    drop(artifact);
    let writer = storage::objects::Directory::enter(&partition).unwrap();
    let mut index = read(&cache.path().join("ownership")).unwrap();
    index.cursor = None;
    save(&cache.path().join("ownership"), &index).unwrap();
    // Select the old owner explicitly to make partition exclusion decisive.
    let old_key = partition.file_name().unwrap().to_str().unwrap();
    index.cursor = index
        .owners
        .keys()
        .rfind(|key| key.as_str() < old_key)
        .cloned();
    save(&cache.path().join("ownership"), &index).unwrap();
    assert!(matches!(
        maintain(
            cache.path(),
            &current.project_key,
            cache_owner(&context.project_root).unwrap(),
            &context.project_root
        ),
        Err(Error::Locked)
    ));
    assert!(old_path.exists());
    drop(writer);
    let unknown = partition.join("acme-retained-note");
    atomic_write(&unknown, b"Acme unrelated bytes").unwrap();
    sweep(cache.path(), &context, &current.project_key);
    assert!(!old_path.exists());
    assert_eq!(fs::read(unknown).unwrap(), b"Acme unrelated bytes");
    assert!(
        read(&cache.path().join("ownership"))
            .unwrap()
            .owners
            .contains_key(old_key)
    );
}
#[test]
fn live_replaced_and_opaque_owners_never_read_or_mutate_other_checkout_state() {
    let cache = private_cache();
    let (old, _, artifact) = publication(cache.path());
    let path = artifact.path();
    let key = artifact.project_key.clone();
    drop(artifact);
    let (_current, context, current) = publication(cache.path());
    let state = old.path().join(".shield/review/state.json");
    fs::write(&state, b"Acme unreadable private checkpoint").unwrap();
    sweep(cache.path(), &context, &current.project_key);
    assert!(path.exists());
    assert_eq!(
        fs::read(&state).unwrap(),
        b"Acme unreadable private checkpoint"
    );
    let moved = assert_fs::TempDir::new().unwrap();
    fs::rename(old.path(), moved.path().join("original")).unwrap();
    fs::create_dir(old.path()).unwrap();
    sweep(cache.path(), &context, &current.project_key);
    assert!(
        path.exists(),
        "changed native identity is foreign/unknown, not proof of orphanhood"
    );
    old.close().unwrap();
    let namespace = cache.path().join("ownership");
    let guard = storage::objects::Directory::enter(&namespace).unwrap();
    let mut index = read(&namespace).unwrap();
    index.owners.get_mut(&key).unwrap()["schema_version"] = json!(91);
    let opaque = index.owners[&key].clone();
    save(&namespace, &index).unwrap();
    drop(guard);
    sweep(cache.path(), &context, &current.project_key);
    assert!(path.exists());
    assert_eq!(read(&namespace).unwrap().owners[&key], opaque);
}
#[test]
fn current_checkpoints_exact_recovery_archives_protect_moved_cache_namespaces() {
    let cache = private_cache();
    let (old, _, artifact) = publication(cache.path());
    let path = artifact.path();
    let bytes = fs::read(old.path().join(".shield/review/state.json")).unwrap();
    let digest = policy::fingerprint(&bytes);
    drop(artifact);
    let (current, context, current_artifact) = publication(cache.path());
    let archive = current
        .path()
        .join(format!(".shield/review/bootstrap-{digest}.json"));
    storage::private_files::write(
        &archive,
        &serde_json::from_slice::<Value>(&bytes).unwrap(),
        INDEX_BYTES,
    )
    .unwrap();
    // Checkpoint hashes name the exact original compact serialization.
    assert_eq!(policy::fingerprint(&fs::read(&archive).unwrap()), digest);
    let store = Store::open(current.path()).unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.operator["bootstrap"]["checkpoint_recovery"] = json!([digest]);
            Ok(Value::Null)
        })
        .unwrap();
    old.close().unwrap();
    sweep(cache.path(), &context, &current_artifact.project_key);
    assert!(
        path.exists(),
        "retained recovery descriptors protect whole namespaces"
    );
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.operator["bootstrap"]["checkpoint_recovery"] = json!([]);
            Ok(Value::Null)
        })
        .unwrap();
    sweep(cache.path(), &context, &current_artifact.project_key);
    assert!(!path.exists());
}
#[test]
fn bounded_tracking_overflow_never_rejects_valid_cache_publication() {
    let cache = private_cache();
    let (root, context) = super::super::tests::fixture();
    let uid = cache_owner(&context.project_root).unwrap();
    let namespace = namespace(cache.path(), uid).unwrap();
    let guard = storage::objects::Directory::enter(&namespace).unwrap();
    let index = Index {
        owners: (0..TRACKED_OWNERS)
            .map(|n| (format!("{n:064x}"), Value::Null))
            .collect(),
        ..Index::default()
    };
    save(&namespace, &index).unwrap();
    drop(guard);
    let state = Store::open(root.path()).unwrap().read_current().unwrap();
    let artifact = super::super::build(&state, Some(cache.path())).unwrap();
    assert!(
        artifact
            .load(&context.project_root, &context.project_id)
            .is_ok()
    );
    let index = read(&namespace).unwrap();
    assert_eq!(index.owners.len(), TRACKED_OWNERS);
    assert!(!index.owners.contains_key(&artifact.project_key));
}
