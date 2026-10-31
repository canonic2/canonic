//! Small native Linux acceptance: literal launch, Git helper refusal and exact grants.
use assert_cmd::cargo::cargo_bin_cmd;
use assert_fs::prelude::*;
use serde_json::{Value, json};

fn fixture(decision: &str) -> assert_fs::TempDir {
    let root = assert_fs::TempDir::new().expect("create Linux execution fixture");
    let rules = ["/bin/echo", "/usr/bin/true", "/usr/bin/false", "/bin/sleep", "/usr/bin/git", "/bin/sh"]
        .iter().enumerate().map(|(index, executable)| json!({"id":format!("acme-{index}"),"decision":decision,
            "reason":"Acme Linux fixture","conditions":{"domain":"argv","executable":executable,"arguments":{"kind":"any"}}}))
        .collect::<Vec<_>>();
    root.child(".shield/config.yml")
        .write_str(r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#)
        .expect("write Linux fixture configuration");
    root.child(".shield/rules.yml")
        .write_str(
            &json!({"schema_version":1,"kind":"custom",
        "capabilities":["argument-rules-v1","structured-rules-v1"],"rules":rules})
            .to_string(),
        )
        .expect("write Linux fixture rules");
    let result = super::super::operator::interactive(&root, &["init"], "yes\n");
    assert_eq!(result["code"], 0, "{result}");
    root
}

#[test]
fn linux_native_commands_keep_literal_arguments_and_refuse_shells() {
    let root = fixture("allow");
    cargo_bin_cmd!("shield")
        .args(["__internal", "run", "--project"])
        .arg(root.path())
        .args(["--", "/bin/echo", "Acme spaces", "$(touch marker);*"])
        .assert()
        .success()
        .stdout("Acme spaces $(touch marker);*\n");
    assert!(!root.child("marker").path().exists());
    cargo_bin_cmd!("shield")
        .args(["__internal", "run", "--project"])
        .arg(root.path())
        .args(["--", "/usr/bin/false"])
        .assert()
        .code(1)
        .stdout("");
    cargo_bin_cmd!("shield")
        .args(["__internal", "run", "--project"])
        .arg(root.path())
        .args(["--", "/bin/sh", "-c", "touch marker"])
        .assert()
        .code(2)
        .stdout("");
    assert!(!root.child("marker").path().exists());
}

#[test]
fn linux_native_git_ignores_configured_helpers_and_preserves_the_index() {
    let root = fixture("allow");
    let git = |arguments: &[&str]| {
        let output = std::process::Command::new("/usr/bin/git")
            .current_dir(root.path())
            .args(["-c", "maintenance.auto=false", "-c", "gc.auto=0"])
            .args(arguments)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    git(&["init", "-q"]);
    root.child("acme.txt").write_str("Acme original\n").unwrap();
    git(&["add", "acme.txt"]);
    git(&[
        "-c",
        "user.name=Acme",
        "-c",
        "user.email=acme@example.com",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "Acme initial",
    ]);
    root.child("acme.txt").write_str("Acme changed\n").unwrap();
    let index = std::fs::read(root.child(".git/index").path()).unwrap();
    git(&["config", "core.fsmonitor", "touch helper-marker"]);
    git(&["config", "diff.external", "touch helper-marker"]);
    for arguments in [vec!["status"], vec!["diff"], vec!["log", "-n", "1"]] {
        cargo_bin_cmd!("shield")
            .args(["__internal", "run", "--project"])
            .arg(root.path())
            .args(["--", "/usr/bin/git"])
            .args(arguments)
            .assert()
            .success();
    }
    assert!(!root.child("helper-marker").path().exists());
    assert_eq!(
        std::fs::read(root.child(".git/index").path()).unwrap(),
        index
    );
}

#[test]
fn linux_native_approval_is_consumed_once() {
    let root = fixture("ask");
    cargo_bin_cmd!("shield")
        .args(["__internal", "run", "--project"])
        .arg(root.path())
        .args(["--", "/usr/bin/true"])
        .assert()
        .code(3);
    let output = cargo_bin_cmd!("shield")
        .args(["request", "list", "--json", "--project"])
        .arg(root.path())
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let request = value["requests"][0]["request_id"].as_str().unwrap();
    let result =
        super::super::operator::interactive(&root, &["request", "approve", request], "yes\n");
    assert_eq!(result["code"], 0, "{result}");
    for expected in [0, 3] {
        cargo_bin_cmd!("shield")
            .args(["__internal", "run", "--project"])
            .arg(root.path())
            .args(["--approval-request", request, "--", "/usr/bin/true"])
            .assert()
            .code(expected)
            .stdout("");
    }
}
