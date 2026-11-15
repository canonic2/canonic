//! The configuration location contract through the project's own stores:
//! a grouped bundle enforced and changed in place, a root bundle shadowing a
//! grouped one, the explicit file agent setup records, and conversion of the
//! earlier state folder with its records intact.
use super::*;
use std::{fs, path::Path};

const CONFIG: &str = "schema_version: 1\npacks: []\nrules: rules.yml\n";
const EMPTY_RULES: &str =
    r#"{"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[]}"#;
const ALLOW_READ: &str = r#"{"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"read","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}]}"#;

fn bundle(root: &Path, directory: &str, rules: &str) {
    fs::create_dir_all(root.join(directory)).unwrap();
    fs::write(root.join(directory).join("config.yml"), CONFIG).unwrap();
    fs::write(root.join(directory).join("rules.yml"), rules).unwrap();
}

fn decision(root: &Path) -> crate::policy::Decision {
    let root = root.canonicalize().unwrap();
    let text = root.to_str().unwrap().to_owned();
    crate::runtime::Project::open(&root)
        .unwrap()
        .snapshot()
        .unwrap()
        .evaluate(crate::policy::Request {
            executable: "acme".into(),
            arguments: vec!["read".into()],
            cwd: text.clone(),
            project: text,
        })
        .unwrap()
        .evaluation()
        .decision
}

#[test]
fn a_grouped_bundle_is_enforced_and_changed_in_place() {
    let root = assert_fs::TempDir::new().unwrap();
    bundle(root.path(), ".config/shield", ALLOW_READ);
    let context = open(root.path()).unwrap();
    assert_eq!(decision(root.path()), crate::policy::Decision::Allow);
    // Runtime state stays under `.shield/state`; no second bundle appears.
    assert!(root.path().join(".shield/state/state.json").is_file());
    assert!(!root.path().join(".shield/config.yml").exists());

    mode::Prepared::new(&context, false)
        .unwrap()
        .commit(&context, &|| Ok(()))
        .unwrap();
    let local = fs::read_to_string(root.path().join(".config/shield/config.local.yml")).unwrap();
    assert!(local.contains("enforcement: paused"), "{local}");
    assert!(!root.path().join(".shield/config.local.yml").exists());
    assert!(!mode::current(root.path()).unwrap());

    let ignored = fs::read_to_string(root.path().join(".config/shield/.gitignore")).unwrap();
    for name in crate::layout::LOCAL_FILES {
        assert!(ignored.lines().any(|line| line == name), "{ignored}");
    }
    let state_ignored = fs::read_to_string(root.path().join(".shield/.gitignore")).unwrap();
    assert!(
        state_ignored.lines().any(|line| line == "state/"),
        "{state_ignored}"
    );

    let status = project::status(&context).unwrap();
    assert_eq!(
        status["configuration"],
        json!({"file":".config/shield/config.yml","source":"grouped","shadowed":[]})
    );
}

#[test]
fn a_root_bundle_beside_a_grouped_one_is_the_only_policy() {
    let root = assert_fs::TempDir::new().unwrap();
    bundle(root.path(), ".config/shield", ALLOW_READ);
    bundle(root.path(), ".shield", EMPTY_RULES);
    let context = open(root.path()).unwrap();
    // The grouped tree's allowance is not merged into the root policy.
    assert_ne!(decision(root.path()), crate::policy::Decision::Allow);
    let status = project::status(&context).unwrap();
    assert_eq!(
        status["configuration"],
        json!({"file":".shield/config.yml","source":"root","shadowed":[".config/shield/config.yml"]})
    );
}

#[test]
fn an_invalid_root_file_is_reported_instead_of_loading_the_grouped_one() {
    let root = assert_fs::TempDir::new().unwrap();
    bundle(root.path(), ".config/shield", ALLOW_READ);
    fs::create_dir_all(root.path().join(".shield")).unwrap();
    fs::write(root.path().join(".shield/config.yml"), "schema_version: [").unwrap();
    assert!(files::load(root.path()).is_err());
    assert!(
        crate::runtime::Project::open(root.path())
            .ok()
            .and_then(|project| project.snapshot().ok())
            .is_none()
    );
}

