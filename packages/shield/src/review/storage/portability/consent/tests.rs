use super::*;
use crate::review;
use std::fs;

struct Observer;
impl Drop for Observer {
    fn drop(&mut self) {
        crate::review::storage::tests::set_observer(None);
    }
}
fn no_pointer_before_writer_returns(stage: crate::review::storage::tests::Stage) {
    if stage == crate::review::storage::tests::Stage::Complete {
        assert!(
            read_index(&namespace(false).unwrap())
                .unwrap()
                .candidates
                .is_empty(),
            "immutable preparation must not publish consent before durable activation returns"
        );
    }
}

#[test]
fn large_reviewed_policy_uses_small_postcommit_pointer_and_source_only_clone() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let root = assert_fs::TempDir::new().unwrap();
    let config = format!(
        "schema_version: 1\npacks: []\n# {}\n",
        "Acme comment ".repeat(1_400_000)
    );
    assert!(config.len() > super::super::LIMIT);
    let bundle = crate::configuration::Bundle::capture(&config, &[], None).unwrap();
    crate::review::storage::tests::set_observer(Some(no_pointer_before_writer_returns));
    let observer = Observer;
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    drop(observer);
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    let index_path = directory().unwrap().join("state.json");
    assert!(fs::metadata(&index_path).unwrap().len() < 16 * 1024);
    assert!(
        fs::metadata(root.path().join(".shield/review/state.json"))
            .unwrap()
            .len()
            < super::super::LIMIT as u64
    );
    assert_eq!(presence(&state)["present"], true);
    let index = read_index(&namespace(false).unwrap()).unwrap();
    let record = index
        .candidates
        .values()
        .next()
        .unwrap()
        .values()
        .next()
        .unwrap();
    let body = namespace(false)
        .unwrap()
        .read_json(&record.context)
        .unwrap();
    for forbidden in [
        "integration",
        "enforcement",
        "path_access",
        "compiled_cache_root",
        "bootstrap",
        "portable_launch_roots",
        "runtime_incarnation",
    ] {
        assert!(body["metadata"].get(forbidden).is_none());
    }
    review::mode::Prepared::new(&context, false)
        .unwrap()
        .commit(&context, &|| Ok(()))
        .unwrap();
    let destination = assert_fs::TempDir::new().unwrap();
    fs::create_dir(destination.path().join(".shield")).unwrap();
    fs::write(destination.path().join(".shield/config.yml"), &config).unwrap();
    let cloned = review::open(destination.path()).unwrap();
    let cloned_state = Store::open(destination.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    assert_ne!(context.project_id, cloned.project_id);
    assert!(cloned_state.active.is_some());
    assert!(cloned_state.enforcement.enabled);
    assert!(
        cloned_state.read_requests.is_empty()
            && cloned_state.run_requests.is_empty()
            && cloned_state.proposals.is_empty()
    );
    assert_eq!(cloned_state.policy_lineage(), state.policy_lineage());
    crate::runtime::Project::open(destination.path())
        .unwrap()
        .snapshot()
        .unwrap();
}

#[test]
fn prepared_graph_pins_survive_gc_but_uncommitted_source_never_becomes_consent() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let (root, context, _, _) = review::read_approval::tests::fixture();
    let store = Store::open(root.path()).unwrap();
    let state = store.read_current().unwrap();
    let mut uncommitted = state.candidates["project"].clone();
    uncommitted["config"] = json!("schema_version: 1\npacks: []\n# Acme uncommitted\n");
    let (references, pin) = store
        .retain_candidates(&BTreeMap::from([("project".into(), uncommitted.clone())]))
        .unwrap();
    let prepared = PreparedConsent::prepare(&store, &references["project"]).unwrap();
    let prepared_key = prepared.key.clone();
    assert_eq!(prepared.project_pin.len(), 1);
    assert_eq!(prepared.user_pin.len(), 1);
    drop(pin);
    let mut unrelated = uncommitted.clone();
    unrelated["config"] = json!("schema_version: 1\npacks: []\n# Acme unrelated\n");
    let (_other, _pin) = store
        .retain_candidates(&BTreeMap::from([("project".into(), unrelated)]))
        .unwrap();
    assert_eq!(
        store.captured_candidate(&references["project"]).unwrap(),
        uncommitted
    );
    let mut user_gc = state.candidates["project"].clone();
    user_gc["config"] = json!("schema_version: 1\npacks: []\n# Acme user GC\n");
    let (_other, _pin) = namespace(false).unwrap().retain_owned(user_gc).unwrap();
    assert_eq!(
        prepared.user.captured(&prepared.source).unwrap(),
        uncommitted
    );
    assert!(
        prepared
            .publish(&store, state.active.as_ref().unwrap().activation_id())
            .is_err()
    );
    assert!(record(&prepared_key).is_none());
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .project_id,
        context.project_id
    );
}

