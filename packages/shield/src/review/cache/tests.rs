//! Real cache repair and current-generation boundaries, without command execution.
use super::*;
use crate::{configuration::Bundle, review, runtime::Project};
mod retention;

pub(super) fn fixture() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle = Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None).unwrap();
    let mut context = review::mcp::initialize_fixture(root.path(), &bundle);
    context.surface = "cli".into();
    (root, context)
}
fn artifact(root: &Path) -> Artifact {
    Store::open(root)
        .unwrap()
        .read_checkpoint()
        .unwrap()
        .active
        .unwrap()
        .compiled
}

#[test]
fn record_only_default_policy_view_uses_canonical_lock_without_loading_a_plan() {
    use clap::Parser as _;
    let (root, context) = fixture();
    let store = Store::open(root.path()).unwrap();
    let (state, evaluator) = store.read_current_evaluated().unwrap();
    let expected = json!(evaluator.effective());
    let mut tampered = state.candidates["project"].clone();
    tampered["lock"]["config_digest"] = json!("0".repeat(64));
    assert!(crate::configuration::recorded_effective(&tampered, &state.policy_revision).is_err());
    let artifact = artifact(root.path());
    fs::remove_file(artifact.path()).unwrap();
    let before = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    let connection = review::mcp::Connection::new("acme-recorded-policy".into());
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    for mcp in [false, true] {
        let trace = log.request("command", "policy_show", None, None).unwrap();
        {
            let _scope = trace.enter();
            let report = if mcp {
                let report =
                    review::mcp::call(&context, &connection, "policy_show", &json!({}), &|| Ok(()));
                assert!(report["error"].is_null(), "{report}");
                report["data"].clone()
            } else {
                review::project::policy(&context).unwrap()
            };
            assert_eq!(report["effective"], expected);
            assert_eq!(trace.recorded_count("policy.preparations"), 0);
            assert_eq!(trace.recorded_count("history.captures"), 0);
        }
        trace.finish("success");
    }
    assert!(!artifact.path().exists());
    assert_eq!(
        fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        before
    );
}

#[test]
fn opaque_coverage_description_preserves_checkpoint_and_policy_authority() {
    let (root, context) = fixture();
    let path = root.path().join(".shield/review/state.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let known = original["active"]["compiled"]["inspection"].clone();
    let mut unknown_field = known.clone();
    unknown_field["future_field"] = json!({"preserved":[1,"Acme"]});
    let mut future_schema = known;
    future_schema["schema_version"] = json!(73);
    for description in [
        json!("unsupported descriptor type"),
        unknown_field,
        future_schema,
    ] {
        let mut raw = original.clone();
        raw["active"]["compiled"]["inspection"] = description.clone();
        let bytes = raw.to_string().into_bytes();
        fs::write(&path, &bytes).unwrap();
        let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
        assert!(state.active.as_ref().unwrap().inspection(&state).is_none());
        assert_eq!(
            json!(state)["active"]["compiled"]["inspection"],
            description
        );
        assert!(Project::open(root.path()).unwrap().snapshot().is_ok());
        let report = review::project::status(&context).unwrap();
        assert_eq!(
            report["coverage"]["policy_inspection"]["availability"],
            "unavailable"
        );
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "inspection never rewrites an opaque description"
        );
    }
}

