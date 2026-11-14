//! A project folder is its files: a move, rename or copy keeps its policy,
//! state and history with no follow-up, and source-only clones reuse exact
//! reviewed policy.
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
    crate::test_support::copy_fixture_directory(from, to);
}

#[test]
fn portable_policy_move_opens_without_writing_and_keeps_identity_and_policy() {
    let (original, before) = fixture();
    let checkpoint = original.path().join(".shield/review/state.json");
    let bytes = fs::read(&checkpoint).unwrap();
    let parent = assert_fs::TempDir::new().unwrap();
    let destination = parent.path().join("moved");
    fs::rename(original.path(), &destination).unwrap();
    let context = review::open(&destination).unwrap();
    assert_eq!(
        Path::new(&context.project_root),
        destination.canonicalize().unwrap()
    );
    assert_eq!(context.project_id, before.project_id);
    let state = Store::open(&destination).unwrap().read_current().unwrap();
    assert_eq!(state.candidates["project"]["rules"], RULES);
    assert!(state.active.is_some());
    assert!(
        crate::runtime::Project::open(&destination)
            .unwrap()
            .snapshot()
            .is_ok()
    );
    // Nothing about the move is recorded: the checkpoint is unchanged.
    assert_eq!(
        fs::read(destination.join(".shield/review/state.json")).unwrap(),
        bytes
    );
}

#[test]
fn portable_policy_copy_keeps_its_state_and_leaves_the_original_alone() {
    let (original, before) = fixture();
    let store = Store::open(original.path()).unwrap();
    let mut state = store.read_current().unwrap();
    let generation = state.history_generation.clone();
    state.enforcement = serde_json::from_value(json!({"enabled":false,"generation":3})).unwrap();
    let request_id = state.id("request");
    let event = state.event(
        "decision",
        &before,
        json!({"request_id":request_id}),
        json!({}),
        json!({"marker":"portable-history"}),
    );
    state.record(event);
    store.write(&mut state).unwrap();
    let original_bytes = fs::read(original.path().join(".shield/review/state.json")).unwrap();
    let destination = assert_fs::TempDir::new().unwrap();
    copy(original.path(), destination.path());
    // An edit in the copy's files is the copy's policy, and only the copy's.
    fs::write(
        destination.path().join(".shield/rules.yml"),
        RULES.replace("allow", "deny"),
    )
    .unwrap();
    let context = review::open(destination.path()).unwrap();
    assert_eq!(context.project_id, before.project_id);
    let copied = Store::open(destination.path())
        .unwrap()
        .read_history()
        .unwrap();
    assert_eq!(copied.history_generation, generation);
    assert!(
        !copied.enforcement.enabled,
        "this user's consented pause applies to identical content in any folder"
    );
    assert!(copied.events.iter().any(|event| {
        event["links"]["request_id"] == request_id
            && event["details"]["marker"] == "portable-history"
    }));
    assert_eq!(
        Store::open(destination.path())
            .unwrap()
            .read_current()
            .unwrap()
            .candidates["project"]["rules"],
        RULES.replace("allow", "deny")
    );
    assert_eq!(
        fs::read(original.path().join(".shield/review/state.json")).unwrap(),
        original_bytes
    );
}

#[test]
fn portable_policy_diagnostics_of_a_copy_report_the_folder_without_writing() {
    let (original, before) = fixture();
    let destination = assert_fs::TempDir::new().unwrap();
    copy(original.path(), destination.path());
    let checkpoint = destination.path().join(".shield/review/state.json");
    let bytes = fs::read(&checkpoint).unwrap();
    let context = review::open_diagnostics(destination.path()).unwrap();
    assert_eq!(context.project_id, before.project_id);
    let status = review::project::status(&context).unwrap();
    assert_eq!(status["project_root"], context.project_root);
    assert!(status.get("runtime_relocation_pending").is_none());
    review::project::doctor(&context).unwrap();
    assert_eq!(fs::read(&checkpoint).unwrap(), bytes);
}

#[test]
fn portable_policy_repeated_moves_keep_working() {
    let (original, before) = fixture();
    let parent = assert_fs::TempDir::new().unwrap();
    let mut current = original.path().to_owned();
    for index in 0..4 {
        let destination = parent.path().join(format!("move-{index}"));
        fs::rename(&current, &destination).unwrap();
        let context = review::open(&destination).unwrap();
        assert_eq!(context.project_id, before.project_id);
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
fn portable_policy_corrupt_activation_in_a_copy_is_refused_unchanged() {
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
