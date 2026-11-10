//! Hints detect metadata changes without loading edited source into authority.
use super::*;

#[cfg(unix)]
fn fixture() -> (assert_fs::TempDir, Context, BTreeMap<String, String>) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let store = storage::Store::open(root.path()).unwrap();
    let state = store.read_current().unwrap();
    let files = crate::review::publication::projections::projections(
        &state.candidates["project"],
        &state.operator,
    )
    .unwrap();
    crate::review::publication::projections::materialize(&context, &files).unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.operator["projection_file_hashes"] =
                json!(crate::review::publication::projections::file_hashes(&files));
            Ok(json!({}))
        })
        .unwrap();
    let state = store.read_checkpoint().unwrap();
    let (changed, manifest) = verify(&context, &state, &files).unwrap();
    assert!(!changed);
    bind(&context, &state, &manifest.unwrap()).unwrap();
    (root, context, files)
}

#[cfg(unix)]
#[test]
fn input_metadata_probe_detects_same_size_preserved_mtime_without_using_edited_source() {
    let (root, context, _) = fixture();
    let store = storage::Store::open(root.path()).unwrap();
    let before = store.read_checkpoint().unwrap();
    assert_eq!(probe(&before)["status"], "metadata-match");
    let path = root.path().join(".shield/config.yml");
    let original = fs::metadata(&path).unwrap();
    let mut bytes = fs::read(&path).unwrap();
    bytes[0] = if bytes[0] == b'?' { b'!' } else { b'?' };
    fs::write(&path, bytes).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(original.modified().unwrap()))
        .unwrap();
    let checkpoint = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    let snapshot = crate::runtime::Project::open(root.path())
        .unwrap()
        .snapshot()
        .unwrap();
    assert_eq!(snapshot.input_status()["status"], "changed");
    assert_eq!(snapshot.input_status()["authorizes"], false);
    assert_eq!(
        snapshot.identity().policy_revision(),
        before.policy_revision
    );
    assert_eq!(
        fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        checkpoint
    );
    let status = crate::review::project::status(&context).unwrap();
    assert_eq!(status["project_file_inspection"], "unchecked");
    assert_eq!(status["project_input_metadata"]["probe"], "not-run");
}

#[cfg(unix)]
#[test]
fn input_metadata_probe_detects_optional_creation_parent_links_and_corrupt_hint() {
    let (root, _, _) = fixture();
    let state = storage::Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    fs::write(root.path().join(".shield/config.local.yml"), "unreviewed").unwrap();
    assert_eq!(probe(&state)["status"], "changed");
    fs::remove_file(root.path().join(".shield/config.local.yml")).unwrap();
    let artifact = &state.active.as_ref().unwrap().compiled;
    let reference = reference(artifact).unwrap();
    let path = artifact
        .path()
        .parent()
        .unwrap()
        .join(format!("{}.probe.json", reference.object));
    fs::write(&path, b"{}").unwrap();
    assert_eq!(probe(&state)["status"], "unchecked");
    let mut oversized = serde_json::to_value(artifact).unwrap();
    oversized["input_hints"]["length"] = json!(MAX_BYTES + 1);
    let oversized: Artifact = serde_json::from_value(oversized).unwrap();
    assert_eq!(availability(&oversized)["availability"], "unavailable");
    // Unsupported derived metadata does not revoke valid compiled authority.
    let mut raw = serde_json::to_value(artifact).unwrap();
    raw["input_hints"] = json!({"schema_version":99,"future":true});
    let parsed: Artifact = serde_json::from_value(raw).unwrap();
    assert_eq!(availability(&parsed)["availability"], "unavailable");
    assert!(parsed.load(&state.project_root, &state.project_id).is_ok());
}

#[cfg(unix)]
#[test]
fn input_metadata_requires_event_bound_exact_descriptor_before_any_input_stat() {
    use clap::Parser as _;
    let (root, context, _) = fixture();
    let store = storage::Store::open(root.path()).unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    for add_path in [false, true] {
        let mut state = store.read_checkpoint().unwrap();
        let artifact = &state.active.as_ref().unwrap().compiled;
        let original = reference(artifact).unwrap();
        let object_path = artifact
            .path()
            .parent()
            .unwrap()
            .join(format!("{}.probe.json", original.object));
        let mut object: Value = serde_json::from_slice(&fs::read(object_path).unwrap()).unwrap();
        if add_path {
            object["inputs"]["new-unapproved.yml"] = Value::Null;
        } else {
            object["inputs"]
                .as_object_mut()
                .unwrap()
                .remove("config.yml");
        }
        let bytes = serde_json::to_vec(&object).unwrap();
        let digest = policy::fingerprint(&bytes);
        let name = format!("{digest}.probe.json");
        let _pin =
            super::super::retention::publish_probe(&context.project_root, artifact, &name, &bytes)
                .unwrap();
        let descriptor = state
            .active
            .as_mut()
            .unwrap()
            .compiled
            .input_hints
            .as_mut()
            .unwrap();
        descriptor["object"] = json!(digest);
        descriptor["length"] = json!(bytes.len());
        // Optional tampering never invalidates the required policy proof.
        state.check(&context).unwrap();
        let trace = log.request("mcp", "policy_evaluate", None, None).unwrap();
        {
            let _scope = trace.enter();
            assert_eq!(probe(&state)["status"], "unchecked");
            assert_eq!(trace.recorded_count("cache.input_metadata_probes"), 0);
        }
        trace.finish("success");
    }
    let mut state = store.read_checkpoint().unwrap();
    state
        .authorization_events
        .retain(|event| event["details"]["purpose"] != "projection-input-metadata");
    state.check(&context).unwrap();
    assert_eq!(probe(&state)["status"], "unchecked");
}

#[cfg(unix)]
#[test]
fn input_metadata_refresh_keeps_one_current_auxiliary_proof() {
    let (root, context, files) = fixture();
    for _ in 0..3 {
        fs::File::options()
            .write(true)
            .open(root.path().join(".shield/config.yml"))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::now()))
            .unwrap();
        refresh(&context, &files).unwrap();
        let state = storage::Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap();
        assert_eq!(
            state
                .authorization_events
                .iter()
                .filter(|event| event["details"]["purpose"] == "projection-input-metadata")
                .count(),
            1
        );
        assert_eq!(probe(&state)["status"], "metadata-match");
        state.check(&context).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn input_metadata_capture_refuses_an_already_edited_baseline() {
    let (root, context, files) = fixture();
    let state = storage::Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    fs::write(root.path().join(".shield/config.yml"), "already edited").unwrap();
    let (changed, manifest) = verify(&context, &state, &files).unwrap();
    assert!(changed);
    assert!(manifest.is_none());
}

#[cfg(unix)]
#[test]
fn input_metadata_never_follows_a_symlink_as_a_clean_hint() {
    use std::os::unix::fs::symlink;
    let (root, context, files) = fixture();
    let state = storage::Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    let path = root.path().join(".shield/config.yml");
    let target = root.path().join("outside.yml");
    fs::rename(&path, &target).unwrap();
    symlink(&target, &path).unwrap();
    assert_eq!(probe(&state)["status"], "unchecked");
    assert!(verify(&context, &state, &files).is_err());
}

#[cfg(not(unix))]
#[test]
fn input_metadata_requires_native_change_time() {
    assert!(Stamp::capture(&fs::metadata(std::env::temp_dir()).unwrap()).is_none());
}