#[test]
fn record_only_status_and_doctor_do_not_decode_policy_or_compiled_payloads() {
    use clap::Parser as _;
    let (root, context) = fixture();
    let current = artifact(root.path());
    let mut bytes = fs::read(current.path()).unwrap();
    bytes[0] ^= 0xff;
    fs::write(current.path(), bytes).unwrap();
    for entry in fs::read_dir(root.path().join(".shield/review/sources")).unwrap() {
        fs::write(entry.unwrap().path(), "unavailable retained source").unwrap();
    }
    let checkpoint = root.path().join(".shield/review/state.json");
    let before = fs::read(&checkpoint).unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    for doctor in [false, true] {
        let trace = log.request("command", "inspection", None, None).unwrap();
        {
            let _scope = trace.enter();
            let report = if doctor {
                review::project::doctor(&context)
            } else {
                review::project::status(&context)
            }
            .unwrap();
            assert_eq!(
                report["coverage"]["policy_inspection"]["availability"],
                "recorded"
            );
            assert_eq!(report["compiled_cache"]["integrity"], "checked-on-load");
            assert_eq!(trace.recorded_count("policy.preparations"), 0);
            assert_eq!(trace.recorded_count("source.hydrated"), 0);
            assert_eq!(trace.recorded_count("history.captures"), 0);
            assert_eq!(trace.recorded_count("history.decoded_records"), 0);
            assert_eq!(trace.recorded_count("history.directory_entries"), 0);
            assert!(report["history"]["available"].is_null());
            assert_eq!(report["history"]["integrity"], "unchecked");
        }
        trace.finish("success");
    }
    assert_eq!(fs::read(checkpoint).unwrap(), before);
}

#[test]
fn coverage_descriptions_require_exact_bounded_event_authenticated_identity() {
    let (root, _) = fixture();
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    assert!(state.active.as_ref().unwrap().inspection(&state).is_some());
    let original = json!(state);
    for (field, value) in [
        ("candidate_digest", json!("0".repeat(64))),
        ("schema_version", json!(2)),
        ("automatic_mcp", json!(true)),
    ] {
        let mut raw = original.clone();
        raw["active"]["compiled"]["inspection"][field] = value;
        let state: State = serde_json::from_value(raw).unwrap();
        assert!(
            state.active.as_ref().unwrap().inspection(&state).is_none(),
            "{field}"
        );
    }
    let mut raw = original.clone();
    raw["active"]["compiled"]
        .as_object_mut()
        .unwrap()
        .remove("inspection");
    let state: State = serde_json::from_value(raw).unwrap();
    assert!(state.active.as_ref().unwrap().inspection(&state).is_none());
    let mut oversized = artifact(root.path()).inspection().unwrap().clone();
    oversized.prefixes.push(policy::agent_outputs::CodexPrefix {
        owner: "custom".into(),
        rule: "large".into(),
        pattern: vec![vec!["x".repeat(INSPECTION_LIMIT)]],
    });
    assert!(!oversized.matches(&oversized.candidate_digest, &oversized.policy_revision));
}

#[test]
fn cache_artifact_description_matches_publication_without_creating_directories() {
    let (root, context) = fixture();
    let state = Store::open(root.path()).unwrap().read_current().unwrap();
    let evaluator = Evaluator::new_for_revision(
        snapshots(&state.candidates).unwrap(),
        &state.policy_revision,
    )
    .unwrap();
    let scratch = assert_fs::TempDir::new().unwrap();
    let directory = scratch.path().canonicalize().unwrap().join("planned-cache");
    let mut operator = state.operator.clone();
    operator["compiled_cache_root"] = json!(directory);
    let described = describe_candidate(
        &context,
        &state.candidates,
        &state.policy_revision,
        &operator,
        &evaluator,
    )
    .unwrap();
    assert!(!directory.exists());
    assert!(matches!(
        describe_candidate(
            &context,
            &state.candidates,
            "unverified-revision",
            &operator,
            &evaluator
        ),
        Err(Error::StaleSnapshot)
    ));
    assert!(!directory.exists());
    let published = build_candidate(
        &context,
        &state.candidates,
        &state.policy_revision,
        &operator,
        &evaluator,
    )
    .unwrap();
    assert_eq!(json!(described), json!(published));
    assert!(
        described
            .load(&context.project_root, &context.project_id)
            .is_ok()
    );
}