#[test]
fn consent_uses_final_metadata_and_ordinary_authority_writes_do_not_publish_it() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let (root, context, _, _) = review::read_approval::tests::fixture();
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    let prepared = PreparedConsent::prepare(
        &store,
        state.active.as_ref().unwrap().project_candidate().unwrap(),
    )
    .unwrap();
    let key = prepared.key.clone();
    remove_record(&key);
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.operator["projection"] = json!("f".repeat(64));
            Ok(Value::Null)
        })
        .unwrap();
    assert!(record(&key).is_none());
    prepared
        .publish(&store, state.active.as_ref().unwrap().activation_id())
        .unwrap();
    let record = read_index(&namespace(false).unwrap())
        .unwrap()
        .candidates
        .remove(&key)
        .unwrap();
    let record = record.values().next().unwrap();
    let body = namespace(false)
        .unwrap()
        .read_json(&record.context)
        .unwrap();
    assert_eq!(body["metadata"]["projection"], "f".repeat(64));
    assert!(metadata(&json!({"integration":{"legacy":"foreign launch"}})).is_err());
}

#[test]
fn retained_upgrade_uses_legacy_receipt_with_an_empty_index() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let (root, _, _, _) = review::read_approval::tests::fixture();
    let store = Store::open(root.path()).unwrap();
    let state = store.read_current().unwrap();
    let raw = state.candidates["project"].clone();
    let body = context(&state);
    let path = directory()
        .unwrap()
        .join(format!("{}.json", key(&raw).unwrap()));
    let mut receipt = json!({"schema_version":1,"candidate":raw,"metadata":body["metadata"],"registry_checkpoints":body["registry_checkpoints"]});
    receipt["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("policy_lineage");
    private_files::write(&path, &receipt, super::super::LIMIT).unwrap();
    fs::remove_file(directory().unwrap().join("state.json")).unwrap();
    let checkpoint = root.path().join(".shield/review/state.json");
    let mut old: Value = serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    for field in [
        "runtime_incarnation",
        "administration_origin",
        "policy_consent_key",
    ] {
        old["operator"].as_object_mut().unwrap().remove(field);
    }
    fs::write(&checkpoint, old.to_string()).unwrap();
    assert!(
        read_index(&namespace(false).unwrap())
            .unwrap()
            .candidates
            .is_empty()
    );
    // The legacy receipt alone admits this exact policy in a copy.
    assert!(presence_for(&raw));
    let moved = assert_fs::TempDir::new().unwrap();
    crate::test_support::copy_fixture_directory(root.path(), moved.path());
    let store = Store::open(moved.path()).unwrap();
    let migrated = store.read_checkpoint().unwrap();
    assert!(migrated.active.is_some());
    assert_eq!(migrated.policy_revision, state.policy_revision);
    assert_eq!(
        private_files::read(&path, super::super::LIMIT).unwrap(),
        receipt
    );
    let mut changed = receipt;
    changed["candidate"]["rules"] = json!("unreviewed source");
    private_files::write(&path, &changed, super::super::LIMIT).unwrap();
    assert!(legacy::retained(&raw).unwrap().is_none());
}

#[test]
fn genuine_legacy_receipts_remain_readable_and_unchanged() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let (root, _, _, _) = review::read_approval::tests::fixture();
    let state = Store::open(root.path()).unwrap().read_current().unwrap();
    let raw = state.candidates["project"].clone();
    let key = super::super::key(&raw).unwrap();
    remove_record(&key);
    let body = context(&state);
    let legacy = json!({"schema_version":1,"candidate":raw,"metadata":body["metadata"],"registry_checkpoints":body["registry_checkpoints"]});
    let path = directory().unwrap().join(format!("{key}.json"));
    private_files::write(&path, &legacy, super::super::LIMIT).unwrap();
    let bytes = fs::read(&path).unwrap();
    let target = assert_fs::TempDir::new().unwrap();
    fs::create_dir(target.path().join(".shield")).unwrap();
    fs::write(
        target.path().join(".shield/config.yml"),
        state.candidates["project"]["config"].as_str().unwrap(),
    )
    .unwrap();
    assert!(review::open(target.path()).is_ok());
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn unavailable_optional_user_store_cannot_revoke_valid_local_policy() {
    let home = assert_fs::TempDir::new().unwrap();
    let blocked = home.path().join("consent");
    fs::write(&blocked, b"Acme unrelated file").unwrap();
    let _scope = scoped_directory(blocked.clone());
    let (root, _, _, _) = review::read_approval::tests::fixture();
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert!(state.active.is_some());
    assert_eq!(state.operator["policy_consent_version"], 2);
    assert_eq!(presence(&state)["present"], false);
    crate::runtime::Project::open(root.path())
        .unwrap()
        .snapshot()
        .unwrap();
    assert_eq!(fs::read(blocked).unwrap(), b"Acme unrelated file");
}

