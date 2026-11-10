//! P0 regressions for copied public-hash-only authority adoption.
use super::*;
use crate::{configuration::Bundle, policy, review};
use std::{collections::BTreeMap, fs, path::Path};

const CONFIG: &str = "schema_version: 1\npacks: []\nrules: rules.yml\n";
const RULES: &str = r#"{"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["inspect"]}}},{"id":"forbidden","decision":"deny","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["forbidden"]}}}]}"#;

fn original() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().expect("reviewed source directory");
    let bundle = Bundle::capture(CONFIG, &[], Some(RULES)).expect("reviewed source policy");
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    fs::write(root.path().join(".shield/config.yml"), CONFIG).expect("authored configuration");
    fs::write(root.path().join(".shield/rules.yml"), RULES).expect("authored rules");
    (root, context)
}

pub(super) fn copied(from: &Path, to: &Path) {
    // Preserve private modes so the regression isolates authority rather than
    // incidental copy permission normalization. All paths are test-owned.
    for entry in fs::read_dir(from).expect("source directory inventory") {
        let entry = entry.expect("source directory entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("source entry type").is_dir() {
            fs::create_dir(&target).expect("copied private directory");
            copied(&entry.path(), &target);
        } else {
            assert!(entry.file_type().expect("source entry type").is_file());
            fs::copy(entry.path(), &target).expect("copied private file");
        }
    }
    fs::set_permissions(
        to,
        fs::metadata(from)
            .expect("source directory metadata")
            .permissions(),
    )
    .expect("copied private permissions");
}

fn evaluate(root: &Path, argument: &str) -> Option<policy::Decision> {
    let project = crate::runtime::Project::open(root).ok()?;
    let snapshot = project.snapshot().ok()?;
    let root = root
        .canonicalize()
        .expect("canonical evaluation root")
        .to_str()
        .expect("UTF-8 evaluation root")
        .to_owned();
    Some(
        snapshot
            .evaluate(policy::Request {
                executable: "acme".into(),
                arguments: vec![argument.into()],
                cwd: root.clone(),
                project: root,
            })
            .ok()?
            .evaluation()
            .decision,
    )
}

#[test]
fn copied_recognized_policy_cannot_promote_fallback_or_path_access_from_checkpoint() {
    let home = assert_fs::TempDir::new().unwrap();
    let _consent = scoped_directory(home.path().join("consent"));
    let (root, context) = original();
    review::operator::integrations::configure_fixture(
        root.path(),
        review::integrations::IntegrationChoice::agent("codex"),
        review::integrations::AskFallback::Block,
    );
    let before = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert_eq!(presence(&before)["present"], true);
    let original_access = review::path_access::load(&before.operator).unwrap();
    let original_fallback = review::integrations::with_fallback(
        root.path(),
        before.active.as_ref().unwrap().activation_id(),
        "codex",
        original_access,
        |value| value,
    )
    .unwrap();
    assert!(matches!(
        original_fallback,
        review::integrations::AskFallback::Block
    ));
    let original_bytes = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    let copy = assert_fs::TempDir::new().unwrap();
    copied(root.path(), copy.path());
    let path = copy.path().join(".shield/review/state.json");
    let mut untrusted: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let activation = untrusted["active"].clone();
    let events = untrusted["authorization_events"].clone();
    untrusted["operator"]["integration"]["ask_fallback"]["codex"] = json!("allow");
    untrusted["operator"]["path_access"] = json!({
        "agent_settings_read":"allow","agent_settings_change":"allow","shield_config_change":"allow"});
    assert_eq!(untrusted["active"], activation);
    assert_eq!(untrusted["authorization_events"], events);
    fs::write(&path, untrusted.to_string()).unwrap();

    let copied_context = review::open(copy.path()).unwrap();
    assert_ne!(copied_context.project_id, context.project_id);
    assert_eq!(
        evaluate(copy.path(), "inspect"),
        Some(policy::Decision::Allow),
        "the already privately reviewed policy stays usable"
    );
    assert_eq!(evaluate(copy.path(), "ask"), Some(policy::Decision::Ask));
    let current = Store::open(copy.path()).unwrap().read_checkpoint().unwrap();
    let access = review::path_access::load(&current.operator).unwrap();
    let fallback = review::integrations::with_fallback(
        copy.path(),
        current.active.as_ref().unwrap().activation_id(),
        "codex",
        access,
        |value| value,
    )
    .unwrap();
    assert!(
        matches!(fallback, review::integrations::AskFallback::Block),
        "a copied checkpoint must not turn a reviewed standing Deny into Allow"
    );
    assert_eq!(
        access,
        review::path_access::PathAccess::default(),
        "copied self-reported protected-file permission must not become current authority"
    );
    assert_eq!(
        fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        original_bytes
    );
}

