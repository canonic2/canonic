//! A folder's authority follows this user's consent to its exact content,
//! whichever folder holds it; one-time grants are spent once across copies.
use super::*;
use crate::{
    configuration::Bundle,
    review::{
        self, Context,
        hook_approval::{self, Identity, Key, Outcome, Summary},
        integrations::{AskFallback, IntegrationChoice},
        operator::integrations::configure_fixture,
        storage::portability::consent,
    },
};
use std::{fs, path::Path};

const CONFIG: &str = "schema_version: 1\npacks: []\n";

/// A per-test user home, so records never leak between tests or runs.
fn user(name: &str) -> assert_fs::TempDir {
    let home = assert_fs::TempDir::new().unwrap();
    consent::fixture_namespace(&format!("{name}:{}", home.path().display()));
    home
}

fn project() -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle = Bundle::capture(CONFIG, &[], None).unwrap();
    let context = review::mcp::initialize_fixture(root.path(), &bundle);
    fs::write(root.path().join(".shield/config.yml"), CONFIG).unwrap();
    (root, context)
}

fn copy(from: &Path) -> assert_fs::TempDir {
    let target = assert_fs::TempDir::new().unwrap();
    crate::test_support::copy_fixture_directory(from, target.path());
    target
}

fn pause(root: &Path) {
    let context = review::open(root).unwrap();
    super::super::Store::open(root)
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.enforcement.enabled = false;
            Ok(Value::Null)
        })
        .unwrap();
}

fn state(root: &Path) -> State {
    super::super::Store::open(root)
        .unwrap()
        .read_checkpoint()
        .unwrap()
}

fn enrolled(root: &Path, agent: &str) -> bool {
    review::cli::enrolled(&review::open(root).unwrap(), agent)
}

#[test]
fn a_copy_keeps_consented_settings_and_registration_for_the_same_user() {
    let _home = user("same-user");
    let (original, _) = project();
    configure_fixture(
        original.path(),
        IntegrationChoice::agent("claude"),
        AskFallback::Block,
    );
    pause(original.path());
    let target = copy(original.path());
    assert!(!state(target.path()).enforcement.enabled);
    assert!(enrolled(target.path(), "claude"));
    assert!(state(target.path()).active.is_some());
}

#[test]
fn another_users_home_gives_defaults_and_no_registration() {
    let _home = user("first-user");
    let (original, _) = project();
    configure_fixture(
        original.path(),
        IntegrationChoice::agent("claude"),
        AskFallback::Block,
    );
    pause(original.path());
    let target = copy(original.path());
    let _other = user("second-user");
    let loaded = state(target.path());
    assert!(loaded.enforcement.enabled);
    assert!(loaded.operator["integration"].is_null());
    assert!(
        loaded.active.is_none(),
        "policy this user never reviewed is inactive"
    );
    assert!(!enrolled(target.path(), "claude"));
}

#[test]
fn a_worktree_with_only_committed_files_adopts_the_reviewed_registration() {
    let _home = user("worktree");
    let (original, _) = project();
    configure_fixture(
        original.path(),
        IntegrationChoice::agent("claude"),
        AskFallback::Block,
    );
    // A worktree has the committed and copied agent settings, not the
    // ignored review state.
    let worktree = copy(original.path());
    fs::remove_dir_all(worktree.path().join(".shield/review")).unwrap();
    assert!(enrolled(worktree.path(), "claude"));

    // An edited Shield entry is not the reviewed content.
    let edited = copy(original.path());
    fs::remove_dir_all(edited.path().join(".shield/review")).unwrap();
    let entries = edited.path().join(".mcp.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&entries).unwrap()).unwrap();
    config["mcpServers"]["shield"]["args"]
        .as_array_mut()
        .unwrap()
        .push(json!("--edited"));
    fs::write(&entries, config.to_string()).unwrap();
    assert!(!enrolled(edited.path(), "claude"));
}

fn identity(context: &Context) -> Value {
    review::runtime::status(context).unwrap()["active"].clone()
}

/// Refuse a request made from the project's `app` folder, as the hook records
/// it: relative to the project.
fn refuse(root: &Path, active: &Value) -> Outcome {
    let folder = root.join("app");
    fs::create_dir_all(&folder).unwrap();
    let cwd = hook_approval::relative_cwd(root, &folder.display().to_string());
    assert_eq!(cwd, "app");
    let key = Key {
        agent: "codex",
        session: "acme-session",
        cwd: &cwd,
        tool: "Bash",
        input_digest: crate::policy::fingerprint(b"acme-discard"),
    };
    let identity = Identity {
        activation_id: active["activation_id"].as_str().unwrap(),
        policy_revision: active["policy_revision"].as_str().unwrap(),
        candidate_digest: active["candidate_digest"].as_str().unwrap(),
    };
    let summary = Summary {
        action: "Run this command:\ngit checkout -- .codex/config.toml".into(),
        reason: "it changes agent settings".into(),
        overrides_deny: false,
        deny_rules: vec![],
        settings_change: true,
    };
    hook_approval::refused(root, &key, &identity, &summary).unwrap()
}

#[test]
fn a_consumed_one_time_grant_cannot_be_used_again_in_a_copy() {
    let _home = user("ledger");
    let (original, context) = project();
    let active = identity(&context);
    let Outcome::Pending(id, _) = refuse(original.path(), &active) else {
        panic!("expected a pending request");
    };
    let mut connection = context.clone();
    connection.surface = "mcp".into();
    connection.agent = Some("codex".into());
    hook_approval::approve(&connection, &json!({"request_id":id}), true).unwrap();
    let target = copy(original.path());
    assert!(matches!(refuse(target.path(), &active), Outcome::Allowed));
    assert!(
        matches!(refuse(original.path(), &active), Outcome::Pending(..)),
        "the same grant was already spent in the copy"
    );
}

fn files(directory: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(directory).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, found);
        } else {
            found.push(path);
        }
    }
}