#[test]
fn activation_requires_its_retained_compiled_plan_reference() {
    let (root, _) = fixture();
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let mut active = json!(state.active.unwrap());
    active.as_object_mut().unwrap().remove("compiled");
    assert!(serde_json::from_value::<review::runtime::Active>(active).is_err());
}

#[cfg(unix)]
#[test]
fn private_cache_directories_require_the_review_owner() {
    use std::os::unix::fs::MetadataExt;
    let (root, context) = fixture();
    let local = root.path().join(".shield/cache");
    prepare(&context, Some(&local)).unwrap();
    let artifact = artifact(root.path());
    let owner = cache_owner(&context.project_root).unwrap();
    for path in [
        artifact.directory.clone(),
        artifact.directory.join("compiled"),
        artifact
            .directory
            .join("compiled")
            .join(&artifact.project_key),
    ] {
        let metadata = fs::symlink_metadata(&path).unwrap();
        let foreign = Some(metadata.uid().wrapping_add(1));
        assert!(check_directory(&path, true, owner).is_ok());
        assert!(check_directory(&path, true, foreign).is_err());
        assert!(prepare_directory(&path, foreign).is_err());
        assert!(check_directory(&path, true, None).is_err());
        let after = fs::symlink_metadata(path).unwrap();
        assert_eq!(after.uid(), metadata.uid());
        assert_eq!(after.mode(), metadata.mode());
    }
    assert!(
        artifact
            .load(&context.project_root, &context.project_id)
            .is_ok()
    );
}

#[cfg(unix)]
#[test]
fn unreadable_plans_report_the_cache_preparation_route() {
    use std::os::unix::fs::PermissionsExt;
    let (root, context) = fixture();
    let current = artifact(root.path());
    fs::set_permissions(current.path(), fs::Permissions::from_mode(0o000)).unwrap();
    let loaded = current.load(&context.project_root, &context.project_id);
    fs::set_permissions(current.path(), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(loaded, Err(Error::CacheUnavailable)));
}

#[cfg(any(unix, windows))]
#[test]
fn preparation_removes_only_exited_writers_temporaries() {
    let (root, context) = fixture();
    let current = artifact(root.path());
    let partition = current.path().parent().unwrap().to_path_buf();
    let plan = &current.plan_key;
    let dead = review::storage::exited_pid();
    let unverified = i32::from(!cfg!(windows));
    let exited = [
        format!("{plan}.{dead}-0-1.tmp"),
        format!("{plan}.inputs.{dead}-0-1.tmp"),
    ];
    let kept = [
        format!("{plan}.{unverified}-0-1.tmp"),
        format!("{plan}.{}-0-1.tmp", std::process::id()),
        format!("{plan}.{dead}-0.tmp"),
        format!("acme.{dead}-0-1.tmp"),
    ];
    for name in exited.iter().chain(&kept) {
        let path = partition.join(name);
        fs::write(&path, b"partial").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        #[cfg(windows)]
        crate::windows_security::make_private(&path).unwrap();
    }
    prepare(&context, None).unwrap();
    for name in &exited {
        assert!(!partition.join(name).exists(), "{name}");
    }
    for name in &kept {
        assert!(partition.join(name).exists(), "{name}");
    }
}

