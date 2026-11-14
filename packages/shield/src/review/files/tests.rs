use super::*;
use assert_fs::prelude::*;

const CONFIG: &str = "schema_version: 1\npacks: []\nrules: rules.yml\n";
const RULES: &str =
    "schema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules: []\n";

fn project() -> assert_fs::TempDir {
    let root = assert_fs::TempDir::new().expect("fixture");
    root.child(".shield/config.yml")
        .write_str(CONFIG)
        .expect("config");
    root.child(".shield/rules.yml")
        .write_str(RULES)
        .expect("rules");
    root
}

#[test]
fn an_edit_to_the_files_is_the_next_policy() {
    let root = project();
    let first = load(root.path()).expect("policy");
    let again = load(root.path()).expect("cached policy");
    assert_eq!(first.digest, again.digest);
    assert_eq!(first.evaluator.revision(), again.evaluator.revision());

    root.child(".shield/rules.yml")
        .write_str(&format!("# Acme rules.\n{RULES}"))
        .expect("edit");
    let edited = load(root.path()).expect("edited policy");
    assert_ne!(edited.digest, first.digest);
    let bundle = serde_json::json!(capture(root.path()).expect("capture"));
    assert_eq!(bundle["rules"], format!("# Acme rules.\n{RULES}"));
}

#[test]
fn a_local_layer_joins_the_shared_files() {
    let root = project();
    let shared = load(root.path()).expect("policy");
    root.child(".shield/config.local.yml")
        .write_str(&format!(
            "schema_version: 1\npacks: []\naccess_paths:\n  read:\n  - {}\n",
            crate::test_support::absolute("/srv/acme")
        ))
        .expect("local");
    let local = load(root.path()).expect("local policy");
    assert_ne!(shared.evaluator.revision(), local.evaluator.revision());
}

#[test]
fn invalid_files_are_an_error_not_an_empty_policy() {
    let root = project();
    root.child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: [\n")
        .expect("broken");
    assert!(load(root.path()).is_err());
}

#[test]
fn proposals_build_on_edited_files_and_keep_the_edit() {
    let (root, context) = crate::review::tests::enrolled();
    // A person adds a rule by editing the files.
    let rules = r#"{"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"person","decision":"allow","reason":"Acme person","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["person"]}}}]}"#;
    root.child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: []\nrules: rules.yml\n")
        .expect("config");
    root.child(".shield/rules.yml")
        .write_str(rules)
        .expect("rules");
    let state = crate::review::project::load(&context).expect("state");
    assert!(
        state.candidates["project"]["rules"]
            .as_str()
            .is_some_and(|rules| rules.contains("person"))
    );
    // An agent's proposal starts from the edited files and keeps the edit.
    let proposal =
        crate::review::tests::propose(&context, &crate::review::tests::input("agent"), &|| Ok(()))
            .expect("proposal");
    crate::review::operator::commit_proposal_fixture(
        &context,
        proposal["proposal_id"].as_str().expect("id"),
    )
    .expect("commit");
    let after = fs::read_to_string(root.path().join(".shield/rules.yml")).expect("rules");
    assert!(
        after.contains("person") && after.contains("agent"),
        "{after}"
    );
}

#[test]
fn rules_that_config_yml_does_not_name_are_reported_not_ignored() {
    let root = project();
    root.child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: []\n")
        .expect("config");
    // The empty rules.yml Shield writes is fine.
    load(root.path()).expect("empty rules");
    root.child(".shield/rules.yml")
        .write_str("schema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1, argument-rules-v1]\nrules:\n- id: acme\n  decision: allow\n  conditions: {domain: argv, executable: acme, arguments: {kind: any}}\n")
        .expect("hand-written rule");
    let Err(error) = load(root.path()) else {
        panic!("an ignored rule must be reported");
    };
    assert!(error.to_string().contains("rules: rules.yml"), "{error}");
}

#[test]
fn followed_edits_are_not_pending_but_live_proposals_are() {
    let (root, context) = crate::review::tests::enrolled();
    root.child(".shield/config.yml")
        .write_str(CONFIG)
        .expect("config");
    root.child(".shield/rules.yml")
        .write_str(&format!("# Acme edit\n{RULES}"))
        .expect("rules");
    // An ordinary write follows the edited files.
    crate::review::storage::Store::open(root.path())
        .expect("store")
        .update(&context, &|| Ok(()), |_| Ok(serde_json::json!({})))
        .expect("follow");
    let status = crate::review::project::status(&context).expect("status");
    assert_eq!(status["pending_change"], false, "{status}");
    crate::review::tests::propose(&context, &crate::review::tests::input("agent"), &|| Ok(()))
        .expect("proposal");
    let status = crate::review::project::status(&context).expect("status");
    assert_eq!(status["pending_change"], true, "{status}");
}

#[test]
fn files_that_cannot_be_followed_are_an_error_not_the_older_copy() {
    let (root, context) = crate::review::tests::enrolled();
    root.child(".shield/config.yml")
        .write_str("schema_version: 1\npacks: [\n")
        .expect("broken");
    assert!(matches!(
        crate::review::project::load(&context),
        Err(Error::PolicyFiles(_))
    ));
    // Status stays available as the route to diagnose and fix them.
    let status = crate::review::project::status(&context).expect("status");
    assert!(status["policy_files"]["error"].is_string(), "{status}");
}

#[test]
fn status_reports_malformed_custom_rules_like_requests_do() {
    for rules in [
        // Not YAML.
        "schema_version: 1\nkind: custom\nrules: [ {broken\n",
        // YAML, but a rule without a decision.
        "schema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules:\n- id: acme\n  conditions: {domain: argv, executable: acme}\n",
    ] {
        let (root, context) = crate::review::tests::enrolled();
        root.child(".shield/config.yml")
            .write_str(CONFIG)
            .expect("config");
        root.child(".shield/rules.yml")
            .write_str(rules)
            .expect("rules");
        assert!(load(root.path()).is_err(), "{rules}");
        let status = crate::review::project::status(&context).expect("status");
        assert!(status["policy_files"]["error"].is_string(), "{status}");
        assert!(status["policy_files"]["digest"].is_null(), "{status}");
    }
    // Valid files report their digest, from a cached plan or validation alone.
    let root = project();
    let cache = super::super::cache::default_directory().expect("cache");
    let uncompiled = check(root.path(), Some(&cache)).expect("validated");
    let policy = load(root.path()).expect("policy");
    assert_eq!(uncompiled, policy.digest);
    assert_eq!(
        check(root.path(), Some(&cache)).expect("cached"),
        policy.digest
    );
}

#[cfg(unix)]
#[test]
fn shared_or_linked_plans_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    let root = project();
    let policy = load(root.path()).expect("policy");
    let cache = super::super::cache::default_directory().expect("cache");
    let path = plan_path(&cache, &policy.digest);
    assert!(read_plan(&path, &policy.digest).is_some());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("share");
    assert!(read_plan(&path, &policy.digest).is_none());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("private");
    let elsewhere = cache.join("elsewhere.bin");
    fs::rename(&path, &elsewhere).expect("move");
    std::os::unix::fs::symlink(&elsewhere, &path).expect("link");
    assert!(read_plan(&path, &policy.digest).is_none());
}

