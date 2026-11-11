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
    let mut raw =
        serde_json::to_value(Store::open(root.path()).unwrap().read_current().unwrap()).unwrap();
    raw["project_root"] = json!(context.project_root);
    let object = raw.as_object_mut().unwrap();
    for key in [
        "hook_requests",
        "session_grants",
        "policy_drafts",
        "dismissed_reviews",
        "enforcement",
    ] {
        object.remove(key);
    }
    legacy_activation(&mut raw);
    raw["history_generation"] = Value::Null;
    (root, context, raw)
}

/// Reproduce the original legacy proof; dropping current required fields alone
/// must never manufacture an accepted historical activation.
pub(super) fn legacy_activation(raw: &mut Value) {
    assert!(
        raw["project_root"].is_string(),
        "the earlier format always recorded its root"
    );
    raw.as_object_mut().unwrap().remove("configuration_changes");
    raw["active"].as_object_mut().unwrap().remove("compiled");
    raw["active"]
        .as_object_mut()
        .unwrap()
        .remove("binding_version");
    let binding = crate::policy::fingerprint(
        json!([
            raw["project_id"],
            raw["project_root"],
            raw["active"]["activation_id"],
            raw["active"]["previous_activation_id"],
            raw["active"]["policy_revision"],
            raw["active"]["candidate_digest"]
        ])
        .to_string()
        .as_bytes(),
    );
    raw["active"]["binding"] = json!(binding);
    for event in raw["authorization_events"].as_array_mut().unwrap() {
        if event["kind"] == "runtime_activated" {
            event["details"]["binding"] = json!(binding);
        }
        event["details"]
            .as_object_mut()
            .unwrap()
            .remove("compiled_digest");
    }
}