#[test]
fn explicit_roots_are_private_ignored_and_isolated_across_enrollments() {
    let (root, context) = fixture();
    let (other, other_context) = fixture();
    let shared = assert_fs::TempDir::new().unwrap();
    let cache = shared.path().join("cache");
    prepare(&context, Some(&cache)).unwrap();
    prepare(&other_context, Some(&cache)).unwrap();
    let a = artifact(root.path());
    let b = artifact(other.path());
    assert_ne!(a.project_key, b.project_key);
    assert!(
        a.load(&other_context.project_root, &other_context.project_id)
            .is_err()
    );
    let local = root.path().join(".shield/cache");
    prepare(&context, Some(&local)).unwrap();
    assert_eq!(
        fs::read_to_string(local.join(".gitignore")).unwrap(),
        "*\n!.gitignore\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let a = artifact(root.path());
        assert_eq!(
            fs::metadata(a.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let linked = root.path().join("cache-link");
        std::os::unix::fs::symlink(shared.path(), &linked).unwrap();
        assert!(prepare(&context, Some(&linked)).is_err());
        let exposed = root.path().join("exposed-cache");
        fs::create_dir(&exposed).unwrap();
        fs::set_permissions(&exposed, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(prepare(&context, Some(&exposed)).is_err());
        assert_eq!(
            fs::metadata(&exposed).unwrap().permissions().mode() & 0o777,
            0o755
        );
        fs::set_permissions(a.path(), fs::Permissions::from_mode(0o400)).unwrap();
        assert!(a.load(&context.project_root, &context.project_id).is_ok());
    }
}

#[test]
fn metadata_status_does_not_prepare_a_missing_plan_but_evaluation_recovers() {
    let (root, context) = fixture();
    fs::remove_file(artifact(root.path()).path()).unwrap();
    let connection = review::mcp::Connection::new("cache-status".into());
    let status = review::mcp::call(&context, &connection, "status", &json!({}), &|| Ok(()));
    assert!(status["error"].is_null(), "{status}");
    assert_eq!(status["data"]["compiled_cache"]["presence"], "missing");
    assert_eq!(
        status["data"]["compiled_cache"]["repair_route"],
        format!("shield cache prepare --project '{}'", context.project_root)
    );
    assert!(!artifact(root.path()).path().exists());
    let evaluation = review::mcp::call(
        &context,
        &connection,
        "policy_evaluate",
        &json!({"input":{"kind":"argv","executable":"acme","arguments":[],"original_input":{}},
            "context":{"cwd":context.project_root,"environment":{}}}),
        &|| Ok(()),
    );
    assert!(evaluation["error"].is_null(), "{evaluation}");
    // MCP retains the recovered evaluator in its immutable diagnostic handle.
    assert!(!artifact(root.path()).path().exists());
}

#[test]
fn status_reports_whether_this_engine_can_use_the_stored_plan() {
    let (root, context) = fixture();
    let status = review::runtime::status(&context).unwrap();
    assert_eq!(status["compiled_cache"]["presence"], "present");
    assert_eq!(status["compiled_cache"]["compatibility"], "current");

    // A plan another engine or format built carries a different stored key.
    let current = artifact(root.path());
    let mut foreign = current.clone();
    foreign.plan_key = "0".repeat(64);
    fs::rename(current.path(), foreign.path()).unwrap();
    let path = root.path().join(".shield/review/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["active"]["compiled"] = json!(foreign);
    fs::write(&path, state.to_string()).unwrap();
    let status = review::runtime::status(&context).unwrap();
    assert_eq!(status["compiled_cache"]["presence"], "present");
    assert_eq!(
        status["compiled_cache"]["compatibility"],
        "rebuild-required"
    );
}

#[test]
fn concurrent_cold_consumers_agree_and_hot_reads_do_not_rewrite_the_plan() {
    let (root, _context) = fixture();
    let identity = Project::open(root.path())
        .unwrap()
        .snapshot()
        .unwrap()
        .identity()
        .clone();
    let plans = review::plan_cache_directory();
    fs::remove_dir_all(&plans).unwrap();
    let start = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                start.wait();
                assert_eq!(
                    Project::open(root.path())
                        .unwrap()
                        .snapshot()
                        .unwrap()
                        .identity(),
                    &identity
                );
            });
        }
    });
    let plan = fs::read_dir(&plans)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|e| e == "bin"))
        .unwrap();
    let modified = fs::metadata(&plan).unwrap().modified().unwrap();
    Project::open(root.path()).unwrap().snapshot().unwrap();
    assert_eq!(fs::metadata(&plan).unwrap().modified().unwrap(), modified);
}