fn forged_activation(state: &mut State) -> Value {
    let mut rules: Value = serde_json::from_str(RULES).expect("fixture source JSON");
    rules["rules"][1]["decision"] = json!("allow");
    let bundle =
        Bundle::capture(CONFIG, &[], Some(&rules.to_string())).expect("unreviewed valid source");
    let evaluator = crate::configuration::Evaluator::new(vec![(
        policy::Scope::Project,
        bundle.snapshot().expect("unreviewed source snapshot"),
    )])
    .expect("valid unreviewed source evaluator");
    let raw = serde_json::to_value(bundle).expect("unreviewed bundle encoding");
    // Reuse the old cache descriptor; relocation rebuilds derived data before
    // consuming it. Every forged binding/event below is just a public hash or
    // literal mechanism claim. This does NOT call a consent publication API.
    let encoded = serde_json::to_value(&*state).expect("checkpoint encoding");
    let artifact = serde_json::from_value(encoded["active"]["compiled"].clone())
        .expect("retained compiled descriptor");
    state.candidates = BTreeMap::from([("project".into(), raw.clone())]);
    state.policy_revision = evaluator.revision().into();
    let context = Context::new(state.project_id.clone(), state.project_root.clone(), "cli");
    review::runtime::publish_confirmed(state, &context, None, artifact, "operator-cli");
    Store::validate_checkpoint(state).expect("public checkpoint integrity accepts forged hashes");
    raw
}

#[test]
fn forged_copied_activation_cannot_launder_unreviewed_source_into_private_consent() {
    let home = assert_fs::TempDir::new().unwrap();
    let _consent = scoped_directory(home.path().join("consent"));
    let (root, _) = original();
    let mut forged = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let original_bytes = fs::read(root.path().join(".shield/review/state.json")).unwrap();
    let forged_source = forged_activation(&mut forged);
    let forged_key = super::super::key(&forged_source).unwrap();
    assert!(
        !read_index(&namespace(false).unwrap())
            .unwrap()
            .candidates
            .contains_key(&forged_key)
    );
    let copy = assert_fs::TempDir::new().unwrap();
    copied(root.path(), copy.path());
    fs::write(
        copy.path().join(".shield/review/state.json"),
        serde_json::to_vec(&forged).unwrap(),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(copy.path().join(".shield/rules.yml")).unwrap(),
        RULES
    );

    // Either retain privately proven policy or expose the unreviewed snapshot
    // as inactive. A public mechanism label cannot authorize the new Allow.
    let _opened = review::open(copy.path());
    assert_ne!(
        evaluate(copy.path(), "forbidden"),
        Some(policy::Decision::Allow)
    );
    assert!(
        !read_index(&namespace(false).unwrap())
            .unwrap()
            .candidates
            .contains_key(&forged_key),
        "automatic relocation must not mint the private evidence it lacked"
    );
    assert_eq!(
        evaluate(root.path(), "forbidden"),
        Some(policy::Decision::Deny)
    );
    assert_eq!(
        fs::read(root.path().join(".shield/review/state.json")).unwrap(),
        original_bytes
    );
}

/// Accepted trade-off: Shield cannot tell an authentic pre-continuity
/// checkpoint from a legacy-shaped record planted at the same root in its
/// owner-only review directory. Both upgrade in place and keep their authority;
/// copies at another root still need private proof (tests above).
#[test]
fn same_root_owned_legacy_shape_upgrades_in_place_with_its_authority() {
    use std::os::unix::fs::MetadataExt as _;
    let home = assert_fs::TempDir::new().unwrap();
    let _consent = scoped_directory(home.path().join("consent"));
    let (root, _) = original();
    let store = Store::open(root.path()).unwrap();
    let mut forged = store.read_checkpoint().unwrap();
    forged_activation(&mut forged);
    for field in [
        "runtime_incarnation",
        "administration_origin",
        "policy_consent_key",
    ] {
        forged.operator.as_object_mut().unwrap().remove(field);
    }
    assert_eq!(
        Path::new(&forged.project_root),
        root.path().canonicalize().unwrap()
    );
    let owner = fs::metadata(&store.directory).unwrap();
    assert_eq!(owner.uid(), rustix::process::geteuid().as_raw());
    assert_eq!(owner.mode() & 0o077, 0);
    let path = store.directory.join("state.json");
    let bytes = serde_json::to_vec(&forged).unwrap();
    fs::write(&path, &bytes).unwrap();
    let upgraded = store.read_checkpoint().unwrap();
    assert_eq!(upgraded.project_id, forged.project_id);
    assert_eq!(
        upgraded
            .active
            .as_ref()
            .map(review::runtime::Active::activation_id),
        forged
            .active
            .as_ref()
            .map(review::runtime::Active::activation_id)
    );
    assert!(upgraded.operator["runtime_incarnation"].is_object());
    // The original bytes remain as the recovery copy.
    let recovery = store
        .directory
        .join(format!("bootstrap-{}.json", policy::fingerprint(&bytes)));
    assert!(recovery.is_file());
    // Repeated access is the ordinary current-state read.
    assert_eq!(
        store.read_checkpoint().unwrap().project_id,
        forged.project_id
    );
}

