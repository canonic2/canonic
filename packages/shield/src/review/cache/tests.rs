//! Real cache repair and current-generation boundaries, without command execution.
use super::*;
use crate::{configuration::Bundle, review, runtime::Project};

fn fixture() -> (assert_fs::TempDir, Context) {
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

#[cfg(unix)]
#[test]
fn preparation_removes_only_exited_writers_temporaries() {
    let (root, context) = fixture();
    let current = artifact(root.path());
    let partition = current.path().parent().unwrap().to_path_buf();
    let plan = &current.plan_key;
    let dead = review::storage::exited_pid();
    let exited = [
        format!("{plan}.{dead}-0-1.tmp"),
        format!("{plan}.inputs.{dead}-0-1.tmp"),
    ];
    let kept = [
        format!("{plan}.1-0-1.tmp"),
        format!("{plan}.{}-0-1.tmp", std::process::id()),
        format!("{plan}.{dead}-0.tmp"),
        format!("acme.{dead}-0-1.tmp"),
    ];
    for name in exited.iter().chain(&kept) {
        fs::write(partition.join(name), b"partial").unwrap();
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
fn missing_and_corrupt_cache_recover_but_linked_artifacts_refuse() {
    let (root, context) = fixture();
    let project = Project::open(root.path()).unwrap();
    let identity = project.snapshot().unwrap().identity().clone();
    let current = artifact(root.path());
    let original = fs::read(current.path()).unwrap();
    for digest in [policy::fingerprint(&original), "blake3-v1:invalid".into()] {
        let mut invalid = current.clone();
        invalid.digest = digest;
        assert!(
            invalid
                .load(&context.project_root, &context.project_id)
                .is_err(),
            "cache integrity requires a valid versioned digest"
        );
    }
    fs::remove_file(current.path()).unwrap();
    assert_eq!(project.snapshot().unwrap().identity(), &identity);
    let mut corrupt = original.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    fs::write(current.path(), &corrupt).unwrap();
    let manifest = current.path().with_extension("inputs.json");
    fs::write(
        manifest,
        json!({"compiled_digest":policy::fingerprint(&corrupt)}).to_string(),
    )
    .unwrap();
    assert_eq!(project.snapshot().unwrap().identity(), &identity);
    assert_eq!(fs::read(current.path()).unwrap(), original);
    let mut mcp = context.clone();
    mcp.surface = "mcp".into();
    assert!(matches!(prepare(&mcp, None), Err(Error::ApprovalRequired)));
    prepare(&context, None).unwrap();
    assert_eq!(fs::read(current.path()).unwrap(), original);
    #[cfg(unix)]
    {
        fs::remove_file(current.path()).unwrap();
        let foreign = root.path().join("foreign.bin");
        fs::write(&foreign, &original).unwrap();
        std::os::unix::fs::symlink(foreign, current.path()).unwrap();
        assert!(project.snapshot().is_err());
        assert!(prepare(&context, None).is_err());
    }
}

#[test]
fn repair_retains_committed_generation_and_pending_source_objects_share_durable_bytes() {
    let (root, _context) = fixture();
    let project = Project::open(root.path()).unwrap();
    let original = project.snapshot().unwrap().identity().clone();
    let before = artifact(root.path());
    let file = root.path().join(".shield/review/state.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(value["candidates"], value["active"]["candidates"]);
    assert_eq!(value["candidates"]["project"]["kind"], "retained-source-v1");
    let rules =
        json!({"schema_version":1,"rules":[{"id":"pending","decision":"allow","reason":"Acme pending","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]})
        .to_string();
    let pending = Bundle::capture(
        r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#,
        &[],
        Some(&rules),
    )
    .unwrap();
    let pending_revision =
        Evaluator::new(vec![(policy::Scope::Project, pending.snapshot().unwrap())])
            .unwrap()
            .revision()
            .to_owned();
    value["candidates"]["project"] = json!(pending);
    value["policy_revision"] = json!(pending_revision);
    fs::write(&file, value.to_string()).unwrap();
    fs::write(root.path().join(".shield/rules.yml"), rules).unwrap();
    fs::remove_file(before.path()).unwrap();
    assert_eq!(project.snapshot().unwrap().identity(), &original);
    let loaded = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(loaded.policy_revision, pending_revision);
    let checkpoint = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_ne!(checkpoint.candidates, checkpoint.active.unwrap().candidates);
    assert_eq!(
        fs::read_dir(root.path().join(".shield/review/sources"))
            .unwrap()
            .count(),
        2
    );
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
fn incompatible_plans_rebuild_on_demand_and_preserve_activation() {
    let (root, context) = fixture();
    let current = artifact(root.path());
    let mut bytes = fs::read(current.path()).unwrap();
    bytes[..8].copy_from_slice(b"INVALID!");
    let mut corrupted = current.clone();
    corrupted.digest = compiled_digest(&bytes);
    fs::write(corrupted.path(), &bytes).unwrap();
    assert!(
        corrupted
            .load(&context.project_root, &context.project_id)
            .is_err()
    );
    let path = root.path().join(".shield/review/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let activation = state["active"]["activation_id"].clone();
    let revision = state["active"]["policy_revision"].clone();
    state["active"]["compiled"] = json!(corrupted);
    for event in state["authorization_events"].as_array_mut().unwrap() {
        if event["details"]["compiled_digest"] == current.digest {
            event["details"]["compiled_digest"] = json!(corrupted.digest);
        }
    }
    fs::write(&path, state.to_string()).unwrap();
    review::project::status(&context).unwrap();
    assert_eq!(fs::read(corrupted.path()).unwrap(), bytes);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
        state
    );
    Project::open(root.path()).unwrap().snapshot().unwrap();
    let prepared = artifact(root.path());
    assert!(prepared.digest.starts_with("blake3-v1:"));
    assert_eq!(&fs::read(prepared.path()).unwrap()[..8], b"SHPLAN08");
    let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert!(state.authorization_events.iter().any(|event| {
        event["kind"] == "compiled_cache_prepared"
            && event["operator"]["mechanism"] == "derived-cache"
    }));
    assert_eq!(
        json!(state.active.as_ref().unwrap().activation_id()),
        activation
    );
    assert_eq!(
        state.active.unwrap().metadata()["policy_revision"],
        revision
    );
}

#[test]
fn concurrent_cold_consumers_recover_and_hot_reads_do_not_republish() {
    let (root, _context) = fixture();
    let identity = Project::open(root.path())
        .unwrap()
        .snapshot()
        .unwrap()
        .identity()
        .clone();
    let current = artifact(root.path());
    fs::remove_file(current.path()).unwrap();
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
    let checkpoint = root.path().join(".shield/review/state.json");
    let before = fs::read(&checkpoint).unwrap();
    let modified = fs::metadata(current.path()).unwrap().modified().unwrap();
    Project::open(root.path()).unwrap().snapshot().unwrap();
    assert_eq!(fs::read(checkpoint).unwrap(), before);
    assert_eq!(
        fs::metadata(current.path()).unwrap().modified().unwrap(),
        modified
    );
}

#[test]
fn cancelled_recovery_cannot_publish_and_missing_retained_source_cannot_use_projections() {
    let (root, context) = fixture();
    let current = artifact(root.path());
    fs::remove_file(current.path()).unwrap();
    assert!(matches!(
        recover(&context, &|| Err(policy::Error::Interrupted)),
        Err(Error::Policy(policy::Error::Interrupted))
    ));
    assert!(!current.path().exists());
    let sources = root.path().join(".shield/review/sources");
    for entry in fs::read_dir(sources).unwrap() {
        fs::remove_file(entry.unwrap().path()).unwrap();
    }
    assert!(Project::open(root.path()).unwrap().snapshot().is_err());
    assert!(!current.path().exists());
}

#[test]
fn recovery_cannot_replace_a_generation_activated_during_compilation() {
    let (root, context) = fixture();
    fs::remove_file(artifact(root.path()).path()).unwrap();
    let status = super::super::runtime::status(&context).unwrap();
    let calls = std::cell::Cell::new(0);
    let result = recover(&context, &|| {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            super::super::runtime::publish_fixture(
                &context,
                status["review"]["policy_revision"].as_str().unwrap(),
                status["review"]["candidate_digest"].as_str().unwrap(),
                status["active"]["activation_id"].as_str(),
                &|| Ok(()),
            )
            .unwrap();
        }
        Ok(())
    });
    assert!(matches!(result, Err(Error::StaleSnapshot)));
    assert_ne!(
        super::super::runtime::status(&context).unwrap()["active"]["activation_id"],
        status["active"]["activation_id"]
    );
    Project::open(root.path()).unwrap().snapshot().unwrap();
}
