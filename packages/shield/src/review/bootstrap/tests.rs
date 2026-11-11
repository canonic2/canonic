//! First enrollment is reviewed exactly once; a moved or copied project stays
//! enrolled and reviewed form metadata follows it.
use super::*;
use crate::review::{self, storage::Store};
use std::{fs, sync::Arc};

const CONFIG: &str = "schema_version: 1\npacks: []\nrules: rules.yml\n";
const RULES: &str = r#"{"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["inspect"]}}}]}"#;

fn fixture() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().expect("fixture root");
    let bundle = configuration::Bundle::capture(CONFIG, &[], Some(RULES)).expect("fixture policy");
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    fs::write(root.path().join(".shield/config.yml"), CONFIG).expect("fixture config");
    fs::write(root.path().join(".shield/rules.yml"), RULES).expect("fixture rules");
    (root, context)
}

fn copy(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).expect("fixture directory") {
        let entry = entry.expect("fixture entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("fixture file kind").is_dir() {
            fs::create_dir(&target).expect("fixture child directory");
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy fixture file");
        }
    }
}
fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, at: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(at).expect("fixture files") {
            let entry = entry.expect("fixture file");
            if entry.file_type().expect("fixture type").is_dir() {
                visit(root, &entry.path(), files);
            } else {
                files.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .expect("relative fixture")
                        .into(),
                    fs::read(entry.path()).expect("fixture bytes"),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}
fn add_approval(root: &Path, context: &Context) {
    let path = Path::new(&context.project_root).join("acme.txt");
    fs::write(&path, "Acme text").expect("fixture text");
    let input = json!({"protocol_version":1,"request_id":"acme-read","expected_project_id":context.project_id,
        "cwd":context.project_root,"operation":{"kind":"read-text","path":path,"offset":null,"limit":null},
        "host":{"agent":"claude-mod","session_id":"acme-session","tool_use_id":"acme-tool","event":"PreToolUse","consumer_id":"acme-session"}});
    let (receipt, code) = crate::enforcement::protocol::evaluate(
        root,
        &serde_json::to_vec(&input).expect("read input"),
    );
    assert_eq!(code, 0, "{receipt}");
    assert_eq!(receipt["code"], "approval-required", "{receipt}");
    let id = receipt["approval"]["request_id"]
        .as_str()
        .expect("pending read request");
    review::read_approval::authorize(context, id, 300, None, false).expect("fixture grant");
}

#[test]
fn bootstrap_config_only_prepare_does_not_create_local_authority() {
    let root = assert_fs::TempDir::new().unwrap();
    fs::create_dir(root.path().join(".shield")).unwrap();
    fs::write(root.path().join(".shield/config.yml"), CONFIG).unwrap();
    fs::write(root.path().join(".shield/rules.yml"), RULES).unwrap();
    let before = files(root.path());
    let plan = prepare(root.path()).unwrap();
    assert_eq!(plan.report()["classification"], "unenrolled");
    assert_eq!(files(root.path()), before);
    let context = plan.commit_cli().unwrap();
    assert_eq!(context.surface, "cli");
    assert_eq!(
        review::open(root.path()).unwrap().project_id,
        context.project_id
    );
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert!(
        state
            .authorization_events
            .iter()
            .any(|event| event["kind"] == "project_enrolled"
                && event["operator"]["mechanism"] == "operator-cli"
                && event["provenance"]["surface"] == "cli")
    );
}

/// A folder with only committed Shield files, awaiting its first enrollment.
fn unenrolled() -> assert_fs::TempDir {
    let root = assert_fs::TempDir::new().expect("fixture root");
    fs::create_dir(root.path().join(".shield")).expect("fixture policy folder");
    fs::write(root.path().join(".shield/config.yml"), CONFIG).expect("fixture config");
    fs::write(root.path().join(".shield/rules.yml"), RULES).expect("fixture rules");
    root
}

#[test]
fn bootstrap_moved_or_copied_projects_stay_enrolled_and_refuse_reenrollment() {
    let (original, old_context) = fixture();
    let parent = assert_fs::TempDir::new().unwrap();
    let moved = parent.path().join("moved");
    fs::rename(original.path(), &moved).unwrap();
    let before = files(&moved);
    let probe = inspect(&moved).unwrap();
    assert!(!probe.bootstrap_required());
    assert_eq!(probe.report()["classification"], "enrolled");
    assert!(prepare(&moved).is_err());
    assert_eq!(files(&moved), before, "probing and refusal write nothing");
    assert_eq!(
        review::open(&moved).unwrap().project_id,
        old_context.project_id
    );
    let copied = assert_fs::TempDir::new().unwrap();
    copy(&moved, copied.path());
    assert!(prepare(copied.path()).is_err());
    assert_eq!(
        review::open(copied.path()).unwrap().project_id,
        old_context.project_id
    );
}

#[test]
fn bootstrap_source_changes_require_a_new_review() {
    let root = unenrolled();
    let plan = prepare(root.path()).unwrap();
    fs::write(root.path().join(".shield/rules.yml"), format!("{RULES}\n")).unwrap();
    assert!(plan.commit().is_err());
    assert!(!root.path().join(".shield/review/state.json").exists());
    let fresh = prepare(root.path()).unwrap();
    fresh.commit().unwrap();
}

#[test]
fn bootstrap_explicit_cache_is_reviewed_before_it_is_created() {
    let root = unenrolled();
    let before = files(root.path());
    let cache = root.path().canonicalize().unwrap().join(".shield/cache");
    let default = prepare(root.path()).unwrap();
    assert!(default.metadata["compiled_cache_root"].is_null());
    let plan = prepare_options(root.path(), None, Some(&cache)).unwrap();
    assert_eq!(plan.report()["cache_dir"], json!(cache));
    assert!(
        plan.message()
            .contains(&serde_json::to_string(cache.to_str().unwrap()).unwrap())
    );
    assert_ne!(plan.fingerprint, default.fingerprint);
    assert!(!cache.exists());
    assert_eq!(
        files(root.path()),
        before,
        "cache selection is read-only before confirmation"
    );
    let context = plan.commit_cli().unwrap();
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.operator["compiled_cache_root"], json!(cache));
    assert_eq!(state.operator["bootstrap"]["cache_dir"], json!(cache));
    assert_eq!(state.project_id, context.project_id);
    assert!(cache.is_dir());
}