fn reviewed_variant(command: &str) -> (assert_fs::TempDir, Value, Value) {
    let config = json!({"schema_version":1,"packs":[{"id":"acme/tools","version":"1.0.0","profile":"recommended","path":"tools.yml"}]}).to_string();
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"1.0.0","description":"Acme variant","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":[command]}}}]}).to_string();
    let raw = json!(crate::configuration::Bundle::capture(&config, &[pack], None).unwrap());
    let candidates = BTreeMap::from([("project".into(), raw.clone())]);
    let evaluator =
        crate::configuration::Evaluator::new(review::snapshots(&candidates).unwrap()).unwrap();
    let merged = crate::configuration::layers::merged(&raw).unwrap();
    let installed = review::integrations::project_sources::installed(&merged, &raw).unwrap();
    let metadata = json!({"sources":{},"default":null,"installed":installed,"projection":null});
    let root = assert_fs::TempDir::new().unwrap();
    Store::initialize_active(
        root.path(),
        candidates,
        evaluator.revision(),
        metadata.clone(),
    )
    .unwrap();
    fs::write(root.path().join(".shield/config.yml"), config).unwrap();
    (root, raw, metadata)
}
fn variant_clone(raw: &Value) -> assert_fs::TempDir {
    let root = assert_fs::TempDir::new().unwrap();
    fs::create_dir(root.path().join(".shield")).unwrap();
    fs::write(
        root.path().join(".shield/config.yml"),
        raw["config"].as_str().unwrap(),
    )
    .unwrap();
    root
}
#[test]
fn exact_portable_lock_selects_old_reviewed_variant_and_absent_lock_refuses_ambiguity() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let (_a, first, first_metadata) = reviewed_variant("status");
    let (_b, second, _) = reviewed_variant("inspect");
    assert_eq!(key(&first).unwrap(), key(&second).unwrap());
    assert_eq!(
        read_index(&namespace(false).unwrap())
            .unwrap()
            .candidates
            .values()
            .next()
            .unwrap()
            .len(),
        2
    );
    let old_clone = variant_clone(&first);
    let locks = review::publication::projections::locks(&first, &first_metadata).unwrap();
    fs::write(
        old_clone.path().join(".shield/shield.lock"),
        &locks["shield.lock"],
    )
    .unwrap();
    review::open(old_clone.path()).unwrap();
    let restored = Store::open(old_clone.path())
        .unwrap()
        .read_current()
        .unwrap();
    assert_eq!(restored.candidates["project"], first);

    let ambiguous = variant_clone(&first);
    assert!(
        matches!(review::open(ambiguous.path()), Err(Error::Invalid(message)) if message.contains("ambiguous"))
    );
    assert!(!ambiguous.path().join(".shield/review/state.json").exists());

    let present = variant_clone(&first);
    fs::write(
        present.path().join(".shield/tools.yml"),
        first["packs"][0].as_str().unwrap(),
    )
    .unwrap();
    review::open(present.path()).unwrap();
    assert_eq!(
        Store::open(present.path())
            .unwrap()
            .read_current()
            .unwrap()
            .candidates["project"],
        first
    );

    for text in ["{bad lock", "{\"schema_version\":1,\"packs\":[]}"] {
        let malformed = variant_clone(&first);
        fs::write(malformed.path().join(".shield/shield.lock"), text).unwrap();
        assert!(review::open(malformed.path()).is_err());
        assert!(!malformed.path().join(".shield/review/state.json").exists());
    }
}