#[test]
fn a_corrupt_cached_plan_is_compiled_again() {
    let root = project();
    let policy = load(root.path()).expect("policy");
    let cache = super::super::cache::default_directory().expect("cache");
    let path = plan_path(&cache, &policy.digest);
    fs::write(&path, b"not a plan").expect("corrupt");
    let reloaded = load(root.path()).expect("recompiled");
    assert_eq!(policy.evaluator.revision(), reloaded.evaluator.revision());
    assert!(
        read_plan(&path, &policy.digest).is_some(),
        "the recompiled plan replaced the corrupt one"
    );
}

#[test]
fn a_plan_copied_under_another_key_is_refused_and_recompiled() {
    let allowing = project();
    let rules = r#"{"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"run","decision":"allow","reason":"Acme","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}]}"#;
    allowing
        .child(".shield/rules.yml")
        .write_str(rules)
        .expect("rules");
    let other = load(allowing.path()).expect("allowing policy");
    let root = project();
    let policy = load(root.path()).expect("policy");
    let cache = super::super::cache::default_directory().expect("cache");
    // Another project's valid plan, put where this project's plan belongs.
    fs::copy(
        plan_path(&cache, &other.digest),
        plan_path(&cache, &policy.digest),
    )
    .expect("swap");
    assert!(read_plan(&plan_path(&cache, &policy.digest), &policy.digest).is_none());
    let reloaded = load(root.path()).expect("recompiled");
    assert_eq!(policy.evaluator.revision(), reloaded.evaluator.revision());
    assert_ne!(other.evaluator.revision(), reloaded.evaluator.revision());
}

#[test]
fn plans_live_in_the_configured_cache_which_the_guard_protects() {
    let (root, context) = crate::review::tests::enrolled();
    let custom = assert_fs::TempDir::new().expect("cache");
    let custom = custom
        .path()
        .canonicalize()
        .expect("canonical")
        .join("shield");
    crate::review::storage::Store::open(root.path())
        .expect("store")
        .update(&context, &|| Ok(()), |state| {
            state.operator["compiled_cache_root"] = serde_json::json!(custom);
            Ok(serde_json::json!({}))
        })
        .expect("configure");
    let policy = load(root.path()).expect("policy");
    assert!(plan_path(&custom, &policy.digest).is_file());
    assert_eq!(
        crate::review::compiled_cache_root(root.path()).expect("cache root"),
        Some(custom)
    );
}