#[test]
fn bootstrap_cache_choices_bind_exact_paths_and_refuse_relative_input() {
    let root = unenrolled();
    let scratch = assert_fs::TempDir::new().unwrap();
    let first = scratch.path().canonicalize().unwrap().join("first");
    let second = scratch.path().canonicalize().unwrap().join("second");
    let first_plan = prepare_options(root.path(), None, Some(&first)).unwrap();
    let second_plan = prepare_options(root.path(), None, Some(&second)).unwrap();
    assert_ne!(first_plan.fingerprint, second_plan.fingerprint);
    assert!(prepare_options(root.path(), None, Some(Path::new("relative-cache"))).is_err());
    fs::write(root.path().join(".shield/rules.yml"), format!("{RULES}\n")).unwrap();
    assert!(matches!(first_plan.commit(), Err(Error::StaleSnapshot)));
    assert!(!first.exists() && !second.exists());
}

#[test]
fn bootstrap_reviewed_form_metadata_follows_moves_and_copies() {
    let root = unenrolled();
    assert!(!forms_reviewed_at(root.path()));
    prepare(root.path()).unwrap().commit().unwrap();
    assert!(forms_reviewed_at(root.path()));
    let parent = assert_fs::TempDir::new().unwrap();
    let moved = parent.path().join("moved");
    fs::rename(root.path(), &moved).unwrap();
    assert!(forms_reviewed_at(&moved));
    let copied = assert_fs::TempDir::new().unwrap();
    copy(&moved, copied.path());
    assert!(forms_reviewed_at(copied.path()));
}

#[test]
fn bootstrap_concurrent_acceptance_commits_one_enrollment() {
    let root = unenrolled();
    let plan = Arc::new(prepare(root.path()).unwrap());
    let mut threads = Vec::new();
    for _ in 0..2 {
        let plan = plan.clone();
        threads.push(std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                match plan.commit() {
                    Ok(context) => break context,
                    Err(Error::Locked) if std::time::Instant::now() < deadline => {
                        std::thread::yield_now();
                    }
                    Err(error) => panic!("concurrent enrollment failed: {error}"),
                }
            }
        }));
    }
    let first = threads.remove(0).join().unwrap();
    let second = threads.remove(0).join().unwrap();
    assert_eq!(first.project_id, second.project_id);
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(
        state
            .authorization_events
            .iter()
            .filter(|event| event["kind"] == "project_enrolled")
            .count(),
        1
    );
}