#[test]
fn existing_only_refresh_preserves_highest_acknowledged_trust_and_catalog_floors() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let (root, context, _, _) = review::read_approval::tests::fixture();
    let store = Store::open(root.path()).unwrap();
    let source = "a".repeat(64);
    for sequence in [3, 1] {
        store.update_checkpoint(&context,&||Ok(()),|state| {
            state.operator["registry_trust"] = json!({&source:{"mode":"versioned-v1","generation":sequence,"head_revision":format!("head-{sequence}"),"object":"retained-head","root_version":1,"root_revision":"root","revocation_sequence":sequence,"revocation_revision":format!("revocations-{sequence}")}});
            state.registry_checkpoints = BTreeMap::from([(source.clone(),json!({"sequence":sequence,"digest":sequence.to_string().repeat(64),"releases":{}}))]);
            Ok(Value::Null)
        }).unwrap();
        assert!(store.refresh_policy_consent());
    }
    let index = read_index(&namespace(false).unwrap()).unwrap();
    let record = index
        .candidates
        .values()
        .next()
        .unwrap()
        .values()
        .next()
        .unwrap();
    let body = namespace(false)
        .unwrap()
        .read_json(&record.context)
        .unwrap();
    assert_eq!(body["metadata"]["registry_trust"][&source]["generation"], 3);
    assert_eq!(body["registry_checkpoints"][&source]["sequence"], 3);
    let mut conflict = body.clone();
    conflict["metadata"]["registry_trust"][&source]["head_revision"] = json!("conflicting-head");
    assert!(merge_observations(&mut conflict, &body).is_err());
    let mut conflict = body.clone();
    conflict["registry_checkpoints"][&source]["digest"] = json!("f".repeat(64));
    assert!(merge_observations(&mut conflict, &body).is_err());
    let state = store.read_checkpoint().unwrap();
    remove_record(state.operator["policy_consent_key"].as_str().unwrap());
    assert!(
        !store.refresh_policy_consent(),
        "metadata refresh cannot invent new consent"
    );
}

#[test]
fn legacy_and_local_lock_checks_refuse_mismatches_and_malformed_files() {
    let root = assert_fs::TempDir::new().unwrap();
    fs::create_dir(root.path().join(".shield")).unwrap();
    let config = root.path().join(".shield/config.yml");
    fs::write(&config, "schema_version: 1\npacks: []\n").unwrap();
    let raw = json!({"config":"schema_version: 1\npacks: []\n","local_config":null});
    fs::write(
        root.path().join(".shield/shield.lock"),
        "{\"schema_version\":1,\"packs\":[]}",
    )
    .unwrap();
    assert!(matches_legacy(&config, &raw, &json!({"installed":[]})).unwrap());
    fs::write(
        root.path().join(".shield/shield.local.lock"),
        "{\"schema_version\":1,\"packs\":[]}",
    )
    .unwrap();
    assert!(!matches_legacy(&config, &raw, &json!({"installed":[]})).unwrap());
    fs::write(
        root.path().join(".shield/shield.local.lock"),
        "invalid JSON",
    )
    .unwrap();
    assert!(matches_legacy(&config, &raw, &json!({"installed":[]})).is_err());
}

#[test]
fn shared_source_capacity_uses_a_bounded_policy_partition() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let (_root, _, _, _) = review::read_approval::tests::fixture();
    let user = namespace(false).unwrap();
    for index in 0..4096 {
        fs::write(user.directory.join(format!("unrelated-{index}")), b"").unwrap();
    }
    let next = assert_fs::TempDir::new().unwrap();
    let bundle = crate::configuration::Bundle::capture(
        "schema_version: 1\npacks: []\n# Acme source capacity\n",
        &[],
        None,
    )
    .unwrap();
    review::mcp::initialize_fixture(next.path(), &bundle);
    let state = Store::open(next.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(presence(&state)["present"], true);
    let key = state.operator["policy_consent_key"].as_str().unwrap();
    assert_ne!(
        partitions::select(key, false).unwrap().directory,
        user.directory
    );
    assert_eq!(fs::read(user.directory.join("unrelated-0")).unwrap(), b"");
}

#[test]
fn interrupted_partition_creation_preserves_shared_proof_and_retries() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let (root, _, _, _) = review::read_approval::tests::fixture();
    let state = Store::open(root.path()).unwrap().read_current().unwrap();
    let raw = &state.candidates["project"];
    let key = key(raw).unwrap();
    let shared = namespace(false).unwrap();
    let base = shared.directory.join("policies");
    private_files::directory(&base, true).unwrap();
    private_files::directory(&base.join(&key), true).unwrap();
    assert_eq!(
        partitions::select(&key, false).unwrap().directory,
        shared.directory
    );
    assert!(retained_record(raw).unwrap().is_some());
    let partition = partitions::create(&key).unwrap();
    assert_ne!(partition.directory, shared.directory);
    assert!(retained_record(raw).unwrap().is_some());
}