#[test]
fn no_persisted_review_file_names_the_project_folder() {
    let _home = user("no-paths");
    let (original, context) = project();
    configure_fixture(
        original.path(),
        IntegrationChoice::agent("codex"),
        AskFallback::Block,
    );
    pause(original.path());
    let active = identity(&context);
    refuse(original.path(), &active);
    let roots = [
        context.project_root.clone(),
        original.path().display().to_string(),
    ];
    let mut found = Vec::new();
    files(&original.path().join(".shield/review"), &mut found);
    assert!(!found.is_empty());
    for path in found {
        let text = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
        for root in &roots {
            assert!(
                !text.contains(root.as_str()),
                "{} names {root}",
                path.display()
            );
        }
    }
}

#[test]
fn a_legacy_checkpoint_at_its_recorded_root_converts_with_its_authority() {
    let _home = user("legacy-source");
    let (original, context) = project();
    configure_fixture(
        original.path(),
        IntegrationChoice::agent("claude"),
        AskFallback::Block,
    );
    pause(original.path());
    // The earlier format recorded its root and no content consent existed.
    let path = original.path().join(".shield/review/state.json");
    legacy_at_recorded_root(original.path(), &context);
    let legacy = copy(original.path());
    let _fresh = user("legacy-upgraded");
    let converted = state(original.path());
    assert!(!converted.enforcement.enabled);
    assert!(converted.active.is_some());
    assert!(enrolled(original.path(), "claude"));
    let written: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(written.get("project_root").is_none());
    assert!(written["operator"].get("runtime_incarnation").is_none());
    // The same legacy bytes elsewhere prove nothing and get defaults.
    let _other = user("legacy-copy");
    let copied = state(legacy.path());
    assert!(copied.enforcement.enabled);
    assert!(copied.operator["integration"].is_null());
}

fn legacy_at_recorded_root(root: &Path, context: &Context) -> Vec<u8> {
    let path = root.join(".shield/review/state.json");
    let mut raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    raw["project_root"] = json!(context.project_root);
    raw["operator"]
        .as_object_mut()
        .unwrap()
        .remove("policy_consent_key");
    let bytes = raw.to_string().into_bytes();
    fs::write(&path, &bytes).unwrap();
    bytes
}

#[test]
fn a_locked_first_access_converts_with_a_recovery_copy() {
    let _home = user("legacy-locked");
    let (original, context) = project();
    let bytes = legacy_at_recorded_root(original.path(), &context);
    let _fresh = user("legacy-locked-upgraded");
    // The first access after the upgrade is a write, not a plain read.
    pause(original.path());
    let review = original.path().join(".shield/review");
    let recovery = review.join(format!(
        "bootstrap-{}.json",
        crate::policy::fingerprint(&bytes)
    ));
    assert!(recovery.exists(), "the original bytes are kept");
    let mut found = Vec::new();
    files(&review, &mut found);
    assert!(
        found
            .iter()
            .any(|path| String::from_utf8_lossy(&fs::read(path).unwrap())
                .contains("checkpoint_converted")),
        "the conversion is recorded"
    );
    let converted = state(original.path());
    assert!(!converted.enforcement.enabled);
    assert!(converted.active.is_some());
}

/// Write the earlier format's committed administration receipt for exactly
/// this checkpoint's settings, in the current user's store.
#[cfg(unix)]
fn administration_receipt(root: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let path = root.join(".shield/review/state.json");
    let mut raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let origin = "a".repeat(64);
    let token = "acme-token";
    let settings = json!({"enabled":raw["enforcement"]["enabled"],
        "path_access":raw["operator"]["path_access"],
        "integration":raw["operator"]["integration"],
        "pending":!raw["operator"]["integration_pending"].is_null()});
    let parent = consent::fixture_directory()
        .unwrap()
        .with_extension("administration");
    let directory = parent.join("administration");
    for folder in [&parent, &directory] {
        fs::create_dir_all(folder).unwrap();
        fs::set_permissions(folder, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let receipt = directory.join(format!("{origin}.json"));
    fs::write(
        &receipt,
        json!({"schema_version":1,"token":token,"project":raw["project_id"],
            "committed":true,"administration":settings})
        .to_string(),
    )
    .unwrap();
    fs::set_permissions(&receipt, fs::Permissions::from_mode(0o600)).unwrap();
    raw["operator"]["administration_origin"] =
        json!({"schema_version":1,"origin":origin,"token":token});
    fs::write(&path, raw.to_string()).unwrap();
}

#[cfg(unix)]
#[test]
fn an_administration_receipt_never_vouches_for_policy() {
    // Policy and setup reviewed by another user; this user holds only a
    // committed receipt for exactly the checkpoint's settings.
    let _author = user("receipt-author");
    let (original, _) = project();
    configure_fixture(
        original.path(),
        IntegrationChoice::agent("claude"),
        AskFallback::Block,
    );
    pause(original.path());
    let _reader = user("receipt-reader");
    administration_receipt(original.path());
    let converted = state(original.path());
    assert!(
        !converted.enforcement.enabled,
        "the receipt proves settings"
    );
    assert!(
        converted.active.is_none(),
        "unreviewed policy stays inactive"
    );
    assert!(
        converted.operator["integration"].is_null(),
        "registration needs reviewed policy"
    );
    // The conversion recorded no policy consent: a later read still finds
    // the policy unreviewed.
    assert!(state(original.path()).active.is_none());
}
