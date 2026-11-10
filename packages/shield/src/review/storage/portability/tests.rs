//! Policy follows moves and source-only clones; transient grants do not.
use super::*;
use crate::{configuration::Bundle, review};
use std::{fs, path::Path};

const CONFIG: &str = "schema_version: 1\npacks: []\nrules: rules.yml\n";
const RULES: &str = r#"{"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"portable-inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["inspect"]}}}]}"#;

fn fixture() -> (assert_fs::TempDir, Context) {
    consent::fixture_namespace(
        std::thread::current()
            .name()
            .unwrap_or("portable-policy-fixture"),
    );
    let root = assert_fs::TempDir::new().unwrap();
    let bundle = Bundle::capture(CONFIG, &[], Some(RULES)).unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    fs::write(root.path().join(".shield/config.yml"), CONFIG).unwrap();
    fs::write(root.path().join(".shield/rules.yml"), RULES).unwrap();
    (root, context)
}

fn copy(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir(&target).unwrap();
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn portable_policy_move_opens_without_confirmation_and_keeps_policy() {
    let (original, before) = fixture();
    let parent = assert_fs::TempDir::new().unwrap();
    let destination = parent.path().join("moved");
    fs::rename(original.path(), &destination).unwrap();
    let context = review::open(&destination).unwrap();
    assert_eq!(
        Path::new(&context.project_root),
        destination.canonicalize().unwrap()
    );
    assert_ne!(context.project_id, before.project_id);
    let state = Store::open(&destination).unwrap().read_current().unwrap();
    assert_eq!(state.candidates["project"]["rules"], RULES);
    assert_eq!(
        fs::read_to_string(destination.join(".shield/rules.yml")).unwrap(),
        RULES
    );
    assert!(
        crate::runtime::Project::open(&destination)
            .unwrap()
            .snapshot()
            .is_ok()
    );
    let again = review::open(&destination).unwrap();
    assert_eq!(
        again.project_id, context.project_id,
        "ordinary reopen is stable"
    );
    let checkpoint = destination.join(".shield/review/state.json");
    let bytes = fs::read(&checkpoint).unwrap();
    let modified = fs::metadata(&checkpoint).unwrap().modified().unwrap();
    let receipt_key = key(&state.candidates["project"]).unwrap();
    let receipt = consent::record(&receipt_key).unwrap();
    for _ in 0..3 {
        review::open(&destination).unwrap();
    }
    assert_eq!(fs::read(&checkpoint).unwrap(), bytes);
    assert_eq!(
        fs::metadata(&checkpoint).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(consent::record(&receipt_key).unwrap(), receipt);
}

#[test]
fn portable_policy_diagnostics_preview_a_move_without_publishing_or_following_the_old_root() {
    let (original, before) = fixture();
    let parent = assert_fs::TempDir::new().unwrap();
    let destination = parent.path().join("moved");
    fs::create_dir(&destination).unwrap();
    copy(original.path(), &destination);
    let checkpoint = destination.join(".shield/review/state.json");
    let bytes = fs::read(&checkpoint).unwrap();
    let context = review::open_diagnostics(&destination).unwrap();
    assert_eq!(
        Path::new(&context.project_root),
        destination.canonicalize().unwrap()
    );
    assert_eq!(context.project_id, before.project_id);
    let state = Store::inspect(&destination)
        .unwrap()
        .read_diagnostic_checkpoint()
        .unwrap()
        .0;
    state.check_inspection(&context).unwrap();
    assert!(
        state.check(&context).is_err(),
        "a diagnostic preview cannot authorize the old epoch here"
    );
    let status = review::project::status(&context).unwrap();
    assert_eq!(status["runtime_relocation_pending"], true);
    assert_eq!(status["project_root"], context.project_root);
    assert_eq!(status["retained_project_root"], before.project_root);
    review::project::doctor(&context).unwrap();
    assert_eq!(fs::read(&checkpoint).unwrap(), bytes);
    assert_eq!(
        review::open(original.path()).unwrap().project_id,
        before.project_id
    );
    let runtime = review::open(&destination).unwrap();
    assert_ne!(runtime.project_id, before.project_id);
}

#[test]
fn portable_policy_diagnostics_neither_seed_receipts_nor_adopt_source_only_clones() {
    let (original, context) = fixture();
    let checkpoint = original.path().join(".shield/review/state.json");
    let mut raw: Value = serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    raw["operator"]
        .as_object_mut()
        .unwrap()
        .remove("portable_policy");
    fs::write(&checkpoint, raw.to_string()).unwrap();
    let bytes = fs::read(&checkpoint).unwrap();
    let state = Store::inspect(original.path())
        .unwrap()
        .read_diagnostic_checkpoint()
        .unwrap()
        .0;
    let candidate = Store::inspect(original.path())
        .unwrap()
        .captured_candidate(state.active.as_ref().unwrap().project_candidate().unwrap())
        .unwrap();
    let receipt = key(&candidate).unwrap();
    consent::remove_record(&receipt);
    review::project::status(&context).unwrap();
    review::project::doctor(&context).unwrap();
    let mut caller = context.clone();
    caller.surface = "mcp".into();
    caller.connection_id = Some("portable-readonly".into());
    let connection = review::mcp::Connection::new("portable-readonly".into());
    let report = review::mcp::call(&caller, &connection, "status", &json!({}), &|| Ok(()));
    assert!(report["error"].is_null(), "{report}");
    assert_eq!(fs::read(&checkpoint).unwrap(), bytes);
    assert!(consent::record(&receipt).is_none());
    let mutex = original.path().join(".shield/review/sources/.objects.lock");
    fs::remove_file(&mutex).unwrap();
    let report = review::mcp::call(&caller, &connection, "status", &json!({}), &|| Ok(()));
    assert!(report["error"].is_null(), "{report}");
    assert!(report["data"]["active_candidate_id"].is_null());
    assert!(
        !mutex.exists(),
        "status never creates a missing source mutex"
    );
    drop(connection);
    // Actual use seeds the developer's receipt; later source-only diagnosis
    // still cannot turn that content consent into a new active checkpoint.
    Store::open(original.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    assert!(consent::record(&receipt).is_some());
    let clone = assert_fs::TempDir::new().unwrap();
    fs::create_dir(clone.path().join(".shield")).unwrap();
    fs::write(clone.path().join(".shield/config.yml"), CONFIG).unwrap();
    fs::write(clone.path().join(".shield/rules.yml"), RULES).unwrap();
    assert!(review::open_diagnostics(clone.path()).is_err());
    assert!(
        review::bootstrap::inspect(clone.path())
            .unwrap()
            .bootstrap_required()
    );
    assert!(!clone.path().join(".shield/review").exists());
    assert!(review::open(clone.path()).is_ok());
}

#[test]
fn portable_policy_move_preserves_lineage_mode_and_existing_journal() {
    let (original, context) = fixture();
    let store = Store::open(original.path()).unwrap();
    let mut state = store.read_current().unwrap();
    let lineage = state.policy_lineage().to_owned();
    let generation = state.history_generation.clone();
    state.enforcement = serde_json::from_value(json!({"enabled":false,"generation":3})).unwrap();
    let request_id = state.id("request");
    let event = state.event(
        "decision",
        &context,
        json!({"request_id":request_id}),
        json!({}),
        json!({"marker":"portable-history"}),
    );
    state.record(event);
    store.write(&mut state).unwrap();
    let destination = assert_fs::TempDir::new().unwrap();
    copy(original.path(), destination.path());
    let moved = Store::open(destination.path())
        .unwrap()
        .read_history()
        .unwrap();
    assert_ne!(moved.project_id, context.project_id);
    assert_eq!(moved.policy_lineage(), lineage);
    assert_eq!(
        moved.history_owner.as_deref(),
        Some(context.project_id.as_str())
    );
    assert_eq!(moved.history_generation, generation);
    assert!(!moved.enforcement.enabled);
    assert!(moved.history_available);
    assert!(
        moved
            .events
            .iter()
            .any(|event| event["links"]["request_id"] == request_id
                && event["details"]["marker"] == "portable-history")
    );
    let active = json!(moved.active);
    assert_eq!(active["binding_version"], 2);
    let unchanged = store.read_current().unwrap();
    assert_eq!(unchanged.project_id, context.project_id);
    assert_eq!(unchanged.policy_lineage(), lineage);
}

thread_local! {
    static RELOCATION: std::cell::RefCell<(PathBuf, usize)> = const {
        std::cell::RefCell::new((PathBuf::new(), 0))
    };
}

fn relocation_prepared(stage: super::super::tests::Stage) {
    if stage != super::super::tests::Stage::RelocationPrepared {
        return;
    }
    RELOCATION.with(|slot| {
        let mut capture = slot.borrow_mut();
        let store = Store::open(&capture.0).unwrap();
        let guard = store
            .lock(&|| Ok(()))
            .expect("preparation holds no authority lock");
        if capture.1 == 0 {
            let mut value: Value =
                serde_json::from_slice(&store.checkpoint_bytes().unwrap()).unwrap();
            value["history_limit"] = json!(17);
            fs::write(store.directory.join("state.json"), value.to_string()).unwrap();
        }
        capture.1 += 1;
        guard.release().unwrap();
    });
}

#[test]
fn portable_policy_relocation_stops_after_repeated_checkpoint_changes() {
    fn always_changes(stage: super::super::tests::Stage) {
        if stage != super::super::tests::Stage::RelocationPrepared {
            return;
        }
        RELOCATION.with(|slot| {
            let mut capture = slot.borrow_mut();
            let store = Store::open(&capture.0).unwrap();
            let mut bytes = store.checkpoint_bytes().unwrap();
            bytes.push(b'\n');
            fs::write(store.directory.join("state.json"), bytes).unwrap();
            capture.1 += 1;
        });
    }
    let (original, _) = fixture();
    let destination = assert_fs::TempDir::new().unwrap();
    copy(original.path(), destination.path());
    RELOCATION.with(|slot| *slot.borrow_mut() = (destination.path().to_owned(), 0));
    super::super::tests::set_observer(Some(always_changes));
    let result = Store::open(destination.path()).unwrap().read_checkpoint();
    super::super::tests::set_observer(None);
    assert!(matches!(result, Err(Error::StaleSnapshot)));
    RELOCATION.with(|slot| assert_eq!(slot.borrow().1, RELOCATION_ATTEMPTS));
    crate::review::publication::ProjectionLease::acquire(destination.path()).unwrap();
}

#[test]
fn portable_policy_relocation_rechecks_competing_changes_after_unlocked_preparation() {
    let (original, _) = fixture();
    let destination = assert_fs::TempDir::new().unwrap();
    copy(original.path(), destination.path());
    RELOCATION.with(|slot| *slot.borrow_mut() = (destination.path().to_owned(), 0));
    super::super::tests::set_observer(Some(relocation_prepared));
    let state = Store::open(destination.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    super::super::tests::set_observer(None);
    assert_eq!(state.history_limit, 17);
    RELOCATION.with(|slot| assert_eq!(slot.borrow().1, 2));
}

#[test]
fn portable_policy_source_clone_reuses_missing_pack_bytes_but_refuses_changed_artifacts() {
    let original = assert_fs::TempDir::new().unwrap();
    let pack = json!({"schema_version":1,"id":"acme/portable","version":"0.1.0","description":"Acme","license":"UNLICENSED","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[],"kind":"pack"}).to_string();
    let config = "schema_version: 1\npacks:\n- id: acme/portable\n  version: 0.1.0\n  profile: recommended\n  path: cache/portable.yml\n";
    let bundle = Bundle::capture(config, std::slice::from_ref(&pack), None).unwrap();
    review::mcp::initialize_fixture(original.path(), &bundle);
    let destination = assert_fs::TempDir::new().unwrap();
    fs::create_dir(destination.path().join(".shield")).unwrap();
    fs::write(destination.path().join(".shield/config.yml"), config).unwrap();
    let context = review::open(destination.path()).unwrap();
    let state = Store::open(destination.path())
        .unwrap()
        .read_current()
        .unwrap();
    assert_eq!(state.candidates["project"]["packs"][0], pack);
    assert!(
        crate::runtime::Project::open(destination.path())
            .unwrap()
            .snapshot()
            .is_ok()
    );
    assert!(state.read_requests.is_empty());
    let changed = assert_fs::TempDir::new().unwrap();
    fs::create_dir_all(changed.path().join(".shield/cache")).unwrap();
    fs::write(changed.path().join(".shield/config.yml"), config).unwrap();
    fs::write(
        changed.path().join(".shield/cache/portable.yml"),
        pack.replace("Acme", "Changed"),
    )
    .unwrap();
    assert!(review::open(changed.path()).is_err());
    assert!(!changed.path().join(".shield/review/state.json").exists());
    assert_ne!(
        review::open(original.path()).unwrap().project_id,
        context.project_id
    );
}

#[test]
fn portable_policy_existing_installation_registers_reviewed_source_once() {
    let (original, _) = fixture();
    let checkpoint = original.path().join(".shield/review/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    state["operator"]
        .as_object_mut()
        .unwrap()
        .remove("portable_policy");
    fs::write(&checkpoint, serde_json::to_vec(&state).unwrap()).unwrap();
    let context = review::open(original.path()).unwrap();
    assert_eq!(context.project_id, state["project_id"]);
    let seeded = fs::read(&checkpoint).unwrap();
    review::open(original.path()).unwrap();
    assert_eq!(fs::read(&checkpoint).unwrap(), seeded);
}

#[test]
fn portable_policy_repeated_moves_bound_recovery_and_keep_latest_runtime() {
    let (original, _) = fixture();
    let parent = assert_fs::TempDir::new().unwrap();
    let mut current = original.path().to_owned();
    for index in 0..4 {
        let destination = parent.path().join(format!("move-{index}"));
        fs::rename(&current, &destination).unwrap();
        let context = review::open(&destination).unwrap();
        assert_eq!(
            Path::new(&context.project_root),
            destination.canonicalize().unwrap()
        );
        let backups = fs::read_dir(destination.join(".shield/review"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("bootstrap-")
            })
            .count();
        assert!(
            backups <= 2,
            "predecessor checkpoints stay bounded: {backups}"
        );
        assert!(
            crate::runtime::Project::open(&destination)
                .unwrap()
                .snapshot()
                .is_ok()
        );
        current = destination;
    }
}

#[cfg(unix)]
#[test]
fn portable_policy_linked_external_configuration_does_not_borrow_its_receipt() {
    let (original, _) = fixture();
    let destination = assert_fs::TempDir::new().unwrap();
    fs::create_dir(destination.path().join(".shield")).unwrap();
    std::os::unix::fs::symlink(
        original.path().join(".shield/config.yml"),
        destination.path().join(".shield/config.yml"),
    )
    .unwrap();
    assert!(review::open(destination.path()).is_err());
    assert!(
        !destination
            .path()
            .join(".shield/review/state.json")
            .exists()
    );
}

#[test]
fn portable_policy_copy_does_not_activate_unreviewed_edits_or_change_original() {
    let (original, before) = fixture();
    let path = original.path().canonicalize().unwrap().join("acme.txt");
    fs::write(&path, "Acme text").unwrap();
    let request = json!({"protocol_version":1,"request_id":"portable-read","expected_project_id":before.project_id,
        "cwd":before.project_root,"operation":{"kind":"read-text","path":path,"offset":null,"limit":null},
        "host":{"agent":"claude-mod","session_id":"acme-session","tool_use_id":"acme-tool","event":"PreToolUse","consumer_id":"acme-session"}});
    let (receipt, code) = crate::enforcement::protocol::evaluate(
        original.path(),
        &serde_json::to_vec(&request).unwrap(),
    );
    assert_eq!(code, 0, "{receipt}");
    review::read_approval::authorize(
        &before,
        receipt["approval"]["request_id"].as_str().unwrap(),
        300,
        None,
        false,
    )
    .unwrap();
    assert!(
        !Store::open(original.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .read_requests
            .is_empty()
    );
    let destination = assert_fs::TempDir::new().unwrap();
    copy(original.path(), destination.path());
    let original_state = fs::read(original.path().join(".shield/review/state.json")).unwrap();
    fs::write(
        destination.path().join(".shield/rules.yml"),
        RULES.replace("allow", "deny"),
    )
    .unwrap();
    let context = review::open(destination.path()).unwrap();
    assert_ne!(context.project_id, before.project_id);
    let state = Store::open(destination.path())
        .unwrap()
        .read_current()
        .unwrap();
    assert_eq!(state.candidates["project"]["rules"], RULES);
    assert!(state.read_requests.is_empty());
    assert!(state.run_requests.is_empty());
    assert!(state.hook_requests.is_empty());
    assert!(state.proposals.is_empty());
    assert_eq!(
        fs::read(original.path().join(".shield/review/state.json")).unwrap(),
        original_state
    );
}

#[test]
fn portable_policy_reuse_keeps_reviewed_source_when_disposable_cache_is_unavailable() {
    let (_original, before) = fixture();
    let destination = assert_fs::TempDir::new().unwrap();
    let cache = assert_fs::TempDir::new().unwrap();
    let blocked = cache.path().join("blocked");
    fs::write(&blocked, b"preserve cache obstruction").unwrap();
    let bundle = Bundle::capture(CONFIG, &[], Some(RULES)).unwrap();
    let candidates = BTreeMap::from([("project".into(), json!(bundle))]);
    let evaluator =
        crate::configuration::Evaluator::new(review::snapshots(&candidates).unwrap()).unwrap();
    Store::initialize_with(
        destination.path(),
        candidates,
        evaluator.revision(),
        json!({"sources":{},"installed":[],"compiled_cache_root":blocked}),
        Some(super::super::InitialActivation::Reused(&evaluator)),
        BTreeMap::new(),
    )
    .unwrap();
    let context = review::open(destination.path()).unwrap();
    assert_ne!(context.project_id, before.project_id);
    let store = Store::open(destination.path()).unwrap();
    let state = store.read_current().unwrap();
    assert_eq!(state.candidates["project"]["rules"], RULES);
    assert!(state.active.is_some());
    assert!(state.read_requests.is_empty() && state.run_requests.is_empty());
    assert_eq!(fs::read(&blocked).unwrap(), b"preserve cache obstruction");
    assert!(matches!(
        crate::runtime::Project::open(destination.path())
            .unwrap()
            .snapshot(),
        Err(crate::runtime::Error::CacheUnavailable)
    ));
    assert_eq!(
        store.read_checkpoint().unwrap().project_id,
        context.project_id
    );
}

#[test]
fn portable_policy_source_only_clone_reuses_exact_private_receipt() {
    let (_original, before) = fixture();
    let destination = assert_fs::TempDir::new().unwrap();
    fs::create_dir(destination.path().join(".shield")).unwrap();
    fs::write(destination.path().join(".shield/config.yml"), CONFIG).unwrap();
    fs::write(destination.path().join(".shield/rules.yml"), RULES).unwrap();
    let context = review::open(destination.path()).unwrap();
    assert_ne!(context.project_id, before.project_id);
    assert_eq!(
        Path::new(&context.project_root),
        destination.path().canonicalize().unwrap()
    );
    let state = Store::open(destination.path())
        .unwrap()
        .read_current()
        .unwrap();
    assert!(
        state
            .authorization_events
            .iter()
            .any(|event| event["kind"] == "runtime_activated"
                && event["operator"]["mechanism"] == "reviewed-policy-reuse")
    );
    assert!(
        crate::runtime::Project::open(destination.path())
            .unwrap()
            .snapshot()
            .is_ok()
    );
    assert!(review::bootstrap::forms_reviewed_at(destination.path()));
    Store::open(destination.path())
        .unwrap()
        .update(&context, &|| Ok(()), |state| {
            let compiled = review::cache::build(state, None)?;
            runtime::publish(state, &context, None, compiled);
            Ok(json!({}))
        })
        .unwrap();
    assert!(
        review::bootstrap::forms_reviewed_at(destination.path()),
        "policy confirmation remains reachable after reactivation and reconnect"
    );
}

#[test]
fn portable_policy_source_edit_and_forged_receipt_stay_unreviewed() {
    let (_original, _) = fixture();
    let destination = assert_fs::TempDir::new().unwrap();
    fs::create_dir(destination.path().join(".shield")).unwrap();
    fs::write(destination.path().join(".shield/config.yml"), CONFIG).unwrap();
    fs::write(
        destination.path().join(".shield/rules.yml"),
        RULES.replace("inspect", "edit"),
    )
    .unwrap();
    assert!(review::open(destination.path()).is_err());
    assert!(
        !destination
            .path()
            .join(".shield/review/state.json")
            .exists()
    );
    let candidate = json!(
        review::integrations::project_sources::capture(
            &destination.path().join(".shield/config.yml")
        )
        .unwrap()
    );
    let receipt = directory()
        .unwrap()
        .join(format!("{}.json", key(&candidate).unwrap()));
    private_files::write(
        &receipt,
        &json!({"schema_version":1,"candidate":{},"metadata":{}}),
        LIMIT,
    )
    .unwrap();
    assert!(review::open(destination.path()).is_err());
    assert!(
        !destination
            .path()
            .join(".shield/review/state.json")
            .exists()
    );
}

#[test]
fn portable_policy_corrupt_activation_is_not_relocated() {
    let (original, _) = fixture();
    let destination = assert_fs::TempDir::new().unwrap();
    copy(original.path(), destination.path());
    let path = destination.path().join(".shield/review/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["active"]["binding"] = json!("invalid");
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(review::open(destination.path()).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn portable_policy_reuse_never_overwrites_an_existing_dangling_checkpoint_link() {
    let (_original, _) = fixture();
    let clone = assert_fs::TempDir::new().unwrap();
    fs::create_dir(clone.path().join(".shield")).unwrap();
    fs::create_dir(clone.path().join(".shield/review")).unwrap();
    fs::write(clone.path().join(".shield/config.yml"), CONFIG).unwrap();
    fs::write(clone.path().join(".shield/rules.yml"), RULES).unwrap();
    let target = clone.path().join("absent-state");
    let path = clone.path().join(".shield/review/state.json");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(review::open(clone.path()).is_err());
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!target.exists());
}