#[test]
fn full_shared_consent_index_does_not_prevent_new_policy_proofs() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let (root, _, _, _) = review::read_approval::tests::fixture();
    let user = namespace(false).unwrap();
    fill_shared_index(&user);
    let before = fs::read(user.directory.join("state.json")).unwrap();
    let next = assert_fs::TempDir::new().unwrap();
    let bundle = crate::configuration::Bundle::capture(
        "schema_version: 1\npacks: []\n# Acme independent policy\n",
        &[],
        None,
    )
    .unwrap();
    review::mcp::initialize_fixture(next.path(), &bundle);
    let state = Store::open(next.path()).unwrap().read_current().unwrap();
    let raw = &state.candidates["project"];
    assert_eq!(presence(&state)["present"], true);
    assert!(retained_record(raw).unwrap().is_some());
    assert_eq!(fs::read(user.directory.join("state.json")).unwrap(), before);
    assert!(Store::open(root.path()).unwrap().read_checkpoint().is_ok());
}

fn fill_shared_index(user: &Namespace) {
    let _guard = objects::Directory::enter(&user.directory).unwrap();
    let mut index = read_index(user).unwrap();
    let record = index
        .candidates
        .values()
        .next()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    let mut count = 0;
    while index.candidates.values().map(BTreeMap::len).sum::<usize>() < RECORD_LIMIT {
        index.candidates.insert(
            format!("{count:064x}"),
            BTreeMap::from([(sources::candidate_identity(&record.source), record.clone())]),
        );
        count += 1;
    }
    private_files::write(
        &user.directory.join("state.json"),
        &json!(index),
        INDEX_LIMIT,
    )
    .unwrap();
}

#[test]
fn prepared_shared_consent_follows_partition_publication_without_losing_metadata() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let (root, context, _, _) = review::read_approval::tests::fixture();
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    let prepared = PreparedConsent::prepare(
        &store,
        state.active.as_ref().unwrap().project_candidate().unwrap(),
    )
    .unwrap();
    let key = prepared.key.clone();
    let user = namespace(false).unwrap();
    fill_shared_index(&user);
    let partition = partitions::create(&key).unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.operator["projection"] = json!("f".repeat(64));
            Ok(Value::Null)
        })
        .unwrap();
    prepared
        .publish(&store, state.active.as_ref().unwrap().activation_id())
        .unwrap();
    let index = read_index(&partition).unwrap();
    let record = index.candidates[&key].values().next().unwrap();
    let body = partition.read_json(&record.context).unwrap();
    assert_eq!(body["metadata"]["projection"], "f".repeat(64));
    let hydrated = store
        .captured_candidate(state.active.as_ref().unwrap().project_candidate().unwrap())
        .unwrap();
    assert!(retained_record(&hydrated).unwrap().is_some());
}

#[test]
fn compact_index_upgrade_preserves_one_variant_and_capacity_counts_all_variants() {
    let home = assert_fs::TempDir::new().unwrap();
    let _scope = scoped_directory(home.path().join("consent"));
    let (_root, raw, _) = reviewed_variant("status");
    let user = namespace(false).unwrap();
    let index = read_index(&user).unwrap();
    let key = key(&raw).unwrap();
    let record = index.candidates[&key].values().next().unwrap().clone();
    let path = user.directory.join("state.json");
    let _guard = objects::Directory::enter(&user.directory).unwrap();
    let legacy = json!({"schema_version":2,"candidates":{&key:record}});
    private_files::write(&path, &legacy, INDEX_LIMIT).unwrap();
    let bytes = fs::read(&path).unwrap();
    let upgraded = read_index(&user).unwrap();
    assert_eq!(upgraded.schema_version, 3);
    assert_eq!(upgraded.candidates[&key].len(), 1);
    assert_eq!(
        fs::read(&path).unwrap(),
        bytes,
        "inspection does not rewrite the index"
    );
    let mut variants = BTreeMap::new();
    for index in 0..=RECORD_LIMIT {
        let mut row = record.clone();
        let digest = format!("{index:064x}");
        row.source["digest"] = json!(digest);
        variants.insert(digest, row);
    }
    private_files::write(
        &path,
        &json!({"schema_version":3,"candidates":{key:variants}}),
        INDEX_LIMIT,
    )
    .unwrap();
    assert!(
        read_index(&user).is_err(),
        "one authoring key cannot bypass the total variant cap"
    );
}