#[test]
fn seed_cannot_bootstrap_private_consent_from_a_checkpoint_without_native_proof() {
    let home = assert_fs::TempDir::new().unwrap();
    let _consent = scoped_directory(home.path().join("consent"));
    let (root, _) = original();
    let store = Store::open(root.path()).unwrap();
    let mut forged = store.read_checkpoint().unwrap();
    let raw = forged_activation(&mut forged);
    let forged_key = super::super::key(&raw).unwrap();
    forged
        .operator
        .as_object_mut()
        .unwrap()
        .remove("runtime_incarnation");
    forged
        .operator
        .as_object_mut()
        .unwrap()
        .remove("portable_policy");
    assert_eq!(
        review::storage::incarnation::readiness(&forged, root.path())["available"],
        false
    );
    fs::write(
        root.path().join(".shield/review/state.json"),
        serde_json::to_vec(&forged).unwrap(),
    )
    .unwrap();
    // Exercise seed separately from conversion so it cannot rely on its caller
    // having proved continuity. The .ok()/None relocation fallback is another
    // reason this boundary must defend itself.
    let _outcome = super::super::seed(&store, forged);
    assert!(
        !read_index(&namespace(false).unwrap())
            .unwrap()
            .candidates
            .contains_key(&forged_key)
    );
}

#[test]
fn copied_checkpoint_cannot_pause_policy_checks_even_when_its_active_is_missing() {
    let home = assert_fs::TempDir::new().unwrap();
    let _consent = scoped_directory(home.path().join("consent"));
    let (root, _) = original();
    let original = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    assert!(original.enforcement.enabled);
    for keep_active in [true, false] {
        let copy = assert_fs::TempDir::new().unwrap();
        copied(root.path(), copy.path());
        let path = copy.path().join(".shield/review/state.json");
        let mut untrusted: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        untrusted["enforcement"] = json!({"enabled":false,"generation":1});
        if !keep_active {
            untrusted["active"] = Value::Null;
        }
        fs::write(&path, untrusted.to_string()).unwrap();
        let (_, paused) = review::open_native(copy.path()).unwrap();
        assert!(
            paused.is_none(),
            "copy-only pause cannot turn absent/unreviewed policy into native bypass"
        );
        let after = Store::open(copy.path()).unwrap().read_checkpoint().unwrap();
        assert!(after.enforcement.enabled);
    }
}

#[test]
fn known_retained_copy_reuses_private_source_despite_dirty_projections_and_pending_handles() {
    let home = assert_fs::TempDir::new().unwrap();
    let _consent = scoped_directory(home.path().join("consent"));
    let (root, context) = original();
    let private_lineage = Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap()
        .policy_lineage()
        .to_owned();
    review::tests::propose(&context, &review::tests::input("acme-extra"), &|| Ok(())).unwrap();
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .proposals
            .len(),
        1
    );
    let copy = assert_fs::TempDir::new().unwrap();
    copied(root.path(), copy.path());
    fs::write(
        copy.path().join(".shield/rules.yml"),
        "Acme dirty unreviewed rules",
    )
    .unwrap();
    let checkpoint = copy.path().join(".shield/review/state.json");
    let mut raw: State = serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    let artifact = serde_json::from_value(json!(&raw.active)["compiled"].clone()).unwrap();
    let forged_lineage = "f".repeat(64);
    raw.operator["policy_lineage"] = json!(forged_lineage);
    raw.operator["default"] = json!("forged-copy-registry");
    let public = Context::new(raw.project_id.clone(), raw.project_root.clone(), "cli");
    review::runtime::publish_confirmed(&mut raw, &public, None, artifact, "operator-cli");
    Store::validate_checkpoint(&raw).unwrap();
    fs::write(&checkpoint, serde_json::to_vec(&raw).unwrap()).unwrap();
    let next = review::open(copy.path()).unwrap();
    assert_ne!(next.project_id, context.project_id);
    let state = Store::open(copy.path()).unwrap().read_checkpoint().unwrap();
    assert_ne!(state.policy_lineage(), private_lineage);
    assert_ne!(state.policy_lineage(), forged_lineage);
    assert_eq!(state.operator["portable_lineage_unverified"], true);
    assert!(
        state.operator["default"].is_null(),
        "policy metadata comes from the private source record"
    );
    assert!(state.proposals.is_empty());
    assert_eq!(state.operator["portable_admin_unverified"], true);
    assert_eq!(
        evaluate(copy.path(), "forbidden"),
        Some(policy::Decision::Deny)
    );
    assert_eq!(
        fs::read_to_string(copy.path().join(".shield/rules.yml")).unwrap(),
        "Acme dirty unreviewed rules"
    );
}

