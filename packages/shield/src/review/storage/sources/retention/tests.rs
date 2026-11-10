//! Repeated real activations reclaim unreachable bytes, preserving exact readers and recovery.
use super::*;
use crate::{
    configuration::{Bundle, Evaluator},
    review::{self, Context},
};
use serde_json::json;
use std::{collections::BTreeMap, io::Write as _};
mod processes;

fn fixture() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().expect("project");
    let bundle = Bundle::capture("schema_version: 1\npacks: []\n", &[], None).expect("bundle");
    let mut context = review::mcp::initialize_fixture(root.path(), &bundle);
    context.surface = "cli".into();
    (root, context)
}

pub(in crate::review) fn activate(context: &Context, name: &str) {
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[
        {"id":name,"decision":"allow","reason":"Acme read","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":[name]}}}
    ]}).to_string();
    let bundle = Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .expect("captured policy");
    let candidates = BTreeMap::from([("project".into(), json!(bundle))]);
    let revision = Evaluator::new(review::snapshots(&candidates).expect("snapshots"))
        .expect("policy")
        .revision()
        .to_owned();
    let digest = super::super::map_digest(&candidates);
    let store = Store::open(Path::new(&context.project_root)).expect("store");
    let previous = store
        .read_checkpoint()
        .expect("checkpoint")
        .active
        .expect("active")
        .activation_id()
        .to_owned();
    store
        .update_checkpoint(context, &|| Ok(()), |state| {
            state.candidates = candidates;
            state.policy_revision = revision.clone();
            Ok(Value::Null)
        })
        .expect("candidate publication");
    review::runtime::publish_fixture(context, &revision, &digest, Some(&previous), &|| Ok(()))
        .expect("activation");
}

fn objects(store: &Store) -> Vec<std::path::PathBuf> {
    fs::read_dir(store.directory.join("sources"))
        .expect("objects")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect()
}

#[test]
fn storage_reclamation_stray_source_neighbors_do_not_block_publication() {
    let (root, context) = fixture();
    let directory = root.path().join(".shield/review/sources");
    fs::create_dir(directory.join("acme-stray")).unwrap();
    fs::write(directory.join("acme-backup"), b"Acme preserved bytes").unwrap();
    activate(&context, "acme-after-stray");
    assert!(directory.join("acme-stray").is_dir());
    assert_eq!(
        fs::read(directory.join("acme-backup")).unwrap(),
        b"Acme preserved bytes"
    );
    assert!(
        Store::open(root.path())
            .unwrap()
            .read_current_evaluated()
            .is_ok()
    );
}

#[test]
fn storage_reclamation_repeated_activations_plateau_and_live_sources_survive_until_release() {
    let (root, context) = fixture();
    let store = Store::open(root.path()).unwrap();
    let (captured, pin) = store.read_pinned_checkpoint().unwrap();
    let original = captured
        .active
        .as_ref()
        .unwrap()
        .project_candidate()
        .unwrap()
        .clone();
    for index in 0..12 {
        activate(&context, &format!("acme-{index}"));
        assert!(
            objects(&store).len() <= 3,
            "only current, predecessor and live capture may remain"
        );
        assert!(store.captured_candidate(&original).is_ok());
    }
    drop(pin);
    activate(&context, "acme-after-release");
    assert!(objects(&store).len() <= 2);
    assert!(store.captured_candidate(&original).is_err());
}

