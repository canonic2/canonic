//! Real Windows launch decisions, one-time grants, cancellation and console restoration.
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

fn system(name: &str) -> PathBuf {
    PathBuf::from(winsafe::GetSystemDirectory().expect("native system directory")).join(name)
}

fn fixture(decision: &str) -> assert_fs::TempDir {
    let root = assert_fs::TempDir::new().expect("owned project");
    fs::create_dir(root.path().join(".shield")).expect("project configuration directory");
    fs::write(
        root.path().join(".shield/config.yml"),
        r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#,
    )
    .expect("write project configuration");
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1","argument-rules-v1","regex-v1"],
        "rules":[{"id":"acme-native","decision":decision,"conditions":{"domain":"argv","executable":{"regex":"(?s).*"},"arguments":{"kind":"any"}}}]}).to_string();
    fs::write(root.path().join(".shield/rules.yml"), rules).expect("write policy fixture");
    crate::windows::enroll(root.path(), false);
    root
}

fn execute(
    root: &Path,
    executable: &Path,
    arguments: &[&str],
    options: &[&str],
) -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("shield");
    command
        .arg("--no-debug-log")
        .args(["__internal", "run", "--project"])
        .arg(root)
        .args(options)
        .arg("--")
        .arg(executable)
        .args(arguments)
        .timeout(Duration::from_secs(10));
    command
}

fn query(root: &Path, arguments: &[&str]) -> Value {
    let output = cargo_bin_cmd!("shield")
        .arg("--no-debug-log")
        .args(arguments)
        .arg("--project")
        .arg(root)
        .arg("--json")
        .assert()
        .success();
    serde_json::from_slice(&output.get_output().stdout).expect("JSON query response")
}

fn events(root: &Path) -> Vec<Value> {
    let output = cargo_bin_cmd!("shield")
        .arg("--no-debug-log")
        .args(["__internal", "history-events", "--project"])
        .arg(root)
        .assert()
        .success();
    serde_json::from_slice::<Value>(&output.get_output().stdout).expect("JSON history response")["events"]
        .as_array()
        .expect("history events array")
        .clone()
}

fn request(root: &Path, executable: &Path, arguments: &[&str]) -> String {
    execute(root, executable, arguments, &[])
        .assert()
        .code(3)
        .stdout("");
    query(root, &["request", "list"])["requests"]
        .as_array()
        .expect("native requests array")
        .last()
        .expect("captured request")["request_id"]
        .as_str()
        .expect("request identifier")
        .to_owned()
}

fn approve(root: &Path, id: &str) {
    crate::windows::run(
        root,
        &["request", "approve", id],
        &[("Review and confirm", b"2\r")],
        0,
    );
}

#[test]
fn windows_checked_launches_follow_policy_and_refuse_unsupported_bindings() {
    for (decision, code) in [("allow", 0), ("deny", 4), ("ask", 3)] {
        let root = fixture(decision);
        let output = execute(root.path(), &system("hostname.exe"), &[], &[])
            .assert()
            .code(code);
        if code == 0 {
            assert_ne!(output.get_output().stdout, [] as [u8; 0]);
            let observed = events(root.path());
            let completed = observed
                .iter()
                .find(|event| event["kind"] == "completion")
                .unwrap();
            assert_eq!(completed["details"]["evidence"], "owned-child");
            assert_eq!(completed["details"]["status"], "completed");
            assert!(completed["details"].get("stdout").is_none());
            assert!(completed["details"].get("stderr").is_none());
            assert!(completed["links"]["decision_event_id"].is_string());
        } else {
            assert_eq!(output.get_output().stdout, [] as [u8; 0]);
        }
    }
    let root = fixture("allow");
    let copy = root.path().join("hostname.exe");
    fs::copy(system("hostname.exe"), &copy).unwrap();
    for (program, arguments) in [
        (system("cmd.exe"), vec!["/c", "echo ACME > marker"]),
        (
            system("WindowsPowerShell/v1.0/powershell.exe"),
            vec!["-NoProfile", "-Command", "Set-Content marker ACME"],
        ),
        (copy, vec![]),
        (system("hostname.exe"), vec!["extra"]),
    ] {
        execute(root.path(), &program, &arguments, &[])
            .assert()
            .code(2)
            .stdout("");
    }
    execute(
        root.path(),
        &system("hostname.exe"),
        &[],
        &["--cwd", root.path().join(".SHIELD").to_str().unwrap()],
    )
    .assert()
    .code(2)
    .stdout("");
    assert!(!root.path().join("marker").exists());
}

