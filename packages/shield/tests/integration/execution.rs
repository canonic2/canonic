//! Native launch fidelity, policy blocks and actual owned child outcomes.
#[cfg(target_os = "macos")]
mod macos {
    use assert_cmd::cargo::cargo_bin_cmd;
    use assert_fs::prelude::*;
    use serde_json::{Value, json};

    const GIT: &str = "/Library/Developer/CommandLineTools/usr/bin/git";

    fn git(root: &assert_fs::TempDir, args: &[&str]) {
        let output = std::process::Command::new(GIT)
            .current_dir(root.path())
            .args(args)
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("native Git fixture");
        assert!(output.status.success(), "{:?}", output.stderr);
    }

    /// Fixture commits race Git's detached automatic maintenance, which holds
    /// objects/maintenance.lock, unless the test configuration turns it off.
    #[test]
    fn fixture_git_runs_without_automatic_maintenance() {
        for (key, value) in [("maintenance.auto", "false"), ("gc.auto", "0")] {
            let output = std::process::Command::new(GIT)
                .args(["config", "--get", key])
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .expect("run git config");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                value,
                "{key}: run tests through cargo so .cargo/config.toml sets GIT_CONFIG_GLOBAL"
            );
        }
    }

    fn repository(decision: &str) -> assert_fs::TempDir {
        let root = fixture(decision);
        git(&root, &["init", "-q"]);
        root.child("acme.txt")
            .write_str("Acme original\n")
            .expect("fixture");
        git(&root, &["add", "acme.txt"]);
        git(
            &root,
            &[
                "-c",
                "user.name=Acme",
                "-c",
                "user.email=acme@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "Acme initial",
            ],
        );
        root.child("acme.txt")
            .write_str("Acme changed\n")
            .expect("fixture");
        root
    }

    #[test]
    fn git_inspection_runs_real_commands_without_configured_helpers_or_index_writes() {
        let root = repository("allow");
        let marker = root.child("helper-marker");
        let script = format!("touch {}; exit 1", marker.path().display());
        for key in [
            "core.fsmonitor",
            "diff.external",
            "diff.acme.textconv",
            "filter.acme.clean",
            "filter.acme.process",
            "core.pager",
        ] {
            git(&root, &["config", key, &script]);
        }
        root.child(".gitattributes")
            .write_str("*.txt diff=acme filter=acme\n")
            .expect("fixture");
        let index = std::fs::read(root.child(".git/index").path()).expect("index");
        for (args, expected) in [
            (vec!["status"], " M acme.txt"),
            (vec!["diff"], "+Acme changed"),
            (vec!["log", "-n", "1"], "Acme initial"),
        ] {
            cargo_bin_cmd!("shield")
                .arg("__internal")
                .args(["run", "--project"])
                .arg(root.path())
                .arg("--")
                .arg(GIT)
                .args(args)
                .assert()
                .success()
                .stdout(predicates::str::contains(expected));
        }
        assert!(!marker.path().exists());
        assert_eq!(
            std::fs::read(root.child(".git/index").path()).expect("index"),
            index
        );
        assert!(!root.child(".git/index.lock").path().exists());
        git(&root, &["config", "--unset", "filter.acme.clean"]);
        git(&root, &["config", "--unset", "filter.acme.process"]);
        git(&root, &["add", "acme.txt"]);
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args(["--", GIT, "diff", "--cached"])
            .assert()
            .success()
            .stdout(predicates::str::contains("+Acme changed"));
    }

    #[test]
    fn git_ask_exact_retry_blocks_replay_changed_arguments_and_configuration() {
        let root = repository("ask");
        let id = request(&root, &[GIT, "status"]);
        cli(&root, &["request", "approve", &id]);
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args(["--approval-request", &id, "--", GIT, "diff"])
            .assert()
            .code(3)
            .stdout("");
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args(["--approval-request", &id, "--", GIT, "status"])
            .assert()
            .success()
            .stdout(predicates::str::contains(" M acme.txt"));
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args(["--approval-request", &id, "--", GIT, "status"])
            .assert()
            .code(3)
            .stdout("");
        let id = request(&root, &[GIT, "diff"]);
        cli(&root, &["request", "approve", &id]);
        git(&root, &["config", "acme.changed", "true"]);
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args(["--approval-request", &id, "--", GIT, "diff"])
            .assert()
            .code(3)
            .stdout("");
    }

    #[test]
    fn git_deny_and_unrecognized_forms_never_launch() {
        let root = repository("deny");
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args(["--", GIT, "status"])
            .assert()
            .code(4)
            .stdout("");
        let root = repository("allow");
        for args in [
            vec!["push"],
            vec!["-c", "alias.acme=!touch helper-marker", "acme"],
            vec!["diff", "--ext-diff"],
            vec!["log", "-n", "101"],
            vec!["status", "--porcelain"],
        ] {
            cargo_bin_cmd!("shield")
                .arg("__internal")
                .args(["run", "--project"])
                .arg(root.path())
                .arg("--")
                .arg(GIT)
                .args(args)
                .assert()
                .code(2)
                .stdout("");
        }
        assert!(!root.child("helper-marker").path().exists());
        root.child(".git/info/attributes")
            .write_str("*.txt filter=acme\n")
            .expect("fixture");
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args(["--", GIT, "status"])
            .assert()
            .code(2)
            .stdout("");
    }

    fn history_settings(root: &assert_fs::TempDir, enabled: bool, purge: bool) {
        let answers = format!(
            "history\n{}\n5000\n{}\nyes\n",
            if enabled { "yes" } else { "no" },
            if purge { "yes" } else { "no" }
        );
        let result = super::super::operator::interactive(root, &["policy", "review"], &answers);
        assert_eq!(result["code"], 0, "{result}");
    }

    fn cli(root: &assert_fs::TempDir, args: &[&str]) -> Value {
        if args.starts_with(&["request", "approve"]) {
            let result = super::super::operator::interactive(root, args, "yes\n");
            assert_eq!(result["code"], 0, "{result}");
            return super::super::operator::query(root, &["request", "show", args[2]]);
        }
        let output = cargo_bin_cmd!("shield")
            .args(args)
            .arg("--json")
            .arg("--project")
            .arg(root.path())
            .assert();
        if args.starts_with(&["request", "show"]) {
            let value: Value =
                serde_json::from_slice(&output.get_output().stdout).expect("request receipt");
            if value["error"]["code"] == "not-found" {
                return value;
            }
        }
        let output = output.success();
        serde_json::from_slice(&output.get_output().stdout).expect("execution fixture evidence")
    }
    /// The history reader's event view, with reader-derived completion links.
    fn events(root: &assert_fs::TempDir) -> Vec<Value> {
        let output = cargo_bin_cmd!("shield")
            .args(["__internal", "history-events", "--project"])
            .arg(root.path())
            .assert()
            .success();
        let view: Value =
            serde_json::from_slice(&output.get_output().stdout).expect("history event view");
        assert_eq!(view["available"], true);
        view["events"].as_array().expect("history events").clone()
    }
    fn fixture(decision: &str) -> assert_fs::TempDir {
        let root = assert_fs::TempDir::new().expect("execution fixture evidence");
        let rules = ["/bin/echo","/bin/sleep","/usr/bin/true","/usr/bin/false","/bin/sh","/Library/Developer/CommandLineTools/usr/bin/git"].iter().enumerate().map(|(n,exe)|json!({"id":format!("acme-{n}"),"decision":decision,"reason":"Acme execution fixture","conditions":{"domain":"argv","executable":exe,"arguments":{"kind":"any"}}})).collect::<Vec<_>>();
        root.child("rules.json")
            .write_str(&json!({"schema_version":1,"rules":rules,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string())
            .expect("execution fixture evidence");
        root.child("config.json")
            .write_str(r#"{"schema_version":1,"packs":[],"rules":"rules.json"}"#)
            .expect("execution fixture evidence");
        root.child(".shield/config.yml")
            .write_str(r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#)
            .expect("config fixture");
        let bytes = std::fs::read(root.child("rules.json").path()).expect("rule fixture");
        root.child(".shield/rules.yml")
            .write_binary(&bytes)
            .expect("rule fixture");
        let result = super::super::operator::interactive(&root, &["init"], "yes\n");
        assert_eq!(result["code"], 0, "{result}");
        root
    }
    #[test]
    fn direct_literals_stdout_exit_codes_and_linked_history_are_real() {
        let root = fixture("allow");
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args([
                "--",
                "/bin/echo",
                "Acme spaces",
                "'quotes'",
                "$(touch marker);*",
            ])
            .assert()
            .success()
            .stdout("Acme spaces 'quotes' $(touch marker);*\n");
        assert!(!root.child("marker").path().exists());
        for (exe, code) in [("/usr/bin/true", 0), ("/usr/bin/false", 1)] {
            cargo_bin_cmd!("shield")
                .arg("__internal")
                .args(["run", "--project"])
                .arg(root.path())
                .args(["--", exe])
                .assert()
                .code(code);
        }
        let events = events(&root);
        let completed = events
            .iter()
            .filter(|e| e["kind"] == "completion")
            .collect::<Vec<_>>();
        assert_eq!(completed.len(), 3);
        assert_eq!(completed[2]["details"]["exit_code"], 1);
        for event in completed {
            assert!(event["links"]["decision_event_id"].is_string());
            assert_eq!(event["details"]["evidence"], "owned-child");
        }
        assert_eq!(
            events
                .iter()
                .find(|e| e["kind"] == "decision")
                .expect("execution fixture evidence")["details"]["binding"]["loader"],
            "/usr/lib/dyld"
        );
    }
    #[test]
    fn shells_paths_and_unsupported_arguments_cannot_launch_despite_allow() {
        let root = fixture("allow");
        for argv in [
            vec!["/bin/sh", "-c", "touch marker"],
            vec!["echo", "touch marker"],
            vec!["/usr/bin/true", "ignored"],
            vec!["/bin/sleep", "nan"],
        ] {
            cargo_bin_cmd!("shield")
                .arg("__internal")
                .args(["run", "--project"])
                .arg(root.path())
                .arg("--")
                .args(argv)
                .assert()
                .code(2);
        }
        assert!(!root.child("marker").path().exists());
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args(["--cwd", "/", "--", "/bin/echo", "blocked"])
            .assert()
            .code(2)
            .stdout("");
    }
    #[test]
    fn ask_and_deny_never_spawn_or_record_completions() {
        for (decision, code) in [("ask", 3), ("deny", 4)] {
            let root = fixture(decision);
            cargo_bin_cmd!("shield")
                .arg("__internal")
                .args(["run", "--project"])
                .arg(root.path())
                .args(["--", "/bin/echo", "must-not-print"])
                .assert()
                .code(code)
                .stdout("");
            let events = events(&root);
            assert_eq!(
                events.iter().filter(|e| e["kind"] == "completion").count(),
                0
            );
            assert_eq!(
                events
                    .iter()
                    .find(|e| e["kind"] == "decision")
                    .expect("execution fixture evidence")["details"]["execution"],
                "not-launched"
            );
        }
    }

    fn request(root: &assert_fs::TempDir, argv: &[&str]) -> String {
        request_options(root, &[], argv)
    }
    fn request_options(root: &assert_fs::TempDir, options: &[&str], argv: &[&str]) -> String {
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args(options)
            .arg("--")
            .args(argv)
            .assert()
            .code(3)
            .stdout("");
        let requests = cli(root, &["request", "list"]);
        requests["requests"]
            .as_array()
            .expect("requests")
            .last()
            .expect("native pending row")["request_id"]
            .as_str()
            .expect("request id")
            .into()
    }
    #[test]
    fn native_ask_approval_has_exact_retry_output_and_linked_authority() {
        let root = fixture("ask");
        let id = request(&root, &["/bin/echo", "Acme spaces", "$(touch marker);*"]);
        let shown = cli(&root, &["request", "show", &id]);
        assert_eq!(
            shown["request"]["arguments"],
            json!(["Acme spaces", "$(touch marker);*"])
        );
        assert_eq!(shown["resource"]["timeout_ms"], 5000);
        let approved = cli(&root, &["request", "approve", &id]);
        for changed in [
            vec![
                "--timeout-ms",
                "5001",
                "--",
                "/bin/echo",
                "Acme spaces",
                "$(touch marker);*",
            ],
            vec![
                "--session-id",
                "other-session",
                "--",
                "/bin/echo",
                "Acme spaces",
                "$(touch marker);*",
            ],
            vec!["--", "/bin/echo", "changed"],
        ] {
            cargo_bin_cmd!("shield")
                .arg("__internal")
                .args(["run", "--project"])
                .arg(root.path())
                .args(["--approval-request", &id])
                .args(changed)
                .assert()
                .code(3)
                .stdout("");
        }
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args([
                "--approval-request",
                &id,
                "--",
                "/bin/echo",
                "Acme spaces",
                "$(touch marker);*",
            ])
            .assert()
            .success()
            .stdout("Acme spaces $(touch marker);*\n");
        assert!(!root.child("marker").path().exists());
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args([
                "--approval-request",
                &id,
                "--",
                "/bin/echo",
                "Acme spaces",
                "$(touch marker);*",
            ])
            .assert()
            .code(3)
            .stdout("");
        let events = events(&root);
        let allowed = events
            .iter()
            .find(|e| e["kind"] == "decision" && e["details"]["host_decision"] == "allow")
            .expect("allowed once");
        assert_eq!(allowed["details"]["decision"], "ask");
        assert_eq!(
            allowed["links"]["approval_id"],
            approved["grant"]["approval_id"]
        );
        let completion = events
            .iter()
            .find(|e| e["kind"] == "completion")
            .expect("completion");
        assert_eq!(
            completion["links"]["decision_event_id"],
            allowed["event_id"]
        );
        assert_eq!(completion["details"]["exit_code"], 0);
        assert_eq!(
            cli(&root, &["request", "show", &id])["error"]["code"],
            "not-found"
        );
    }

    #[test]
    fn foreign_requests_cannot_authorize_native_ask_or_bypass_deny() {
        let root = fixture("ask");
        // A read grant selector cannot select native authority.
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args([
                "--approval-request",
                "read-request-other",
                "--",
                "/bin/echo",
                "Acme",
            ])
            .assert()
            .code(2)
            .stdout("");
        let denied = fixture("deny");
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(denied.path())
            .args([
                "--approval-request",
                "read-request-other",
                "--",
                "/bin/echo",
                "Acme",
            ])
            .assert()
            .code(4)
            .stdout("");
        assert_eq!(cli(&denied, &["request", "list"])["requests"], json!([]));
        let allowed = fixture("allow");
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(allowed.path())
            .args([
                "--approval-request",
                "read-request-other",
                "--",
                "/bin/echo",
                "Acme",
            ])
            .assert()
            .code(2)
            .stdout("");
    }

    #[test]
    fn parallel_native_retries_launch_once_with_history_disabled() {
        use std::process::{Command, Stdio};
        let root = fixture("ask");
        history_settings(&root, false, false);
        let id = request(&root, &["/usr/bin/false"]);
        cli(&root, &["request", "approve", &id]);
        let children = (0..4)
            .map(|_| {
                Command::new(assert_cmd::cargo::cargo_bin!("shield"))
                    .arg("__internal")
                    .args(["run", "--project"])
                    .arg(root.path())
                    .args(["--approval-request", &id, "--", "/usr/bin/false"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("owned retry")
            })
            .collect::<Vec<_>>();
        let mut results = children
            .into_iter()
            .map(|child| child.wait_with_output().expect("reap retry"))
            .collect::<Vec<_>>();
        assert_eq!(
            results
                .iter()
                .filter(|r| r.status.code() == Some(1))
                .count(),
            1
        );
        // The store deliberately has a bounded lock wait. Reconcile only an
        // explicit busy response after all concurrent writers have finished;
        // the consumed grant must still refuse that exact retry without launch.
        for result in &mut results {
            assert_eq!(result.stdout, b"");
            if result.status.code() == Some(2) {
                assert!(
                    String::from_utf8_lossy(&result.stderr)
                        .contains("review state is locked; retry after the owning writer finishes"),
                    "unexpected failure: {result:?}"
                );
                *result = Command::new(assert_cmd::cargo::cargo_bin!("shield"))
                    .arg("__internal")
                    .args(["run", "--project"])
                    .arg(root.path())
                    .args(["--approval-request", &id, "--", "/usr/bin/false"])
                    .output()
                    .expect("reconcile busy retry");
            }
        }
        assert_eq!(
            results
                .iter()
                .filter(|r| r.status.code() == Some(3))
                .count(),
            3,
            "native retry outcomes: {results:?}"
        );
        assert!(results.iter().all(|r| r.stdout.is_empty()));
        history_settings(&root, true, true);
        assert_eq!(
            cli(&root, &["request", "show", &id])["error"]["code"],
            "not-found"
        );
    }

    #[test]
    fn approved_native_sleep_times_out_and_never_restores_grant() {
        let root = fixture("ask");
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            // Preparation shares this deadline; leave enough time to reach the
            // running-child boundary that this fixture specifically verifies.
            .args(["--timeout-ms", "5000", "--", "/bin/sleep", "60"])
            .assert()
            .code(3);
        let list = cli(&root, &["request", "list"]);
        let id = list["requests"][0]["request_id"]
            .as_str()
            .expect("pending sleep");
        cli(&root, &["request", "approve", id]);
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args([
                "--timeout-ms",
                "5000",
                "--approval-request",
                id,
                "--",
                "/bin/sleep",
                "60",
            ])
            .assert()
            .code(124)
            .stdout("");
        assert_eq!(
            cli(&root, &["request", "show", id])["error"]["code"],
            "not-found"
        );
        cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            .args([
                "--timeout-ms",
                "5000",
                "--approval-request",
                id,
                "--",
                "/bin/sleep",
                "60",
            ])
            .assert()
            .code(3)
            .stdout("");
    }

    #[test]
    fn cli_term_is_forwarded_and_native_child_is_reaped() {
        cancelled_run("allow");
        cancelled_run("ask");
    }
    fn cancelled_run(decision: &str) {
        use nix::{
            sys::signal::{Signal, kill},
            unistd::Pid,
        };
        use std::{
            process::Command,
            time::{Duration, Instant},
        };
        let root = fixture(decision);
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("shield"));
        command.arg("__internal");
        if decision == "ask" {
            let id = request_options(&root, &["--timeout-ms", "3000"], &["/bin/sleep", "10"]);
            cli(&root, &["request", "approve", &id]);
            command.args(["run", "--approval-request", &id]);
        } else {
            command.arg("run");
        }
        let mut child = command
            .arg("--project")
            .arg(root.path())
            .args(["--timeout-ms", "3000", "--", "/bin/sleep", "10"])
            .spawn()
            .expect("owned Shield process");
        let start = Instant::now();
        let pid = loop {
            let observed = events(&root);
            if let Some(pid) = observed
                .iter()
                .find_map(|e| e["details"]["child_pid"].as_i64())
            {
                break pid;
            }
            if start.elapsed() > Duration::from_secs(2) {
                child.wait().expect("reap fixture deadline");
                panic!("native launch was not observed");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        kill(
            Pid::from_raw(i32::try_from(child.id()).expect("Shield PID")),
            Signal::SIGTERM,
        )
        .expect("signal owned Shield");
        assert_eq!(child.wait().expect("Shield reap").code(), Some(143));
        assert_eq!(
            kill(Pid::from_raw(i32::try_from(pid).expect("native PID")), None),
            Err(nix::errno::Errno::ESRCH)
        );
        let events = events(&root);
        let result = events
            .iter()
            .find(|e| e["kind"] == "completion")
            .expect("completion");
        assert_eq!(result["details"]["status"], "cancelled");
        assert_eq!(result["details"]["signal"], 15);
    }

    #[test]
    fn native_timeout_kills_and_records_actual_signal_without_output() {
        let root = fixture("allow");
        let output = cargo_bin_cmd!("shield")
            .arg("__internal")
            .args(["run", "--project"])
            .arg(root.path())
            // Preparation shares the deadline; verify cancellation after launch.
            .args(["--timeout-ms", "5000", "--", "/bin/sleep", "60"])
            .assert()
            .code(124)
            .stdout("");
        let events = events(&root);
        let event = events
            .iter()
            .find(|e| e["kind"] == "completion")
            .unwrap_or_else(|| {
                panic!(
                    "missing completion: {:?}; history: {events:?}",
                    output.get_output()
                )
            });
        assert_eq!(event["details"]["status"], "timed-out");
        assert_eq!(event["details"]["signal"], 15);
    }
}