#[test]
fn storage_reclamation_reads_committed_references_after_acquiring_the_directory() {
    let (root, _context) = fixture();
    let store = Store::open(root.path()).unwrap();
    let bundle = Bundle::capture(
        "# Acme next checkpoint\nschema_version: 1\npacks: []\n",
        &[],
        None,
    )
    .unwrap();
    let (references, pins) = store
        .retain_candidates(&BTreeMap::from([("project".into(), json!(bundle))]))
        .unwrap();
    let path = store.directory.join("state.json");
    let original = fs::read(&path).unwrap();
    let mut next: Value = serde_json::from_slice(&original).unwrap();
    next["candidates"] = json!(references);
    let guard = Directory::enter(&store.directory.join("sources")).unwrap();
    // An unreadable intermediate checkpoint must not be observed while the
    // namespace lease belongs to its publisher.
    fs::write(&path, b"incomplete checkpoint").unwrap();
    let (sent, received) = std::sync::mpsc::channel();
    let project = root.path().to_path_buf();
    let worker = std::thread::spawn(move || {
        let store = Store::open(&project).unwrap();
        let result = Batch::new(&Value::Null, false).unwrap().admit(
            &Namespace::project(&store),
            &format!("{}.json", "a".repeat(64)),
            1,
        );
        sent.send(result).unwrap();
    });
    assert!(matches!(
        received.recv_timeout(std::time::Duration::from_millis(100)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    fs::write(&path, serde_json::to_vec(&next).unwrap()).unwrap();
    drop(pins);
    drop(guard);
    received.recv().unwrap().unwrap();
    worker.join().unwrap();
    assert!(store.captured_candidate(&references["project"]).is_ok());
}

#[test]
#[cfg(unix)]
fn storage_reclamation_concurrent_preparers_pin_the_published_source_inode() {
    use std::{os::unix::fs::MetadataExt, sync::Arc};
    let (root, _context) = fixture();
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let workers = (0..4)
        .map(|_| {
            let project = root.path().to_path_buf();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = Store::open(&project).unwrap();
                let bundle = Bundle::capture(
                    "# Acme shared preparation\nschema_version: 1\npacks: []\n",
                    &[],
                    None,
                )
                .unwrap();
                barrier.wait();
                store
                    .retain_candidates(&BTreeMap::from([("project".into(), json!(bundle))]))
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let preparations = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    for (references, pins) in &preparations {
        let name = super::super::manifest::name(&references["project"]).unwrap();
        let path = root.path().join(".shield/review/sources").join(name);
        let published = fs::metadata(path).unwrap();
        assert_eq!(pins.len(), 1);
        let held = pins[0].metadata().unwrap();
        assert_eq!((held.dev(), held.ino()), (published.dev(), published.ino()));
    }
}

#[test]
fn storage_reclamation_outside_lock_preparation_stays_pinned_until_publication_or_drop() {
    let (root, context) = fixture();
    let store = Store::open(root.path()).unwrap();
    let bundle = Bundle::capture(
        "# Acme uncommitted preparation\nschema_version: 1\npacks: []\n",
        &[],
        None,
    )
    .unwrap();
    let (references, pins) = store
        .retain_candidates(&BTreeMap::from([("project".into(), json!(bundle))]))
        .unwrap();
    let reference = &references["project"];
    for index in 0..3 {
        activate(&context, &format!("acme-{index}"));
    }
    assert!(store.captured_candidate(reference).is_ok());
    drop(pins);
    activate(&context, "acme-preparation-released");
    assert!(store.captured_candidate(reference).is_err());
}

#[test]
fn storage_reclamation_preserves_upgrade_and_enrollment_recovery_sources() {
    for prefix in ["upgrade", "bootstrap"] {
        let (root, context) = fixture();
        let store = Store::open(root.path()).unwrap();
        let original = store
            .read_checkpoint()
            .unwrap()
            .active
            .unwrap()
            .project_candidate()
            .unwrap()
            .clone();
        let bytes = fs::read(store.directory.join("state.json")).unwrap();
        let digest = crate::policy::fingerprint(&bytes);
        let path = store.directory.join(format!("{prefix}-{digest}.json"));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        super::super::super::private(&mut options);
        options.open(&path).unwrap().write_all(&bytes).unwrap();
        store
            .update_checkpoint(&context, &|| Ok(()), |state| {
                if prefix == "upgrade" {
                    state.operator["upgrade_recovery"] = json!([digest]);
                } else {
                    state.operator["bootstrap"]["checkpoint_recovery"] = json!([digest]);
                }
                Ok(Value::Null)
            })
            .unwrap();
        for index in 0..3 {
            activate(&context, &format!("acme-{index}"));
        }
        assert!(store.captured_candidate(&original).is_ok());
        assert!(path.exists());
        store
            .update_checkpoint(&context, &|| Ok(()), |state| {
                if prefix == "upgrade" {
                    state.operator["upgrade_recovery"] = json!([]);
                } else {
                    state.operator["bootstrap"]["checkpoint_recovery"] = json!([]);
                }
                Ok(Value::Null)
            })
            .unwrap();
        activate(&context, "acme-released");
        assert!(store.captured_candidate(&original).is_err());
    }
}

#[test]
fn storage_reclamation_preserves_pending_review_captures_and_consumed_grants_stay_consumed() {
    let (root, context, evidence, resource) = review::read_approval::tests::fixture();
    let consumed =
        review::read_approval::gate(&context, "acme-factory", &evidence, &resource, None, || {
            true
        })
        .unwrap();
    let id = consumed["approval"]["request_id"].as_str().unwrap();
    review::read_approval::authorize(&context, id, 300, None, false).unwrap();
    assert_eq!(
        review::read_approval::gate(
            &context,
            "acme-factory",
            &evidence,
            &resource,
            Some(id),
            || true
        )
        .unwrap()["code"],
        "allowed-once"
    );
    let proposal =
        review::tests::propose(&context, &review::tests::input("acme-review"), &|| Ok(())).unwrap();
    let store = Store::open(root.path()).unwrap();
    let pending = store
        .read_checkpoint()
        .unwrap()
        .proposals
        .into_iter()
        .find(|p| json!(p.id) == proposal["proposal_id"])
        .unwrap();
    let original = store.captured_candidate(&pending.candidate).unwrap();
    for index in 0..4 {
        activate(&context, &format!("acme-{index}"));
    }
    assert_eq!(
        store.captured_candidate(&pending.candidate).unwrap(),
        original
    );
    assert!(store.captured_candidate(&pending.base_candidate).is_ok());
    let latest = crate::runtime::Project::open(root.path())
        .unwrap()
        .snapshot()
        .unwrap()
        .evaluate_resource(
            evidence.request().clone(),
            evidence.resource().unwrap().clone(),
        )
        .unwrap();
    assert_ne!(
        review::read_approval::gate(
            &context,
            "acme-factory",
            &latest,
            &resource,
            Some(id),
            || true
        )
        .unwrap()["code"],
        "allowed-once"
    );
}
