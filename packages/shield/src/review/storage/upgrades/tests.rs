use super::*;
use crate::review::{self, runtime::consumer, storage::Store};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

struct OwnedChild(std::process::Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn historical_fixture() -> (assert_fs::TempDir, review::Context, Value) {
    let root = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],
        "rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["status"]}}},
        {"id":"destroy","decision":"deny","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["reset","--hard"]}}}]}).to_string();
    let bundle = crate::configuration::Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    let mut raw = earlier_checkpoint(root.path());
    raw["project_root"] = json!(context.project_root);
    let object = raw.as_object_mut().unwrap();
    for key in [
        "hook_requests",
        "session_grants",
        "policy_drafts",
        "dismissed_reviews",
        "enforcement",
        "followed_files",
    ] {
        object.remove(key);
    }
    legacy_activation(&mut raw);
    raw["history_generation"] = Value::Null;
    (root, context, raw)
}

/// The current checkpoint as earlier formats wrote it, with a copy of the
/// policy the files hold.
pub(super) fn earlier_checkpoint(root: &Path) -> Value {
    let current = Store::open(root).unwrap().read_following_files().unwrap();
    let mut raw = serde_json::to_value(&current).unwrap();
    raw["candidates"] = json!(current.candidates);
    raw["policy_revision"] = json!(current.policy_revision);
    raw
}

/// Reproduce the original legacy proof; dropping current required fields alone
/// must never manufacture an accepted historical activation.
pub(super) fn legacy_activation(raw: &mut Value) {
    assert!(
        raw["project_root"].is_string(),
        "the earlier format always recorded its root"
    );
    raw.as_object_mut().unwrap().remove("configuration_changes");
    let candidate_digest =
        crate::review::map_digest(&serde_json::from_value(raw["candidates"].clone()).unwrap());
    let mut active = json!({"activation_id":"acme-original-activation",
        "previous_activation_id":null,"policy_revision":raw["policy_revision"],
        "candidate_digest":candidate_digest,"candidates":raw["candidates"]});
    let binding = crate::policy::fingerprint(
        json!([
            raw["project_id"],
            raw["project_root"],
            active["activation_id"],
            active["previous_activation_id"],
            active["policy_revision"],
            active["candidate_digest"]
        ])
        .to_string()
        .as_bytes(),
    );
    active["binding"] = json!(binding);
    raw["active"] = active;
    raw["authorization_events"].as_array_mut().unwrap().push(
        json!({"schema_version":1,"event_id":"acme-original-activation-event",
            "kind":"runtime_activated","links":{"activation_id":"acme-original-activation"},
            "details":{"binding":binding}}),
    );
}

fn save(root: &Path, raw: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(raw).unwrap();
    fs::write(root.join(".shield/state/state.json"), &bytes).unwrap();
    bytes
}

fn assert_policy(_root: &Path, context: &review::Context) {
    let snapshot = consumer::load(context, &|| Ok(())).unwrap();
    for (arguments, expected) in [
        (vec!["status"], crate::policy::Decision::Allow),
        (vec!["reset", "--hard"], crate::policy::Decision::Deny),
        (vec!["push"], crate::policy::Decision::Ask),
    ] {
        let evaluated = snapshot
            .evaluate(crate::policy::Request {
                executable: "git".into(),
                arguments: arguments.into_iter().map(str::to_owned).collect(),
                cwd: context.project_root.clone(),
                project: context.project_root.clone(),
            })
            .unwrap();
        assert_eq!(evaluated.evaluation().decision, expected);
    }
}

/// A checkpoint an earlier release wrote with its copy of the policy converts
/// on the first runtime read: the copy is gone, the original bytes are kept
/// privately, the conversion is recorded once and the files stay the policy.
#[test]
fn upgrade_checkpoint_with_a_policy_copy_converts_on_first_read() {
    let (root, _) = review::tests::enrolled();
    let mut raw = earlier_checkpoint(root.path());
    raw["followed_files"] = json!(review::files::digest(root.path()).unwrap());
    let revision = raw["policy_revision"].clone();
    let bytes = save(root.path(), &raw);
    let store = Store::open(root.path()).unwrap();
    // Inspection converts in memory and writes nothing.
    let (_, pending) = store.read_diagnostic_checkpoint().unwrap();
    assert!(pending);
    assert_eq!(store.checkpoint_bytes().unwrap(), bytes);
    let state = store.read_checkpoint().unwrap();
    assert_eq!(state.policy_revision, "");
    assert!(state.candidates.is_empty());
    let saved: Value = serde_json::from_slice(&store.checkpoint_bytes().unwrap()).unwrap();
    for key in ["candidates", "policy_revision", "followed_files"] {
        assert!(saved.get(key).is_none(), "{key}: {saved}");
    }
    let original = crate::policy::fingerprint(&bytes);
    assert_eq!(saved["operator"]["upgrade_recovery"], json!([original]));
    assert_eq!(
        fs::read(store.directory.join(format!("upgrade-{original}.json"))).unwrap(),
        bytes
    );
    // The old copy and the files hold the same policy.
    assert_eq!(saved["operator"]["upgrade_notice"]["policy"]["same"], true);
    assert_eq!(super::fixtures::conversions(root.path()), 1);
    // Repeated access converts nothing again.
    let after = store.checkpoint_bytes().unwrap();
    store.read_checkpoint().unwrap();
    assert_eq!(store.checkpoint_bytes().unwrap(), after);
    assert_eq!(super::fixtures::conversions(root.path()), 1);
    assert_eq!(
        store.read_following_files().unwrap().policy_revision,
        revision
    );
    let (_, pending) = store.read_diagnostic_checkpoint().unwrap();
    assert!(!pending);
}

#[test]
fn upgrade_dispatch_keeps_current_policy_and_approval_state_read_only() {
    let (root, _) = review::tests::enrolled();
    let path = root.path().join(".shield/state/state.json");
    let bytes = fs::read(&path).unwrap();
    let before: Value = serde_json::from_slice(&bytes).unwrap();
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    assert_eq!(state.project_id, before["project_id"]);
    // A checkpoint holds no copy of the policy.
    assert!(before.get("policy_revision").is_none());
    assert!(before.get("candidates").is_none());
    assert_eq!(state.policy_revision, "");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    for version in [0, 2] {
        let mut raw = before.clone();
        raw["schema_version"] = json!(version);
        let bytes = save(root.path(), &raw);
        let Err(error) = store.read_checkpoint() else {
            panic!("unsupported schema accepted")
        };
        assert!(matches!(error, Error::StateFormat { .. }));
        let message = error.to_string();
        assert!(
            message.contains(&format!("observed schema {version}")),
            "{message}"
        );
        assert!(message.contains("supported schema 1"));
        assert!(message.contains("state.json"));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn upgrade_historical_active_source_preserves_identity_decisions_and_evidence() {
    let (root, context, raw) = historical_fixture();
    let original = save(root.path(), &raw);
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    assert_policy(root.path(), &context);
    // The policy files are the policy: the recorded activation and copy are
    // dropped, nothing replaces them, and the files give the same revision.
    let saved: Value = serde_json::from_slice(&store.checkpoint_bytes().unwrap()).unwrap();
    for key in ["candidates", "policy_revision", "active"] {
        assert!(saved.get(key).is_none(), "{key}: {saved}");
    }
    assert_eq!(
        store.read_following_files().unwrap().policy_revision,
        raw["policy_revision"]
    );
    assert!(
        !serde_json::to_value(&state)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("active")
    );
    assert_eq!(retained(&store, &source_name(&original)).unwrap(), original);
    let target = store.checkpoint_bytes().unwrap();
    store.read_checkpoint().unwrap();
    assert_eq!(store.checkpoint_bytes().unwrap(), target);
    let mut context = context.clone();
    context.surface = "mcp".into();
    context.connection_id = Some("upgrade-reconnected".into());
    let connection = review::mcp::Connection::enrolled(&context).unwrap();
    let report = review::mcp::call(&context, &connection, "status", &json!({}), &|| Ok(()));
    assert!(report["error"].is_null(), "{report}");
}

#[test]
fn upgrade_array_history_is_converted_to_a_journal() {
    let (root, _, mut raw) = historical_fixture();
    let store = Store::open(root.path()).unwrap();
    let events = json!([{"schema_version":1,"event_id":"old-array-observation","kind":"decision",
        "links":{"request_id":"old-array-request"},"details":{"decision":"allow"}}]);
    let bytes = serde_json::to_vec(&events).unwrap();
    let digest = crate::policy::fingerprint(&bytes);
    let path = store.directory.join(format!("history-{digest}.json"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    options.open(&path).unwrap().write_all(&bytes).unwrap();
    raw["history_generation"] = json!(digest);
    save(root.path(), &raw);
    let converted = store.read_history().unwrap();
    assert!(converted.history_available);
    assert!(converted.history_generation.starts_with("journal-"));
    assert!(
        converted
            .events
            .iter()
            .any(|event| event["event_id"] == "old-array-observation")
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn upgrade_full_authorization_journal_keeps_original_evidence() {
    let (root, context, mut raw) = historical_fixture();
    while raw["authorization_events"].as_array().unwrap().len() < 1024 {
        let id = raw["authorization_events"].as_array().unwrap().len();
        raw["authorization_events"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "kind":"proposal_rejected","event_id":format!("retained-event-{id}"),
                "links":{"proposal_id":format!("retained-proposal-{id}")}
            }));
    }
    save(root.path(), &raw);
    let store = Store::open(root.path()).unwrap();
    let converted = store.read_checkpoint().unwrap();
    // Every original record is kept.
    assert_eq!(
        json!(converted.authorization_events),
        raw["authorization_events"]
    );
    assert_policy(root.path(), &context);
}

#[test]
fn upgrade_incomplete_proposals_remain_inspectable_without_approval_handles() {
    let (root, context, mut raw) = historical_fixture();
    let proposed =
        review::tests::propose(&context, &review::tests::input("old-capture"), &|| Ok(())).unwrap();
    let id = proposed["proposal_id"].as_str().unwrap();
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    raw["proposals"] = json!(state.proposals);
    raw["sequence"] = json!(state.sequence);
    raw["proposals"][0]["status"] = json!("pending");
    raw["proposals"][0]
        .as_object_mut()
        .unwrap()
        .remove("interaction");
    raw["proposals"][0]
        .as_object_mut()
        .unwrap()
        .remove("closed_reason");
    raw["proposals"][0]
        .as_object_mut()
        .unwrap()
        .remove("base_candidate");
    raw["proposals"][0]["base_metadata"] = Value::Null;
    raw["proposals"][0]["candidate_metadata"] = Value::Null;
    save(root.path(), &raw);
    let mut session = context.clone();
    session.surface = "mcp".into();
    session.connection_id = Some("historical-proposal-reader".into());
    let connection = review::mcp::Connection::enrolled(&session).unwrap();
    let shown = review::mcp::call(
        &session,
        &connection,
        "policy_show",
        &json!({"proposal_id":id}),
        &|| Ok(()),
    );
    assert!(shown["error"].is_null(), "{shown}");
    assert_eq!(shown["data"]["status"], "withdrawn");
    assert_eq!(
        shown["data"]["capture_availability"],
        "historical_capture_unavailable"
    );
    assert!(shown["data"]["before_candidate_id"].is_null());
    assert!(shown["data"]["after_candidate_id"].is_null());
    assert!(shown["data"]["apply_arguments"].is_null());
    assert_eq!(
        shown["data"]["closed_reason"],
        "historical_capture_unavailable"
    );
    let fresh =
        review::tests::propose(&context, &review::tests::input("fresh-capture"), &|| Ok(()))
            .unwrap();
    assert_ne!(fresh["proposal_id"], id);
}

#[test]
fn upgrade_historical_inactive_preview_never_creates_authority() {
    let (root, _, mut raw) = historical_fixture();
    raw["operator"] = Value::Null;
    raw["active"] = Value::Null;
    raw["authorization_events"] = json!([]);
    raw.as_object_mut().unwrap().remove("registry_checkpoints");
    raw.as_object_mut().unwrap().remove("read_requests");
    raw.as_object_mut().unwrap().remove("run_requests");
    raw.as_object_mut().unwrap().remove("history_generation");
    save(root.path(), &raw);
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(state.authorization_events, [] as [serde_json::Value; 0]);
    assert!(state.settings.registries.sources.is_empty());
    assert!(state.read_requests.is_empty());
}

#[test]
fn upgrade_refuses_corrupt_unknown_and_modern_incomplete_state() {
    let (root, _, raw) = historical_fixture();
    let store = Store::open(root.path()).unwrap();
    for fault in ["future", "unknown", "modern"] {
        let mut broken = raw.clone();
        match fault {
            "future" => broken["schema_version"] = json!(2),
            "unknown" => broken["unrecognized"] = json!(true),
            _ => broken["session_grants"] = json!({}),
        }
        let bytes = save(root.path(), &broken);
        let error = store
            .read_checkpoint()
            .err()
            .expect("invalid checkpoint accepted");
        assert_eq!(
            matches!(error, Error::StateFormat { .. }),
            matches!(fault, "future" | "unknown"),
            "{fault}: {error}"
        );
        if fault == "modern" {
            assert!(matches!(error, Error::StateCorrupt { .. }), "{error}");
        }
        assert_eq!(store.checkpoint_bytes().unwrap(), bytes, "{fault}");
    }
}

#[test]
fn upgrade_reports_corrupt_current_checkpoint_with_path_and_preserves_bytes() {
    let (root, _, _) = historical_fixture();
    let store = Store::open(root.path()).unwrap();
    let current: Value = serde_json::from_slice(&store.checkpoint_bytes().unwrap()).unwrap();
    for fault in ["missing", "type", "syntax"] {
        let mut broken = current.clone();
        match fault {
            "missing" => {
                broken.as_object_mut().unwrap().remove("sequence");
            }
            "type" => broken["session_grants"] = json!(false),
            _ => (),
        }
        let bytes = if fault == "syntax" {
            let bytes = b"{\"schema_version\":1,".to_vec();
            fs::write(store.directory.join("state.json"), &bytes).unwrap();
            bytes
        } else {
            save(root.path(), &broken)
        };
        let error = store
            .read_checkpoint()
            .err()
            .expect("corrupt checkpoint accepted");
        assert!(
            matches!(error, Error::StateCorrupt { .. }),
            "{fault}: {error}"
        );
        assert!(error.to_string().contains("corrupt review state"));
        assert!(
            error
                .to_string()
                .contains(&store.directory.join("state.json").display().to_string())
        );
        assert!(!error.to_string().contains("Use a Shield version"));
        assert_eq!(store.checkpoint_bytes().unwrap(), bytes);
    }
}

#[test]
fn upgrade_preparation_releases_authority_and_rechecks_changed_checkpoint() {
    let (root, context, raw) = historical_fixture();
    save(root.path(), &raw);
    PREPARATION.with(|slot| *slot.borrow_mut() = (Some(root.path().to_owned()), 0));
    super::super::tests::set_observer(Some(prepared_without_authority));
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    super::super::tests::set_observer(None);
    PREPARATION.with(|slot| {
        assert_eq!(
            slot.borrow().1,
            2,
            "changed checkpoint must be prepared again"
        );
        *slot.borrow_mut() = (None, 0);
    });
    assert!(state.sequence >= raw["sequence"].as_u64().unwrap() + 100);
    assert_policy(root.path(), &context);
}

thread_local! {
    static PREPARATION: std::cell::RefCell<(Option<std::path::PathBuf>, usize)> = const {
        std::cell::RefCell::new((None, 0))
    };
}

fn prepared_without_authority(stage: super::super::tests::Stage) {
    if !matches!(stage, super::super::tests::Stage::UpgradePrepared) {
        return;
    }
    PREPARATION.with(|slot| {
        let mut capture = slot.borrow_mut();
        let root = capture.0.as_ref().unwrap();
        let store = Store::open(root).unwrap();
        let guard = store
            .lock(&|| Ok(()))
            .expect("preparation must not hold authority");
        if capture.1 == 0 {
            let mut raw: Value =
                serde_json::from_slice(&store.checkpoint_bytes().unwrap()).unwrap();
            raw["sequence"] = json!(raw["sequence"].as_u64().unwrap() + 100);
            save(root, &raw);
        }
        guard.release().unwrap();
        capture.1 += 1;
    });
}

#[test]
fn upgrade_retries_write_failure_and_serializes_concurrent_converters() {
    let (root, context, raw) = historical_fixture();
    let bytes = save(root.path(), &raw);
    let store = Store::open(root.path()).unwrap();
    let backup = store.directory.join(source_name(&bytes));
    fs::create_dir(&backup).unwrap();
    assert!(store.read_checkpoint().is_err());
    assert_eq!(store.checkpoint_bytes().unwrap(), bytes);
    fs::remove_dir(backup).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let root = root.path();
            scope.spawn(move || {
                review::tests::retry_locked(|| Store::open(root)?.read_checkpoint());
            });
        }
    });
    assert_policy(root.path(), &context);
    assert_eq!(
        json!(store.read_checkpoint().unwrap().authorization_events),
        raw["authorization_events"]
    );
}

#[test]
fn upgrade_diagnostics_remain_read_only_when_publication_is_blocked() {
    let (root, context, mut raw) = historical_fixture();
    let cache = root.path().join("diagnostic-cache");
    raw["operator"]["compiled_cache_root"] = json!(cache);
    let bytes = save(root.path(), &raw);
    let store = Store::open(root.path()).unwrap();
    let backup = store.directory.join(source_name(&bytes));
    fs::create_dir(&backup).unwrap();
    let names = || {
        let mut names = fs::read_dir(&store.directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    let before = names();
    for command in ["status", "doctor"] {
        let input = <crate::cli::Cli as clap::Parser>::try_parse_from([
            "shield",
            "--project",
            root.path().to_str().unwrap(),
            "--json",
            command,
        ])
        .unwrap();
        assert_eq!(
            review::operator::run(&input),
            std::process::ExitCode::SUCCESS
        );
    }
    let mut mcp_context = review::cli::context(&crate::cli::ReviewContext {
        project: root.path().into(),
        agent: None,
        session_id: None,
        tool_use_id: None,
    })
    .unwrap();
    mcp_context.connection_id = Some("acme-diagnostic-connection".into());
    let connection = review::mcp::Connection::enrolled(&mcp_context).unwrap();
    for _ in 0..2 {
        let status = review::project::status(&context).unwrap();
        let doctor = review::project::doctor(&context).unwrap();
        for report in [&status, &doctor] {
            // The checkpoint's earlier copy of the policy is not shown; the
            // policy files are.
            assert!(report["policy_revision"].is_null());
            assert!(report["policy_files"]["digest"].is_string());
            assert_eq!(report["checkpoint_upgrade_pending"], true);
        }
        for tool in ["status", "doctor"] {
            let reply = review::mcp::call_with_confirmation(
                &mcp_context,
                &connection,
                tool,
                &json!({}),
                &|| Ok(()),
                false,
            );
            assert!(reply["error"].is_null(), "{tool}: {reply}");
            assert_eq!(reply["data"]["checkpoint_upgrade_pending"], true);
        }
        assert!(
            doctor["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| check["check"] == "checkpoint_upgrade")
        );
        let (state, pending) = store.read_diagnostics().unwrap();
        assert!(pending);
        assert!(state.unconverted);
        assert!(state.check_operations().is_err());
        assert!(!cache.exists());
        assert_eq!(store.checkpoint_bytes().unwrap(), bytes);
        assert_eq!(names(), before);
    }
    // Enforcement cannot consume the in-memory diagnostic conversion.
    assert!(consumer::load(&context, &|| Ok(())).is_err());
    assert_eq!(store.checkpoint_bytes().unwrap(), bytes);
    fs::remove_dir(backup).unwrap();
    store.read_checkpoint().unwrap();
    assert_policy(root.path(), &context);
    let committed = store.checkpoint_bytes().unwrap();
    assert_eq!(
        review::project::status(&context).unwrap()["checkpoint_upgrade_pending"],
        false
    );
    assert_eq!(store.checkpoint_bytes().unwrap(), committed);
}

#[cfg(unix)]
#[test]
fn upgrade_diagnostics_inspect_a_read_only_review_directory_without_writes() {
    use std::os::unix::fs::PermissionsExt;
    let (root, context, raw) = historical_fixture();
    let original = save(root.path(), &raw);
    let store = Store::open(root.path()).unwrap();
    let before = fs::read_dir(&store.directory).unwrap().count();
    fs::set_permissions(&store.directory, fs::Permissions::from_mode(0o500)).unwrap();
    let status = review::project::status(&context);
    let doctor = review::project::doctor(&context);
    fs::set_permissions(&store.directory, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(status.unwrap()["checkpoint_upgrade_pending"], true);
    assert_eq!(doctor.unwrap()["checkpoint_upgrade_pending"], true);
    assert_eq!(store.checkpoint_bytes().unwrap(), original);
    assert_eq!(fs::read_dir(&store.directory).unwrap().count(), before);
}

#[test]
#[ignore = "owned child for upgrade_crashes_retry_without_duplicate_authority"]
fn upgrade_writer_process() {
    let root = std::env::var("SHIELD_TEST_UPGRADE_ROOT").unwrap();
    super::super::tests::set_observer(Some(upgrade_barrier));
    Store::open(Path::new(&root))
        .unwrap()
        .read_checkpoint()
        .unwrap();
}

fn upgrade_barrier(stage: super::super::tests::Stage) {
    if std::env::var("SHIELD_TEST_UPGRADE_STAGE").unwrap() == format!("{stage:?}") {
        let root = std::env::var("SHIELD_TEST_UPGRADE_ROOT").unwrap();
        fs::write(Path::new(&root).join("upgrade-ready"), b"ready").unwrap();
        loop {
            std::thread::park();
        }
    }
}

#[test]
fn upgrade_crashes_retry_without_duplicate_authority() {
    for stage in ["Created", "Written", "Synced", "Renamed", "Complete"] {
        let (root, context, raw) = historical_fixture();
        let original = save(root.path(), &raw);
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "review::storage::upgrades::tests::upgrade_writer_process",
                    "--ignored",
                    "--nocapture",
                ])
                .env("SHIELD_TEST_UPGRADE_ROOT", root.path())
                .env("SHIELD_TEST_UPGRADE_STAGE", stage)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !root.path().join("upgrade-ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "upgrade child exited at {stage}"
            );
            assert!(
                Instant::now() < deadline,
                "upgrade child timed out at {stage}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let store = Store::open(root.path()).unwrap();
        let state = store.read_checkpoint().unwrap();
        assert_policy(root.path(), &context);
        assert_eq!(retained(&store, &source_name(&original)).unwrap(), original);
        assert_eq!(
            json!(state.authorization_events),
            raw["authorization_events"],
            "{stage}"
        );
    }
}