fn save(root: &Path, raw: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(raw).unwrap();
    fs::write(root.join(".shield/review/state.json"), &bytes).unwrap();
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

#[test]
fn upgrade_dispatch_keeps_current_policy_and_approval_state_read_only() {
    let (root, _) = review::tests::enrolled();
    let path = root.path().join(".shield/review/state.json");
    let bytes = fs::read(&path).unwrap();
    let before: Value = serde_json::from_slice(&bytes).unwrap();
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    assert_eq!(state.project_id, before["project_id"]);
    assert_eq!(state.policy_revision, before["policy_revision"]);
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
    let active = serde_json::to_value(state.active.as_ref().unwrap()).unwrap();
    for field in [
        "activation_id",
        "previous_activation_id",
        "policy_revision",
        "candidate_digest",
    ] {
        assert_eq!(active[field], raw["active"][field]);
    }
    assert_converted_binding(&state, &raw);
    assert_eq!(
        state
            .authorization_events
            .iter()
            .filter(|event| event["kind"] == "runtime_activated")
            .count(),
        1
    );
    assert_eq!(
        state
            .authorization_events
            .iter()
            .filter(|event| event["kind"] == "compiled_cache_prepared")
            .count(),
        1
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

fn cache_fault(cache: &Path, scratch: &Path, fault: &str) -> bool {
    #[cfg(not(unix))]
    let _ = scratch;
    match fault {
        "file" | "parent-file" => fs::write(cache, b"preserved cache obstacle").unwrap(),
        "partition-file" => {
            fs::create_dir(cache).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(cache, fs::Permissions::from_mode(0o700)).unwrap();
            }
            fs::write(cache.join("compiled"), b"preserved cache obstacle").unwrap();
        }
        #[cfg(unix)]
        "public-directory" => {
            use std::os::unix::fs::PermissionsExt;
            fs::create_dir(cache).unwrap();
            fs::set_permissions(cache, fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(unix)]
        "symlink" => std::os::unix::fs::symlink(scratch, cache).unwrap(),
        #[cfg(unix)]
        "readonly" => {
            use std::os::unix::fs::PermissionsExt;
            fs::create_dir(cache).unwrap();
            fs::set_permissions(cache, fs::Permissions::from_mode(0o500)).unwrap();
            let probe = cache.join("write-probe");
            if fs::write(&probe, b"probe").is_ok() {
                // A privileged test host cannot model this filesystem refusal.
                fs::remove_file(probe).unwrap();
                fs::set_permissions(cache, fs::Permissions::from_mode(0o700)).unwrap();
                return false;
            }
        }
        _ => unreachable!(),
    }
    true
}

/// A converted activation keeps its identity and confirmation; its binding no
/// longer names the folder, and its activation event records the original.
pub(super) fn assert_converted_binding(state: &State, raw: &Value) {
    let active = serde_json::to_value(state.active.as_ref().unwrap()).unwrap();
    assert_eq!(active["binding_version"], 2);
    assert_ne!(active["binding"], raw["active"]["binding"]);
    assert!(state.authorization_events.iter().any(|event| {
        event["kind"] == "runtime_activated"
            && event["details"]["binding"] == active["binding"]
            && event["details"]["converted_from_binding"] == raw["active"]["binding"]
    }));
}

fn assert_historical_cache_binding(raw: &Value, active: &Value, selected: &Path, fault: &str) {
    for field in [
        "activation_id",
        "previous_activation_id",
        "policy_revision",
        "candidate_digest",
    ] {
        assert_eq!(active[field], raw["active"][field], "{fault}: {field}");
    }
    assert_eq!(active["binding_version"], 2, "{fault}: binding");
    assert!(
        active["compiled"]["digest"]
            .as_str()
            .unwrap()
            .starts_with("blake3-v1:")
    );
    assert_eq!(active["compiled"]["directory"], json!(selected));
}

#[test]
fn upgrade_cache_write_failures_keep_inspection_and_in_session_repair_reachable() {
    let faults = [
        "file",
        "parent-file",
        "partition-file",
        #[cfg(unix)]
        "public-directory",
        #[cfg(unix)]
        "symlink",
        #[cfg(unix)]
        "readonly",
    ];
    for fault in faults {
        let (root, context, mut raw) = historical_fixture();
        let scratch = assert_fs::TempDir::new().unwrap();
        let cache = scratch.path().join("old-cache");
        let selected = if fault == "parent-file" {
            cache.join("nested")
        } else {
            cache.clone()
        };
        if !cache_fault(&cache, scratch.path(), fault) {
            continue;
        }
        raw["operator"]["compiled_cache_root"] = json!(selected);
        let original = save(root.path(), &raw);
        let store = Store::open(root.path()).unwrap();
        let converted = store
            .read_checkpoint()
            .unwrap_or_else(|error| panic!("{fault}: {error}"));
        let active = json!(converted.active.as_ref().unwrap());
        assert_historical_cache_binding(&raw, &active, &selected, fault);
        assert!(
            matches!(
                consumer::load(&context, &|| Ok(())),
                Err(Error::CacheUnavailable)
            ),
            "{fault}"
        );
        let checkpoint = store.checkpoint_bytes().unwrap();
        store.read_checkpoint().unwrap();
        assert_eq!(store.checkpoint_bytes().unwrap(), checkpoint);
        assert_eq!(retained(&store, &source_name(&original)).unwrap(), original);
        let mut session = context.clone();
        session.surface = "mcp".into();
        session.agent = Some("codex".into());
        let server = crate::mcp::Server::for_review(session).unwrap();
        let transport = crate::mcp::tests::Transport::start(server);
        for message in crate::mcp::tests::initialize(&json!({"elicitation":{"form":{}}}), "codex") {
            transport.send(&message);
        }
        assert!(transport.receive()["result"].is_object());
        for tool in ["status", "doctor"] {
            let response = transport.call(json!(2), tool, &json!({}));
            assert_eq!(response["result"]["isError"], false, "{fault}: {response}");
        }
        let failed = transport.call(json!(3), "cache_prepare", &json!({}));
        let error = &failed["result"]["structuredContent"]["error"];
        assert_eq!(error["code"], "cache_unavailable", "{fault}: {failed}");
        assert_eq!(
            error["operator_route"], "cache_prepare",
            "{fault}: {failed}"
        );
        assert!(error["message"].as_str().unwrap().contains("cache_dir"));
        let repaired = scratch.path().join("repaired-cache");
        let form = transport.call(json!(3), "cache_prepare", &json!({"cache_dir":repaired}));
        assert_eq!(form["method"], "elicitation/create", "{fault}: {form}");
        assert!(!repaired.exists());
        let response = transport.request(&json!({"jsonrpc":"2.0","id":form["id"],
            "result":{"action":"accept","content":{}}}));
        assert_eq!(response["result"]["isError"], false, "{fault}: {response}");
        assert_policy(root.path(), &context);
        let after = store.read_checkpoint().unwrap();
        assert_eq!(
            after.active.as_ref().unwrap().activation_id(),
            active["activation_id"]
        );
        assert_eq!(after.policy_revision, converted.policy_revision);
        assert_eq!(
            after
                .authorization_events
                .iter()
                .filter(|event| event["kind"] == "runtime_activated")
                .count(),
            1
        );
        if ["file", "parent-file"].contains(&fault) {
            assert_eq!(fs::read(&cache).unwrap(), b"preserved cache obstacle");
        }
        #[cfg(unix)]
        if fault == "readonly" {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cache, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
}

#[test]
fn upgrade_array_history_with_an_existing_compiled_snapshot_is_not_skipped() {
    let (root, _, mut raw) = historical_fixture();
    let store = Store::open(root.path()).unwrap();
    let current: Value = serde_json::from_slice(&store.checkpoint_bytes().unwrap()).unwrap();
    raw["active"]["compiled"] = current["active"]["compiled"].clone();
    raw["authorization_events"][0]["details"]["compiled_digest"] =
        current["active"]["compiled"]["digest"].clone();
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
    assert_eq!(converted.authorization_events.len(), 1025);
    // Every original record is kept; the activation's own record carries its
    // converted binding beside the original one.
    let mut expected = raw["authorization_events"].clone();
    for event in expected.as_array_mut().unwrap() {
        if event["kind"] == "runtime_activated" {
            event["details"]["converted_from_binding"] = event["details"]["binding"].clone();
            event["details"]["binding"] = json!(
                converted
                    .active
                    .as_ref()
                    .map(|active| { serde_json::to_value(active).unwrap()["binding"].clone() })
            );
        }
    }
    assert_eq!(json!(converted.authorization_events[..1024]), expected);
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
    let outcome = review::mcp::call(
        &session,
        &connection,
        "policy_receipt",
        &json!({"proposal_id":id}),
        &|| Ok(()),
    );
    assert!(outcome["error"].is_null(), "{outcome}");
    assert_eq!(
        outcome["data"]["closed_reason"],
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
    assert!(state.active.is_none());
    assert!(state.authorization_events.is_empty());
    assert_eq!(state.operator["sources"], json!({}));
    assert!(state.read_requests.is_empty());
}

#[test]
fn upgrade_refuses_corrupt_unknown_and_modern_incomplete_state_without_rebinding() {
    let (root, _, raw) = historical_fixture();
    let store = Store::open(root.path()).unwrap();
    for fault in ["binding", "source", "event", "future", "unknown", "modern"] {
        let mut broken = raw.clone();
        match fault {
            "binding" => broken["active"]["binding"] = json!("0".repeat(64)),
            "source" => broken["active"]["candidates"]["project"]["config"] = json!("changed"),
            "event" => broken["authorization_events"] = json!([]),
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
    for fault in ["missing", "type", "activation", "syntax"] {
        let mut broken = current.clone();
        match fault {
            "missing" => {
                broken.as_object_mut().unwrap().remove("policy_revision");
            }
            "type" => broken["session_grants"] = json!(false),
            "activation" => {
                broken["active"].as_object_mut().unwrap().remove("compiled");
            }
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
    assert_eq!(
        json!(state.active.as_ref().unwrap())["activation_id"],
        raw["active"]["activation_id"]
    );
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
    let state = store.read_checkpoint().unwrap();
    assert_eq!(
        state
            .authorization_events
            .iter()
            .filter(|e| e["kind"] == "compiled_cache_prepared")
            .count(),
        1
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
            assert_eq!(report["policy_revision"], raw["policy_revision"]);
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
        assert!(state.active.is_none());
        let preview = json!(state.legacy_preview.as_ref().unwrap());
        assert_eq!(preview["activation_id"], raw["active"]["activation_id"]);
        assert_eq!(preview["recorded_proof"], "validated");
        assert_eq!(preview["model"], "unchecked");
        assert!(state.check_operations().is_err());
        assert!(!cache.exists());
        assert_eq!(store.checkpoint_bytes().unwrap(), bytes);
        assert_eq!(names(), before);
    }
    // Enforcement cannot consume the in-memory diagnostic conversion.
    assert!(consumer::load(&context, &|| Ok(())).is_err());
    assert_eq!(store.checkpoint_bytes().unwrap(), bytes);
    let mut corrupt = raw.clone();
    corrupt["active"]["binding"] = json!("0".repeat(64));
    save(root.path(), &corrupt);
    assert!(review::project::status(&context).is_err());
    assert!(review::project::doctor(&context).is_err());
    save(root.path(), &raw);
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

#[test]
fn legacy_record_only_diagnostics_do_not_prepare_source_cache_or_history() {
    use clap::Parser as _;
    let (root, context, mut raw) = historical_fixture();
    let invalid_source = json!({"schema_version":999,"kind":"custom","rules":[]}).to_string();
    raw["active"]["candidates"]["project"]["rules"] = json!(invalid_source);
    raw["active"]["candidate_digest"] = json!(crate::review::storage::sources::map_digest(
        &serde_json::from_value(raw["active"]["candidates"].clone()).unwrap()
    ));
    raw["candidates"] = raw["active"]["candidates"].clone();
    legacy_activation(&mut raw);
    // Unsupported optional derived metadata is opaque to record inspection.
    raw["active"]["compiled"] = json!({"future_descriptor":true});
    let cache = root.path().join("unprepared-legacy-cache");
    raw["operator"]["compiled_cache_root"] = json!(cache);
    let bytes = save(root.path(), &raw);
    let logdir = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logdir.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let connection = review::mcp::Connection::new("legacy-record-only".into());
    for command in ["status", "doctor"] {
        let trace = log.request("command", command, None, None).unwrap();
        {
            let _scope = trace.enter();
            let report = if command == "status" {
                review::project::status(&context).unwrap()
            } else {
                review::project::doctor(&context).unwrap()
            };
            assert_eq!(report["classification"], "historical-reviewed-record");
            assert_eq!(report["checkpoint_upgrade_pending"], true);
            assert_eq!(report["legacy_activation"]["recorded_proof"], "validated");
            assert_eq!(report["policy_validation"]["model"], "unchecked");
            assert_eq!(report["history"]["integrity"], "unchecked");
            assert!(report["active"].is_null());
            let reply = review::mcp::call(&context, &connection, command, &json!({}), &|| Ok(()));
            assert!(reply["error"].is_null(), "{reply}");
            assert!(reply["data"]["active_candidate_id"].is_null());
            for counter in [
                "policy.preparations",
                "source.hydrated",
                "history.captures",
                "history.lock_attempts",
                "history.directory_entries",
                "history.decoded_records",
            ] {
                assert_eq!(trace.recorded_count(counter), 0, "{command}: {counter}");
            }
        }
        trace.finish("success");
    }
    assert!(!cache.exists());
    let store = Store::inspect(root.path()).unwrap();
    let (preview, pending) = store.read_diagnostic_checkpoint().unwrap();
    assert!(pending);
    assert!(preview.active.is_none());
    assert!(preview.check_operations().is_err());
    assert!(review::project::policy(&context).is_err());
    assert_eq!(store.checkpoint_bytes().unwrap(), bytes);
    // Actual use retains strict source/cache validation and original bytes.
    assert!(store.read_checkpoint().is_err());
    assert_eq!(store.checkpoint_bytes().unwrap(), bytes);
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
            state
                .authorization_events
                .iter()
                .filter(|event| event["kind"] == "runtime_activated")
                .count(),
            1
        );
        assert_eq!(
            state
                .authorization_events
                .iter()
                .filter(|event| event["kind"] == "compiled_cache_prepared")
                .count(),
            1
        );
    }
}
