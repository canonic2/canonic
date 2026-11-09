//! Repeated changed/older-engine plans plateau without losing live or current bindings.
use super::*;

fn candidate(context: &Context, name: &str) -> Artifact {
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[
        {"id":name,"decision":"allow","reason":"Acme read","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":[name]}}}
    ]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .expect("bundle");
    let candidates = std::collections::BTreeMap::from([("project".into(), json!(bundle))]);
    let evaluator =
        Evaluator::new(review::snapshots(&candidates).expect("snapshots")).expect("policy");
    let state = Store::open(Path::new(&context.project_root))
        .expect("store")
        .read_checkpoint()
        .expect("state");
    build_candidate(
        context,
        &candidates,
        evaluator.revision(),
        &state.operator,
        &evaluator,
    )
    .expect("candidate plan")
}

fn binaries(artifact: &Artifact) -> Vec<PathBuf> {
    fs::read_dir(artifact.path().parent().expect("partition"))
        .expect("partition")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "bin"))
        .collect()
}

#[test]
fn storage_reclamation_stray_neighbors_preserve_missing_plan_recovery() {
    let (root, context) = fixture();
    let current = artifact(root.path());
    let partition = current.path().parent().unwrap().to_path_buf();
    fs::create_dir(partition.join("acme-stray-directory")).unwrap();
    fs::write(partition.join("acme-stray-file"), b"Acme retained bytes").unwrap();
    fs::remove_file(current.path()).unwrap();
    prepare(&context, None).unwrap();
    assert!(
        artifact(root.path())
            .load(&context.project_root, &context.project_id)
            .is_ok()
    );
    assert!(partition.join("acme-stray-directory").is_dir());
    let report = crate::review::project::doctor(&context).unwrap();
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["path"] == json!(partition.join("acme-stray-directory")))
    );
}

#[test]
fn storage_reclamation_cache_versions_plateau_and_inflight_plans_stay_pinned() {
    let (root, context) = fixture();
    let current = artifact(root.path());
    let identity = Project::open(root.path())
        .unwrap()
        .snapshot()
        .unwrap()
        .identity()
        .clone();
    let inflight = candidate(&context, "acme-inflight");
    for index in 0..12 {
        let built = candidate(&context, &format!("acme-{index}"));
        assert!(binaries(&built).len() <= 3);
        assert!(
            inflight
                .load(&context.project_root, &context.project_id)
                .is_ok()
        );
        assert!(current.path().exists());
    }
    let old = inflight.path();
    drop(inflight);
    let built = candidate(&context, "acme-after-release");
    assert!(!old.exists());
    assert!(binaries(&built).len() <= 2);
    assert_eq!(
        Project::open(root.path())
            .unwrap()
            .snapshot()
            .unwrap()
            .identity(),
        &identity
    );
}

#[test]
fn storage_reclamation_prunes_old_engine_keys_without_touching_another_enrollment() {
    let (root, context) = fixture();
    let (other, other_context) = fixture();
    let current = artifact(root.path());
    let other_artifact = artifact(other.path());
    let original = fs::read(other_artifact.path()).unwrap();
    let bytes = fs::read(current.path()).unwrap();
    let partition = current.path().parent().unwrap().to_path_buf();
    let old_key = policy::fingerprint(b"Acme older engine and plan format");
    atomic_write(&partition.join(format!("{old_key}.bin")), &bytes).unwrap();
    atomic_write(&partition.join(format!("{old_key}.inputs.json")), b"{}").unwrap();
    prepare(&context, None).unwrap();
    assert!(!partition.join(format!("{old_key}.bin")).exists());
    assert!(!partition.join(format!("{old_key}.inputs.json")).exists());
    assert_eq!(fs::read(other_artifact.path()).unwrap(), original);
    assert!(
        other_artifact
            .load(&other_context.project_root, &other_context.project_id)
            .is_ok()
    );
    assert!(
        current
            .load(&context.project_root, &context.project_id)
            .is_ok()
    );
}