#[test]
fn bootstrap_probe_of_a_copy_with_malformed_policy_writes_nothing() {
    let (original, _) = fixture();
    let copied = assert_fs::TempDir::new().unwrap();
    copy(original.path(), copied.path());
    fs::write(
        copied.path().join(".shield/rules.yml"),
        "this is not a rules document",
    )
    .unwrap();
    let before = files(copied.path());
    let probe = inspect(copied.path()).unwrap();
    assert!(!probe.bootstrap_required());
    assert_eq!(probe.project_root(), copied.path().canonicalize().unwrap());
    assert_eq!(probe.report()["classification"], "enrolled");
    assert!(prepare(copied.path()).is_err());
    assert_eq!(files(copied.path()), before);
}

#[test]
fn bootstrap_refuses_corrupt_future_or_healthy_state_without_writes() {
    let (original, _) = fixture();
    assert!(prepare(original.path()).is_err());
    let copied = assert_fs::TempDir::new().unwrap();
    copy(original.path(), copied.path());
    let path = copied.path().join(".shield/review/state.json");
    let mut future: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    future["schema_version"] = json!(2);
    for bytes in [future.to_string().into_bytes(), b"not valid JSON".to_vec()] {
        fs::write(&path, &bytes).unwrap();
        let before = files(copied.path());
        assert!(prepare(copied.path()).is_err());
        assert_eq!(files(copied.path()), before);
    }
}

#[test]
fn bootstrap_probe_of_config_only_or_current_projects_reads_only_location_metadata() {
    let root = assert_fs::TempDir::new().unwrap();
    fs::create_dir(root.path().join(".shield")).unwrap();
    fs::write(
        root.path().join(".shield/config.yml"),
        "malformed: [ configuration",
    )
    .unwrap();
    let before = files(root.path());
    let probe = inspect(root.path()).unwrap();
    assert!(probe.bootstrap_required());
    assert_eq!(probe.report()["classification"], "unenrolled");
    assert!(prepare(root.path()).is_err());
    assert_eq!(files(root.path()), before);
    assert!(!root.path().join(".shield/review").exists());
    let (enrolled, _) = fixture();
    let before = files(enrolled.path());
    let current = inspect(enrolled.path()).unwrap();
    assert!(!current.bootstrap_required());
    assert_eq!(current.report()["classification"], "enrolled");
    assert_eq!(files(enrolled.path()), before);
}

#[test]
fn bootstrap_probe_refuses_future_and_corrupt_checkpoint_headers_without_writes() {
    let (original, _) = fixture();
    let copied = assert_fs::TempDir::new().unwrap();
    copy(original.path(), copied.path());
    let path = copied.path().join(".shield/review/state.json");
    let mut raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    raw["schema_version"] = json!(2);
    fs::write(&path, raw.to_string()).unwrap();
    let before = files(copied.path());
    assert!(matches!(
        inspect(copied.path()),
        Err(Error::StateFormat { .. })
    ));
    assert_eq!(files(copied.path()), before);
    for malformed in [
        "not JSON",
        r#"{"schema_version":1,"project_root":"relative"}"#,
    ] {
        fs::write(&path, malformed).unwrap();
        let before = files(copied.path());
        assert!(matches!(
            inspect(copied.path()),
            Err(Error::StateCorrupt { .. })
        ));
        assert_eq!(files(copied.path()), before);
    }
}

#[cfg(unix)]
#[test]
fn bootstrap_rejects_symlinked_configuration_and_escapes_untrusted_path_display() {
    let root = assert_fs::TempDir::new().unwrap();
    let external = assert_fs::TempDir::new().unwrap();
    fs::create_dir(root.path().join(".shield")).unwrap();
    fs::write(external.path().join("config.yml"), CONFIG).unwrap();
    std::os::unix::fs::symlink(
        external.path().join("config.yml"),
        root.path().join(".shield/config.yml"),
    )
    .unwrap();
    assert!(prepare(root.path()).is_err());
    assert_eq!(
        display("/acme\nconfirm\u{202e}", 256),
        "\"/acme\\nconfirm\\u202e\""
    );
}

fn record_integration_review(
    state: &mut review::model::State,
    context: &Context,
    review_id: &str,
    mechanism: &str,
) -> Value {
    let marker = json!({"project_id":state.project_id,"review_id":review_id});
    state.operator["integration_review"] = marker.clone();
    let mut event = state.event(
        "integration_review_confirmed",
        context,
        json!({}),
        json!({}),
        json!({"review_channel":marker}),
    );
    event["operator"] = json!({"operator_id":null,"display_name":null,"mechanism":mechanism});
    state.record(event);
    marker
}

