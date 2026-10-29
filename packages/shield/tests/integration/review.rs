//! Real CLI workflows retain project ownership and never execute submitted commands.
use assert_cmd::cargo::cargo_bin_cmd;
use assert_fs::prelude::*;
use serde_json::Value;

fn fixture() -> assert_fs::TempDir {
    let root = assert_fs::TempDir::new().expect("owned project");
    let initialized = super::operator::interactive(&root, &["init"], "yes\n");
    assert_eq!(initialized["code"], 0, "{initialized}");
    root
}

#[test]
fn retained_runtime_ignores_manual_edits_and_public_status_discovers_the_owner() {
    let root = fixture();
    let project = shield::runtime::Project::open(root.path()).unwrap();
    let active = project.snapshot().unwrap();
    root.child(".shield/config.yml")
        .write_str("invalid candidate")
        .unwrap();
    let evidence = active
        .evaluate(shield::policy::Request {
            executable: "acme".into(),
            arguments: vec!["read".into()],
            cwd: active.identity().project_root().to_str().unwrap().into(),
            project: active.identity().project_root().to_str().unwrap().into(),
        })
        .unwrap();
    assert_eq!(
        evidence.evaluation().decision,
        shield::policy::Decision::Ask
    );
    root.child("nested").create_dir_all().unwrap();
    let discovered = cargo_bin_cmd!("shield")
        .current_dir(root.child("nested").path())
        .args(["status", "--json"])
        .assert()
        .success();
    let value: Value = serde_json::from_slice(&discovered.get_output().stdout).unwrap();
    assert_eq!(
        value["active"]["policy_revision"],
        active.identity().policy_revision()
    );
}

#[test]
fn unsupported_internal_commands_leave_enrolled_state_unchanged() {
    let root = fixture();
    let path = root.path().join(".shield/review/state.json");
    let before = std::fs::read(&path).unwrap();
    for args in [
        vec!["about"],
        vec!["check", "--", "acme"],
        vec!["explain", "--", "acme"],
        vec!["snapshot"],
        vec!["compose"],
        vec!["check-config"],
        vec!["explain-config"],
        vec!["diff-config"],
        vec!["diff-argv-config"],
        vec!["diff-policy-config"],
        vec!["preview-request"],
        vec!["bundle"],
        vec!["explain-bundle"],
        vec!["discover"],
        vec!["analyze-shell"],
        vec!["inspect-executable"],
        vec!["runtime", "status"],
        vec!["runtime", "check", "--", "acme"],
        vec!["proposals", "list"],
        vec!["history", "list"],
        vec!["history", "hook"],
        vec!["read-approvals", "list"],
        vec!["run-approvals", "list"],
        vec!["review", "show"],
        vec!["review", "check", "--", "acme"],
    ] {
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(args)
            .arg("--project")
            .arg(root.path())
            .assert()
            .code(2);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