#[test]
fn verified_native_rename_preserves_paused_intent_and_registration() {
    let home = assert_fs::TempDir::new().unwrap();
    let _consent = scoped_directory(home.path().join("consent"));
    let (root, context) = original();
    review::operator::integrations::configure_fixture(
        root.path(),
        review::integrations::IntegrationChoice::agent("codex"),
        review::integrations::AskFallback::Block,
    );
    review::mode::Prepared::new(&context, false)
        .unwrap()
        .commit(&context, &|| Ok(()))
        .unwrap();
    let before = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let parent = assert_fs::TempDir::new().unwrap();
    let moved = parent.path().join("moved");
    fs::rename(root.path(), &moved).unwrap();
    let (_, paused) = review::open_native(&moved).unwrap();
    assert!(paused.is_some());
    let after = Store::open(&moved).unwrap().read_checkpoint().unwrap();
    assert!(!after.enforcement.enabled);
    assert_eq!(after.policy_lineage(), before.policy_lineage());
    assert_eq!(
        after.operator["integration"]["ask_fallback"],
        before.operator["integration"]["ask_fallback"]
    );
    assert!(after.operator["portable_admin_unverified"].is_null());
    assert_ne!(after.project_id, before.project_id);
}

#[test]
fn forged_legacy_copy_is_refused_before_conversion_and_remains_inspectable() {
    let home = assert_fs::TempDir::new().unwrap();
    let _consent = scoped_directory(home.path().join("consent"));
    let (root, _) = original();
    let mut forged = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
    let source = forged_activation(&mut forged);
    let copy = assert_fs::TempDir::new().unwrap();
    copied(root.path(), copy.path());
    let mut raw = json!(forged);
    for key in [
        "hook_requests",
        "session_grants",
        "policy_drafts",
        "dismissed_reviews",
        "enforcement",
        "configuration_changes",
    ] {
        raw.as_object_mut().unwrap().remove(key);
    }
    raw["history_generation"] = Value::Null;
    let unprepared = home.path().join("unprepared-forged-cache");
    raw["operator"]["compiled_cache_root"] = json!(unprepared);
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
    let path = copy.path().join(".shield/review/state.json");
    fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    for _ in 0..2 {
        assert!(matches!(
            review::open(copy.path()),
            Err(Error::Invalid(
                "retained policy has no private reviewed-content proof; checkpoint unchanged"
            ))
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    let diagnostic = review::open_diagnostics(copy.path()).unwrap();
    review::project::status(&diagnostic).unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(
        !unprepared.exists(),
        "unreviewed legacy policy cannot prepare cache artifacts"
    );
    assert!(
        !read_index(&namespace(false).unwrap())
            .unwrap()
            .candidates
            .contains_key(&super::super::key(&source).unwrap())
    );
}

#[test]
fn identical_private_content_does_not_attach_an_earlier_copy_to_another_lineage() {
    let home = assert_fs::TempDir::new().unwrap();
    let _consent = scoped_directory(home.path().join("consent"));
    let (first, _) = original();
    let first_state = Store::open(first.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    let (second, _) = original();
    let second_state = Store::open(second.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    assert_ne!(first_state.policy_lineage(), second_state.policy_lineage());
    for private_origin in [false, true] {
        let copy = assert_fs::TempDir::new().unwrap();
        copied(first.path(), copy.path());
        if !private_origin {
            let path = copy.path().join(".shield/review/state.json");
            let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            state["operator"]
                .as_object_mut()
                .unwrap()
                .remove("administration_origin");
            fs::write(path, state.to_string()).unwrap();
        }
        review::open(copy.path()).unwrap();
        let state = Store::open(copy.path()).unwrap().read_checkpoint().unwrap();
        assert_ne!(state.policy_lineage(), second_state.policy_lineage());
        if private_origin {
            assert_eq!(state.policy_lineage(), first_state.policy_lineage());
            assert!(state.operator["portable_lineage_unverified"].is_null());
        } else {
            assert_ne!(state.policy_lineage(), first_state.policy_lineage());
            assert_eq!(state.operator["portable_lineage_unverified"], true);
        }
        assert_eq!(
            evaluate(copy.path(), "forbidden"),
            Some(policy::Decision::Deny)
        );
    }
}