#[test]
fn integration_review_forms_require_exact_human_proof_and_current_project() {
    for mechanism in ["host-elicitation", "operator-cli"] {
        let (root, context) = fixture();
        let store = Store::open(root.path()).unwrap();
        assert!(!forms_reviewed_at(root.path()));
        store
            .update(&context, &|| Ok(()), |state| {
                record_integration_review(state, &context, "review-confirmed", mechanism);
                Ok(json!({}))
            })
            .unwrap();
        assert!(forms_reviewed_at(root.path()));
        let path = root.path().join(".shield/review/state.json");
        let baseline = fs::read(&path).unwrap();
        for fault in [
            "mechanism",
            "review-id",
            "empty-id",
            "missing-event",
            "project-id",
            "event-project",
        ] {
            let mut raw: Value = serde_json::from_slice(&baseline).unwrap();
            let marker = &mut raw["operator"]["integration_review"];
            match fault {
                "review-id" => marker["review_id"] = json!("another-review"),
                "empty-id" => marker["review_id"] = json!(""),
                "project-id" => marker["project_id"] = json!("f".repeat(64)),
                _ => {}
            }
            let events = raw["authorization_events"].as_array_mut().unwrap();
            if fault == "missing-event" {
                events.retain(|event| event["kind"] != "integration_review_confirmed");
            } else {
                let event = events
                    .iter_mut()
                    .find(|event| event["kind"] == "integration_review_confirmed")
                    .unwrap();
                match fault {
                    "mechanism" => event["operator"]["mechanism"] = json!("host-tool-permission"),
                    "empty-id" => event["details"]["review_channel"]["review_id"] = json!(""),
                    "event-project" => event["provenance"]["project_id"] = json!("f".repeat(64)),
                    _ => {}
                }
            }
            fs::write(&path, raw.to_string()).unwrap();
            assert!(!forms_reviewed_at(root.path()), "{mechanism}: {fault}");
        }
        fs::write(&path, baseline).unwrap();
        assert!(forms_reviewed_at(root.path()));
        let copied = assert_fs::TempDir::new().unwrap();
        copy(root.path(), copied.path());
        assert!(
            forms_reviewed_at(copied.path()),
            "reviewed form capability metadata is portable"
        );
    }
}

#[test]
fn integration_review_compaction_and_history_purge_preserve_only_current_proof() {
    let (root, context) = fixture();
    add_approval(root.path(), &context);
    let store = Store::open(root.path()).unwrap();
    let before = store.read_checkpoint().unwrap();
    let authority = json!({"active":before.active,"read":before.read_requests,"run":before.run_requests,
        "hooks":before.hook_requests,"sessions":before.session_grants});
    store
        .update(&context, &|| Ok(()), |state| {
            state.history_enabled = false;
            record_integration_review(state, &context, "older-review", "host-elicitation");
            let current =
                record_integration_review(state, &context, "current-review", "host-elicitation");
            let mut foreign = state
                .authorization_events
                .iter()
                .find(|event| {
                    event["kind"] == "integration_review_confirmed"
                        && event["details"]["review_channel"] == current
                })
                .unwrap()
                .clone();
            foreign["provenance"]["project_id"] = json!("f".repeat(64));
            state.record(foreign);
            state.compact_authorization();
            let retained: Vec<_> = state
                .authorization_events
                .iter()
                .filter(|event| event["kind"] == "integration_review_confirmed")
                .collect();
            assert_eq!(retained.len(), 1);
            assert_eq!(retained[0]["details"]["review_channel"], current);
            Ok(json!({}))
        })
        .unwrap();
    assert!(forms_reviewed_at(root.path()));
    review::admin::prepare(
        &context,
        "history_configure",
        &json!({"enabled":false,"purge":true}),
        &|| Ok(()),
    )
    .unwrap()
    .commit(&context, &|| Ok(()))
    .unwrap();
    let after = store.read_checkpoint().unwrap();
    assert!(!after.history_enabled);
    assert!(store.read_history().unwrap().events.is_empty());
    assert!(forms_reviewed_at(root.path()));
    assert_eq!(
        json!({"active":after.active,"read":after.read_requests,"run":after.run_requests,
        "hooks":after.hook_requests,"sessions":after.session_grants}),
        authority
    );
    assert_eq!(
        after
            .authorization_events
            .iter()
            .filter(|event| event["kind"] == "integration_review_confirmed")
            .count(),
        1
    );
}