#[test]
fn windows_checked_grant_requires_the_exact_request_and_cannot_replay() {
    let root = fixture("ask");
    let executable = system("hostname.exe");
    let id = request(root.path(), &executable, &[]);
    approve(root.path(), &id);
    execute(
        root.path(),
        &system("whoami.exe"),
        &[],
        &["--approval-request", &id],
    )
    .assert()
    .code(3)
    .stdout("");
    let output = execute(root.path(), &executable, &[], &["--approval-request", &id])
        .assert()
        .success();
    assert_ne!(output.get_output().stdout, [] as [u8; 0]);
    execute(root.path(), &executable, &[], &["--approval-request", &id])
        .assert()
        .code(3)
        .stdout("");
    let observed = events(root.path());
    let allowed = observed
        .iter()
        .find(|event| event["kind"] == "decision" && event["details"]["code"] == "allowed-once")
        .unwrap();
    assert_eq!(allowed["details"]["execution"], "spawned");
    assert!(allowed["details"]["child_pid"].is_number());
}

#[test]
fn windows_checked_timeout_and_console_break_keep_grants_consumed_and_restore_console() {
    let root = fixture("ask");
    let executable = system("timeout.exe");
    for cancel in [false, true] {
        let id = request(root.path(), &executable, &["/t", "60", "/nobreak"]);
        approve(root.path(), &id);
        let mut arguments = Vec::new();
        if cancel {
            arguments.push("__run-break-probe");
        }
        arguments.extend([
            "__internal",
            "run",
            "--approval-request",
            &id,
            "--",
            executable.to_str().unwrap(),
            "/t",
            "60",
            "/nobreak",
        ]);
        crate::windows::run(root.path(), &arguments, &[], if cancel { 130 } else { 124 });
        execute(
            root.path(),
            &executable,
            &["/t", "60", "/nobreak"],
            &["--approval-request", &id],
        )
        .assert()
        .code(3)
        .stdout("");
    }
    let observed = events(root.path());
    for status in ["timed-out", "cancelled"] {
        let completed = observed
            .iter()
            .find(|event| event["kind"] == "completion" && event["details"]["status"] == status)
            .unwrap();
        assert_eq!(completed["details"]["evidence"], "owned-child");
        assert_eq!(completed["details"]["signal"], Value::Null);
    }
}

fn git_fixture(root: &Path, executable: &Path, arguments: &[&str]) -> std::process::Output {
    std::process::Command::new(executable)
        .env_clear()
        .env(
            "SystemRoot",
            system("hostname.exe")
                .parent()
                .and_then(Path::parent)
                .expect("Windows directory"),
        )
        .env("GIT_CONFIG_SYSTEM", "NUL")
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .args(["-c", "maintenance.auto=false", "-c", "gc.auto=0"])
        .args(arguments)
        .current_dir(root)
        .output()
        .expect("owned Git fixture command")
}