#[test]
fn earlier_state_is_converted_with_its_project_and_records() {
    let root = assert_fs::TempDir::new().unwrap();
    bundle(root.path(), ".shield", EMPTY_RULES);
    let first = open(root.path()).unwrap();
    let before = fs::read(root.path().join(".shield/state/state.json")).unwrap();
    // A folder an earlier release used keeps its state under `review`.
    fs::rename(
        root.path().join(".shield/state"),
        root.path().join(".shield/review"),
    )
    .unwrap();
    // Its ignore file names only the earlier folder.
    fs::write(
        root.path().join(".shield/.gitignore"),
        "review/\ncache/\nconfig.local.yml\nrules.local.yml\nshield.local.lock\n",
    )
    .unwrap();
    // A read-only open, as a hook's evaluation makes, converts it and keeps the
    // new folder out of Git before any write happens.
    storage::Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    assert!(!root.path().join(".shield/review").exists());
    let ignored = fs::read_to_string(root.path().join(".shield/.gitignore")).unwrap();
    assert!(ignored.lines().any(|line| line == "state/"), "{ignored}");
    assert!(ignored.lines().any(|line| line == "review/"), "{ignored}");
    assert!(!root.path().join(".shield/state/state.lock").exists());
    let reopened = open(root.path()).unwrap();
    assert_eq!(reopened.project_id, first.project_id);
    assert_eq!(
        fs::read(root.path().join(".shield/state/state.json")).unwrap(),
        before
    );
}

/// `shield --config policy/acme.yml init --agent claude` plans agent setup
/// before init writes the file, so planning must accept the destination.
#[test]
fn init_plans_agent_setup_for_an_explicit_file_it_will_create() {
    let root = assert_fs::TempDir::new().unwrap();
    let folder = root.path().canonicalize().unwrap();
    crate::layout::bind(
        &folder.join("policy/acme.yml"),
        crate::layout::Base::Invocation,
    );
    let destination = crate::layout::init_destination(&folder);
    assert!(!destination.exists());

    integrations::setup::desired(&folder).unwrap();
    let hook = integrations::native::hook_command(&folder, "claude", "PreToolUse").unwrap();
    assert!(hook.contains("--config 'policy/acme.yml'"), "{hook}");
    let entry = crate::agents::find("claude")
        .unwrap()
        .mcp_entry(&folder, &folder, &[]);
    let entry: Value = serde_json::from_str(&entry).unwrap();
    assert_eq!(
        entry["args"].as_array().unwrap()[6..],
        [json!("--config"), json!("policy/acme.yml")]
    );
    // Ordinary commands still refuse the missing file rather than a default.
    assert!(open(&folder).is_err());
    assert!(!folder.join(".shield/config.yml").exists());
}

#[test]
fn agent_setup_records_an_explicit_file_relative_to_the_project() {
    let root = assert_fs::TempDir::new().unwrap();
    bundle(root.path(), "policy", ALLOW_READ);
    let folder = root.path().canonicalize().unwrap();
    crate::layout::bind(
        &folder.join("policy/config.yml"),
        crate::layout::Base::Invocation,
    );
    let context = open(&folder).unwrap();
    assert_eq!(decision(&folder), crate::policy::Decision::Allow);
    assert!(!folder.join(".shield/config.yml").exists());
    assert!(folder.join(".shield/state/state.json").is_file());

    let hook = integrations::native::hook_command(&folder, "claude", "PreToolUse").unwrap();
    assert!(
        hook.contains("--project '.' --config 'policy/config.yml' --agent claude"),
        "{hook}"
    );
    let entry = crate::agents::find("claude")
        .unwrap()
        .mcp_entry(&folder, &folder, &[]);
    let entry: Value = serde_json::from_str(&entry).unwrap();
    assert_eq!(
        entry["args"],
        json!([
            "mcp",
            "serve",
            "--project",
            ".",
            "--agent",
            "claude",
            "--config",
            "policy/config.yml"
        ])
    );
    let status = project::status(&context).unwrap();
    assert_eq!(status["configuration"]["source"], "explicit");
    assert_eq!(status["configuration"]["file"], "policy/config.yml");
}