fn git_repository(executable: &Path, decision: &str) -> assert_fs::TempDir {
    let root = fixture(decision);
    for arguments in [
        vec!["init", "-q"],
        vec!["config", "user.name", "Acme"],
        vec!["config", "user.email", "acme@example.com"],
        vec!["config", "commit.gpgsign", "false"],
    ] {
        let output = git_fixture(root.path(), executable, &arguments);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::write(root.path().join("acme.txt"), "Acme original\n").expect("tracked fixture");
    for arguments in [
        vec!["add", "acme.txt"],
        vec!["commit", "-qm", "Acme initial"],
    ] {
        let output = git_fixture(root.path(), executable, &arguments);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::write(root.path().join("acme.txt"), "Acme changed\n").expect("changed fixture");
    root
}

fn protected_git() -> PathBuf {
    let runtime = if cfg!(target_arch = "aarch64") {
        "clangarm64"
    } else {
        "ucrt64"
    };
    PathBuf::from(std::env::var_os("ProgramFiles").expect("protected program directory"))
        .join("Git")
        .join(runtime)
        .join("bin/git.exe")
}

#[test]
#[ignore = "requires a protected native Git for Windows 2.56.0.windows.3 installation"]
fn windows_git_inspections_bind_resources_and_disable_helpers() {
    let executable = protected_git();
    let root = git_repository(&executable, "allow");
    fs::write(
        root.path().join("helper.sh"),
        "#!/bin/sh\nprintf 'invoked\\n' > helper-ran\n",
    )
    .unwrap();
    let helper = root.path().join("helper.sh");
    let helper = format!(
        "'{}'",
        helper
            .to_str()
            .unwrap()
            .replace('\\', "/")
            .replace('\'', "'\\''")
    );
    for arguments in [
        vec!["config", "core.fsmonitor", &helper],
        vec!["config", "diff.external", &helper],
        vec!["config", "filter.acme.process", &helper],
    ] {
        let output = git_fixture(root.path(), &executable, &arguments);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // Establish that the configured helper is executable before testing its suppression.
    let control = git_fixture(root.path(), &executable, &["status", "--short"]);
    assert!(
        root.path().join("helper-ran").exists(),
        "raw Git must reach the helper: {} / {} / {}",
        control.status,
        String::from_utf8_lossy(&control.stdout),
        String::from_utf8_lossy(&control.stderr)
    );
    fs::remove_file(root.path().join("helper-ran")).unwrap();
    fs::write(root.path().join(".gitattributes"), "*.txt filter=acme\n").unwrap();
    let index = fs::read(root.path().join(".git/index")).unwrap();
    for arguments in [
        vec!["status"],
        vec!["diff"],
        vec!["diff", "--cached"],
        vec!["log", "-n", "1"],
    ] {
        execute(root.path(), &executable, &arguments, &[])
            .assert()
            .success();
        assert!(!root.path().join("helper-ran").exists(), "{arguments:?}");
        assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    }
    fs::create_dir_all(root.path().join(".git/info")).unwrap();
    fs::write(
        root.path().join(".git/info/attributes"),
        "*.txt filter=acme\n",
    )
    .unwrap();
    execute(root.path(), &executable, &["diff"], &[])
        .assert()
        .code(2)
        .stdout("");
}

#[test]
#[ignore = "requires a protected native Git for Windows 2.56.0.windows.3 installation"]
fn windows_git_inspections_consume_only_exact_grants() {
    let executable = protected_git();
    let denied = git_repository(&executable, "deny");
    execute(denied.path(), &executable, &["status"], &[])
        .assert()
        .code(4)
        .stdout("");
    let asked = git_repository(&executable, "ask");
    let id = request(asked.path(), &executable, &["status"]);
    approve(asked.path(), &id);
    execute(
        asked.path(),
        &executable,
        &["diff"],
        &["--approval-request", &id],
    )
    .assert()
    .code(3)
    .stdout("");
    execute(
        asked.path(),
        &executable,
        &["status"],
        &["--approval-request", &id],
    )
    .assert()
    .success();
    execute(
        asked.path(),
        &executable,
        &["status"],
        &["--approval-request", &id],
    )
    .assert()
    .code(3)
    .stdout("");
    let changed = request(asked.path(), &executable, &["status"]);
    approve(asked.path(), &changed);
    assert!(
        git_fixture(
            asked.path(),
            &executable,
            &["config", "acme.changed", "true"]
        )
        .status
        .success()
    );
    execute(
        asked.path(),
        &executable,
        &["status"],
        &["--approval-request", &changed],
    )
    .assert()
    .code(3)
    .stdout("");
}

#[test]
#[ignore = "requires a protected native Git for Windows 2.56.0.windows.3 installation"]
fn windows_git_inspections_refuse_wrappers_copies_and_other_argv() {
    let executable = protected_git();
    let root = fixture("allow");
    for arguments in [
        vec!["--version"],
        vec!["status", "--short"],
        vec!["log", "-n", "0"],
        vec!["log", "-n", "101"],
        vec!["config", "--list"],
    ] {
        execute(root.path(), &executable, &arguments, &[])
            .assert()
            .code(2)
            .stdout("");
    }
    let wrapper = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .unwrap()
        .join("cmd/git.exe");
    execute(root.path(), &wrapper, &["status"], &[])
        .assert()
        .code(2)
        .stdout("");
    let runtime = executable
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .file_name()
        .unwrap();
    let local = root.path().join(runtime).join("bin");
    fs::create_dir_all(&local).unwrap();
    fs::copy(&executable, local.join("git.exe")).unwrap();
    execute(root.path(), &local.join("git.exe"), &["status"], &[])
        .assert()
        .code(2)
        .stdout("");
}
